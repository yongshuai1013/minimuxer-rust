//! gw-limd M7 operations: the RemotePairing routes
//! (inventory §5/§6, diff list `docs/m7-rp-diff.md`).
//!
//! Every operation runs through the `withRSDService` shape:
//! open the named service's stream on the cached RP session,
//! perform the RSD checkin (send + two frames), then speak
//! 4-byte-BE + XML plists on it — Swift limd's RP branches
//! verbatim, including their per-call receive timeouts.
//! Session establishment and caching live in
//! `LibimobiledeviceGateway` (Swift `withRPClient` creates
//! the pair-verify client fresh inside every establishment;
//! the tunnel/RSD pair is what gets cached).

use mm_common::{DeviceGatewayCode, MmErrorInfo};
use mm_rp::RpStream;
use plist::{Dictionary, Value};

use crate::gateway::gw_error;
use crate::gw_idevice_rp::RpSessionHandle;
use crate::rsd_afc;

/// RSD directory names (the idevice stack's
/// `rsd_service_name` values, which address the same
/// directory; resolution still goes through mm-rp's
/// six-level fuzzy match).
pub const SVC_LOCKDOWND: &str = "com.apple.mobile.lockdown.remote.trusted";
pub const SVC_HEARTBEAT: &str = "com.apple.mobile.heartbeat.shim.remote";
pub const SVC_MOUNTER: &str = "com.apple.mobile.mobile_image_mounter.shim.remote";
pub const SVC_INSTPROXY: &str = "com.apple.mobile.installation_proxy.shim.remote";
pub const SVC_AFC: &str = "com.apple.afc.shim.remote";
pub const SVC_HOUSE_ARREST: &str = "com.apple.mobile.house_arrest.shim.remote";
/// Swift limd's `DeviceService` literal for misagent
/// (inventory §2.7) — note the idevice stack's own name for
/// the same service differs (`com.apple.misagent.shim.
/// remote`); the divergence is recorded as D-M7-10.
pub const SVC_MISAGENT: &str = "com.apple.mobile.MCInstall.shim.remote";

fn service_error(reason: String) -> MmErrorInfo {
    gw_error(DeviceGatewayCode::ServiceError, reason)
}

fn connection_failed(reason: String) -> MmErrorInfo {
    gw_error(DeviceGatewayCode::ConnectionFailed, reason)
}

/// Swift `withRSDService`: session → service stream →
/// RSDCheckin → `op`. Stream-open and checkin failures are
/// connection failures on this backend.
pub async fn with_rsd_service<T, F, Fut>(
    session: &RpSessionHandle,
    service: &str,
    op: F,
) -> Result<T, MmErrorInfo>
where
    F: FnOnce(RpStream) -> Fut,
    Fut: std::future::Future<Output = Result<T, MmErrorInfo>>,
{
    let mut guard = session.lock().await;
    let mut stream = guard.open_stream(service).await.map_err(|e| {
        connection_failed(format!(
            "Failed to connect to RSD service '{service}': {}",
            e.message
        ))
    })?;
    stream.rsd_checkin().await.map_err(|e| {
        connection_failed(format!(
            "RSDCheckin failed for service '{service}': {}",
            e.message
        ))
    })?;
    op(stream).await
}

fn dict(entries: Vec<(&str, Value)>) -> Dictionary {
    let mut d = Dictionary::new();
    for (k, v) in entries {
        d.insert(k.into(), v);
    }
    d
}

fn string(value: &str) -> Value {
    Value::String(value.to_string())
}

// ---------------------------------------------------------------------------
// lockdownd + heartbeat
// ---------------------------------------------------------------------------

/// Swift `syncGetLockdownValue` (RP branch): GetValue on
/// the RSD lockdownd stream; the response's `Value` must be
/// a string.
pub async fn get_lockdown_value(
    session: &RpSessionHandle,
    key: &str,
) -> Result<String, MmErrorInfo> {
    let key = key.to_string();
    with_rsd_service(session, SVC_LOCKDOWND, |mut stream| async move {
        let request = dict(vec![
            ("Label", string("SideStore")),
            ("Request", string("GetValue")),
            ("Key", string(&key)),
        ]);
        stream.send_plist(&request).await.map_err(|e| {
            service_error(format!(
                "Failed to get lockdown value for key '{key}': {}",
                e.message
            ))
        })?;
        let response = stream.recv_plist().await.map_err(|e| {
            service_error(format!(
                "Failed to get lockdown value for key '{key}': {}",
                e.message
            ))
        })?;
        match response.get("Value") {
            Some(Value::String(s)) => Ok(s.clone()),
            _ => Err(service_error(format!(
                "Lockdown value for key '{key}' could not be decoded as string"
            ))),
        }
    })
    .await
}

