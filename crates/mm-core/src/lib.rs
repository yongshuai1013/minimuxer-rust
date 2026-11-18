//! `mm-core` — core orchestration crate in the minimuxer
//! Rust-rewrite workspace.
//!
//! M3 lands the usbmuxd proxy (Swift `UsbmuxdProxyServer` +
//! `RawPacket`) and the MinimuxerImpl proxy orchestration
//! equivalents; the rest of the core (MinimuxerImpl state
//! machine, target/heartbeat/AFC/install surfaces) lands in
//! M4+.

pub mod usbmuxd_proxy;

/// Crate name, for wiring/smoke checks.
pub const CRATE_NAME: &str = "mm-core";
