//! `IdeviceGateway` — port of Swift `IdeviceGateway`
//! (yongshuai1013/minimuxer @714aafa3) over the `idevice` crate.
//!
//! M3 covers the lockdown basic chain: `start`/`stop`,
//! `fetch_udid`, `get_lockdown_value` (fresh TCP connection +
//! StartSession per call, exactly like Swift's
//! `performWithEitherService` → `performWithTcpService` route), an
//! explicit session slot, and `open_service` (StartService +
//! connect). The RP route of `performWithEitherService` and the
//! usbmuxd route (`performWithUsbmuxdService`, which has no call
//! sites in Swift) are implemented as helpers; RP operations
//! themselves return `unsupportedOperation` until M7 (diff list
//! D-1/D-2).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;

use idevice::lockdown::LockdownClient;
use idevice::pairing_file::PairingFile;
use idevice::provider::{IdeviceProvider, TcpProvider, UsbmuxdProvider};
use idevice::usbmuxd::UsbmuxdAddr;
use idevice::{Idevice, IdeviceError, IdeviceService};
use mm_common::constants::{APP_NAME, USBMUXD_SOCKET};
use mm_common::logging::debug_log;
use mm_common::{DeviceGatewayCode, MmErrorInfo, PairingProtocol};

use crate::base::GatewayCore;
use crate::gateway::{gw_error, unsupported, DeviceGateway, GatewayBackend, GatewayFuture, OpenedService};

/// Swift `IdeviceGateway` (`requiresUsbmuxd = false`).
pub struct IdeviceGateway {
    core: GatewayCore,
    /// The parsed IDevice pairing file (Swift's `pairingFile`
    /// FFI handle), built at `start` for lockdown files.
    pairing_file: Mutex<Option<PairingFile>>,
    /// Explicit session slot (see `start_session`).
    session: Mutex<Option<SessionSlot>>,
    /// Open service connections by service identifier.
    services: Mutex<HashMap<String, ServiceSlot>>,
}

struct SessionSlot {
    #[allow(dead_code)]
    client: LockdownClient,
    generation: u64,
}

struct ServiceSlot {
    #[allow(dead_code)]
    connection: Idevice,
    generation: u64,
}

impl Default for IdeviceGateway {
    fn default() -> Self {
        Self::new()
    }
}

impl IdeviceGateway {
    pub fn new() -> Self {
        Self {
            core: GatewayCore::new(),
            pairing_file: Mutex::new(None),
            session: Mutex::new(None),
            services: Mutex::new(HashMap::new()),
        }
    }

    /// Swift `cleanup()` (subclass half): drop the pairing file
    /// handle and every cached connection, then the base cleanup.
    fn cleanup(&self) {
        debug_log("[IdeviceGateway] cleanup() called");
        *self.pairing_file.lock().unwrap() = None;
        *self.session.lock().unwrap() = None;
        self.services.lock().unwrap().clear();
        self.core.cleanup();
    }

    /// Swift `verifyInitialized()` — flag only (the limd backend is
    /// stricter; see the diff list §3).
    fn verify_initialized(&self) -> Result<(), MmErrorInfo> {
        if self.core.is_initialized() {
            Ok(())
        } else {
            Err(gw_error(DeviceGatewayCode::NotInitialized, ""))
        }
    }

    fn require_endpoint_ip(&self) -> Result<String, MmErrorInfo> {
        self.core
            .device_endpoint_ip()
            .filter(|ip| !ip.is_empty())
            .ok_or_else(|| gw_error(DeviceGatewayCode::DeviceEndpointIpNotAvailable, ""))
    }

