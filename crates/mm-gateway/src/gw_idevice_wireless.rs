//! gw-idevice wireless pairing (M8; behavior list
//! `docs/m8-wireless-behavior.md` §B G1–G11, diff list
//! `docs/m8-wireless-diff.md`).
//!
//! Line-by-line port of IdeviceGateway's
//! `syncStartWirelessPair` / `syncTriggerWirelessPair`
//! and their private helpers (`generatePairingFile`,
//! `resolveFinalPairingPath`,
//! `finalizeAndSavePairedDevice`). The pair-setup
//! engine is mm-rp's wiring over idevice 0.1.68
//! (verdict D-M8-1); neither function consults gateway
//! state — Swift's don't either (no verifyInitialized
//! on these paths).

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::Path;
use std::time::Duration;

use mm_common::constants::{DEFAULT_RP_PAIRING_FILE_NAME, RP_PAIRING_FILE_SUFFIX};
use mm_common::{DeviceGatewayCode, MmErrorInfo};
use mm_rp::idevice68::remote_pairing::RpPairingFile;
use mm_rp::{pair_network, PairHostPreparation, TcpAcceptor};

use crate::gateway::{
    gw_error, posix_error, PairedDeviceRecord, WirelessPairEvents, WirelessPairStart,
    WirelessPairTrigger,
};

fn service_error(reason: impl Into<String>) -> MmErrorInfo {
    gw_error(DeviceGatewayCode::ServiceError, reason)
}

/// The cancellation failure (diff D-M8-2): Swift's
/// start path reports a failed/cancelled accept with
/// exactly this shape; the trigger path reuses it for
/// a mid-flight cancel, which Swift's blocking calls
/// cannot produce at all.
fn cancelled_error() -> MmErrorInfo {
    service_error("Pairing failed or cancelled")
}

/// Swift `generatePairingFile(hostName:)`: generates a
/// fresh identity and reads its identifier back out of
/// the serialized plist (diff D-M8-8). The crate's
/// generate/serialize steps are infallible, so Swift's
/// "Failed to generate pairing file" / "Generated
/// pairing file is nil" / "Failed to serialize pairing
/// file to bytes" branches have no Rust counterpart;
/// the identifier branch keeps its shape.
fn generate_pairing_file(
    host_name: &str,
) -> Result<(RpPairingFile, String), MmErrorInfo> {
    let file = RpPairingFile::generate(host_name);
    let bytes = file.to_bytes();
    let identifier = plist::Value::from_reader(std::io::Cursor::new(bytes))
        .ok()
        .and_then(|v| {
            v.as_dictionary()
                .and_then(|d| d.get("identifier"))
                .and_then(|i| i.as_string())
                .map(str::to_string)
        })
        .unwrap_or_default();
    if identifier.is_empty() {
        return Err(service_error(
            "Failed to parse identifier from pairing file",
        ));
    }
    Ok((file, identifier))
}

/// Swift `resolveFinalPairingPath` (behavior G11).
pub fn resolve_final_pairing_path(
    out_path: &str,
    peer_name: &str,
    peer_model: &str,
    events: &dyn WirelessPairEvents,
) -> String {
    let path = Path::new(out_path);
    let out_dir: String = if path.is_dir() {
        out_path.to_string()
    } else if path.extension().is_none() {
        out_path.to_string()
    } else {
        path.parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    };

    let last_component = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file_name: String =
        if let Some(resolved) = events.resolve_file_name(peer_name, peer_model) {
            resolved
        } else if !last_component.is_empty() && path.extension().is_some() {
            last_component
        } else {
            let replaced = format!("{peer_name}_{peer_model}")
                .replace(' ', "_")
                .replace('/', "_")
                .replace(':', "_");
            let sanitized: String = replaced
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if sanitized.is_empty() {
                DEFAULT_RP_PAIRING_FILE_NAME.to_string()
            } else {
                format!("{sanitized}{RP_PAIRING_FILE_SUFFIX}")
            }
        };

    if out_dir.is_empty() {
        file_name
    } else {
        Path::new(&out_dir)
            .join(&file_name)
            .to_string_lossy()
            .into_owned()
    }
}

/// Swift `finalizeAndSavePairedDevice` (behavior G10):
/// writes the pairing file to `out_path`, parses the
/// serialized content back into a `PairingFile` (with
/// Swift's read-from-disk fallback), and returns the
/// record. Swift's pairedUdid/altIrkHex computations
/// feed only its logs and are not reproduced
/// (diff D-M8-10).
pub async fn finalize_and_save_paired_device(
    rpf: &RpPairingFile,
    peer_name: &str,
    peer_model: &str,
    out_path: &str,
) -> Result<PairedDeviceRecord, MmErrorInfo> {
    rpf.write_to_file(out_path).await.map_err(|_| {
        service_error("Failed to write pairing file to path")
    })?;
    let bytes = rpf.to_bytes();
    let parsed = String::from_utf8(bytes)
        .ok()
        .and_then(|content| mm_common::parse_pairing_content(&content, None).ok());
    let pairing_file = match parsed {
        Some(parsed) => parsed,
        None => {
            // Swift: read the file back from disk and
            // parse again, throwing the failure verbatim.
            let disk = std::fs::read_to_string(out_path)
                .map_err(|e| posix_error(&e))?;
            mm_common::parse_pairing_content(&disk, None)
                .map_err(|e| e.to_info())?
        }
    };
    Ok(PairedDeviceRecord {
        name: peer_name.to_string(),
        model: peer_model.to_string(),
        pairing_file_path: out_path.to_string(),
        pairing_file,
    })
}

