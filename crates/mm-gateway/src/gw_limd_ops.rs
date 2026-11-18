//! gw-limd M5 operations (diff list
//! `docs/m5-install-ddi-debug-diff.md` §4), over
//! rusty_libimobiledevice's typed clients. All functions are
//! synchronous C work; the trait methods run them inside
//! `spawn_blocking`, like the rest of this backend.
//!
//! Client construction follows the wrapper's two shapes:
//! descriptor clients (misagent, mobile image mounter, house
//! arrest) take a `LockdowndService` from
//! `lockdownd.start_service` with Swift's `DeviceService`
//! names — the descriptor is consumed directly, never the
//! wrapper's port-0 surface (M3 note) — and start-service
//! clients (instproxy, AFC, debugserver) are built from the
//! `Device` convenience constructors.
//!
//! Error reasons embed the C error code exactly like Swift's
//! `code <n>` formats; the codes are recovered from the
//! wrapper's error enums via the reverse tables below.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mm_common::constants::APP_NAME;
use mm_common::{DeviceGatewayCode, MmErrorInfo};
use plist_plus::Plist;
use rusty_libimobiledevice::error::{
    AfcError, DebugServerError, HouseArrestError, InstProxyError, MisagentError,
    MobileImageMounterError,
};
use rusty_libimobiledevice::idevice::Device;
use rusty_libimobiledevice::services::afc::{AfcClient, AfcFileMode};
use rusty_libimobiledevice::services::debug_server::{DebugServer, DebugServerCommand};
use rusty_libimobiledevice::services::house_arrest::HouseArrest;
use rusty_libimobiledevice::services::instproxy::InstProxyClient;
use rusty_libimobiledevice::services::lockdownd::LockdowndService;
use rusty_libimobiledevice::services::misagent::MisagentClient;
use rusty_libimobiledevice::services::mobile_image_mounter::MobileImageMounter;

use crate::gateway::gw_error;
use crate::rsp::{process_search_terms, string_to_hex};

// Swift `DeviceService` names (LibimobiledeviceGateway).
const SVC_MISAGENT: &str = "com.apple.mobile.MCInstall.shim.remote";
const SVC_MOUNTER: &str = "com.apple.mobile.mobile_image_mounter";
const SVC_HOUSE_ARREST: &str = "com.apple.mobile.house_arrest";

fn service_error(reason: String) -> MmErrorInfo {
    gw_error(DeviceGatewayCode::ServiceError, reason)
}

// ---------------------------------------------------------------------------
// Reverse error-code tables (of the wrapper's From<i32> impls;
// UnknownError has no recoverable code, -999 marks the loss)
// ---------------------------------------------------------------------------

pub fn misagent_error_code(err: &MisagentError) -> i32 {
    use MisagentError::*;
    match err {
        Success => 0,
        InvalidArg => -1,
        PlistError => -2,
        ConnFailed => -3,
        RequestFailed => -4,
        UnknownError => -999,
    }
}

pub fn instproxy_error_code(err: &InstProxyError) -> i32 {
    use InstProxyError::*;
    match err {
        Success => 0,
        InvalidArg => -1,
        PlistError => -2,
        ConnFailed => -3,
        OpInProgress => -4,
        OpFailed => -5,
        RecieveTimeout => -6,
        AlreadyArchived => -7,
        ApiInternalerror => -8,
        ApplicationAlreadyInstalled => -9,
        ApplicationMoveFailed => -10,
        ApplicationSinfCaptureFailed => -11,
        ApplicationSandboxFailed => -12,
        ApplicationVerificationFailed => -13,
        ArchiveDestructionFailed => -14,
        BundleVerificationFailed => -15,
        CarrierBundleCopyFailed => -16,
        CarrierBundleDirectoryCreationFailed => -17,
        CarrierBundleMissingSupportedSims => -18,
        CommCenterNotificationFailed => -19,
        ContainerCreationFailed => -20,
        ContainerP0wnFailed => -21,
        ContainerRemovalFailed => -22,
        EmbeddedProfileInstallFailed => -23,
        ExecutableTwiddleFailed => -24,
        ExistenceCheckFailed => -25,
        InstallMapUpdateFailed => -26,
        ManifestCaptureFailed => -27,
        MapGenerationFailed => -28,
        MissingBundleExecutable => -29,
        MissingBundleIdentifier => -30,
        MissingBundlePath => -31,
        MissingContainer => -32,
        NotificationFailed => -33,
        PackageExtractionFailed => -34,
        PackageInspectionFailed => -35,
        PackageMoveFailed => -36,
        PathConversionFailed => -37,
        RestoreConversionFailed => -38,
        SeatbeltProfileRemovalFailed => -39,
        StageCreationFailed => -40,
        SymlinkFailed => -41,
        UnknownCommand => -42,
        ItunesArtworkCaptureFailed => -43,
        ItunesMetadataCaptureFailed => -44,
        DeviceOsVersionTooLow => -45,
        DeviceFamilyNotSupported => -46,
        PackagePatchFailed => -47,
        IncorrectArchitecture => -48,
        PluginCopyFailed => -49,
        BreadcrumbFailed => -50,
        BreadcrumbUnlockFailed => -51,
        GeoJsonCaptureFailed => -52,
        NewsstandArtworkCaptureFailed => -53,
        MissingCommand => -54,
        NotEntitled => -55,
        MissingPackagePath => -56,
        MissingContainerPath => -57,
        MissingApplicationIdentifier => -58,
        MissingAttributeValue => -59,
        LookupFailed => -60,
        DictCreationFailed => -61,
        InstallProhibited => -62,
        UninstallProhibited => -63,
        MissingBundleVersion => -64,
        MissingObjectDepenency => -100,
        UnknownError => -999,
    }
}