    fn require_pairing_file(&self) -> Result<PairingFile, MmErrorInfo> {
        self.pairing_file
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| {
                gw_error(
                    DeviceGatewayCode::ConnectionFailed,
                    "pairingFileData is nil",
                )
            })
    }

    /// Swift `performWithTcpService` provider construction: a
    /// `TcpProvider` to the endpoint IP at the *constant* lockdownd
    /// port (the provider connects to 62078 itself; the port table
    /// is not consulted on this path — diff list D-4).
    fn tcp_provider(&self) -> Result<TcpProvider, MmErrorInfo> {
        let ip_string = self.require_endpoint_ip()?;
        let pairing_file = self.require_pairing_file()?;
        let addr = parse_endpoint_ip(&ip_string)?;
        Ok(TcpProvider {
            addr,
            pairing_file,
            label: APP_NAME.to_string(),
        })
    }

    /// The M5 operations context (endpoint + pairing file).
    fn m5_ctx(&self) -> Result<crate::gw_idevice_ops::IdeviceCtx, MmErrorInfo> {
        let ip_string = self.require_endpoint_ip()?;
        let addr = parse_endpoint_ip(&ip_string)?;
        let pairing_file = self.require_pairing_file()?;
        Ok(crate::gw_idevice_ops::IdeviceCtx {
            endpoint_ip: addr,
            pairing_file,
        })
    }

    /// Connects a fresh lockdownd client over TCP (the lockdown
    /// half of Swift `performWithEitherService`).
    async fn connect_lockdown(&self) -> Result<LockdownClient, MmErrorInfo> {
        let provider = self.tcp_provider()?;
        LockdownClient::connect(&provider).await.map_err(|e| {
            gw_error(
                DeviceGatewayCode::ServiceError,
                format!("Failed to connect to lockdownd, error: ({e})"),
            )
        })
    }

    /// Starts a session on `client` with the loaded pairing file,
    /// mapping failures with Swift's `syncGetLockdownValue`
    /// classification (pairing-class → invalidPairingFile, else
    /// connectionFailed).
    async fn start_session_on(
        &self,
        client: &mut LockdownClient,
    ) -> Result<(), MmErrorInfo> {
        let pairing_file = self.require_pairing_file()?;
        client.start_session(&pairing_file).await.map_err(|e| {
            if is_pairing_error(&e) {
                gw_error(
                    DeviceGatewayCode::InvalidPairingFile,
                    format!("Lockdown session failed (pairing invalid): {e}"),
                )
            } else {
                gw_error(
                    DeviceGatewayCode::ConnectionFailed,
                    format!("Lockdown session failed: {e}"),
                )
            }
        })
    }

    fn drop_stale_services(&self) {
        let generation = self.core.generation();
        self.services
            .lock()
            .unwrap()
            .retain(|_, slot| slot.generation == generation);
    }
}

impl DeviceGateway for IdeviceGateway {
    fn backend(&self) -> GatewayBackend {
        GatewayBackend::Idevice
    }

    fn core(&self) -> &GatewayCore {
        &self.core
    }

    fn set_logging(&self, enabled: bool) {
        // Swift sets the IDevice FFI logger level; the Rust crate
        // logs through the `log` facade, whose level is a host
        // concern (diff list D-9). Only the core flag is kept here.
        self.core.set_logging(enabled);
    }