/// Swift `syncStartWirelessPair` (behavior G1–G5).
pub async fn start_wireless_pair(
    args: WirelessPairStart<'_>,
) -> Result<PairedDeviceRecord, MmErrorInfo> {
    let prep = PairHostPreparation::prepare(args.host_name, args.host_model);
    let service_id = prep.service_id().to_string();
    let txt_records = prep.txt_records();

    let acceptor = TcpAcceptor::bind()
        .await
        .map_err(|e| posix_error(&e))?;
    args.events
        .on_ready(&service_id, acceptor.port(), &txt_records);

    let stream = tokio::select! {
        accepted = acceptor.accept() => {
            accepted.map_err(|e| posix_error(&e))?
        }
        _ = args.cancel.cancelled() => {
            return Err(cancelled_error());
        }
    };

    let events = args.events.clone();
    let accept = prep.accept(stream, move |pin| {
        let events = events.clone();
        async move {
            events.on_pin(&pin);
        }
    });
    let (rpf, peer) = tokio::select! {
        result = accept => {
            // Swift collapses every accept_fd failure
            // into this one shape (behavior G5).
            result.map_err(|_| cancelled_error())?
        }
        _ = args.cancel.cancelled() => {
            return Err(cancelled_error());
        }
    };

    // Behavior G4: peer fields are adopted only when
    // non-empty; name/model fall back to the caller's.
    let peer_name = if peer.name.is_empty() {
        args.host_name
    } else {
        &peer.name
    };
    let peer_model = if peer.model.is_empty() {
        args.host_model
    } else {
        &peer.model
    };
    let final_path = resolve_final_pairing_path(
        args.out_path,
        peer_name,
        peer_model,
        args.events.as_ref(),
    );
    finalize_and_save_paired_device(&rpf, peer_name, peer_model, &final_path).await
}

/// Swift `withSockaddr` (behavior G6): builds the
/// target socket address, including the IPv6 scope
/// rules — an explicit `%zone` wins; a link-local
/// address without a zone falls back to en0, then
/// awdl0, then lo0.
fn target_sockaddr(ip: &str, port: u16) -> Result<SocketAddr, MmErrorInfo> {
    let invalid = |kind: &str| {
        gw_error(
            DeviceGatewayCode::InvalidTargetEndpoint,
            format!("Invalid {kind} address: {ip}"),
        )
    };
    if ip.contains(':') {
        let (addr_part, zone) = match ip.split_once('%') {
            Some((addr, zone)) => (addr, Some(zone)),
            None => (ip, None),
        };
        let addr: Ipv6Addr = addr_part.parse().map_err(|_| invalid("IPv6"))?;
        let scope_id = if let Some(zone) = zone {
            if_nametoindex(zone)
        } else if addr.is_unicast_link_local() {
            ["en0", "awdl0", "lo0"]
                .iter()
                .map(|name| if_nametoindex(name))
                .find(|idx| *idx != 0)
                .unwrap_or(0)
        } else {
            0
        };
        Ok(SocketAddr::V6(SocketAddrV6::new(addr, port, 0, scope_id)))
    } else {
        let addr: Ipv4Addr = ip.parse().map_err(|_| invalid("IPv4"))?;
        Ok(SocketAddr::V4(SocketAddrV4::new(addr, port)))
    }
}

fn if_nametoindex(name: &str) -> u32 {
    match std::ffi::CString::new(name) {
        Ok(c_name) => unsafe { libc::if_nametoindex(c_name.as_ptr()) },
        Err(_) => 0,
    }
}

/// Swift `syncTriggerWirelessPair` (behavior G6–G8).
pub async fn trigger_wireless_pair(
    args: WirelessPairTrigger<'_>,
) -> Result<PairedDeviceRecord, MmErrorInfo> {
    let (mut rpf, _identifier) = generate_pairing_file(args.host_name)?;

    if args.target_ip.is_empty() || args.target_port == 0 {
        return Err(gw_error(
            DeviceGatewayCode::InvalidTargetEndpoint,
            format!(
                "Target endpoint IP ({}) or port ({}) is invalid",
                args.target_ip, args.target_port
            ),
        ));
    }
    let target = target_sockaddr(args.target_ip, args.target_port)?;

    // Behavior G7 / diff D-M8-4: the 60-second PIN wait
    // limit lives here, as in Swift's FFI callback; a
    // timeout substitutes "000000" and pairing proceeds
    // (the FFI get_pin nil fallback).
    let pin_events = args.events.clone();
    let pin_callback = move || {
        let events = pin_events.clone();
        async move {
            match tokio::time::timeout(Duration::from_secs(60), events.request_pin())
                .await
            {
                Ok(Ok(pin)) => pin,
                // Timeout (or an events-layer failure):
                // Swift's resolver always answers, so the
                // FFI fallback applies (behavior G7).
                _ => "000000".to_string(),
            }
        }
    };
    let pair = pair_network(target, args.host_name, &mut rpf, pin_callback);
    let peer = tokio::select! {
        result = pair => {
            // Swift: serviceError with the FFI message
            // verbatim (behavior G7).
            result.map_err(|e| service_error(e.message))?
        }
        _ = args.cancel.cancelled() => {
            return Err(cancelled_error());
        }
    };

    // Behavior G8: peer fields are adopted
    // unconditionally when a peer device exists (an
    // empty string overrides the caller's values,
    // unlike the start flow's G4 checks). A pure
    // pair-verify success leaves the caller's values.
    let (peer_name, peer_model) = match &peer {
        Some(peer) => (peer.name.as_str(), peer.model.as_str()),
        None => (args.host_name, args.host_model),
    };
    let final_path = resolve_final_pairing_path(
        args.out_path,
        peer_name,
        peer_model,
        args.events.as_ref(),
    );
    finalize_and_save_paired_device(&rpf, peer_name, peer_model, &final_path).await
}