pub fn afc_error_code(err: &AfcError) -> i32 {
    use AfcError::*;
    match err {
        Success => 0,
        UnknownError => 1,
        OpHeaderInvalid => 2,
        NoResources => 3,
        ReadError => 4,
        WriteError => 5,
        UnknownPacketType => 6,
        InvalidArg => 7,
        ObjectNotFound => 8,
        ObjectIsDir => 9,
        PermDenied => 10,
        ServiceNotConnected => 11,
        OpTimeout => 12,
        TooMuchData => 13,
        EndOfData => 14,
        OpNotSupported => 15,
        ObjectExists => 16,
        ObjectBusy => 17,
        NoSpaceLeft => 18,
        OpWouldBlock => 19,
        IoError => 20,
        OpInterrupted => 21,
        OpInProgress => 22,
        InternalError => 23,
        MuxError => 30,
        NoMem => 31,
        NotEnoughData => 32,
        DirNotEmpty => 33,
        ForceSignedType => -999,
    }
}

pub fn mounter_error_code(err: &MobileImageMounterError) -> i32 {
    use MobileImageMounterError::*;
    match err {
        Success => 0,
        InvalidArg => -1,
        PlistError => -2,
        ConnFailed => -3,
        CommandFailed => -4,
        DeviceLocked => -5,
        DmgNotFound => -100,
        SignatureNotFound => -101,
        MissingObjectDepenency => -102,
        UnknownError => -999,
    }
}

pub fn house_arrest_error_code(err: &HouseArrestError) -> i32 {
    use HouseArrestError::*;
    match err {
        Success => 0,
        InvalidArg => -1,
        PlistError => -2,
        ConnFailed => -3,
        InvalidMode => -4,
        UnknownError => -999,
    }
}

pub fn debugserver_error_code(err: &DebugServerError) -> i32 {
    use DebugServerError::*;
    match err {
        Success => 0,
        InvalidArg => 1,
        MuxError => 2,
        SslError => 3,
        ResponseError => 4,
        Timeout => 5,
        UnknownError => -999,
    }
}

// ---------------------------------------------------------------------------
// Connection helpers (Swift withDevice/withLockdown/withService).
// Every wrapper client borrows the `Device` (phantom lifetime),
// so each operation runs inside a closure over a locally owned
// device — the same shape as the legacy crate's call sites.
// ---------------------------------------------------------------------------

fn with_device<T>(
    udid: &str,
    body: impl FnOnce(&Device) -> Result<T, MmErrorInfo>,
) -> Result<T, MmErrorInfo> {
    let device = rusty_libimobiledevice::idevice::get_device(udid).map_err(|e| {
        gw_error(
            DeviceGatewayCode::ConnectionFailed,
            format!(
                "idevice_new_with_options failed with code {}",
                crate::gw_limd::idevice_error_code(&e)
            ),
        )
    })?;
    body(&device)
}

