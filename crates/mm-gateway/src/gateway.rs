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
use std::sync::Arc;

use mm_common::{DeviceGatewayCode, DeviceGatewayError, MmErrorInfo, PairingProtocol};
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
}

/// Swift `DeviceProvider`: a level of indirection so subsystems
/// (the usbmuxd proxy above all) always see the gateway currently
/// selected, across backend switches.
pub trait GatewayProvider: Send + Sync {
    fn current_gateway(&self) -> Option<Arc<dyn DeviceGateway>>;
}