/// Swift `syncPerformHeartbeat` (RP branch): Pico out, the
/// response's `Interval` (integer of either sign class)
/// wins when positive.
pub async fn perform_heartbeat(
    session: &RpSessionHandle,
    interval: u64,
) -> Result<u64, MmErrorInfo> {
    with_rsd_service(session, SVC_HEARTBEAT, |mut stream| async move {
        let request = dict(vec![("Command", string("Pico"))]);
        stream.send_plist(&request).await.map_err(|e| {
            service_error(format!("heartbeat_send failed: {}", e.message))
        })?;
        let response = stream.recv_plist_timeout(5_000).await.map_err(|e| {
            service_error(format!(
                "heartbeat_receive_with_timeout failed: {}",
                e.message
            ))
        })?;
        let new_interval = response
            .get("Interval")
            .and_then(|v| v.as_unsigned_integer())
            .unwrap_or(interval);
        Ok(if new_interval > 0 {
            new_interval
        } else {
            interval
        })
    })
    .await
}

// ---------------------------------------------------------------------------
// Image mounter
// ---------------------------------------------------------------------------

/// Swift `isDDIMounted` (RP branch): LookupImage answers
/// with an `ImageSignature` entry when a developer image
/// is mounted. Any failure reads as "not mounted".
pub async fn is_ddi_mounted(session: &RpSessionHandle) -> bool {
    let result = with_rsd_service(session, SVC_MOUNTER, |mut stream| async move {
        let request = dict(vec![
            ("Command", string("LookupImage")),
            ("ImageType", string("Developer")),
        ]);
        stream
            .send_plist(&request)
            .await
            .map_err(|e| service_error(e.message))?;
        let response = stream
            .recv_plist()
            .await
            .map_err(|e| service_error(e.message))?;
        Ok(response.contains_key("ImageSignature"))
    })
    .await;
    result.unwrap_or(false)
}

/// The shared ReceiveBytes → raw bytes → MountImage
/// sequence of Swift's two mount operations (RP branch).
/// `signature_field` is the signature for Developer mounts
/// and the build manifest for Personalized ones (Swift
/// puts the manifest in `ImageSignature` there).
async fn mount_image(
    session: &RpSessionHandle,
    image_type: &str,
    image: &[u8],
    signature_field: &[u8],
    trustcache: Option<&[u8]>,
    fail_reason: &str,
) -> Result<(), MmErrorInfo> {
    let image = image.to_vec();
    let signature_field = signature_field.to_vec();
    let trustcache = trustcache.map(|t| t.to_vec());
    let image_type = image_type.to_string();
    let fail_reason = fail_reason.to_string();
    with_rsd_service(session, SVC_MOUNTER, |mut stream| async move {
        let fail = |detail: String| service_error(format!("{fail_reason}: {detail}"));
        let request = dict(vec![
            ("Command", string("ReceiveBytes")),
            ("ImageType", string(&image_type)),
            ("ImageSize", Value::Integer((image.len() as i64).into())),
        ]);
        stream
            .send_plist(&request)
            .await
            .map_err(|e| fail(e.message))?;
        let ack = stream
            .recv_plist_timeout(30_000)
            .await
            .map_err(|e| fail(e.message))?;
        if ack.get("Status").and_then(|v| v.as_string()) != Some("ReceiveBytesAck") {
            return Err(fail(format!(
                "unexpected ReceiveBytes response: {ack:?}"
            )));
        }
        stream.send_raw(&image).await.map_err(|e| fail(e.message))?;
        let done = stream
            .recv_plist_timeout(30_000)
            .await
            .map_err(|e| fail(e.message))?;
        if done.get("Status").and_then(|v| v.as_string()) != Some("Complete") {
            return Err(fail(format!(
                "unexpected upload response: {done:?}"
            )));
        }
        let mut mount = dict(vec![
            ("Command", string("MountImage")),
            ("ImageType", string(&image_type)),
            (
                "ImageSignature",
                Value::Data(signature_field),
            ),
        ]);
        if let Some(tc) = trustcache {
            mount.insert("ImageTrustCache".into(), Value::Data(tc));
        }
        stream
            .send_plist(&mount)
            .await
            .map_err(|e| fail(e.message))?;
        let response = stream
            .recv_plist_timeout(30_000)
            .await
            .map_err(|e| fail(e.message))?;
        if response.get("Status").and_then(|v| v.as_string()) != Some("Complete") {
            return Err(fail(format!(
                "unexpected MountImage response: {response:?}"
            )));
        }
        Ok(())
    })
    .await
}

