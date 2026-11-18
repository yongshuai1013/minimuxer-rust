//! `mm-core` — core orchestration crate in the minimuxer
//! Rust-rewrite workspace.
//!
//! M3 landed the usbmuxd proxy (Swift `UsbmuxdProxyServer` +
//! `RawPacket`). M4 lands the rest of the core composition:
//! `MinimuxerCore` (Swift `MinimuxerImpl`: lifecycle state
//! machine, nine-gate isReady, status bus, `runWithChecks`
//! mapping) and `HeartbeatService` (Swift's, at the 714aafa3
//! keep-alive behaviour). M5 adds the `Mounter` and the
//! install/AFC/profile/debug forwards. See
//! `docs/m4-core-diff.md` and
//! `docs/m5-install-ddi-debug-diff.md`.

pub mod core;
pub mod heartbeat;
pub mod mounter;
pub mod usbmuxd_proxy;
pub mod wireless_pair;

pub use core::{
    map_gateway_error, run_with_checks, CoreGatewayProvider, CoreStatus,
    MinimuxerCore, MinimuxerParams, OsVersionProvider, ProfileDumpMode,
    StatusHandler, SystemOsVersion,
};
pub use mounter::{ImageDownloader, Mounter, ReqwestDownloader};
pub use wireless_pair::{WirelessPairAppEvents, WirelessPairService};
pub use heartbeat::{
    clamp_interval, next_unreachable_backoff, pacing_duration, HeartbeatClock,
    HeartbeatDeps, HeartbeatFuture, HeartbeatService, TokioClock,
    UNREACHABLE_RETRY_INITIAL,
};

/// Crate name, for wiring/smoke checks.
pub const CRATE_NAME: &str = "mm-core";
