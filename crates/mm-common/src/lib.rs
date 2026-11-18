//! mm-common — shared substrate of the minimuxer Rust rewrite.
//!
//! Port of the Swift `MinimuxerCommon` module (inventory subsystem 12):
//! constants, the pairing-file model + parser, the TCP reachability
//! probe, the unified error bus, and the logging facade. No subsystem
//! logic lives here; every other mm-* crate builds on these types.

pub mod constants;
pub mod error;
pub mod logging;
pub mod pairing;
pub mod probe;

pub use error::{
    lookup_error_case, render_description, DeviceGatewayCode, DeviceGatewayError, EmProxyError,
    ErrorCaseDef, InternalError, MinimuxerComponent, MinimuxerError, MinimuxerServiceError,
    MmErrorInfo, PairingError, ERROR_CASES,
};
pub use pairing::{
    lockdown_missing_keys, parse_pairing_bytes, parse_pairing_content, rp_missing_keys,
    validate_pairing_file, LockdownPairingFile, PairingFile, PairingProtocol, RpPairingFile,
    LOCKDOWN_REQUIRED_KEYS, RP_REQUIRED_KEYS,
};
pub use probe::{probe_tcp_outcome, test_tcp, test_tcp_default, TcpProbeOutcome};
