//! `mm-gateway` — the Rust `DeviceGateway` (inventory subsystem 11).
//!
//! M3 scope: the trait + `GatewayCore` (Swift `BaseDeviceGateway`
//! equivalent state) and the lockdown basic chain for both backends:
//!
//! - [`gw_idevice::IdeviceGateway`] — the `idevice` crate over a
//!   direct TCP connection to the device endpoint (Swift
//!   `performWithTcpService` route of `performWithEitherService`).
//! - `gw_limd::LibimobiledeviceGateway` (feature `limd`) —
//!   rusty_libimobiledevice, whose usbmuxd lookups are retargeted to
//!   the mm-core usbmuxd proxy (Swift `withDevice`/`withLockdown`).
//!
//! RemotePairing connection paths land in M7 (mm-rp); until then
//! rppairing-mode operations return `unsupportedOperation`, per the
//! design's no-fake-compatibility rule. The line-by-line Swift
//! comparison this implementation follows is
//! `docs/m3-gateway-diff.md` in the repo.
//!
//! M5 adds the operation surface (install pipeline, DDI mount
//! bodies, debugserver basics) for the lockdown paths of both
//! backends, the RSP tools (`rsp`), the RSD AFC codec
//! (`rsd_afc`, wired in M7), and the packet-level `AfcSession`
//! gw-idevice's file operations run on — see
//! `docs/m5-install-ddi-debug-diff.md`.

pub mod afc_session;
pub mod base;
pub mod gateway;
pub mod gw_idevice;
pub mod gw_idevice_ops;
#[cfg(feature = "limd")]
pub mod gw_limd;
pub mod rawconn;
pub mod rsp;
pub mod rsd_afc;

pub use afc_session::AfcSession;
pub use base::GatewayCore;
pub use gateway::{
    unsupported, AfcFileInfo, DeviceGateway, GatewayBackend, GatewayFuture,
    GatewayProvider, OpenedService,
};
pub use rsp::{
    decode_hex_ascii, find_process_pid, process_search_terms, string_to_hex,
    RspClient, RspProcessInfo, RspStopReply,
};
pub use gw_idevice::IdeviceGateway;
#[cfg(feature = "limd")]
pub use gw_limd::LibimobiledeviceGateway;

/// Crate name, for wiring/smoke checks.
pub const CRATE_NAME: &str = "mm-gateway";

/// Builds a fresh gateway instance for a backend (Swift
/// `Minimuxer.makeGateway(for:)` — a new instance per selection, not
/// a shared singleton).
///
/// Without the `limd` feature the libimobiledevice backend is
/// unavailable and this returns the idevice backend regardless;
/// shipped builds always enable `limd` (see D-7 in the diff list).
pub fn make_gateway(backend: GatewayBackend) -> std::sync::Arc<dyn DeviceGateway> {
    match backend {
        GatewayBackend::Idevice => std::sync::Arc::new(IdeviceGateway::new()),
        GatewayBackend::Libimobiledevice => {
            #[cfg(feature = "limd")]
            {
                std::sync::Arc::new(LibimobiledeviceGateway::new())
            }
            #[cfg(not(feature = "limd"))]
            {
                std::sync::Arc::new(IdeviceGateway::new())
            }
        }
    }
}
