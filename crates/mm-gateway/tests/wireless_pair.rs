//! M8 host tests for the gateway wireless pair paths
//! (behavior list `docs/m8-wireless-behavior.md`
//! G4–G11). The SRP wire flow is real-device
//! territory; verified here: path resolution, the
//! finalize round trip, endpoint validation, the
//! refused-connection error shape, and cancellation.

use std::sync::{Arc, Mutex};

use mm_gateway::gateway::{
    GatewayFuture, WirelessPairEvents, WirelessPairStart, WirelessPairTrigger,
};
use mm_gateway::gw_idevice_wireless::{
    finalize_and_save_paired_device, resolve_final_pairing_path,
};
use mm_gateway::{DeviceGateway, IdeviceGateway, PairCancelToken};
use mm_rp::RpIdentity;

#[derive(Default)]
struct FakeEvents {
    resolver: Option<String>,
    ready: Mutex<Option<(String, u16, Vec<(String, String)>)>>,
    pin: Mutex<Option<String>>,
}

impl WirelessPairEvents for FakeEvents {
    fn on_ready(
        &self,
        service_id: &str,
        port: u16,
        txt_records: &[(String, String)],
    ) {
        *self.ready.lock().unwrap() =
            Some((service_id.to_string(), port, txt_records.to_vec()));
    }

    fn on_pin(&self, pin: &str) {
        *self.pin.lock().unwrap() = Some(pin.to_string());
    }

    fn request_pin<'a>(&'a self) -> GatewayFuture<'a, String> {
        Box::pin(async { Ok("123456".to_string()) })
    }

    fn resolve_file_name(&self, _name: &str, _model: &str) -> Option<String> {
        self.resolver.clone()
    }
}

fn events() -> Arc<FakeEvents> {
    Arc::new(FakeEvents::default())
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mm-m8-gw-{tag}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

// -- G11: resolveFinalPairingPath -------------------------------------------

#[test]
fn resolve_existing_dir_uses_sanitized_peer_name() {
    let dir = temp_dir("dir");
    let out = resolve_final_pairing_path(
        dir.to_str().unwrap(),
        "My Device",
        "iPhone15,2",
        events().as_ref(),
    );
    // Space → "_"; the comma is outside the allowed
    // set and is dropped; suffix appended.
    assert_eq!(
        out,
        dir.join("My_Device_iPhone152_rp.plist")
            .to_string_lossy()
            .into_owned()
    );
}

#[test]
fn resolve_file_path_keeps_its_file_name() {
    let dir = temp_dir("file");
    let out_path = dir.join("chosen.plist");
    let out = resolve_final_pairing_path(
        out_path.to_str().unwrap(),
        "Peer",
        "Model",
        events().as_ref(),
    );
    assert_eq!(out, out_path.to_string_lossy().into_owned());
}

#[test]
fn resolve_extensionless_path_is_a_directory() {
    let dir = temp_dir("extless").join("outdir");
    let out = resolve_final_pairing_path(
        dir.to_str().unwrap(),
        "Peer",
        "Model",
        events().as_ref(),
    );
    assert_eq!(
        out,
        dir.join("Peer_Model_rp.plist")
            .to_string_lossy()
            .into_owned()
    );
}

#[test]
fn resolve_resolver_wins_over_everything() {
    let dir = temp_dir("resolver");
    let mut ev = FakeEvents::default();
    ev.resolver = Some("from_app.plist".to_string());
    let out = resolve_final_pairing_path(
        dir.join("chosen.plist").to_str().unwrap(),
        "Peer",
        "Model",
        &ev,
    );
    assert_eq!(
        out,
        dir.join("from_app.plist").to_string_lossy().into_owned()
    );
}

#[test]
fn resolve_sanitize_replaces_separators_and_colons() {
    let dir = temp_dir("sanitize");
    let out = resolve_final_pairing_path(
        dir.to_str().unwrap(),
        "a/b:c d",
        "e",
        events().as_ref(),
    );
    assert_eq!(
        out,
        dir.join("a_b_c_d_e_rp.plist")
            .to_string_lossy()
            .into_owned()
    );
}

// -- G10/P3: finalize round trip ---------------------------------------------

#[tokio::test]
async fn finalize_writes_loadable_rp_pairing_file() {
    let dir = temp_dir("finalize");
    let path = dir.join("Peer_Model_rp.plist");
    let rpf =
        mm_rp::idevice68::remote_pairing::RpPairingFile::generate("SideStore");
    let record = finalize_and_save_paired_device(
        &rpf,
        "Peer",
        "Model",
        path.to_str().unwrap(),
    )
    .await
    .expect("finalize");
    assert_eq!(record.name, "Peer");
    assert_eq!(record.model, "Model");
    assert_eq!(record.pairing_file_path, path.to_string_lossy());
    assert_eq!(
        record.pairing_file.mode(),
        mm_common::PairingProtocol::RpPairing
    );
    // The written file loads through the M3/M7 path.
    let bytes = std::fs::read(&path).expect("file written");
    let identity = RpIdentity::load(&bytes).expect("identity loads");
    assert_eq!(
        identity.identifier(),
        "01c72866-bd42-3c7d-bf6c-fe55dc1e5ae9"
    );
}

#[tokio::test]
async fn finalize_write_failure_is_service_error() {
    let rpf =
        mm_rp::idevice68::remote_pairing::RpPairingFile::generate("SideStore");
    let err = finalize_and_save_paired_device(
        &rpf,
        "Peer",
        "Model",
        "/nonexistent-dir-mm-m8/out.plist",
    )
    .await
    .expect_err("unwritable path must fail");
    assert_eq!(err.domain, "DeviceGatewayError");
    assert_eq!(err.code, "serviceError");
    assert_eq!(err.reason, "Failed to write pairing file to path");
}

// -- G6: trigger endpoint validation ------------------------------------------

fn trigger_args<'a>(
    ip: &'a str,
    port: u16,
    events: &'a Arc<FakeEvents>,
    cancel: &'a PairCancelToken,
) -> WirelessPairTrigger<'a> {
    WirelessPairTrigger {
        target_ip: ip,
        target_port: port,
        host_name: "SideStore",
        host_model: "Mac17,7",
        out_path: "/tmp",
        events: events.clone(),
        cancel: cancel.clone(),
    }
}