/// Swift `mountDeveloperImage` (RP branch).
pub async fn mount_developer_image(
    session: &RpSessionHandle,
    image: &[u8],
    signature: &[u8],
) -> Result<(), MmErrorInfo> {
    mount_image(
        session,
        "Developer",
        image,
        signature,
        None,
        "Failed to mount developer image",
    )
    .await
}

/// Swift `mountPersonalizedDdi` (RP branch).
pub async fn mount_personalized_ddi(
    session: &RpSessionHandle,
    image: &[u8],
    trustcache: &[u8],
    build_manifest: &[u8],
) -> Result<(), MmErrorInfo> {
    mount_image(
        session,
        "Personalized",
        image,
        build_manifest,
        Some(trustcache),
        "Failed to mount personalized DDI",
    )
    .await
}

// ---------------------------------------------------------------------------
// misagent + installation proxy
// ---------------------------------------------------------------------------

/// Swift's misagent rule (both RP operations): a `Status`
/// integer other than 0 in the response is a failure.
fn check_misagent_status(
    response: &Dictionary,
    fail_reason: &str,
) -> Result<(), MmErrorInfo> {
    let status = response
        .get("Status")
        .and_then(|v| v.as_signed_integer())
        .unwrap_or(0);
    if status != 0 {
        return Err(service_error(format!(
            "{fail_reason}: status {status}"
        )));
    }
    Ok(())
}

pub async fn misagent_install(
    session: &RpSessionHandle,
    profile: &[u8],
) -> Result<(), MmErrorInfo> {
    let profile = profile.to_vec();
    with_rsd_service(session, SVC_MISAGENT, |mut stream| async move {
        let request = dict(vec![
            ("MessageType", string("Install")),
            ("Profile", Value::Data(profile)),
        ]);
        stream.send_plist(&request).await.map_err(|e| {
            service_error(format!("misagent_install failed: {}", e.message))
        })?;
        let response = stream.recv_plist().await.map_err(|e| {
            service_error(format!("misagent_install failed: {}", e.message))
        })?;
        check_misagent_status(&response, "misagent_install failed")
    })
    .await
}

pub async fn misagent_remove(
    session: &RpSessionHandle,
    profile_id: &str,
) -> Result<(), MmErrorInfo> {
    let profile_id = profile_id.to_string();
    with_rsd_service(session, SVC_MISAGENT, |mut stream| async move {
        let request = dict(vec![
            ("MessageType", string("Remove")),
            ("ProfileID", string(&profile_id)),
        ]);
        stream.send_plist(&request).await.map_err(|e| {
            service_error(format!("misagent_remove failed: {}", e.message))
        })?;
        let response = stream.recv_plist().await.map_err(|e| {
            service_error(format!("misagent_remove failed: {}", e.message))
        })?;
        check_misagent_status(&response, "misagent_remove failed")
    })
    .await
}

pub async fn misagent_copy_all(
    session: &RpSessionHandle,
) -> Result<Vec<Vec<u8>>, MmErrorInfo> {
    with_rsd_service(session, SVC_MISAGENT, |mut stream| async move {
        let request = dict(vec![("MessageType", string("CopyAll"))]);
        stream.send_plist(&request).await.map_err(|e| {
            service_error(format!("misagent_copy_all failed: {}", e.message))
        })?;
        // Swift reads the CopyAll response with a 10 s timeout.
        let response = stream.recv_plist_timeout(10_000).await.map_err(|e| {
            service_error(format!("misagent_copy_all failed: {}", e.message))
        })?;
        let mut out = Vec::new();
        if let Some(Value::Array(items)) = response.get("ProfileArray") {
            for item in items {
                if let Value::Data(data) = item {
                    out.push(data.clone());
                }
            }
        }
        Ok(out)
    })
    .await
}

