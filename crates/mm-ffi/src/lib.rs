//! mm-ffi — the swift-bridge boundary of the minimuxer Rust rewrite.
//!
//! M1 skeleton: global tokio runtime ownership, EventSink registration,
//! and the error/type conversion bus, exposing the smallest useful
//! surface — version query, `MmErrorInfo` crossing, the mm-common
//! pairing-parse entry, the TCP probe, and an async ping proving the
//! async bridge. Subsystem APIs land here milestone by milestone.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// The single multi-threaded tokio runtime every bridge call spawns on
/// (design §3.3). Held for the App's lifetime.
pub fn global_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("mm-ffi")
            .build()
            .expect("failed to build mm-ffi tokio runtime")
    })
}

#[swift_bridge::bridge]
mod ffi {
    // FFI twin of `mm_common::MmErrorInfo` — the only error shape that
    // crosses the boundary (domain = Swift error type name, code =
    // Swift case name verbatim, reason = payload string).
    #[swift_bridge(swift_repr = "struct")]
    struct MmErrorInfo {
        domain: String,
        code: String,
        reason: String,
    }

    // FFI twin of `mm_common::PairingProtocol`.
    enum MmPairingProtocol {
        RpPairing,
        Lockdown,
        Unknown,
    }

    // Result of a pairing-file parse: on failure `error` carries the
    // PairingError as MmErrorInfo and `protocol` is `Unknown`.
    #[swift_bridge(swift_repr = "struct")]
    struct MmPairingOutcome {
        success: bool,
        protocol: MmPairingProtocol,
        error: MmErrorInfo,
    }

    extern "Rust" {
        fn mm_version() -> String;
        fn mm_parse_pairing_file(
            content: String,
            preferred: Option<MmPairingProtocol>,
        ) -> MmPairingOutcome;
        fn mm_describe_error(info: MmErrorInfo) -> String;
        fn mm_test_tcp(ip: String, port: u16, timeout_ms: i32) -> bool;
        async fn mm_async_ping() -> String;
        fn mm_register_event_sink(sink: MmEventSink);
        fn mm_clear_event_sink();
        fn mm_emit_log(message: String);
        fn mm_emit_status(success: bool, domain: String, code: String, reason: String);
    }

    extern "Swift" {
        type MmEventSink;
        fn on_log(self: &MmEventSink, message: String);
        fn on_status(
            self: &MmEventSink,
            seq: u64,
            success: bool,
            domain: String,
            code: String,
            reason: String,
        );
    }
}

// ---------------------------------------------------------------------------
// Conversions between the bridge types and the mm-common domain types.
// ---------------------------------------------------------------------------

impl From<mm_common::MmErrorInfo> for ffi::MmErrorInfo {
    fn from(info: mm_common::MmErrorInfo) -> Self {
        Self {
            domain: info.domain,
            code: info.code,
            reason: info.reason,
        }
    }
}

impl From<ffi::MmErrorInfo> for mm_common::MmErrorInfo {
    fn from(info: ffi::MmErrorInfo) -> Self {
        Self {
            domain: info.domain,
            code: info.code,
            reason: info.reason,
        }
    }
}

impl ffi::MmPairingProtocol {
    fn to_common(self) -> mm_common::PairingProtocol {
        match self {
            ffi::MmPairingProtocol::RpPairing => mm_common::PairingProtocol::RpPairing,
            ffi::MmPairingProtocol::Lockdown => mm_common::PairingProtocol::Lockdown,
            ffi::MmPairingProtocol::Unknown => mm_common::PairingProtocol::Unknown,
        }
    }

    fn from_common(protocol: mm_common::PairingProtocol) -> Self {
        match protocol {
            mm_common::PairingProtocol::RpPairing => ffi::MmPairingProtocol::RpPairing,
            mm_common::PairingProtocol::Lockdown => ffi::MmPairingProtocol::Lockdown,
            mm_common::PairingProtocol::Unknown => ffi::MmPairingProtocol::Unknown,
        }
    }
}

// ---------------------------------------------------------------------------
// EventSink registry (design §1/§3.3): the shell registers one sink; Rust
// only holds the bridge handle and calls it for status/log events.
// ---------------------------------------------------------------------------

struct SinkHolder(ffi::MmEventSink);
// The sink is only ever touched under the registry mutex; the Swift side
// guarantees the object stays alive while registered (shell unregisters
// in deinit), so moving the handle between threads is safe.
unsafe impl Send for SinkHolder {}

static EVENT_SINK: Mutex<Option<SinkHolder>> = Mutex::new(None);
static STATUS_SEQ: AtomicU64 = AtomicU64::new(0);

fn with_sink<R>(f: impl FnOnce(&ffi::MmEventSink) -> R) -> Option<R> {
    let guard = EVENT_SINK.lock().ok()?;
    guard.as_ref().map(|holder| f(&holder.0))
}

