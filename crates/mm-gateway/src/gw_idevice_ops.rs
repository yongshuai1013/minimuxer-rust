//! gw-idevice M5 operations (diff list `docs/m5-install-ddi-debug-diff.md`).
//!
//! Free functions over an endpoint + pairing file pair, called
//! by `IdeviceGateway`'s trait methods. Every function is one
//! Swift `IdeviceGateway` operation with its error reasons
//! verbatim; the transports are the ones the diff list
//! records: the idevice crate's typed clients where they
//! cover the operation (`MisagentClient`, `ImageMounter`,
//! `InstallationProxyClient::get_apps`, `AfcClient`), and
//! `rawconn` connections where the crate has no surface
//! (instproxy install/uninstall, house arrest).

use std::net::IpAddr;
use std::path::Path;

use idevice::afc::opcode::AfcFopenMode;
use idevice::afc::AfcClient;
use idevice::lockdown::LockdownClient;
use idevice::misagent::MisagentClient;
use idevice::mobile_image_mounter::ImageMounter;
use idevice::pairing_file::PairingFile;
use idevice::provider::TcpProvider;
use idevice::{Idevice, IdeviceError, IdeviceService};
use mm_common::constants::APP_NAME;
use mm_common::{DeviceGatewayCode, MmErrorInfo};
use plist::{Dictionary, Value};

use crate::afc_session::AfcSession;
use crate::gateway::gw_error;
use crate::rawconn::{
    mux_device_stream, read_plist_frame, write_plist_frame, ServiceConnector,
};
use crate::rsp::RspClient;
use idevice::usbmuxd::UsbmuxdAddr;

/// Everything an M5 operation needs to reach the device.
#[derive(Clone)]
pub struct IdeviceCtx {
    pub endpoint_ip: IpAddr,
    pub pairing_file: PairingFile,
}

impl IdeviceCtx {
    pub fn tcp_provider(&self) -> TcpProvider {
        TcpProvider {
            addr: self.endpoint_ip,
            pairing_file: self.pairing_file.clone(),
            label: APP_NAME.to_string(),
        }
    }

    fn service_connector(&self) -> ServiceConnector {
        ServiceConnector::new(
            self.endpoint_ip,
            self.pairing_file.clone(),
            APP_NAME.to_string(),
        )
    }
}

fn service_error(reason: String) -> MmErrorInfo {
    gw_error(DeviceGatewayCode::ServiceError, reason)
}

fn idevice_msg(err: &IdeviceError) -> String {
    err.to_string()
}

// ---------------------------------------------------------------------------
// misagent (profiles)
// ---------------------------------------------------------------------------

pub async fn misagent_install(
    ctx: &IdeviceCtx,
    profile: &[u8],
) -> Result<(), MmErrorInfo> {
    let provider = ctx.tcp_provider();
    let mut client = MisagentClient::connect(&provider).await.map_err(|e| {
        service_error(format!(
            "Failed to install profile, error: ({})",
            idevice_msg(&e)
        ))
    })?;
    client.install(profile.to_vec()).await.map_err(|e| {
        service_error(format!(
            "Failed to install profile, error: ({})",
            idevice_msg(&e)
        ))
    })
}

pub async fn misagent_remove(
    ctx: &IdeviceCtx,
    profile_id: &str,
) -> Result<(), MmErrorInfo> {
    let provider = ctx.tcp_provider();
    let mut client = MisagentClient::connect(&provider).await.map_err(|e| {
        service_error(format!(
            "Failed to remove profile, error: ({})",
            idevice_msg(&e)
        ))
    })?;
    client.remove(profile_id).await.map_err(|e| {
        service_error(format!(
            "Failed to remove profile, error: ({})",
            idevice_msg(&e)
        ))
    })
}

/// The payloads of `misagent_copy_all` (Swift iterates the
/// FFI array the same way in `dumpProfiles`).
pub async fn misagent_copy_all(
    ctx: &IdeviceCtx,
) -> Result<Vec<Vec<u8>>, MmErrorInfo> {
    let provider = ctx.tcp_provider();
    let mut client = MisagentClient::connect(&provider).await.map_err(|e| {
        service_error(format!(
            "Failed to copy profiles from misagent, error: ({})",
            idevice_msg(&e)
        ))
    })?;
    client.copy_all().await.map_err(|e| {
        service_error(format!(
            "Failed to copy profiles from misagent, error: ({})",
            idevice_msg(&e)
        ))
    })
}

// ---------------------------------------------------------------------------
// instproxy (raw plist protocol over rawconn — D-M5-2)
// ---------------------------------------------------------------------------