/// Swift `dumpProfiles` (RP branch): CopyAll payloads are
/// written verbatim as `Profile_<i>.mobileprovision`.
pub async fn dump_profiles(
    session: &RpSessionHandle,
    docs_path: &str,
) -> Result<String, MmErrorInfo> {
    let payloads = misagent_copy_all(session).await?;
    let docs = docs_path.strip_prefix("file://").unwrap_or(docs_path);
    let dump_dir = if docs.ends_with("/Profiles") || docs.ends_with("/Profiles/") {
        docs.to_string()
    } else {
        format!("{docs}/Profiles")
    };
    let _ = std::fs::create_dir_all(&dump_dir);
    for (i, bytes) in payloads.iter().enumerate() {
        let _ = std::fs::write(
            format!("{dump_dir}/Profile_{i}.mobileprovision"),
            bytes,
        );
    }
    Ok(dump_dir)
}

/// Swift `installIpa`/`installAppBundle` (RP branch):
/// Install with `PackageType: Developer`, responses read
/// until `Status: Complete` or an `Error` entry.
pub async fn instproxy_install(
    session: &RpSessionHandle,
    package_path: &str,
) -> Result<(), MmErrorInfo> {
    let package_path = package_path.to_string();
    with_rsd_service(session, SVC_INSTPROXY, |mut stream| async move {
        let options = dict(vec![("PackageType", string("Developer"))]);
        let request = dict(vec![
            ("Command", string("Install")),
            ("PackagePath", string(&package_path)),
            ("ClientOptions", Value::Dictionary(options)),
        ]);
        stream.send_plist(&request).await.map_err(|e| {
            service_error(format!("instproxy_install failed: {}", e.message))
        })?;
        loop {
            let response = stream.recv_plist().await.map_err(|e| {
                service_error(format!("instproxy_install failed: {}", e.message))
            })?;
            if let Some(err) = response.get("Error") {
                let detail = match err {
                    Value::String(s) => s.clone(),
                    other => format!("{other:?}"),
                };
                return Err(service_error(format!(
                    "instproxy_install failed: {detail}"
                )));
            }
            if response.get("Status").and_then(|v| v.as_string())
                == Some("Complete")
            {
                return Ok(());
            }
        }
    })
    .await
}

/// Swift `removeApp` (RP branch): the Uninstall response
/// is discarded under a 30 s timeout and the whole call is
/// `try?` — failures are swallowed.
pub async fn instproxy_uninstall(session: &RpSessionHandle, bundle_id: &str) {
    let bundle_id = bundle_id.to_string();
    let _ = with_rsd_service(session, SVC_INSTPROXY, |mut stream| async move {
        let request = dict(vec![
            ("Command", string("Uninstall")),
            ("ApplicationIdentifier", string(&bundle_id)),
        ]);
        stream.send_plist(&request).await.map_err(|e| {
            service_error(format!("instproxy_uninstall failed: {}", e.message))
        })?;
        let _ = stream.recv_plist_timeout(30_000).await;
        Ok(())
    })
    .await;
}

// ---------------------------------------------------------------------------
// AFC over RSD (the M5 rsd_afc packet module) + house arrest
// ---------------------------------------------------------------------------

/// Swift `sendIpaAfc` (RP branch): the IPA lands at
/// `PublicStaging/<bundleId>.ipa` (this backend's staging
/// layout differs from gw-idevice's).
pub async fn send_ipa_afc(
    session: &RpSessionHandle,
    bundle_id: &str,
    ipa_bytes: &[u8],
) -> Result<(), MmErrorInfo> {
    let bundle_id = bundle_id.to_string();
    let ipa_bytes = ipa_bytes.to_vec();
    with_rsd_service(session, SVC_AFC, |mut stream| async move {
        let mut packet_num: u64 = 0;
        let _ = rsd_afc::make_dir(&mut stream, "PublicStaging", &mut packet_num).await;
        let remote_path = format!("PublicStaging/{bundle_id}.ipa");
        let fd = rsd_afc::file_open(&mut stream, &remote_path, 3, &mut packet_num)
            .await?;
        let written = rsd_afc::write_data(
            &mut stream,
            fd,
            &ipa_bytes,
            Some(&remote_path),
            &mut packet_num,
        )
        .await;
        let _ = rsd_afc::file_close(&mut stream, fd, &mut packet_num).await;
        written
    })
    .await
}