/// Swift `withService`'s first half: handshake lockdown
/// client + StartService, with the descriptor consumed by
/// `body` while the lockdown client is alive (the
/// descriptor's phantom lifetime ties it to the client).
fn with_descriptor<T>(
    udid: &str,
    name: &str,
    body: impl FnOnce(&Device, LockdowndService) -> Result<T, MmErrorInfo>,
) -> Result<T, MmErrorInfo> {
    with_device(udid, |device| {
        let mut lockdown = device.new_lockdownd_client(APP_NAME).map_err(|e| {
            gw_error(
                DeviceGatewayCode::ConnectionFailed,
                format!(
                    "lockdownd_client_new_with_handshake failed with code {}",
                    crate::gw_limd::lockdownd_error_code(&e)
                ),
            )
        })?;
        let descriptor = lockdown.start_service(name, false).map_err(|e| {
            gw_error(
                DeviceGatewayCode::ServiceError,
                format!(
                    "lockdownd_start_service failed with code {}",
                    crate::gw_limd::lockdownd_error_code(&e)
                ),
            )
        })?;
        body(device, descriptor)
    })
}

fn with_misagent<T>(
    udid: &str,
    body: impl FnOnce(&MisagentClient) -> Result<T, MmErrorInfo>,
) -> Result<T, MmErrorInfo> {
    with_descriptor(udid, SVC_MISAGENT, |device, descriptor| {
        let client = MisagentClient::new(device, descriptor).map_err(|e| {
            service_error(format!(
                "misagent_client_new failed with code {}",
                misagent_error_code(&e)
            ))
        })?;
        body(&client)
    })
}

fn with_mounter<T>(
    udid: &str,
    body: impl FnOnce(&MobileImageMounter) -> Result<T, MmErrorInfo>,
) -> Result<T, MmErrorInfo> {
    with_descriptor(udid, SVC_MOUNTER, |device, descriptor| {
        let client = MobileImageMounter::new(device, descriptor).map_err(|e| {
            service_error(format!(
                "mobile_image_mounter_new failed with code {}",
                mounter_error_code(&e)
            ))
        })?;
        body(&client)
    })
}

fn with_instproxy<T>(
    udid: &str,
    body: impl FnOnce(&InstProxyClient) -> Result<T, MmErrorInfo>,
) -> Result<T, MmErrorInfo> {
    with_device(udid, |device| {
        let client = device.new_instproxy_client(APP_NAME).map_err(|e| {
            service_error(format!(
                "instproxy_client_new failed with code {}",
                instproxy_error_code(&e)
            ))
        })?;
        body(&client)
    })
}

fn with_plain_afc<T>(
    udid: &str,
    body: impl FnOnce(&AfcClient) -> Result<T, MmErrorInfo>,
) -> Result<T, MmErrorInfo> {
    with_device(udid, |device| {
        let client = device.new_afc_client(APP_NAME).map_err(|e| {
            service_error(format!(
                "afc_client_new failed with code {}",
                afc_error_code(&e)
            ))
        })?;
        body(&client)
    })
}

// ---------------------------------------------------------------------------
// Profiles (misagent)
// ---------------------------------------------------------------------------

pub fn install_profile(udid: &str, profile: &[u8]) -> Result<(), MmErrorInfo> {
    if profile.is_empty() {
        return Err(service_error("Invalid profile buffer".to_string()));
    }
    with_misagent(udid, |client| {
        client
            .install(Plist::new_data(profile))
            .map_err(|e| {
                service_error(format!(
                    "misagent_install failed with code {}",
                    misagent_error_code(&e)
                ))
            })
    })
}

pub fn remove_profile(udid: &str, profile_id: &str) -> Result<(), MmErrorInfo> {
    with_misagent(udid, |client| {
        client.remove(profile_id).map_err(|e| {
            service_error(format!(
                "misagent_remove failed with code {}",
                misagent_error_code(&e)
            ))
        })
    })
}

/// Swift `dumpProfiles` (limd, lockdown): build the dump
/// directory, then return the copy-all result plist rendered
/// as XML (the facade's `.raw` mode consumes it as-is).
pub fn dump_profiles_raw(udid: &str, docs_path: &str) -> Result<String, MmErrorInfo> {
    let docs = docs_path.strip_prefix("file://").unwrap_or(docs_path);
    let dump_dir = if docs.ends_with("/Profiles") || docs.ends_with("/Profiles/") {
        docs.to_string()
    } else {
        format!("{docs}/Profiles")
    };
    let _ = std::fs::create_dir_all(&dump_dir);
    with_misagent(udid, |client| {
        let result = client.copy(false).map_err(|e| {
            service_error(format!(
                "misagent_copy_all failed with code {}",
                misagent_error_code(&e)
            ))
        })?;
        Ok(result.to_string())
    })
}