fn mm_register_event_sink(sink: ffi::MmEventSink) {
    if let Ok(mut guard) = EVENT_SINK.lock() {
        *guard = Some(SinkHolder(sink));
    }
    // Route mm-common's logging facade into the same sink (design §2:
    // logging callbacks travel the sink path like status events).
    mm_common::logging::set_log_sink(Some(std::sync::Arc::new(|level, msg| {
        let tag = match level {
            mm_common::logging::LogLevel::Debug => "debug",
            mm_common::logging::LogLevel::Verbose => "verbose",
        };
        mm_emit_log(format!("[{tag}] {msg}"));
    })));
    mm_common::logging::set_logging_enabled(true);
}

fn mm_clear_event_sink() {
    if let Ok(mut guard) = EVENT_SINK.lock() {
        *guard = None;
    }
}

fn mm_emit_log(message: String) {
    with_sink(|sink| sink.on_log(message));
}

fn mm_emit_status(success: bool, domain: String, code: String, reason: String) {
    let seq = STATUS_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    with_sink(|sink| sink.on_status(seq, success, domain, code, reason));
}

// ---------------------------------------------------------------------------
// Bridge entry points.
// ---------------------------------------------------------------------------

fn mm_version() -> String {
    format!(
        "minimuxer-rust rewrite skeleton (mm-ffi {}, mm-common {})",
        env!("CARGO_PKG_VERSION"),
        mm_common::ERROR_CASES.len()
    )
}

fn mm_parse_pairing_file(
    content: String,
    preferred: Option<ffi::MmPairingProtocol>,
) -> ffi::MmPairingOutcome {
    let preferred = preferred.map(|p| p.to_common());
    match mm_common::parse_pairing_content(&content, preferred) {
        Ok(file) => ffi::MmPairingOutcome {
            success: true,
            protocol: ffi::MmPairingProtocol::from_common(file.mode()),
            error: mm_common::MmErrorInfo::new("", "", "").into(),
        },
        Err(err) => ffi::MmPairingOutcome {
            success: false,
            protocol: ffi::MmPairingProtocol::Unknown,
            error: err.to_info().into(),
        },
    }
}

fn mm_describe_error(info: ffi::MmErrorInfo) -> String {
    mm_common::MmErrorInfo::from(info).description()
}

fn mm_test_tcp(ip: String, port: u16, timeout_ms: i32) -> bool {
    mm_common::test_tcp(&ip, port, timeout_ms)
}

async fn mm_async_ping() -> String {
    // Proves the async bridge end-to-end: the call is driven by the
    // caller's executor, the work itself runs on the global runtime.
    let handle = global_runtime().spawn(async { "pong".to_string() });
    match handle.await {
        Ok(text) => text,
        Err(_) => "join error".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCKDOWN_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n<key>WiFiMACAddress</key>\n<string>AA:BB:CC:DD:EE:FF</string>\n<key>SystemBUID</key>\n<string>BUID-1</string>\n<key>RootPrivateKey</key>\n<data>AAECAw==</data>\n<key>HostPrivateKey</key>\n<data>AAECAw==</data>\n<key>HostID</key>\n<string>H1</string>\n<key>RootCertificate</key>\n<data>AAECAw==</data>\n<key>UDID</key>\n<string>UDID-1</string>\n<key>EscrowBag</key>\n<data>AAECAw==</data>\n<key>HostCertificate</key>\n<data>AAECAw==</data>\n<key>DeviceCertificate</key>\n<data>AAECAw==</data>\n</dict>\n</plist>\n";

    #[test]
    fn version_is_nonempty() {
        assert!(mm_version().contains("mm-ffi"));
    }

    #[test]
    fn parse_pairing_crosses_boundary() {
        let ok = mm_parse_pairing_file(LOCKDOWN_XML.to_string(), None);
        assert!(ok.success);
        assert!(matches!(ok.protocol, ffi::MmPairingProtocol::Lockdown));

        let bad = mm_parse_pairing_file("nope".to_string(), None);
        assert!(!bad.success);
        assert_eq!(bad.error.domain, "PairingError");
        assert_eq!(bad.error.code, "invalidPlist");
    }

    #[test]
    fn describe_error_renders_swift_text() {
        let info = ffi::MmErrorInfo {
            domain: "MinimuxerError".to_string(),
            code: "noVPN".to_string(),
            reason: "No utun interface detected".to_string(),
        };
        assert_eq!(
            mm_describe_error(info),
            "NoVPN: No utun interface detected"
        );
    }

    #[test]
    fn async_ping_runs_on_global_runtime() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        assert_eq!(rt.block_on(mm_async_ping()), "pong");
    }

    #[test]
    fn shell_crates_are_linked() {
        assert_eq!(mm_net::CRATE_NAME, "mm-net");
        assert_eq!(mm_rp::CRATE_NAME, "mm-rp");
        assert_eq!(mm_gateway::CRATE_NAME, "mm-gateway");
        assert_eq!(mm_emproxy::CRATE_NAME, "mm-emproxy");
        assert_eq!(mm_core::CRATE_NAME, "mm-core");
    }
}
