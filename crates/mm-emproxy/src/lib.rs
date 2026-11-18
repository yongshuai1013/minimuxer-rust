//! `mm-emproxy` — EMProxy (design subsystem 8): the Rust port of
//! Swift `EMProxyImpl` (yongshuai1013/minimuxer @714aafa3) over the
//! `em_proxy` crate (SideStore/em_proxy, pinned to tag v0.9.3 —
//! the exact source of the xcframework the Swift package links;
//! decision + evidence in `docs/m6-emproxy-diff.md`).
//!
//! Behaviour spec: `docs/m6-emproxy-behavior.md` (converted from
//! `Tests/EMProxyTests.swift` + `EMProxyImpl.swift`). The server
//! core itself is NOT reimplemented: `em_proxy::start_loopback`
//! runs the boringtun WireGuard loopback server; this crate adds
//! the wrapper semantics — handshake-client configuration, the
//! C-code → `EmProxyError` mapping, log forwarding, and the
//! post-start VPN handshake probe (trigger / candidate-scan).

use std::ffi::{c_char, c_int, CStr};
use std::net::SocketAddrV4;
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use mm_common::constants::{EMP_SERVER_HOST, EMP_SERVER_PORT, LOCKDOWND_PORT, VPN_HANDSHAKE_TIMEOUT_NS};
use mm_common::logging::{debug_log, verbose_log, LogLevel};
use mm_common::probe::{probe_tcp_outcome, TcpProbeOutcome};
use mm_common::EmProxyError;

/// Crate name, for wiring/smoke checks.
pub const CRATE_NAME: &str = "mm-emproxy";

/// Swift `EMProxyImpl.HandshakeConfig` (private struct).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeConfig {
    pub host: String,
    pub port: u16,
    pub enabled: bool,
}

/// Timing knobs of the handshake probe. Production values are the
/// Swift literals (`EMProxyImpl`: 200 ms between attempts, a 1 s
/// per-attempt cap inside `probeHost`, and
/// `MinimuxerConstants.vpnHandshakeTimeoutNs` = 10 s overall);
/// tests inject shortened values to exercise the rhythm quickly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeParams {
    pub attempt_timeout: Duration,
    pub retry_interval: Duration,
    pub total_timeout: Duration,
}

impl Default for ProbeParams {
    fn default() -> Self {
        Self {
            attempt_timeout: Duration::from_secs(1),
            retry_interval: Duration::from_millis(200),
            total_timeout: Duration::from_nanos(VPN_HANDSHAKE_TIMEOUT_NS),
        }
    }
}

/// Source of candidate peer IPs for the scan probe path (Swift
/// `discoverUtunCandidateDestinations`). Injected so the crate
/// graph stays as designed (diff D-M6-7): mm-core assembles the
/// production provider from mm-net's scan + candidate derivation.
pub type CandidateProvider = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

/// Which probe path `start` takes after the server is up
/// (behavior list B1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbePlan {
    /// Probe one designated peer until it answers.
    Trigger { host: String, port: u16 },
    /// Probe utun-derived candidates in parallel rounds.
    Candidates { port: u16 },
}

/// The B1 dispatch truth table, as a pure function: designated
/// probing only when the client is enabled AND the host is
/// non-empty after trimming; every other combination scans
/// candidates on the configured port (or lockdownd's when the
/// configured port is 0).
pub fn probe_plan(config: &HandshakeConfig) -> ProbePlan {
    if config.enabled && !config.host.trim().is_empty() {
        ProbePlan::Trigger {
            host: config.host.clone(),
            port: config.port,
        }
    } else {
        ProbePlan::Candidates {
            port: if config.port != 0 {
                config.port
            } else {
                LOCKDOWND_PORT
            },
        }
    }
}

/// Swift `start_emotional_damage` return-code mapping
/// (EMProxyImpl.start). `address` is the full `"host:port"` string
/// the C layer received — the `invalidSocketAddress` payload.
pub fn map_start_code(code: i32, address: &str) -> Result<(), EmProxyError> {
    match code {
        0 => Ok(()),
        -1 => Err(EmProxyError::InvalidBindAddressPointer),
        -2 => Err(EmProxyError::InvalidUtf8String),
        -3 => Err(EmProxyError::InvalidSocketAddress(address.to_string())),
        -4 => Err(EmProxyError::SocketBindFailed),
        -5 => Err(EmProxyError::CryptoInitFailed),
        other => Err(EmProxyError::UnknownError(other)),
    }
}