    fn start<'a>(
        &'a self,
        pairing_file_content: &'a str,
        preferred: Option<PairingProtocol>,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            debug_log(&format!(
                "[IdeviceGateway] start() called, pairingFileContent length: {}",
                pairing_file_content.len()
            ));
            self.cleanup();
            // Swift rethrows the parser's PairingError unwrapped.
            let parsed = mm_common::parse_pairing_content(pairing_file_content, preferred)
                .map_err(|e| e.to_info())?;
            self.core
                .set_pairing_file_data(Some(parsed.raw_data().to_vec()));
            self.core.set_pairing_file_type(parsed.mode());
            match parsed.mode() {
                PairingProtocol::Lockdown => {
                    let pf = PairingFile::from_bytes(parsed.raw_data()).map_err(|e| {
                        debug_log(&format!(
                            "[IdeviceGateway] start() idevice_pairing_file_from_bytes failed: {e}"
                        ));
                        gw_error(
                            DeviceGatewayCode::InvalidPairingFile,
                            "idevice_pairing_file_from_bytes failed",
                        )
                    })?;
                    *self.pairing_file.lock().unwrap() = Some(pf);
                }
                PairingProtocol::RpPairing => {
                    // Diff list D-2: the RP FFI pairing object is an
                    // M7 (mm-rp) concern; M3 stores the parsed state
                    // and marks initialized, RP operations report
                    // unsupportedOperation.
                }
                PairingProtocol::Unknown => {
                    // The parser never yields Unknown on success.
                    return Err(gw_error(
                        DeviceGatewayCode::InvalidPairingFile,
                        "Unknown pairing file format",
                    ));
                }
            }
            self.core.set_initialized(true);
            Ok(())
        })
    }

    fn stop<'a>(&'a self) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.cleanup();
            Ok(())
        })
    }

    fn fetch_udid<'a>(&'a self) -> GatewayFuture<'a, String> {
        Box::pin(async move {
            let udid = self.get_lockdown_value("UniqueDeviceID").await?;
            if udid.is_empty() {
                return Err(gw_error(
                    DeviceGatewayCode::ServiceError,
                    "UniqueDeviceID not found on device",
                ));
            }
            Ok(udid)
        })
    }

    fn get_lockdown_value<'a>(&'a self, key: &'a str) -> GatewayFuture<'a, String> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("getLockdownValue"));
            }
            let mut client = self.connect_lockdown().await?;
            self.start_session_on(&mut client).await?;
            let value = client.get_value(key).await.map_err(|e| {
                if is_pairing_error(&e) {
                    gw_error(
                        DeviceGatewayCode::InvalidPairingFile,
                        format!("Failed to get lockdown value for key {key}, error: ({e})"),
                    )
                } else {
                    gw_error(
                        DeviceGatewayCode::ServiceError,
                        format!("Failed to get lockdown value for key {key}, error: ({e})"),
                    )
                }
            })?;
            match value {
                plist::Value::String(s) => Ok(s),
                _ => Err(gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("Lockdown value for key '{key}' could not be decoded as string"),
                )),
            }
        })
    }

    fn start_session<'a>(&'a self) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("startSession"));
            }
            let generation = self.core.generation();
            {
                let slot = self.session.lock().unwrap();
                if let Some(slot) = slot.as_ref() {
                    if slot.generation == generation {
                        return Ok(());
                    }
                }
            }
            let mut client = self.connect_lockdown().await?;
            self.start_session_on(&mut client).await?;
            *self.session.lock().unwrap() = Some(SessionSlot { client, generation });
            Ok(())
        })
    }

    fn stop_session<'a>(&'a self) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            *self.session.lock().unwrap() = None;
            Ok(())
        })
    }

    fn perform_heartbeat<'a>(&'a self, interval: u64) -> GatewayFuture<'a, u64> {
        Box::pin(async move {
            debug_log(&format!(
                "[IdeviceGateway] performHeartbeat() called, interval: {interval}"
            ));
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                // The RSD heartbeat path is M7 (diff list D-2).
                return Err(unsupported("performHeartbeat"));
            }
            // Swift `performWithEitherService` (lockdown route):
            // a fresh heartbeat connection per beat.
            let provider = self.tcp_provider()?;
            let mut client =
                idevice::heartbeat::HeartbeatClient::connect(&provider)
                    .await
                    .map_err(|e| {
                        gw_error(
                            DeviceGatewayCode::ServiceError,
                            format!("Failed to connect to heartbeat, error: ({e})"),
                        )
                    })?;
            let new_interval = client.get_marco(interval).await.map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("Heartbeat receive failed, error: ({e})"),
                )
            })?;
            client.send_polo().await.map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("Heartbeat send failed, error: ({e})"),
                )
            })?;
            debug_log(&format!(
                "[IdeviceGateway] performHeartbeat() succeeded, newInterval: {new_interval}"
            ));
            Ok(if new_interval > 0 {
                new_interval
            } else {
                interval
            })
        })
    }

    fn open_service<'a>(&'a self, service_name: &'a str) -> GatewayFuture<'a, OpenedService> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("openService"));
            }
            self.drop_stale_services();
            let provider = self.tcp_provider()?;
            let mut client = LockdownClient::connect(&provider).await.map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("Failed to connect to lockdownd, error: ({e})"),
                )
            })?;
            let (port, enable_ssl) =
                client.start_service(service_name).await.map_err(|e| {
                    gw_error(
                        DeviceGatewayCode::ServiceError,
                        format!("Failed to connect to {service_name}, error: ({e})"),
                    )
                })?;
            let connection = provider.connect(port).await.map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("Failed to connect to {service_name}, error: ({e})"),
                )
            })?;
            let generation = self.core.generation();
            self.services.lock().unwrap().insert(
                service_name.to_string(),
                ServiceSlot {
                    connection,
                    generation,
                },
            );
            Ok(OpenedService {
                service_name: service_name.to_string(),
                port,
                enable_ssl,
            })
        })
    }
    // ------------------------------------------------------------------
    // M5: DDI mount, install pipeline, profiles, debug
    // (docs/m5-install-ddi-debug-diff.md). RP routes return
    // unsupported until M7, per the established convention.
    // ------------------------------------------------------------------

    fn is_ddi_mounted<'a>(&'a self) -> GatewayFuture<'a, bool> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("isDDIMounted"));
            }
            let ctx = self.m5_ctx()?;
            Ok(crate::gw_idevice_ops::is_ddi_mounted(&ctx).await)
        })
    }

    fn mount_developer_image<'a>(
        &'a self,
        image: &'a [u8],
        signature: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("mountDeveloperImage"));
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::mount_developer_image(&ctx, image, signature).await
        })
    }

    fn mount_personalized_ddi<'a>(
        &'a self,
        image: &'a [u8],
        trustcache: &'a [u8],
        build_manifest: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("mountPersonalizedDdi"));
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::mount_personalized_ddi(
                &ctx,
                image,
                trustcache,
                build_manifest,
            )
            .await
        })
    }

    fn install_provisioning_profile<'a>(
        &'a self,
        profile: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("installProvisioningProfile"));
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::misagent_install(&ctx, profile).await
        })
    }

    fn remove_provisioning_profile<'a>(
        &'a self,
        profile_id: &'a str,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("removeProvisioningProfile"));
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::misagent_remove(&ctx, profile_id).await
        })
    }

    fn dump_profiles<'a>(&'a self, docs_path: &'a str) -> GatewayFuture<'a, String> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("dumpProfiles"));
            }
            let ctx = self.m5_ctx()?;
            let payloads = crate::gw_idevice_ops::misagent_copy_all(&ctx).await?;
            Ok(dump_profiles_to_disk(docs_path, &payloads))
        })
    }

    fn remove_app<'a>(&'a self, bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("removeApp"));
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::instproxy_uninstall(&ctx, bundle_id).await
        })
    }

    fn send_ipa_afc<'a>(
        &'a self,
        bundle_id: &'a str,
        ipa_bytes: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("sendIpaAfc"));
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::send_ipa_afc(&ctx, bundle_id, ipa_bytes).await
        })
    }

    fn send_app_bundle_afc<'a>(
        &'a self,
        bundle_id: &'a str,
        app_path: &'a str,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("sendAppBundleAfc"));
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::send_app_bundle_afc(
                &ctx,
                bundle_id,
                std::path::Path::new(app_path),
            )
            .await
        })
    }

    fn install_ipa<'a>(&'a self, bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("installIpa"));
            }
            let ctx = self.m5_ctx()?;
            let package_path = format!("PublicStaging/{bundle_id}/app.ipa");
            crate::gw_idevice_ops::instproxy_install(
                &ctx,
                &package_path,
                None,
                "Failed to install IPA",
            )
            .await
        })
    }

    fn install_app_bundle<'a>(
        &'a self,
        bundle_id: &'a str,
        app_name: &'a str,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("installAppBundle"));
            }
            let ctx = self.m5_ctx()?;
            let package_path = format!("PublicStaging/{bundle_id}/{app_name}");
            let mut options = plist::Dictionary::new();
            options.insert(
                "PackageType".into(),
                plist::Value::String("Developer".into()),
            );
            crate::gw_idevice_ops::instproxy_install(
                &ctx,
                &package_path,
                Some(options),
                "Failed to install App directory",
            )
            .await
        })
    }

    fn wipe_container<'a>(&'a self, bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("wipeContainer"));
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::wipe_container(&ctx, bundle_id).await
        })
    }

    fn debug_app<'a>(&'a self, bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("debugApp"));
            }
            let ctx = self.m5_ctx()?;
            let mux_addr = usbmuxd_addr_from_env(
                std::env::var(mm_common::constants::USBMUXD_ENV_KEY)
                    .ok()
                    .as_deref(),
            );
            crate::gw_idevice_ops::debug_app(&ctx, bundle_id, &mux_addr).await
        })
    }

    fn debug_process<'a>(&'a self, _pid: u32) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            // Swift parity (D-M5-1): the idevice backend's
            // lockdown debugProcess throws unsupportedOperation
            // with this reason, verbatim.
            Err(gw_error(
                DeviceGatewayCode::UnsupportedOperation,
                "Attaching to process by PID is only supported in.rppairing mode",
            ))
        })
    }

    fn afc_list_directory<'a>(
        &'a self,
        bundle_id: &'a str,
        path: &'a str,
    ) -> GatewayFuture<'a, Vec<String>> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("afcListDirectory"));
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::afc_list_directory(&ctx, bundle_id, path).await
        })
    }

    fn afc_read_file<'a>(
        &'a self,
        bundle_id: &'a str,
        path: &'a str,
    ) -> GatewayFuture<'a, Vec<u8>> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("afcReadFile"));
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::afc_read_file(&ctx, bundle_id, path).await
        })
    }

    fn afc_get_file_info<'a>(
        &'a self,
        bundle_id: &'a str,
        path: &'a str,
    ) -> GatewayFuture<'a, crate::gateway::AfcFileInfo> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return Err(unsupported("afcGetFileInfo"));
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::afc_get_file_info(&ctx, bundle_id, path).await
        })
    }
}