// ---------------------------------------------------------------------------
// DDI mount
// ---------------------------------------------------------------------------

pub fn is_ddi_mounted(udid: &str) -> bool {
    with_mounter(udid, |mounter| {
        match mounter.lookup_image("Developer") {
            Ok(result) => Ok(result.dict_get_item("ImageSignature").is_ok()),
            Err(_) => Ok(false),
        }
    })
    .unwrap_or(false)
}

/// Swift limd mounts without uploading (the image is expected
/// at the device path already); the wrapper's `mount_image`
/// only accepts local files, so the bytes are staged to a
/// local temp file and the wrapper's own upload+mount pair
/// runs — the legacy crate's proven shape (diff list D-M5-6).
pub fn mount_developer_image(
    udid: &str,
    image: &[u8],
    signature: &[u8],
) -> Result<(), MmErrorInfo> {
    let dir = staging_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| service_error(format!("Failed to stage DDI files: {e}")))?;
    let dmg = dir.join("DeveloperDiskImage.dmg");
    let sig = dir.join("DeveloperDiskImage.dmg.signature");
    std::fs::write(&dmg, image)
        .and_then(|_| std::fs::write(&sig, signature))
        .map_err(|e| service_error(format!("Failed to stage DDI files: {e}")))?;
    let dmg_s = dmg.to_string_lossy().into_owned();
    let sig_s = sig.to_string_lossy().into_owned();
    let result = with_mounter(udid, |mounter| {
        mounter
            .upload_image(dmg_s.clone(), "Developer", sig_s.clone())
            .map_err(|e| {
                service_error(format!(
                    "mobile_image_mounter_upload_image failed with code {}",
                    mounter_error_code(&e)
                ))
            })?;
        mounter
            .mount_image(dmg_s.clone(), "Developer", sig_s.clone())
            .map_err(|e| {
                service_error(format!(
                    "mobile_image_mounter_mount_image failed with code {}",
                    mounter_error_code(&e)
                ))
            })?;
        Ok(())
    });
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn staging_dir() -> PathBuf {
    std::env::temp_dir().join(format!("mm-limd-ddi-{}", std::process::id()))
}

// ---------------------------------------------------------------------------
// Install pipeline (instproxy + plain AFC)
// ---------------------------------------------------------------------------

pub fn remove_app(udid: &str, bundle_id: &str) -> Result<(), MmErrorInfo> {
    with_instproxy(udid, |client| {
        client.uninstall(bundle_id, None).map_err(|e| {
            service_error(format!(
                "instproxy_uninstall failed with code {}",
                instproxy_error_code(&e)
            ))
        })
    })
}

pub fn install_ipa(udid: &str, bundle_id: &str) -> Result<(), MmErrorInfo> {
    let path = format!("PublicStaging/{bundle_id}.ipa");
    with_instproxy(udid, |client| {
        client.install(path, None).map_err(|(e, _desc)| {
            service_error(format!(
                "instproxy_install failed with code {}",
                instproxy_error_code(&e)
            ))
        })
    })
}

pub fn install_app_bundle(
    udid: &str,
    bundle_id: &str,
    app_name: &str,
) -> Result<(), MmErrorInfo> {
    let path = format!("PublicStaging/{bundle_id}/{app_name}");
    let mut options = Plist::new_dict();
    let _ = options.dict_set_item(
        "PackageType",
        Plist::new_string("Developer"),
    );
    with_instproxy(udid, |client| {
        client.install(path, Some(options)).map_err(|(e, _desc)| {
            service_error(format!(
                "instproxy_install failed with code {}",
                instproxy_error_code(&e)
            ))
        })
    })
}

/// Swift `sendIpaAfc` (limd): plain AFC, no mkdir, RW open,
/// one write of the whole buffer.
pub fn send_ipa_afc(udid: &str, bundle_id: &str, ipa: &[u8]) -> Result<(), MmErrorInfo> {
    if ipa.is_empty() {
        return Err(service_error("Invalid IPA buffer".to_string()));
    }
    let remote_path = format!("PublicStaging/{bundle_id}.ipa");
    with_plain_afc(udid, |afc| {
        let handle = afc
            .file_open(remote_path, AfcFileMode::ReadWrite)
            .map_err(|e| {
                service_error(format!(
                    "afc_file_open failed with code {}",
                    afc_error_code(&e)
                ))
            })?;
        let result = afc.file_write(handle, ipa.to_vec()).map_err(|e| {
            service_error(format!(
                "afc_file_write failed with code {}",
                afc_error_code(&e)
            ))
        });
        let _ = afc.file_close(handle);
        result
    })
}