const INSTPROXY_SERVICE: &str = "com.apple.mobile.installation_proxy";

async fn instproxy_rpc(
    ctx: &IdeviceCtx,
    request: Dictionary,
    fail_reason: &str,
) -> Result<Dictionary, MmErrorInfo> {
    let mut stream = ctx
        .service_connector()
        .connect_service(INSTPROXY_SERVICE)
        .await
        .map_err(|e| service_error(format!("{fail_reason}, error: ({})", idevice_msg(&e))))?;
    write_plist_frame(&mut stream, request)
        .await
        .map_err(|e| service_error(format!("{fail_reason}, error: ({})", idevice_msg(&e))))?;
    // Read responses until a terminal one: Status Complete,
    // or any response carrying an Error key.
    loop {
        let response = read_plist_frame(&mut stream).await.map_err(|e| {
            service_error(format!("{fail_reason}, error: ({})", idevice_msg(&e)))
        })?;
        if let Some(err) = response.get("Error") {
            let detail = match err {
                Value::String(s) => s.clone(),
                other => format!("{other:?}"),
            };
            return Err(service_error(format!(
                "{fail_reason}, error: ({detail})"
            )));
        }
        match response.get("Status") {
            Some(Value::String(s)) if s == "Complete" => return Ok(response),
            Some(_) => continue,
            None => return Ok(response),
        }
    }
}

pub async fn instproxy_install(
    ctx: &IdeviceCtx,
    package_path: &str,
    client_options: Option<Dictionary>,
    fail_reason: &str,
) -> Result<(), MmErrorInfo> {
    let mut request = Dictionary::new();
    request.insert("Command".into(), Value::String("Install".into()));
    request.insert(
        "PackagePath".into(),
        Value::String(package_path.to_string()),
    );
    if let Some(options) = client_options {
        request.insert("ClientOptions".into(), Value::Dictionary(options));
    }
    instproxy_rpc(ctx, request, fail_reason).await?;
    Ok(())
}

pub async fn instproxy_uninstall(
    ctx: &IdeviceCtx,
    bundle_id: &str,
) -> Result<(), MmErrorInfo> {
    let mut request = Dictionary::new();
    request.insert("Command".into(), Value::String("Uninstall".into()));
    request.insert(
        "ApplicationIdentifier".into(),
        Value::String(bundle_id.to_string()),
    );
    instproxy_rpc(ctx, request, "Failed to uninstall app").await?;
    Ok(())
}

/// Swift `getAppPaths` (idevice): `(container, bundlePath)`.
/// The executable name is resolved inside Swift's helper but
/// only logged there; the pair is what callers use.
pub async fn get_app_paths(
    ctx: &IdeviceCtx,
    bundle_id: &str,
) -> Result<(String, String), MmErrorInfo> {
    let provider = ctx.tcp_provider();
    let mut client =
        idevice::installation_proxy::InstallationProxyClient::connect(&provider)
            .await
            .map_err(|e| {
                service_error(format!(
                    "Failed to lookup app paths, error: ({})",
                    idevice_msg(&e)
                ))
            })?;
    let apps = client
        .get_apps(None, Some(vec![bundle_id.to_string()]))
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to lookup app paths, error: ({})",
                idevice_msg(&e)
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
// AFC (plain + house arrest)
// ---------------------------------------------------------------------------

pub async fn plain_afc_session(ctx: &IdeviceCtx) -> Result<AfcSession, MmErrorInfo> {
    let provider = ctx.tcp_provider();
    let client = AfcClient::connect(&provider).await.map_err(|e| {
        service_error(format!(
            "Failed to connect to AFC service, error: ({})",
            idevice_msg(&e)
        ))
    })?;
    Ok(AfcSession::new(client))
}

const HOUSE_ARREST_SERVICE: &str = "com.apple.mobile.house_arrest";

/// Swift `getAfcClientForApp`: vend the app's container, then
/// speak AFC on the same connection.
pub async fn house_arrest_session(
    ctx: &IdeviceCtx,
    bundle_id: &str,
) -> Result<AfcSession, MmErrorInfo> {
    let fail = |detail: String| {
        service_error(format!(
            "Failed to vend container for {bundle_id}, error: ({detail})"
        ))
    };
    let mut stream = ctx
        .service_connector()
        .connect_service(HOUSE_ARREST_SERVICE)
        .await
        .map_err(|e| fail(idevice_msg(&e)))?;
    let mut request = Dictionary::new();
    request.insert(
        "Command".into(),
        Value::String("VendContainer".into()),
    );
    request.insert(
        "Identifier".into(),
        Value::String(bundle_id.to_string()),
    );
    write_plist_frame(&mut stream, request)
        .await
        .map_err(|e| fail(idevice_msg(&e)))?;
    let response = read_plist_frame(&mut stream)
        .await
        .map_err(|e| fail(idevice_msg(&e)))?;
    if let Some(err) = response.get("Error") {
        let detail = match err {
            Value::String(s) => s.clone(),
            other => format!("{other:?}"),
        };
        return Err(fail(detail));
    }
    let idevice = Idevice::new(stream, APP_NAME.to_string());
    Ok(AfcSession::new(AfcClient::new(idevice)))
}

/// Swift `sendIpaAfc` (idevice): plain AFC, staging dirs
/// created best-effort, one open + one write.
pub async fn send_ipa_afc(
    ctx: &IdeviceCtx,
    bundle_id: &str,
    ipa_bytes: &[u8],
) -> Result<(), MmErrorInfo> {
    let mut afc = plain_afc_session(ctx).await?;
    let _ = afc.make_dir("PublicStaging").await;
    let _ = afc.make_dir(&format!("PublicStaging/{bundle_id}")).await;
    let remote_path = format!("PublicStaging/{bundle_id}/app.ipa");
    let handle = afc
        .file_open(&remote_path, AfcFopenMode::Wr)
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to open remote AFC file, error: ({})",
                idevice_msg(&e)
            ))
        })?;
    let written = afc.file_write(handle, ipa_bytes).await;
    let _ = afc.file_close(handle).await;
    written.map_err(|e| {
        service_error(format!(
            "Failed to write to AFC file, error: ({})",
            idevice_msg(&e)
        ))
    })
}