/// Swift `withSockaddr` address parsing: `%scope` suffixes are
/// resolved through `if_nametoindex` for the sockaddr; the
/// `TcpProvider` carries only the address, so the scope is stripped
/// here (the tunnel interfaces this port targets are point-to-point
/// links where the route is unambiguous). Invalid input maps to
/// Swift's `invalidTargetEndpoint` reasons, verbatim.
fn parse_endpoint_ip(ip: &str) -> Result<IpAddr, MmErrorInfo> {
    let bare = ip.split('%').next().unwrap_or(ip);
    bare.parse::<IpAddr>().map_err(|_| {
        if ip.contains(':') {
            gw_error(
                DeviceGatewayCode::InvalidTargetEndpoint,
                format!("Invalid IPv6 address: {ip}"),
            )
        } else {
            gw_error(
                DeviceGatewayCode::InvalidTargetEndpoint,
                format!("Invalid IPv4 address: {ip}"),
            )
        }
    })
}

/// Swift `isPairingError`: FFI codes 18 (InvalidHostID) and 103
/// (RemotePairing), or a message containing (case-insensitively)
/// `invalidconf`, `pairing`, `handshake`, `bad_certificate` or
/// `certificate_unknown`. Applied to the idevice crate's error
/// surface: `InvalidHostID` hits directly, the rest go through the
/// message rule on the error's Display text.
pub fn is_pairing_error(err: &IdeviceError) -> bool {
    if matches!(err, IdeviceError::InvalidHostID) {
        return true;
    }
    let msg = err.to_string().to_lowercase();
    [
        "invalidconf",
        "pairing",
        "handshake",
        "bad_certificate",
        "certificate_unknown",
    ]
    .iter()
    .any(|needle| msg.contains(needle))
}

