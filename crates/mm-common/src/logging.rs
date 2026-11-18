//! Logging facade — the Rust counterpart of the Swift modules'
//! `debugLog`/`verboseLog` free functions plus the
//! setLogging/isLoggingEnabled switches (MinimuxerCommonLogging et al.).
//!
//! Output goes to a single process-wide sink registered by the owning
//! layer (mm-ffi forwards it to the Swift EventSink); with no sink
//! registered, messages are dropped. This mirrors the Swift side, where
//! the log functions are no-ops unless logging is enabled.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Verbose,
}

pub type LogSink = Arc<dyn Fn(LogLevel, &str) + Send + Sync>;

static SINK: RwLock<Option<LogSink>> = RwLock::new(None);
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Swift `setLogging(_:)`.
pub fn set_logging_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Swift `isLoggingEnabled`.
pub fn is_logging_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Registers (or clears, with `None`) the process-wide log sink.
pub fn set_log_sink(sink: Option<LogSink>) {
    if let Ok(mut guard) = SINK.write() {
        *guard = sink;
    }
}

fn emit(level: LogLevel, message: &str) {
    if !is_logging_enabled() {
        return;
    }
    if let Ok(guard) = SINK.read() {
        if let Some(sink) = guard.as_ref() {
            sink(level, message);
        }
    }
}

/// Swift `debugLog(_:)`.
pub fn debug_log(message: &str) {
    emit(LogLevel::Debug, message);
}

/// Swift `verboseLog(_:)`.
pub fn verbose_log(message: &str) {
    emit(LogLevel::Verbose, message);
}