/// Swift `stop_emotional_damage` return-code mapping
/// (EMProxyImpl.stop).
pub fn map_stop_code(code: i32) -> Result<(), EmProxyError> {
    match code {
        0 => Ok(()),
        -1 => Err(EmProxyError::ServerNotRunning),
        -2 => Err(EmProxyError::StopSignalFailed),
        -3 => Err(EmProxyError::ThreadJoinFailed),
        other => Err(EmProxyError::UnknownError(other)),
    }
}

// ---------------------------------------------------------------------------
// Log forwarding (behavior A8): EMProxyImpl.init registers the C
// log callback once; messages are prefixed "[EMProxy] " and routed
// level <= 1 → verboseLog, level >= 2 → debugLog (em_proxy levels:
// 0=DEBUG, 1=INFO, 2=WARN, 3=ERROR). The inversion is Swift's own
// (diff D-M6-5) and is replicated verbatim.
// ---------------------------------------------------------------------------

static LOG_CALLBACK_ONCE: Once = Once::new();

fn register_log_callback_once() {
    LOG_CALLBACK_ONCE.call_once(|| unsafe {
        em_proxy::set_log_callback(Some(emp_log_callback));
    });
}

unsafe extern "C" fn emp_log_callback(level: c_int, msg: *const c_char) -> bool {
    if msg.is_null() {
        return false;
    }
    let text = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
    forward_emproxy_log(level, &text);
    true
}

/// The routing core of the log callback, split out for tests.
pub fn forward_emproxy_log(level: i32, msg: &str) -> LogLevel {
    let line = format!("[EMProxy] {msg}");
    if level <= 1 {
        verbose_log(&line);
        LogLevel::Verbose
    } else {
        debug_log(&line);
        LogLevel::Debug
    }
}

struct State {
    config: Option<HandshakeConfig>,
    handle: Option<em_proxy::ProxyHandle>,
}

/// Swift `EMProxyImpl` (the facade's `emproxy` instance).
pub struct EmProxy {
    state: Mutex<State>,
    candidates: CandidateProvider,
    probe_params: ProbeParams,
}

impl Default for EmProxy {
    fn default() -> Self {
        Self::new()
    }
}

impl EmProxy {
    /// No candidate source (the scan probe path finds nothing,
    /// like Swift on a device with no utun interface).
    pub fn new() -> Self {
        Self::with_options(None, ProbeParams::default())
    }

    pub fn with_candidate_provider(provider: CandidateProvider) -> Self {
        Self::with_options(Some(provider), ProbeParams::default())
    }

    pub fn with_options(
        candidates: Option<CandidateProvider>,
        probe_params: ProbeParams,
    ) -> Self {
        register_log_callback_once();
        Self {
            state: Mutex::new(State {
                config: None,
                handle: None,
            }),
            candidates: candidates
                .unwrap_or_else(|| Arc::new(Vec::new)),
            probe_params,
        }
    }

    /// Swift `setHandshakeClient(host:port:enabled:)`.
    pub fn set_handshake_client(&self, host: &str, port: u16, enabled: bool) {
        let mut st = self.state.lock().unwrap();
        st.config = Some(HandshakeConfig {
            host: host.to_string(),
            port,
            enabled,
        });
    }

    pub fn handshake_config(&self) -> Option<HandshakeConfig> {
        self.state.lock().unwrap().config.clone()
    }

    pub fn is_running(&self) -> bool {
        self.state.lock().unwrap().handle.is_some()
    }

    /// Swift `start(host:port:)`. On success the handshake probe
    /// runs to completion (its outcome never fails the start —
    /// behavior §B preamble).
    pub async fn start(&self, host: &str, port: u16) -> Result<(), EmProxyError> {
        let config = match self.handshake_config() {
            Some(c) => c,
            None => return Err(EmProxyError::HandshakeClientNotConfigured),
        };
        let address = format!("{host}:{port}");
        {
            let mut st = self.state.lock().unwrap();
            if st.handle.is_some() {
                // C layer: "Proxy already exists, skipping" → 0.
                debug_log("[EMProxy] start skipped: proxy already running");
            } else {
                let socket_addr: SocketAddrV4 = match address.parse() {
                    Ok(a) => a,
                    Err(_) => return map_start_code(-3, &address),
                };
                match em_proxy::start_loopback(socket_addr) {
                    Ok(handle) => st.handle = Some(handle),
                    Err(code) => return map_start_code(code, &address),
                }
            }
        }
        self.run_handshake_probe(&config).await;
        Ok(())
    }

