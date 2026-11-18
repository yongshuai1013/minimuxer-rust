//! gw-idevice M7 operations: the RemotePairing routes
//! (inventory §4/§6, diff list `docs/m7-rp-diff.md`).
//!
//! Every function is one Swift `IdeviceGateway` operation on
//! its RP route: the service client is connected over the
//! established [`RpSession`]'s RSD handshake (the FFI's
//! `*_connect_rsd` family) and the operation runs with the
//! same error reason templates as the lockdown routes in
//! `gw_idevice_ops`. Retry/invalidation around these calls
//! lives in `IdeviceGateway::perform_rp` (Swift
//! `performWithService`'s invalidate-rebuild-retry-once).

use std::sync::Arc;

use mm_common::{DeviceGatewayCode, MmErrorInfo};
use mm_rp::idevice68::services::afc::opcode::AfcFopenMode;
use mm_rp::idevice68::services::afc::AfcClient;
use mm_rp::idevice68::services::heartbeat::HeartbeatClient;
use mm_rp::idevice68::services::house_arrest::HouseArrestClient;
use mm_rp::idevice68::services::installation_proxy::InstallationProxyClient;
use mm_rp::idevice68::services::lockdown::LockdownClient;
use mm_rp::idevice68::services::misagent::MisagentClient;
use mm_rp::idevice68::services::mobile_image_mounter::ImageMounter;
use mm_rp::idevice68::IdeviceError;
use mm_rp::RpSession;
use plist::{Dictionary, Value};
use tokio::sync::Mutex;

use crate::gateway::gw_error;

/// The shared handle to a gateway's cached RP session.
pub type RpSessionHandle = Arc<Mutex<RpSession>>;

fn service_error(reason: String) -> MmErrorInfo {
    gw_error(DeviceGatewayCode::ServiceError, reason)
}

fn msg68(err: &IdeviceError) -> String {
    err.to_string()
}

// ---------------------------------------------------------------------------
// lockdownd + heartbeat
// ---------------------------------------------------------------------------

/// Swift `getLockdownValue` (RP route): no StartSession —
/// the RSD lockdownd answers `get_value` directly.
pub async fn get_lockdown_value(
    session: &RpSessionHandle,
    key: &str,
) -> Result<String, MmErrorInfo> {
    let mut session = session.lock().await;
    let mut client = session
        .connect_service::<LockdownClient>()
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to connect to lockdownd, error: ({})",
                e.message
            ))
        })?;
    let value = client
        .get_value(Some(key), None)
        .await
        .map_err(|e| {
            if mm_rp::is_pairing_error68(&e) {
                gw_error(
                    DeviceGatewayCode::InvalidPairingFile,
                    format!("Failed to get lockdown value for key {key}, error: ({})", msg68(&e)),
                )
            } else {
                service_error(format!(
                    "Failed to get lockdown value for key {key}, error: ({})",
                    msg68(&e)
                ))
            }
        })?;
    match value {
        Value::String(s) => Ok(s),
        _ => Err(service_error(format!(
            "Lockdown value for key '{key}' could not be decoded as string"
        ))),
    }
}

/// Swift `performHeartbeat` (RP route): Marco/Polo over the
/// RSD heartbeat client, same sequence as the lockdown route.
pub async fn perform_heartbeat(
    session: &RpSessionHandle,
    interval: u64,
) -> Result<u64, MmErrorInfo> {
    let mut session = session.lock().await;
    let mut client = session
        .connect_service::<HeartbeatClient>()
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to connect to heartbeat, error: ({})",
                e.message
            ))
        })?;
    let new_interval = client.get_marco(interval).await.map_err(|e| {
        service_error(format!(
            "Heartbeat receive failed, error: ({})",
            msg68(&e)
        ))
    })?;
    client.send_polo().await.map_err(|e| {
        service_error(format!(
            "Heartbeat send failed, error: ({})",
            msg68(&e)
        ))
    })?;
    Ok(if new_interval > 0 {
        new_interval
    } else {
        interval
    })
}

// ---------------------------------------------------------------------------
// misagent + installation proxy
// ---------------------------------------------------------------------------