/// Recursive directory walk shared by `send_app_bundle_afc`:
/// directories become remote mkdirs (errors ignored, as in
/// Swift), files are opened Wr and written when non-empty.
fn collect_bundle_files(
    dir: &Path,
    out: &mut Vec<(std::path::PathBuf, bool)>,
) -> Result<(), MmErrorInfo> {
    let entries = std::fs::read_dir(dir).map_err(|_| {
        service_error(format!("Failed to enumerate {}", dir.display()))
    })?;
    let mut paths: Vec<std::path::PathBuf> =
        entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        let is_dir = path.is_dir();
        out.push((path.clone(), is_dir));
        if is_dir {
            collect_bundle_files(&path, out)?;
        }
    }
    Ok(())
}

/// Swift `sendAppBundleAfc` (idevice).
pub async fn send_app_bundle_afc(
    ctx: &IdeviceCtx,
    bundle_id: &str,
    app_path: &Path,
) -> Result<(), MmErrorInfo> {
    let app_name = app_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut afc = plain_afc_session(ctx).await?;
    let _ = afc.make_dir("PublicStaging").await;
    let _ = afc.make_dir(&format!("PublicStaging/{bundle_id}")).await;
    let remote_app_path = format!("PublicStaging/{bundle_id}/{app_name}");
    let _ = afc.make_dir(&remote_app_path).await;

    let mut items = Vec::new();
    collect_bundle_files(app_path, &mut items)?;
    for (local_path, is_dir) in &items {
        let relative = local_path
            .strip_prefix(app_path)
            .unwrap_or(local_path)
            .to_string_lossy()
            .replace('\\', "/");
        let remote_path = format!("{remote_app_path}/{relative}");
        if *is_dir {
            let _ = afc.make_dir(&remote_path).await;
            continue;
        }
        let data = std::fs::read(local_path).map_err(|e| {
            service_error(format!(
                "Failed to open remote AFC file {remote_path}: {}",
                e
            ))
        })?;
        let handle = afc
            .file_open(&remote_path, AfcFopenMode::Wr)
            .await
            .map_err(|e| {
                service_error(format!(
                    "Failed to open remote AFC file {remote_path}: {}",
                    idevice_msg(&e)
                ))
            })?;
        let result = if data.is_empty() {
            Ok(())
        } else {
            afc.file_write(handle, &data).await
        };
        let _ = afc.file_close(handle).await;
        result.map_err(|e| {
            service_error(format!(
                "Failed to write to AFC file {remote_path}: {}",
                idevice_msg(&e)
            ))
        })?;
    }
    Ok(())
}

/// Swift `wipeContainer` (idevice): recursive delete where
/// info/list errors propagate and remove errors are
/// swallowed.
pub async fn wipe_container(ctx: &IdeviceCtx, bundle_id: &str) -> Result<(), MmErrorInfo> {
    let mut afc = house_arrest_session(ctx, bundle_id).await?;
    wipe_recursive(&mut afc, "/").await
}

