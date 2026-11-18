//! `mm-rp` — the RemotePairing session layer (M7).
//!
//! Spec: `docs/m7-rp-inventory.md` (the M0(c) make-up survey of
//! RemotePairingKit and both Swift gateways' RP sections) and
//! `docs/m7-rp-diff.md`. The survey's verdict was (a) *wire up
//! the idevice crate's own RP stack*: idevice 0.1.68 — the same
//! code line the Swift idevice backend binds as IDevice
//! v0.1.68-ss — implements pair-verify, the TLS-PSK CDTunnel,
//! the jktcp user-space TCP stack and the RSD RemoteXPC
//! handshake. This crate wraps that stack in the session
//! shapes the two gateways need:
//!
//! - [`RpIdentity`] — a loaded `_rp.plist` identity
//!   (RPK `rppairing_identity_from_plist`).
//! - [`RpSession`] — one established session: pair-verify →
//!   tunnel listener → TLS-PSK tunnel → TCP adapter → RSD
//!   handshake, held as a unit and dropped as a unit (Swift
//!   `ensureRPConnection`'s cache / limd's `withRPTunnel` +
//!   `getActiveRSD`). Invalidation is the caller's generation
//!   counter, exactly like the gateway core's other caches.
//! - [`RpStream`] — one raw RSD service stream with the
//!   4-byte-BE + XML plist framing and the RSDCheckin sequence
//!   of Swift limd's `withRSDService`.
//!
//! Pair-setup (SRP) and the wireless pairing host are M8; this
//! crate's `pair_verify` never falls back to setup, mirroring
//! `rppairing_pair_verify`'s contract.

pub mod session;
pub mod stream;

/// The idevice 0.1.68 crate, re-exported so gateway RP code
/// names its service clients through one path (D-M7-2).
pub use idevice68;

pub use session::{
    is_pairing_error68, resolve_service_port, RpError, RpIdentity, RpResult, RpSession,
    RpTunnelInfo,
};
pub use stream::RpStream;

/// Crate name, for wiring/smoke checks.
pub const CRATE_NAME: &str = "mm-rp";
