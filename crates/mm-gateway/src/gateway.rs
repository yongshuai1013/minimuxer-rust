//! The `DeviceGateway` trait — Rust shape of Swift
//! `DeviceGatewayAPI` (inventory subsystem 11), M3 lockdown subset,
//! plus the backend-selection surface (`GatewayBackend`, Swift
//! `MinimuxerApi.swift`) and the `GatewayProvider` indirection
//! (Swift `DeviceProvider`) that lets mm-core's usbmuxd proxy always
//! read the *current* gateway.
//!
//! All operations report errors as `mm_common::MmErrorInfo` — the
//! single error shape of the port's error bus. Pairing-file parse
//! failures keep the `PairingError` domain (Swift `start` rethrows
//! the parser's error unwrapped); operational failures use the
//! `DeviceGatewayError` domain with the Swift reason formats
//! catalogued in `docs/m3-gateway-diff.md`.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use mm_common::{
    DeviceGatewayCode, DeviceGatewayError, MmErrorInfo, PairingFile, PairingProtocol,
};
use plist::Dictionary;

use crate::base::GatewayCore;

/// Swift `GatewayBackend: String` (`libimobiledevice` / `idevice`).
/// The App-visible default is `idevice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GatewayBackend {
    Idevice,
    Libimobiledevice,
}

impl GatewayBackend {
    pub fn raw_value(&self) -> &'static str {
        match self {
            GatewayBackend::Idevice => "idevice",
            GatewayBackend::Libimobiledevice => "libimobiledevice",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        match raw {
            "idevice" => Some(GatewayBackend::Idevice),
            "libimobiledevice" => Some(GatewayBackend::Libimobiledevice),
            _ => None,
        }
    }

    pub fn requires_usbmuxd(&self) -> bool {
        matches!(self, GatewayBackend::Libimobiledevice)
    }
}

impl Default for GatewayBackend {
    fn default() -> Self {
        GatewayBackend::Idevice
    }
}

impl std::fmt::Display for GatewayBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.raw_value())
    }
}

/// Result of opening a service channel on the device (Swift: the
/// `lockdownd_start_service` descriptor step inside every
/// `withService`/`performWith*Service` call). The live connection is
/// held by the gateway until invalidation or stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedService {
    pub service_name: String,
    pub port: u16,
    pub enable_ssl: bool,
}

/// Boxed async result used by the gateway trait (the idevice crate's
/// own provider trait uses the same shape).
pub type GatewayFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, MmErrorInfo>> + Send + 'a>>;

/// Builds a `DeviceGatewayError`-domain error info.
pub fn gw_error(code: DeviceGatewayCode, reason: impl Into<String>) -> MmErrorInfo {
    DeviceGatewayError::new(code, reason).to_info()
}

/// Swift's `unsupportedOperation` for surface that a later milestone
/// owns (the reason is the operation name, per the Swift
/// description template "Operation '<reason>' is not supported.").
pub fn unsupported(operation: &str) -> MmErrorInfo {
    gw_error(DeviceGatewayCode::UnsupportedOperation, operation)
}

/// Swift's `POSIXError` as it crosses the port's error bus
/// (diff list D-M8-5): the domain is NSPOSIXErrorDomain,
/// the code the errno number, the reason the io error's
/// message — the shell rebuilds a POSIXError from the
/// triple. Used by the wireless pair acceptor paths
/// (Swift `TCPAcceptor` throws POSIXError verbatim).
pub fn posix_error(err: &std::io::Error) -> MmErrorInfo {
    MmErrorInfo::new(
        "NSPOSIXErrorDomain",
        &err.raw_os_error().unwrap_or(0).to_string(),
        &err.to_string(),
    )
}

/// The device gateway. Implementations: [`crate::IdeviceGateway`]
/// and (feature `limd`) `LibimobiledeviceGateway`.
///
/// State accessors are synchronous and delegate to the shared
/// [`GatewayCore`], exactly like Swift's `BaseDeviceGateway`
/// properties; only device-touching operations are async.
pub trait DeviceGateway: Send + Sync {
    fn backend(&self) -> GatewayBackend;

    /// Swift `requiresUsbmuxd`.
    fn requires_usbmuxd(&self) -> bool {
        self.backend().requires_usbmuxd()
    }

    fn core(&self) -> &GatewayCore;