pub async fn misagent_install(
    session: &RpSessionHandle,
    profile: &[u8],
) -> Result<(), MmErrorInfo> {
    let mut session = session.lock().await;
    let mut client = session
        .connect_service::<MisagentClient>()
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to install profile, error: ({})",
                e.message
            ))
        })?;
    client.install(profile.to_vec()).await.map_err(|e| {
        service_error(format!(
            "Failed to install profile, error: ({})",
            msg68(&e)
        ))
    })
}

pub async fn misagent_remove(
    session: &RpSessionHandle,
    profile_id: &str,
) -> Result<(), MmErrorInfo> {
    let mut session = session.lock().await;
    let mut client = session
        .connect_service::<MisagentClient>()
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to remove profile, error: ({})",
                e.message
            ))
        })?;
    client.remove(profile_id).await.map_err(|e| {
        service_error(format!(
            "Failed to remove profile, error: ({})",
            msg68(&e)
        ))
    })
}

pub async fn misagent_copy_all(
    session: &RpSessionHandle,
) -> Result<Vec<Vec<u8>>, MmErrorInfo> {
    let mut session = session.lock().await;
    let mut client = session
        .connect_service::<MisagentClient>()
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to copy profiles from misagent, error: ({})",
                e.message
            ))
        })?;
    client.copy_all().await.map_err(|e| {
        service_error(format!(
            "Failed to copy profiles from misagent, error: ({})",
            msg68(&e)
        ))
    })
}

async fn instproxy_client(
    session: &RpSessionHandle,
) -> Result<(tokio::sync::MutexGuard<'_, RpSession>, InstallationProxyClient), MmErrorInfo> {
    let mut guard = session.lock().await;
    let client = guard
        .connect_service::<InstallationProxyClient>()
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to connect to installation proxy, error: ({})",
                e.message
            ))
        })?;
    Ok((guard, client))
}

pub async fn instproxy_install(
    session: &RpSessionHandle,
    package_path: &str,
    client_options: Option<Dictionary>,
    fail_reason: &str,
) -> Result<(), MmErrorInfo> {
    let (_guard, mut client) = instproxy_client(session).await?;
    client
        .install(
            package_path.to_string(),
            client_options.map(Value::Dictionary),
        )
        .await
        .map_err(|e| service_error(format!("{fail_reason}, error: ({})", msg68(&e))))
}

pub async fn instproxy_uninstall(
    session: &RpSessionHandle,
    bundle_id: &str,
) -> Result<(), MmErrorInfo> {
    let (_guard, mut client) = instproxy_client(session).await?;
    client.uninstall(bundle_id, None).await.map_err(|e| {
        service_error(format!(
            "Failed to uninstall app, error: ({})",
            msg68(&e)
        ))
    })
}

/// Swift `getAppPaths` (RP route via the RSD instproxy).
pub async fn get_app_paths(
    session: &RpSessionHandle,
    bundle_id: &str,
) -> Result<(String, String), MmErrorInfo> {
    let (_guard, mut client) = instproxy_client(session).await?;
    let apps = client
        .get_apps(None, Some(vec![bundle_id.to_string()]))
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to lookup app paths, error: ({})",
                msg68(&e)
            ))
        })?;
    let entry = match apps.get(bundle_id) {
        Some(Value::Dictionary(d)) => d,
        _ => {
            return Err(service_error(format!("App not found: {bundle_id}")));
        }
    };
    let get_string = |key: &str| match entry.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    let container = get_string("Container");
    let path = get_string("Path");
    if container.is_empty() || path.is_empty() {
        return Err(service_error("Failed to resolve app paths".to_string()));
    }
    Ok((container, path))
}

// ---------------------------------------------------------------------------
// AFC (plain + house arrest) over RSD
// ---------------------------------------------------------------------------

async fn plain_afc(session: &RpSessionHandle) -> Result<AfcClient, MmErrorInfo> {
    let mut session = session.lock().await;
    session.connect_service::<AfcClient>().await.map_err(|e| {
        service_error(format!(
            "Failed to connect to AFC service, error: ({})",
            e.message
        ))
    })
}