/// Recursive walk for `send_app_bundle_afc` (limd): mkdirs
/// are best-effort, files are opened RW and written when
/// non-empty.
fn collect_bundle_files(
    dir: &Path,
    out: &mut Vec<(PathBuf, bool)>,
) -> Result<(), MmErrorInfo> {
    let entries = std::fs::read_dir(dir).map_err(|_| {
        service_error(format!("Failed to enumerate {}", dir.display()))
    })?;
    let mut paths: Vec<PathBuf> =
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

pub fn send_app_bundle_afc(
    udid: &str,
    bundle_id: &str,
    app_path: &Path,
) -> Result<(), MmErrorInfo> {
    let app_name = app_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut items = Vec::new();
    collect_bundle_files(app_path, &mut items)?;
    with_plain_afc(udid, |afc| {
        let _ = afc.make_directory("PublicStaging");
        let _ = afc.make_directory(format!("PublicStaging/{bundle_id}"));
        let remote_app = format!("PublicStaging/{bundle_id}/{app_name}");
        let _ = afc.make_directory(remote_app.clone());
        for (local_path, is_dir) in &items {
            let relative = local_path
                .strip_prefix(app_path)
                .unwrap_or(local_path)
                .to_string_lossy()
                .replace('\\', "/");
            let remote_path = format!("{remote_app}/{relative}");
            if *is_dir {
                let _ = afc.make_directory(remote_path);
                continue;
            }
            let data = std::fs::read(local_path).map_err(|_| {
                service_error("Invalid file buffer".to_string())
            })?;
            let handle = afc
                .file_open(remote_path.clone(), AfcFileMode::ReadWrite)
                .map_err(|e| {
                    service_error(format!(
                        "afc_file_open failed for {remote_path} with code {}",
                        afc_error_code(&e)
                    ))
                })?;
            let result = if data.is_empty() {
                Ok(())
            } else {
                afc.file_write(handle, data).map_err(|e| {
                    service_error(format!(
                        "afc_file_write failed for {remote_path} with code {}",
                        afc_error_code(&e)
                    ))
                })
            };
            let _ = afc.file_close(handle);
            result?;
        }
        Ok(())
    })
}

/// Swift `wipeContainer` (limd): vend only — Swift's limd
/// implementation stops after the vend result (D-M5-10).
pub fn wipe_container(udid: &str, bundle_id: &str) -> Result<(), MmErrorInfo> {
    with_descriptor(udid, SVC_HOUSE_ARREST, |device, descriptor| {
        let arrest = HouseArrest::new(device, &descriptor).map_err(|e| {
            service_error(format!(
                "house_arrest_client_new failed with code {}",
                house_arrest_error_code(&e)
            ))
        })?;
        arrest.send_command("VendContainer", bundle_id).map_err(|e| {
            service_error(format!(
                "house_arrest_send_command failed with code {}",
                house_arrest_error_code(&e)
            ))
        })?;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// AFC queries (limd: plain AFC, bundle id ignored — Swift
// parity, D-M5-11)
// ---------------------------------------------------------------------------

pub fn afc_list_directory(
    udid: &str,
    path: &str,
) -> Result<Vec<String>, MmErrorInfo> {
    with_plain_afc(udid, |afc| {
        afc.read_directory(path).map_err(|e| {
            service_error(format!(
                "afc_read_directory failed with code {}",
                afc_error_code(&e)
            ))
        })
    })
}

pub fn afc_read_file(udid: &str, path: &str) -> Result<Vec<u8>, MmErrorInfo> {
    with_plain_afc(udid, |afc| {
        let handle = afc.file_open(path, AfcFileMode::ReadOnly).map_err(|e| {
            service_error(format!(
                "afc_file_open failed with code {}",
                afc_error_code(&e)
            ))
        })?;
        let mut out: Vec<u8> = Vec::new();
        let result = loop {
            match afc.file_read(handle, 65536) {
                Ok(chunk) => {
                    if chunk.is_empty() {
                        break Ok(());
                    }
                    out.extend(chunk.iter().map(|b| *b as u8));
                }
                Err(e) => {
                    break Err(service_error(format!(
                        "afc_file_read failed with code {}",
                        afc_error_code(&e)
                    )));
                }
            }
        };
        let _ = afc.file_close(handle);
        result.map(|_| out)
    })
}

pub fn afc_get_file_info(
    udid: &str,
    path: &str,
) -> Result<crate::gateway::AfcFileInfo, MmErrorInfo> {
    with_plain_afc(udid, |afc| {
        let info: HashMap<String, String> =
            afc.get_file_info(path).map_err(|e| {
                service_error(format!(
                    "afc_get_file_info failed with code {}",
                    afc_error_code(&e)
                ))
            })?;
        Ok(crate::gateway::AfcFileInfo {
            // Swift compares the FFI string for equality here.
            is_directory: info.get("st_ifmt").map(|v| v == "S_IFDIR").unwrap_or(false),
            file_size: info
                .get("st_size")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
        })
    })
}

// ---------------------------------------------------------------------------
// Debug (limd lockdown path)
// ---------------------------------------------------------------------------

fn with_debug_server<T>(
    udid: &str,
    body: impl FnOnce(&DebugServer) -> Result<T, MmErrorInfo>,
) -> Result<T, MmErrorInfo> {
    with_device(udid, |device| {
        let client = device.new_debug_server(APP_NAME).map_err(|e| {
            service_error(format!(
                "debugserver_client_new failed with code {}",
                debugserver_error_code(&e)
            ))
        })?;
        body(&client)
    })
}

/// One debugserver command with Swift's error reasons; an
/// empty wrapper response is Swift's nil.
fn send_debug_command(
    client: &DebugServer,
    name: &str,
) -> Result<Option<String>, MmErrorInfo> {
    let command = DebugServerCommand::new(name, Vec::new()).map_err(|_| {
        service_error(format!(
            "debugserver_command_new failed with code {}",
            debugserver_error_code(&DebugServerError::InvalidArg)
        ))
    })?;
    match client.send_command(command) {
        Ok(response) => Ok(if response.is_empty() {
            None
        } else {
            Some(response)
        }),
        Err(e) => Err(service_error(format!(
            "debugserver_client_send_command failed with code {}",
            debugserver_error_code(&e)
        ))),
    }
}

/// Swift `findProcessPID` (sync form): the term order and
/// matching rules are the shared `crate::rsp` ones; a
/// successful name attach answers 0 (Swift attaches inside
/// the search and reports "already attached" as pid 0).
fn find_pid_sync(
    client: &DebugServer,
    app_id: &str,
) -> Result<Option<u32>, MmErrorInfo> {
    // Swift resolves the app's paths first for the term list;
    // the limd debug path passes only the app id (its
    // getAppPaths equivalent feeds nothing else here).
    let terms = process_search_terms(app_id, None, None);
    for term in &terms {
        let name = format!("vAttachName;{}", string_to_hex(term));
        let response = match send_debug_command(client, &name) {
            Ok(r) => r,
            Err(_) => None,
        };
        if let Some(response) = response {
            if !response.is_empty() && !response.starts_with('E') {
                let _ = send_debug_command(client, "D");
                return Ok(Some(0));
            }
        }
    }
    // Swift's search is exactly this loop (BaseDeviceGateway
    // `findProcessPID`): no process-list phase.
    Ok(None)
}

/// Swift `debugApp` (limd, lockdown).
pub fn debug_app(udid: &str, app_id: &str) -> Result<(), MmErrorInfo> {
    with_debug_server(udid, |client| {
        let pid = find_pid_sync(client, app_id)?;
        match pid {
            None => Err(service_error(format!(
                "App '{app_id}' is not running. Please open the app and keep it in the background, then enable JIT."
            ))),
            Some(0) => Ok(()),
            Some(pid) => {
                send_debug_command(client, &format!("vAttach;{pid:x}"))?;
                send_debug_command(client, "D")?;
                Ok(())
            }
        }
    })
}

/// Swift `debugProcess` (limd, lockdown).
pub fn debug_process(udid: &str, pid: u32) -> Result<(), MmErrorInfo> {
    with_debug_server(udid, |client| {
        send_debug_command(client, &format!("vAttach;{pid:x}"))?;
        send_debug_command(client, "D")?;
        Ok(())
    })
}