    /// Swift `EMProxyAPI.start()` convenience (default bind).
    pub async fn start_default(&self) -> Result<(), EmProxyError> {
        self.start(EMP_SERVER_HOST, EMP_SERVER_PORT).await
    }

    /// Swift `stop()`. The stop signal is sent and the server
    /// thread joined before the result is decided, with the C
    /// layer's precedence: signal failure (-2) beats join
    /// failure (-3).
    pub fn stop(&self) -> Result<(), EmProxyError> {
        let handle = {
            let mut st = self.state.lock().unwrap();
            st.handle.take()
        };
        let mut handle = match handle {
            Some(h) => h,
            None => return map_stop_code(-1),
        };
        let send_res = handle.sender.send(());
        let join_res = match handle.join_handle.take() {
            Some(jh) => jh.join(),
            None => Ok(()),
        };
        if send_res.is_err() {
            return map_stop_code(-2);
        }
        if join_res.is_err() {
            return map_stop_code(-3);
        }
        map_stop_code(0)
    }
}

// ---------------------------------------------------------------------------
// Handshake probe (behavior §B). Neither path ever fails the
// caller: Swift logs and moves on.
// ---------------------------------------------------------------------------

impl EmProxy {
    async fn run_handshake_probe(&self, config: &HandshakeConfig) {
        match probe_plan(config) {
            ProbePlan::Trigger { host, port } => {
                // Swift triggerVPNHandshake re-checks emptiness.
                if host.trim().is_empty() {
                    debug_log("[EMProxy] triggerVPNHandshake skipped: host is empty");
                    return;
                }
                self.probe_trigger(&host, port).await;
            }
            ProbePlan::Candidates { port } => {
                self.probe_candidates(port).await;
            }
        }
    }

    /// Swift `triggerVPNHandshake`: probe the designated peer
    /// every `retry_interval` until it answers or the total
    /// timeout elapses.
    async fn probe_trigger(&self, host: &str, port: u16) {
        let started = Instant::now();
        while started.elapsed() < self.probe_params.total_timeout {
            if self.probe_host(host, port).await {
                return;
            }
            tokio::time::sleep(self.probe_params.retry_interval).await;
        }
        debug_log("[EMProxy] triggerVPNHandshake timed out");
    }

    /// Swift `probeVPNHandshake`: rounds of parallel probes over
    /// the candidate set; the first success wins the round (and
    /// the probe). An empty candidate set skips immediately.
    async fn probe_candidates(&self, port: u16) {
        let candidates = (self.candidates)();
        if candidates.is_empty() {
            debug_log("[EMProxy] probeVPNHandshake skipped: no utun interface candidates found");
            return;
        }
        debug_log(&format!(
            "[EMProxy] probeVPNHandshake starting for candidates: {candidates:?}, port: {port}"
        ));
        let started = Instant::now();
        while started.elapsed() < self.probe_params.total_timeout {
            if self.probe_round(&candidates, port).await {
                debug_log("[EMProxy] probeVPNHandshake succeeded!");
                return;
            }
            tokio::time::sleep(self.probe_params.retry_interval).await;
        }
        debug_log("[EMProxy] probeVPNHandshake timed out");
    }

    async fn probe_round(&self, candidates: &[String], port: u16) -> bool {
        let mut set = tokio::task::JoinSet::new();
        for candidate in candidates {
            let candidate = candidate.clone();
            let timeout = self.probe_params.attempt_timeout;
            set.spawn(async move { probe_host_blocking(&candidate, port, timeout) });
        }
        while let Some(res) = set.join_next().await {
            if res.unwrap_or(false) {
                set.abort_all();
                return true;
            }
        }
        false
    }

    /// Swift `probeHost` (behavior B4): success iff the outcome
    /// is Connected or Refused (ECONNREFUSED counts — the tunnel
    /// routes, nothing listens yet).
    pub async fn probe_host(&self, host: &str, port: u16) -> bool {
        let host = host.to_string();
        let timeout = self.probe_params.attempt_timeout;
        tokio::task::spawn_blocking(move || probe_host_blocking(&host, port, timeout))
            .await
            .unwrap_or(false)
    }
}

fn probe_host_blocking(host: &str, port: u16, timeout: Duration) -> bool {
    let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    matches!(
        probe_tcp_outcome(host, port, timeout_ms),
        TcpProbeOutcome::Connected | TcpProbeOutcome::Refused
    )
}