/// Swift `performWithUsbmuxdService`'s address resolution, kept as
/// a pure function: the `USBMUXD_SOCKET_ADDRESS` env value when set
/// (host:port → TCP, otherwise a unix socket path), else the
/// default — which for this port is the mm-core proxy address
/// (`idevice_usbmuxd_default_addr_new`'s equivalent in-App is the
/// retargeted env value; the constant default mirrors it).
pub fn usbmuxd_addr_from_env(value: Option<&str>) -> UsbmuxdAddr {
    match value {
        Some(v) if v.contains(':') => {
            if let Ok(addr) = v.parse() {
                UsbmuxdAddr::TcpSocket(addr)
            } else {
                UsbmuxdAddr::TcpSocket(USBMUXD_SOCKET.parse().unwrap())
            }
        }
        #[cfg(unix)]
        Some(v) => UsbmuxdAddr::UnixSocket(v.to_string()),
        #[cfg(not(unix))]
        Some(_) => UsbmuxdAddr::TcpSocket(USBMUXD_SOCKET.parse().unwrap()),
        None => UsbmuxdAddr::TcpSocket(USBMUXD_SOCKET.parse().unwrap()),
    }
}

/// Builds the `UsbmuxdProvider` half of Swift
/// `performWithUsbmuxdService` (diff list D-1: defined in Swift but
/// uncalled there; provided here for parity and used by tests
/// against a full usbmuxd). Lists devices on the mux and takes the
/// first, exactly like the Swift helper.
pub async fn usbmuxd_provider(
    addr: UsbmuxdAddr,
    label: &str,
) -> Result<UsbmuxdProvider, MmErrorInfo> {
    let socket = addr.to_socket().await.map_err(|e| {
        gw_error(
            DeviceGatewayCode::ConnectionFailed,
            format!("Failed to connect to usbmuxd: {e}"),
        )
    })?;
    let mut conn = idevice::usbmuxd::UsbmuxdConnection::new(socket, 0);
    let devices = conn.get_devices().await.map_err(|e| {
        gw_error(
            DeviceGatewayCode::ConnectionFailed,
            format!("Failed to list usbmuxd devices: {e}"),
        )
    })?;
    let device = devices.into_iter().next().ok_or_else(|| {
        gw_error(
            DeviceGatewayCode::ConnectionFailed,
            "No devices found on usbmuxd",
        )
    })?;
    Ok(UsbmuxdProvider {
        addr,
        tag: 0,
        udid: device.udid,
        device_id: device.device_id,
        label: label.to_string(),
    })
}