/// Swift `sendAppBundleAfc` (RP branch): the bundle tree
/// under `PublicStaging/<bundleId>/<appName>/`, one
/// open/write/close per file.
pub async fn send_app_bundle_afc(
    session: &RpSessionHandle,
    bundle_id: &str,
    app_path: &std::path::Path,
) -> Result<(), MmErrorInfo> {
    let app_name = app_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bundle_id = bundle_id.to_string();
    let app_path = app_path.to_path_buf();
    with_rsd_service(session, SVC_AFC, |mut stream| async move {
        let mut packet_num: u64 = 0;
        let _ = rsd_afc::make_dir(&mut stream, "PublicStaging", &mut packet_num).await;
        let staging = format!("PublicStaging/{bundle_id}");
        let _ = rsd_afc::make_dir(&mut stream, &staging, &mut packet_num).await;
        let remote_app_path = format!("{staging}/{app_name}");
        let _ = rsd_afc::make_dir(&mut stream, &remote_app_path, &mut packet_num).await;
        let mut items = Vec::new();
        crate::gw_idevice_ops::collect_bundle_files(&app_path, &mut items)?;
        for (local_path, is_dir) in &items {
            let relative = local_path
                .strip_prefix(&app_path)
                .unwrap_or(local_path)
                .to_string_lossy()
                .replace('\\', "/");
            let remote_path = format!("{remote_app_path}/{relative}");
            if *is_dir {
                let _ = rsd_afc::make_dir(&mut stream, &remote_path, &mut packet_num)
                    .await;
                continue;
            }
            let data = std::fs::read(local_path).map_err(|e| {
                service_error(format!(
                    "Failed to open remote AFC file {remote_path}: {}",
                    e
                ))
            })?;
            let fd = rsd_afc::file_open(
                &mut stream,
                &remote_path,
                3,
                &mut packet_num,
            )
            .await?;
            let written = if data.is_empty() {
                Ok(())
            } else {
                rsd_afc::write_data(
                    &mut stream,
                    fd,
                    &data,
                    Some(&remote_path),
                    &mut packet_num,
                )
                .await
            };
            let _ = rsd_afc::file_close(&mut stream, fd, &mut packet_num).await;
            written?;
        }
        Ok(())
    })
    .await
}

/// Swift `wipeContainer` (RP branch): VendContainer, the
/// response discarded under a 10 s timeout, the call
/// `try?` — failures are swallowed.
pub async fn wipe_container(session: &RpSessionHandle, bundle_id: &str) {
    let bundle_id = bundle_id.to_string();
    let _ = with_rsd_service(session, SVC_HOUSE_ARREST, |mut stream| async move {
        let request = dict(vec![
            ("Command", string("VendContainer")),
            ("Identifier", string(&bundle_id)),
        ]);
        stream
            .send_plist(&request)
            .await
            .map_err(|e| service_error(e.message))?;
        let _ = stream.recv_plist_timeout(10_000).await;
        Ok(())
    })
    .await;
}

// ---------------------------------------------------------------------------
// Debug (RP branch): connect + checkin is the whole operation
// ---------------------------------------------------------------------------

/// Swift `debugApp`/`debugProcess` (RP branch): opening
/// the debugserver stream (with checkin) and returning is
/// the entire behavior — no RSP commands are sent.
pub async fn debug_connect_only(session: &RpSessionHandle) -> Result<(), MmErrorInfo> {
    let mut guard = session.lock().await;
    let mut stream = guard.open_debug_stream().await.map_err(|e| {
        connection_failed(format!(
            "Failed to connect to RSD service 'debugserver': {}",
            e.message
        ))
    })?;
    stream.rsd_checkin().await.map_err(|e| {
        connection_failed(format!(
            "RSDCheckin failed for service 'debugserver': {}",
            e.message
        ))
    })?;
    Ok(())
}