    fn pairing_file_type(&self) -> PairingProtocol {
        self.core().pairing_file_type()
    }

    fn pairing_file_data(&self) -> Option<Vec<u8>> {
        self.core().pairing_file_data()
    }

    fn pairing_data_dict(&self) -> Option<Dictionary> {
        self.core().pairing_data_dict()
    }

    fn get_port(&self, protocol: PairingProtocol) -> u16 {
        self.core().get_port(protocol)
    }

    fn set_port(&self, port: u16, protocol: PairingProtocol) {
        self.core().set_port(port, protocol);
    }

    /// Swift `servicePort` (extension): the port for the currently
    /// loaded pairing file's protocol.
    fn service_port(&self) -> u16 {
        self.get_port(self.pairing_file_type())
    }

    fn set_device_endpoint_ip(&self, ip: Option<&str>) {
        self.core().set_device_endpoint_ip(ip);
    }

    fn set_logging(&self, enabled: bool);

    /// Swift `start(pairingFileContent:preferred:)`.
    fn start<'a>(
        &'a self,
        pairing_file_content: &'a str,
        preferred: Option<PairingProtocol>,
    ) -> GatewayFuture<'a, ()>;

    /// Swift `stop()`.
    fn stop<'a>(&'a self) -> GatewayFuture<'a, ()>;

    /// Swift `fetchUDID()`.
    fn fetch_udid<'a>(&'a self) -> GatewayFuture<'a, String>;

    /// Swift `getLockdownValue(key:)`.
    fn get_lockdown_value<'a>(&'a self, key: &'a str) -> GatewayFuture<'a, String>;

    /// Establishes (and caches) an authenticated lockdownd session,
    /// verifying the loaded pairing record against the device.
    /// Swift performs this step inside `getLockdownValue` on every
    /// call (lockdown mode); it is surfaced separately here so the
    /// FFI can drive pairing verification/session lifetime
    /// explicitly. `get_lockdown_value` keeps the Swift per-call
    /// behavior and does not reuse this cached session.
    fn start_session<'a>(&'a self) -> GatewayFuture<'a, ()>;

    /// Tears down the session cached by [`Self::start_session`].
    /// Idempotent.
    fn stop_session<'a>(&'a self) -> GatewayFuture<'a, ()>;

    /// Opens a service channel: asks lockdownd to start
    /// `service_name` (a lockdownd service identifier, e.g.
    /// `com.apple.mobile.heartbeat`) and connects to it. The
    /// connection is held by the gateway until invalidation/stop.
    fn open_service<'a>(&'a self, service_name: &'a str) -> GatewayFuture<'a, OpenedService>;

    /// Swift `performHeartbeat(interval:)`: one heartbeat round
    /// trip on a fresh connection (idevice backend: Marco/Polo;
    /// libimobiledevice backend: Pico plist + `Interval` reply),
    /// returning the device-reported interval — or the input
    /// interval when the device reports none (both Swift backends
    /// end with `newInterval > 0 ? newInterval : interval`).
    /// Both backends override this for the lockdown (M4) and RP
    /// (M7) paths. The trait default only serves test doubles and
    /// reports unsupported (M9 audit appendix A, class b-cleaned).
    fn perform_heartbeat<'a>(&'a self, _interval: u64) -> GatewayFuture<'a, u64> {
        Box::pin(async move { Err(unsupported("performHeartbeat")) })
    }

    /// Swift `isDDIMounted()`. Both backends override this
    /// (M5); the default only serves test doubles.
    fn is_ddi_mounted<'a>(&'a self) -> GatewayFuture<'a, bool> {
        Box::pin(async move { Err(unsupported("isDDIMounted")) })
    }

    // -- Operation surface (diff lists docs/m5-install-ddi-debug-diff.md,
    // docs/m7-rp-diff.md). Defaults report `unsupportedOperation`
    // with the Swift operation name and only serve test doubles;
    // both backends override every operation below for the
    // lockdown and RP paths (M5/M7). The remaining same-shape
    // unsupported returns live inside the backends themselves
    // (M9 audit appendix A, class a).

    /// Swift `mountDeveloperImage(image:signature:)` (pre-17 DDI).
    fn mount_developer_image<'a>(
        &'a self,
        _image: &'a [u8],
        _signature: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Err(unsupported("mountDeveloperImage")) })
    }

    /// Swift `mountPersonalizedDdi(image:trustcache:manifest:)`.
    fn mount_personalized_ddi<'a>(
        &'a self,
        _image: &'a [u8],
        _trustcache: &'a [u8],
        _manifest: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Err(unsupported("mountPersonalizedDdi")) })
    }

    /// Swift `installProvisioningProfile(profile:)`.
    fn install_provisioning_profile<'a>(
        &'a self,
        _profile: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Err(unsupported("installProvisioningProfile")) })
    }

    /// Swift `removeProvisioningProfile(id:)`.
    fn remove_provisioning_profile<'a>(
        &'a self,
        _id: &'a str,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Err(unsupported("removeProvisioningProfile")) })
    }

    /// Swift `dumpProfiles(docsPath:)` — the return value's
    /// meaning is backend-specific in Swift (a directory path
    /// for gw-idevice; an XML string for gw-limd's lockdown
    /// path) and is replicated per backend.
    fn dump_profiles<'a>(&'a self, _docs_path: &'a str) -> GatewayFuture<'a, String> {
        Box::pin(async move { Err(unsupported("dumpProfiles")) })
    }

    /// Swift `removeApp(bundleId:)`.
    fn remove_app<'a>(&'a self, _bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Err(unsupported("removeApp")) })
    }

    /// Swift `sendIpaAfc(bundleId:ipaBytes:)`.
    fn send_ipa_afc<'a>(
        &'a self,
        _bundle_id: &'a str,
        _ipa_bytes: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Err(unsupported("sendIpaAfc")) })
    }

    /// Swift `sendAppBundleAfc(bundleId:appURL:)` — the URL is
    /// carried as a plain filesystem path across the port.
    fn send_app_bundle_afc<'a>(
        &'a self,
        _bundle_id: &'a str,
        _app_path: &'a str,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Err(unsupported("sendAppBundleAfc")) })
    }

    /// Swift `installIpa(bundleId:)`.
    fn install_ipa<'a>(&'a self, _bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Err(unsupported("installIpa")) })
    }

    /// Swift `installAppBundle(bundleId:appName:)`.
    fn install_app_bundle<'a>(
        &'a self,
        _bundle_id: &'a str,
        _app_name: &'a str,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Err(unsupported("installAppBundle")) })
    }

    /// Swift `wipeContainer(identifier:)`.
    fn wipe_container<'a>(&'a self, _identifier: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Err(unsupported("wipeContainer")) })
    }

    /// Swift `debugApp(appId:)`.
    fn debug_app<'a>(&'a self, _app_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Err(unsupported("debugApp")) })
    }

    /// Swift `debugProcess(pid:)`.
    fn debug_process<'a>(&'a self, _pid: u32) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Err(unsupported("debugProcess")) })
    }

    /// Swift `afcListDirectory(bundleId:path:)`.
    fn afc_list_directory<'a>(
        &'a self,
        _bundle_id: &'a str,
        _path: &'a str,
    ) -> GatewayFuture<'a, Vec<String>> {
        Box::pin(async move { Err(unsupported("afcListDirectory")) })
    }

    /// Swift `afcReadFile(bundleId:path:)`.
    fn afc_read_file<'a>(
        &'a self,
        _bundle_id: &'a str,
        _path: &'a str,
    ) -> GatewayFuture<'a, Vec<u8>> {
        Box::pin(async move { Err(unsupported("afcReadFile")) })
    }

    /// Swift `afcGetFileInfo(bundleId:path:)`.
    fn afc_get_file_info<'a>(
        &'a self,
        _bundle_id: &'a str,
        _path: &'a str,
    ) -> GatewayFuture<'a, AfcFileInfo> {
        Box::pin(async move { Err(unsupported("afcGetFileInfo")) })
    }

    // -- M8 wireless pairing (behavior list
    // docs/m8-wireless-behavior.md §B). Defaults report
    // `unsupportedOperation` with the Swift operation
    // name; gw-idevice implements both, gw-limd
    // overrides with Swift's verbatim lockdown reasons
    // (behavior G12).

    /// Swift `startWirelessPair(hostName:hostModel:
    /// outPath:resolveFileName:onReady:onPin:)`.
    fn start_wireless_pair<'a>(
        &'a self,
        _args: WirelessPairStart<'a>,
    ) -> GatewayFuture<'a, PairedDeviceRecord> {
        Box::pin(async move { Err(unsupported("startWirelessPair")) })
    }

    /// Swift `triggerWirelessPair(targetIp:targetPort:
    /// hostName:hostModel:outPath:resolveFileName:
    /// onRequestPin:)`.
    fn trigger_wireless_pair<'a>(
        &'a self,
        _args: WirelessPairTrigger<'a>,
    ) -> GatewayFuture<'a, PairedDeviceRecord> {
        Box::pin(async move { Err(unsupported("triggerWirelessPair")) })
    }
}