/// Swift `IdeviceGateway.dumpProfiles`' file half: split each
/// misagent payload at its plist markers and write
/// `<UUID>.mobileprovision` (full payload) + `<UUID>.plist`
/// (plist bytes), or `unknown_<i>.mobileprovision` when the
/// plist carries no UUID; payloads without a plist section
/// are skipped; all write errors are swallowed, as in Swift.
fn dump_profiles_to_disk(docs_path: &str, payloads: &[Vec<u8>]) -> String {
    let docs = docs_path.strip_prefix("file://").unwrap_or(docs_path);
    let dump_dir = if docs.ends_with("/Profiles") || docs.ends_with("/Profiles/") {
        docs.to_string()
    } else {
        format!("{docs}/Profiles")
    };
    let _ = std::fs::create_dir_all(&dump_dir);
    for (i, bytes) in payloads.iter().enumerate() {
        let Some(start) = find_subslice(bytes, b"<?xml version=") else {
            continue;
        };
        let Some(end_rel) = find_subslice(&bytes[start..], b"</plist>") else {
            continue;
        };
        let plist_bytes = &bytes[start..start + end_rel + b"</plist>".len()];
        let uuid = plist::Value::from_reader(std::io::Cursor::new(plist_bytes))
            .ok()
            .and_then(|v| match v {
                plist::Value::Dictionary(d) => match d.get("UUID") {
                    Some(plist::Value::String(u)) => Some(u.clone()),
                    _ => None,
                },
                _ => None,
            });
        match uuid {
            Some(uuid) => {
                let _ = std::fs::write(
                    format!("{dump_dir}/{uuid}.mobileprovision"),
                    bytes,
                );
                let _ = std::fs::write(
                    format!("{dump_dir}/{uuid}.plist"),
                    plist_bytes,
                );
            }
            None => {
                let _ = std::fs::write(
                    format!("{dump_dir}/unknown_{i}.mobileprovision"),
                    bytes,
                );
            }
        }
    }
    dump_dir
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}