#[tokio::test]
async fn trigger_rejects_empty_ip_and_zero_port() {
    let gw = IdeviceGateway::new();
    let ev = events();
    let cancel = PairCancelToken::new();
    let err = gw
        .trigger_wireless_pair(trigger_args("", 49152, &ev, &cancel))
        .await
        .expect_err("empty ip must fail");
    assert_eq!(err.code, "invalidTargetEndpoint");
    assert_eq!(
        err.reason,
        "Target endpoint IP () or port (49152) is invalid"
    );
    let err = gw
        .trigger_wireless_pair(trigger_args("10.0.0.1", 0, &ev, &cancel))
        .await
        .expect_err("zero port must fail");
    assert_eq!(err.code, "invalidTargetEndpoint");
    assert_eq!(
        err.reason,
        "Target endpoint IP (10.0.0.1) or port (0) is invalid"
    );
}

#[tokio::test]
async fn trigger_rejects_unparseable_addresses() {
    let gw = IdeviceGateway::new();
    let ev = events();
    let cancel = PairCancelToken::new();
    let err = gw
        .trigger_wireless_pair(trigger_args("999.1.1.1", 49152, &ev, &cancel))
        .await
        .expect_err("bad ipv4 must fail");
    assert_eq!(err.code, "invalidTargetEndpoint");
    assert_eq!(err.reason, "Invalid IPv4 address: 999.1.1.1");
    let err = gw
        .trigger_wireless_pair(trigger_args("fe80::xyz", 49152, &ev, &cancel))
        .await
        .expect_err("bad ipv6 must fail");
    assert_eq!(err.code, "invalidTargetEndpoint");
    assert_eq!(err.reason, "Invalid IPv6 address: fe80::xyz");
}

/// Behavior G7 (shape): a refused connection surfaces
/// as serviceError carrying the connection failure
/// message (Swift: the FFI message verbatim).
#[tokio::test]
async fn trigger_refused_is_service_error_with_message() {
    let gw = IdeviceGateway::new();
    let ev = events();
    let cancel = PairCancelToken::new();
    // Bind then drop a listener to obtain a closed port.
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let err = gw
        .trigger_wireless_pair(trigger_args("127.0.0.1", port, &ev, &cancel))
        .await
        .expect_err("refused must fail");
    assert_eq!(err.code, "serviceError");
    assert!(
        err.reason.to_lowercase().contains("refused"),
        "reason carries the connect failure: {}",
        err.reason
    );
}

// -- G1/D-M8-2: start waits, onReady fires, cancel ends it --------------------

#[tokio::test]
async fn start_fires_on_ready_then_cancel_ends_it() {
    let gw = IdeviceGateway::new();
    let ev = events();
    let cancel = PairCancelToken::new();
    let dir = temp_dir("start");
    let out_path = dir.to_string_lossy().into_owned();
    let task_events = ev.clone();
    let task_cancel = cancel.clone();
    let handle = tokio::spawn(async move {
        let args = WirelessPairStart {
            host_name: "SideStore",
            host_model: "Mac17,7",
            out_path: &out_path,
            events: task_events,
            cancel: task_cancel,
        };
        gw.start_wireless_pair(args).await
    });
    // Wait for onReady (acceptor bound, advertising data ready).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if ev.ready.lock().unwrap().is_some() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "onReady must fire");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    {
        let ready = ev.ready.lock().unwrap();
        let (service_id, port, txt) = ready.as_ref().unwrap();
        assert_eq!(service_id, "01c72866-bd42-3c7d-bf6c-fe55dc1e5ae9");
        assert_ne!(*port, 0);
        assert_eq!(txt.len(), 7);
    }
    cancel.cancel();
    let err = handle
        .await
        .expect("task completes")
        .expect_err("cancelled start must fail");
    assert_eq!(err.code, "serviceError");
    assert_eq!(err.reason, "Pairing failed or cancelled");
}