async fn house_arrest_afc(
    session: &RpSessionHandle,
    bundle_id: &str,
) -> Result<AfcClient, MmErrorInfo> {
    let fail = |detail: String| {
        service_error(format!(
            "Failed to vend container for {bundle_id}, error: ({detail})"
        ))
    };
    let mut session = session.lock().await;
    let client = session
        .connect_service::<HouseArrestClient>()
        .await
        .map_err(|e| fail(e.message))?;
    client
        .vend_container(bundle_id.to_string())
        .await
        .map_err(|e| fail(msg68(&e)))
}

/// Swift `sendIpaAfc` (RP route): plain RSD AFC, staging
/// dirs best-effort, one open + one write (the crate's
/// `write_entire` is the FFI `afc_file_write` equivalent).
pub async fn send_ipa_afc(
    session: &RpSessionHandle,
    bundle_id: &str,
    ipa_bytes: &[u8],
) -> Result<(), MmErrorInfo> {
    let mut afc = plain_afc(session).await?;
    let _ = afc.mk_dir("PublicStaging").await;
    let _ = afc.mk_dir(format!("PublicStaging/{bundle_id}")).await;
    let remote_path = format!("PublicStaging/{bundle_id}/app.ipa");
    let mut file = afc
        .open(remote_path, AfcFopenMode::Wr)
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to open remote AFC file, error: ({})",
                msg68(&e)
            ))
        })?;
    let written = file.write_entire(ipa_bytes).await;
    let _ = file.close().await;
    written.map_err(|e| {
        service_error(format!(
            "Failed to write to AFC file, error: ({})",
            msg68(&e)
        ))
    })
}

/// Swift `sendAppBundleAfc` (RP route).
pub async fn send_app_bundle_afc(
    session: &RpSessionHandle,
    bundle_id: &str,
    app_path: &std::path::Path,
) -> Result<(), MmErrorInfo> {
    let app_name = app_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut afc = plain_afc(session).await?;
    let _ = afc.mk_dir("PublicStaging").await;
    let _ = afc.mk_dir(format!("PublicStaging/{bundle_id}")).await;
    let remote_app_path = format!("PublicStaging/{bundle_id}/{app_name}");
    let _ = afc.mk_dir(remote_app_path.clone()).await;

    let mut items = Vec::new();
    crate::gw_idevice_ops::collect_bundle_files(app_path, &mut items)?;
    for (local_path, is_dir) in &items {
        let relative = local_path
            .strip_prefix(app_path)
            .unwrap_or(local_path)
            .to_string_lossy()
            .replace('\\', "/");
        let remote_path = format!("{remote_app_path}/{relative}");
        if *is_dir {
            let _ = afc.mk_dir(remote_path).await;
            continue;
        }
        let data = std::fs::read(local_path).map_err(|e| {
            service_error(format!(
                "Failed to open remote AFC file {remote_path}: {}",
                e
            ))
        })?;
        let mut file = afc
            .open(remote_path.clone(), AfcFopenMode::Wr)
            .await
            .map_err(|e| {
                service_error(format!(
                    "Failed to open remote AFC file {remote_path}: {}",
                    msg68(&e)
                ))
            })?;
        let result = if data.is_empty() {
            Ok(())
        } else {
            file.write_entire(&data).await
        };
        let _ = file.close().await;
        result.map_err(|e| {
            service_error(format!(
                "Failed to write to AFC file {remote_path}: {}",
                msg68(&e)
            ))
        })?;
    }
    Ok(())
}

/// Swift `wipeContainer` (RP route): recursive delete over
/// the house-arrest AFC client; info/list errors propagate,
/// remove errors are swallowed.
pub async fn wipe_container(
    session: &RpSessionHandle,
    bundle_id: &str,
) -> Result<(), MmErrorInfo> {
    let mut afc = house_arrest_afc(session, bundle_id).await?;
    wipe_recursive(&mut afc, "/").await
}