/// Swift `afcGetFileInfo`'s return tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfcFileInfo {
    pub is_directory: bool,
    pub file_size: i64,
}

/// Swift `PairedDeviceRecord` (DeviceGatewayAPI): the
/// product of a wireless pairing. `pairing_file` is the
/// parsed pairing file (Swift `any PairingFile`); over
/// the FFI it crosses as its serialized content (diff
/// list D-M8-10).
#[derive(Debug, Clone)]
pub struct PairedDeviceRecord {
    pub name: String,
    pub model: String,
    pub pairing_file_path: String,
    pub pairing_file: PairingFile,
}

/// Cancellation for one wireless pair operation (diff
/// list D-M8-2): the service cancels on stop() and on
/// same-slot/same-key re-entry; the gateway races its
/// blocking waits against the token and fails with the
/// Swift-shaped cancellation error. Swift's own
/// Task.cancel cannot interrupt its blocking FFI calls
/// — this token is the port's real cancellation, an
/// explicit, recorded deviation.
#[derive(Clone, Default)]
pub struct PairCancelToken {
    flag: Arc<AtomicBool>,
    notify: Arc<tokio::sync::Notify>,
}

impl PairCancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Resolves once the token is cancelled.
    pub async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let notified = self.notify.notified();
            tokio::pin!(notified);
            // Register before re-checking the flag, so a
            // cancel racing the check is not missed.
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