fn wipe_recursive<'a>(
    afc: &'a mut AfcSession,
    path: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), MmErrorInfo>> + Send + 'a>> {
    Box::pin(async move {
        let info = afc.get_file_info(path).await.map_err(|e| {
            service_error(format!(
                "Failed to get file info at {path}, error: ({})",
                idevice_msg(&e)
            ))
        })?;
        let is_dir = info
            .iter()
            .any(|(k, v)| k == "st_ifmt" && v.contains("S_IFDIR"));
        if is_dir {
            let entries = afc.list_dir(path).await.map_err(|e| {
                service_error(format!(
                    "Failed to list directory at {path}, error: ({})",
                    idevice_msg(&e)
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
            let _ = afc.remove_path(path).await;
        }
        Ok(())
    })
}

/// The AFC query operations over a house-arrest session
/// (Swift `afcListDirectory`/`afcReadFile`/`afcGetFileInfo`).
pub async fn afc_list_directory(
    ctx: &IdeviceCtx,
    bundle_id: &str,
    path: &str,
) -> Result<Vec<String>, MmErrorInfo> {
    let mut afc = house_arrest_session(ctx, bundle_id).await?;
    afc.list_dir(path).await.map_err(|e| {
        service_error(format!(
            "Failed to list directory: {path}, error: ({})",
            idevice_msg(&e)
        ))
    })
}

pub async fn afc_read_file(
    ctx: &IdeviceCtx,
    bundle_id: &str,
    path: &str,
) -> Result<Vec<u8>, MmErrorInfo> {
    let mut afc = house_arrest_session(ctx, bundle_id).await?;
    let handle = afc
        .file_open(path, AfcFopenMode::RdOnly)
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to open file: {path}, error: ({})",
                idevice_msg(&e)
            ))
        })?;
    let result = afc.file_read_entire(handle).await;
    let _ = afc.file_close(handle).await;
    result.map_err(|e| {
        service_error(format!(
            "Failed to read file: {path}, error: ({})",
            idevice_msg(&e)
        ))
    })
}

pub async fn afc_get_file_info(
    ctx: &IdeviceCtx,
    bundle_id: &str,
    path: &str,
) -> Result<crate::gateway::AfcFileInfo, MmErrorInfo> {
    let mut afc = house_arrest_session(ctx, bundle_id).await?;
    let info = afc.get_file_info(path).await.map_err(|_| {
        service_error(format!("Failed to get info for path: {path}"))
    })?;
    let mut is_directory = false;
    let mut file_size: i64 = 0;
    for (key, value) in &info {
        match key.as_str() {
            // Swift checks `contains` on the FFI string here.
            "st_ifmt" => is_directory = value.contains("S_IFDIR"),
            "st_size" => file_size = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    Ok(crate::gateway::AfcFileInfo {
        is_directory,
        file_size,
    })
}

// ---------------------------------------------------------------------------
// Image mounter (DDI)
// ---------------------------------------------------------------------------

pub async fn image_mounter(ctx: &IdeviceCtx) -> Result<ImageMounter, MmErrorInfo> {
    let provider = ctx.tcp_provider();
    ImageMounter::connect(&provider).await.map_err(|e| {
        service_error(format!(
            "Failed to connect to image mounter, error: ({})",
            idevice_msg(&e)
        ))
    })
}

/// Swift `isDDIMounted` (idevice): a personalized developer
/// image mounted at /System/Developer; copy_devices errors
/// read as "not mounted".
pub async fn is_ddi_mounted(ctx: &IdeviceCtx) -> bool {
    let Ok(mut mounter) = image_mounter(ctx).await else {
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

/// Swift `mountDeveloperImage` (idevice): upload then mount,
/// no trust cache.
pub async fn mount_developer_image(
    ctx: &IdeviceCtx,
    image: &[u8],
    signature: &[u8],
) -> Result<(), MmErrorInfo> {
    let mut mounter = image_mounter(ctx).await?;
    mounter
        .upload_image("Developer", image, signature.to_vec())
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to upload developer image, error: ({})",
                idevice_msg(&e)
            ))
        })?;
    mounter
        .mount_image("Developer", signature.to_vec(), None, None)
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to mount developer image, error: ({})",
                idevice_msg(&e)
            ))
        })
}