fn wipe_recursive<'a>(
    afc: &'a mut AfcClient,
    path: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), MmErrorInfo>> + Send + 'a>> {
    Box::pin(async move {
        let info = afc.get_file_info_raw(path.to_string()).await.map_err(|e| {
            service_error(format!(
                "Failed to get file info at {path}, error: ({})",
                msg68(&e)
            ))
        })?;
        let is_dir = info
            .get("st_ifmt")
            .map(|v| v.contains("S_IFDIR"))
            .unwrap_or(false);
        if is_dir {
            let entries = afc.list_dir(path.to_string()).await.map_err(|e| {
                service_error(format!(
                    "Failed to list directory at {path}, error: ({})",
                    msg68(&e)
                ))
            })?;
            for entry in entries {
                if entry == "." || entry == ".." {
                    continue;
                }
                let child = if path == "/" {
                    format!("/{entry}")
                } else {
                    format!("{path}/{entry}")
                };
                wipe_recursive(afc, &child).await?;
            }
        }
        if path != "/" {
            let _ = afc.remove(path.to_string()).await;
        }
        Ok(())
    })
}

pub async fn afc_list_directory(
    session: &RpSessionHandle,
    bundle_id: &str,
    path: &str,
) -> Result<Vec<String>, MmErrorInfo> {
    let mut afc = house_arrest_afc(session, bundle_id).await?;
    afc.list_dir(path.to_string()).await.map_err(|e| {
        service_error(format!(
            "Failed to list directory: {path}, error: ({})",
            msg68(&e)
        ))
    })
}

pub async fn afc_read_file(
    session: &RpSessionHandle,
    bundle_id: &str,
    path: &str,
) -> Result<Vec<u8>, MmErrorInfo> {
    let mut afc = house_arrest_afc(session, bundle_id).await?;
    let mut file = afc
        .open(path.to_string(), AfcFopenMode::RdOnly)
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to open file: {path}, error: ({})",
                msg68(&e)
            ))
        })?;
    let result = file.read_entire().await;
    let _ = file.close().await;
    result.map_err(|e| {
        service_error(format!(
            "Failed to read file: {path}, error: ({})",
            msg68(&e)
        ))
    })
}

pub async fn afc_get_file_info(
    session: &RpSessionHandle,
    bundle_id: &str,
    path: &str,
) -> Result<crate::gateway::AfcFileInfo, MmErrorInfo> {
    let mut afc = house_arrest_afc(session, bundle_id).await?;
    let info = afc
        .get_file_info_raw(path.to_string())
        .await
        .map_err(|_| service_error(format!("Failed to get info for path: {path}")))?;
    let is_directory = info
        .get("st_ifmt")
        .map(|v| v.contains("S_IFDIR"))
        .unwrap_or(false);
    let file_size: i64 = info
        .get("st_size")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    Ok(crate::gateway::AfcFileInfo {
        is_directory,
        file_size,
    })
}

// ---------------------------------------------------------------------------
// Image mounter (DDI) over RSD
// ---------------------------------------------------------------------------

async fn image_mounter(
    session: &RpSessionHandle,
) -> Result<(tokio::sync::MutexGuard<'_, RpSession>, ImageMounter), MmErrorInfo> {
    let mut guard = session.lock().await;
    let client = guard
        .connect_service::<ImageMounter>()
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to connect to image mounter, error: ({})",
                e.message
            ))
        })?;
    Ok((guard, client))
}

/// Swift `isDDIMounted` (RP route): same device predicate
/// as the lockdown route.
pub async fn is_ddi_mounted(session: &RpSessionHandle) -> bool {
    let Ok((_guard, mut mounter)) = image_mounter(session).await else {
        return false;
    };
    let Ok(devices) = mounter.copy_devices().await else {
        return false;
    };
    devices.iter().any(|entry| {
        let Value::Dictionary(d) = entry else {
            return false;
        };
        let get = |k: &str| match d.get(k) {
            Some(Value::String(s)) => Some(s.as_str()),
            _ => None,
        };
        get("MountPath") == Some("/System/Developer")
            && get("PersonalizedImageType") == Some("DeveloperDiskImage")
            && matches!(get("DiskImageType"), None | Some("Personalized"))
    })
}