/// The event surface a wireless pair operation reports
/// through (Swift's gateway callbacks `onReady` /
/// `onPin` / `onRequestPin` plus the `resolveFileName`
/// closure, gathered into one object the service
/// supplies).
pub trait WirelessPairEvents: Send + Sync {
    /// Swift gateway `onReady`: the pairable host is
    /// prepared and listening; advertise
    /// (service_id, port, txt_records).
    fn on_ready(&self, service_id: &str, port: u16, txt_records: &[(String, String)]);

    /// Swift gateway `onPin`: the host-generated PIN to
    /// show the user (start flow).
    fn on_pin(&self, pin: &str);

    /// Swift gateway `onRequestPin`'s answer path:
    /// resolves with the PIN the user entered for the
    /// device-displayed code (trigger flow). The
    /// service answers "000000" itself when the App has
    /// no PIN handler (behavior W6); the gateway owns
    /// the 60-second wait limit (behavior G7).
    fn request_pin<'a>(&'a self) -> GatewayFuture<'a, String>;

    /// Swift's `resolveFileName(name, model)` closure:
    /// `Some(file_name)` when a resolver is installed,
    /// `None` for the default naming (behavior G11).
    fn resolve_file_name(&self, name: &str, model: &str) -> Option<String>;
}

/// Arguments of [`DeviceGateway::start_wireless_pair`]
/// (Swift `startWirelessPair`'s parameters).
pub struct WirelessPairStart<'a> {
    pub host_name: &'a str,
    pub host_model: &'a str,
    pub out_path: &'a str,
    pub events: Arc<dyn WirelessPairEvents>,
    pub cancel: PairCancelToken,
}

/// Arguments of [`DeviceGateway::trigger_wireless_pair`]
/// (Swift `triggerWirelessPair`'s parameters).
pub struct WirelessPairTrigger<'a> {
    pub target_ip: &'a str,
    pub target_port: u16,
    pub host_name: &'a str,
    pub host_model: &'a str,
    pub out_path: &'a str,
    pub events: Arc<dyn WirelessPairEvents>,
    pub cancel: PairCancelToken,
}

/// Swift `DeviceProvider`: a level of indirection so subsystems
/// (the usbmuxd proxy above all) always see the gateway currently
/// selected, across backend switches.
pub trait GatewayProvider: Send + Sync {
    fn current_gateway(&self) -> Option<Arc<dyn DeviceGateway>>;
}