/// Swift `mountPersonalizedDdi` (idevice): UniqueChipID via a
/// session-less read first, a session read on failure; then
/// the crate's personalized mount (which handles the
/// personalization manifest / TSS leg internally).
pub async fn mount_personalized_ddi(
    ctx: &IdeviceCtx,
    image: &[u8],
    trustcache: &[u8],
    build_manifest: &[u8],
) -> Result<(), MmErrorInfo> {
    let provider = ctx.tcp_provider();
    let chip_id = unique_chip_id(ctx).await?;
    let mut mounter = ImageMounter::connect(&provider).await.map_err(|_| {
        service_error("Failed to connect to image mounter".to_string())
    })?;
    mounter
        .mount_personalized_with_callback(
            &provider,
            image.to_vec(),
            trustcache.to_vec(),
            build_manifest,
            None,
            chip_id,
            |_| std::future::ready(()),
            (),
        )
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to mount personalized DDI, error: ({})",
                idevice_msg(&e)
            ))
        })
}

async fn unique_chip_id(ctx: &IdeviceCtx) -> Result<u64, MmErrorInfo> {
    let provider = ctx.tcp_provider();
    let mut lockdown = LockdownClient::connect(&provider)
        .await
        .map_err(|_| gw_error(DeviceGatewayCode::NoConnection, ""))?;
    // First attempt without a session (Swift's order).
    if let Ok(value) = lockdown.get_value("UniqueChipID").await {
        if let Some(id) = value.as_unsigned_integer() {
            return Ok(id);
        }
        if value.as_dictionary().map(|d| d.contains_key("Error")) == Some(true) {
            return Err(service_error(
                "Failed to get UniqueChipID: Prohibited".to_string(),
            ));
        }
    }
    lockdown
        .start_session(&ctx.pairing_file)
        .await
        .map_err(|_| gw_error(DeviceGatewayCode::NoConnection, ""))?;
    let value = lockdown
        .get_value("UniqueChipID")
        .await
        .map_err(|_| service_error("Failed to get UniqueChipID".to_string()))?;
    value
        .as_unsigned_integer()
        .ok_or_else(|| service_error("Failed to get UniqueChipID".to_string()))
}

// ---------------------------------------------------------------------------
// Debug (lockdown path — Swift's `launchAppPre17`)
// ---------------------------------------------------------------------------

const DEBUGSERVER_SERVICE: &str = "com.apple.debugserver";

/// Swift `startDebugserverService`: StartService via a
/// session lockdownd connection; returns the service port.
pub async fn start_debugserver_service(ctx: &IdeviceCtx) -> Result<u16, MmErrorInfo> {
    let (port, _ssl) = ctx
        .service_connector()
        .start_service(DEBUGSERVER_SERVICE)
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to start debugserver service, error: ({})",
                idevice_msg(&e)
            ))
        })?;
    Ok(port)
}

/// Swift `sendDebugProxyCommand`: a failed send maps to the
/// construct/send reasons verbatim.
pub async fn send_debug_command(
    client: &mut RspClient,
    name: &str,
    args: &[String],
) -> Result<Option<String>, MmErrorInfo> {
    client.send_command(name, args).await.map_err(|e| {
        service_error(format!(
            "Failed to send command to debug proxy: {name}, error: ({})",
            idevice_msg(&e)
        ))
    })
}

/// Swift `launchAppPre17` behind `debugApp` (the idevice
/// backend's lockdown debug path): resolve paths, start
/// debugserver, connect the proxy through usbmuxd, then
/// QSetMaxPacketSize / QSetWorkingDir / argv / qLaunchSuccess
/// / D.
pub async fn debug_app(
    ctx: &IdeviceCtx,
    bundle_id: &str,
    mux_addr: &UsbmuxdAddr,
) -> Result<(), MmErrorInfo> {
    let (container, bundle_path) = get_app_paths(ctx, bundle_id).await?;
    let port = start_debugserver_service(ctx).await?;
    let stream = mux_device_stream(mux_addr, port, "minimuxer-debug").await?;
    let mut client = RspClient::new(stream);
    send_debug_command(
        &mut client,
        "QSetMaxPacketSize",
        &["1024".to_string()],
    )
    .await?;
    send_debug_command(
        &mut client,
        "QSetWorkingDir",
        &[container],
    )
    .await?;
    client
        .set_argv(&[bundle_path.clone(), bundle_path])
        .await
        .map_err(|e| {
            service_error(format!(
                "Failed to set debug proxy argv, error: ({})",
                idevice_msg(&e)
            ))
        })?;
    send_debug_command(&mut client, "qLaunchSuccess", &[]).await?;
    send_debug_command(&mut client, "D", &[]).await?;
    Ok(())
}