/// Swift `mountDeveloperImage` (RP route): upload then
/// mount, no trust cache.
pub async fn mount_developer_image(
    session: &RpSessionHandle,
    image: &[u8],
    signature: &[u8],
) -> Result<(), MmErrorInfo> {
    let (_guard, mut mounter) = image_mounter(session).await?;
    mounter
        .upload_image("Developer", image, signature.to_vec())
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to upload developer image, error: ({})",
                msg68(&e)
            ))
        })?;
    mounter
        .mount_image("Developer", signature.to_vec(), None, None)
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to mount developer image, error: ({})",
                msg68(&e)
            ))
        })
}

/// Swift `mountPersonalizedDdi` (RP route): the dedicated
/// RSD path — UniqueChipID via the RSD lockdownd, then the
/// crate's RSD personalized mount (mm-rp session method).
pub async fn mount_personalized_ddi(
    session: &RpSessionHandle,
    image: &[u8],
    trustcache: &[u8],
    build_manifest: &[u8],
) -> Result<(), MmErrorInfo> {
    let mut session = session.lock().await;
    session
        .mount_personalized_ddi(image, trustcache, build_manifest)
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to mount personalized DDI, error: ({})",
                e.message
            ))
        })
}

// ---------------------------------------------------------------------------
// Debug over RSD
// ---------------------------------------------------------------------------

/// Opens the debugserver's RSD stream, performs the RSD
/// checkin and wraps it in the port's `RspClient` (D-M7-9:
/// the crate's generic `DebugProxyClient` cannot be held
/// inside the gateway retry closures; the wire sequence —
/// checkin, then RSP — is the same one the typed client
/// performs).
async fn debug_client(
    session: &RpSessionHandle,
) -> Result<
    (
        tokio::sync::MutexGuard<'_, RpSession>,
        crate::rsp::RspClient<mm_rp::RpStream>,
    ),
    MmErrorInfo,
> {
    let mut guard = session.lock().await;
    let mut stream = guard.open_debug_stream().await.map_err(|e| {
        service_error(format!(
            "Failed to connect to debugserver, error: ({})",
            e.message
        ))
    })?;
    stream.rsd_checkin().await.map_err(|e| {
        service_error(format!(
            "Failed to connect to debugserver, error: ({})",
            e.message
        ))
    })?;
    Ok((guard, crate::rsp::RspClient::new(stream)))
}

async fn send_debug_command(
    client: &mut crate::rsp::RspClient<mm_rp::RpStream>,
    name: &str,
    args: &[String],
) -> Result<Option<String>, MmErrorInfo> {
    client.send_command(name, args).await.map_err(|e| {
        service_error(format!(
            "Failed to send command to debug proxy: {name}, error: ({})",
            e
        ))
    })
}

/// Swift `debugApp` (RP route): the lockdown route's
/// launch sequence (paths → QSetMaxPacketSize →
/// QSetWorkingDir → argv → qLaunchSuccess → D) with the
/// debugserver reached over RSD instead of usbmuxd.
pub async fn debug_app(
    session: &RpSessionHandle,
    bundle_id: &str,
) -> Result<(), MmErrorInfo> {
    let (container, bundle_path) = get_app_paths(session, bundle_id).await?;
    let (_guard, mut client) = debug_client(session).await?;
    send_debug_command(&mut client, "QSetMaxPacketSize", &["1024".to_string()]).await?;
    send_debug_command(&mut client, "QSetWorkingDir", &[container]).await?;
    client
        .set_argv(&[bundle_path.clone(), bundle_path])
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to set debug proxy argv, error: ({})",
                e
            ))
        })?;
    send_debug_command(&mut client, "qLaunchSuccess", &[]).await?;
    send_debug_command(&mut client, "D", &[]).await?;
    Ok(())
}

/// Swift `debugProcess` (RP route — the route where this
/// operation exists at all): vAttach by PID, then detach.
pub async fn debug_process(
    session: &RpSessionHandle,
    pid: u32,
) -> Result<(), MmErrorInfo> {
    let (_guard, mut client) = debug_client(session).await?;
    send_debug_command(&mut client, &format!("vAttach;{pid:x}"), &[]).await?;
    send_debug_command(&mut client, "D", &[]).await?;
    Ok(())
}
