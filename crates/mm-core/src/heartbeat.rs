//! `HeartbeatService` — port of Swift `HeartbeatService`
//! (yongshuai1013/minimuxer @714aafa3, which carries our
//! keep-alive fix: the loop lives and dies with core start/stop
//! and paces itself at 75% of the clamped interval).
//!
//! Behaviour follows `docs/m4-core-diff.md` §3 line by line. The
//! one planned deviation (DESIGN §4 M4, diff list D-M4-1): the
//! two *unreachable* paths (endpoint uninitialized, TCP probe
//! failed) sleep on the d044c752 backoff curve — 5 s doubling to
//! a 60 s cap, reset once the device is reachable — instead of
//! Swift's fixed 5 s. The success path and the heartbeat-failure
//! path keep Swift's pacing exactly (75% of the clamped
//! interval, 4 s floor), so the success rhythm is unchanged.
//!
//! Time is a seam ([`HeartbeatClock`]) so tests can drive the
//! loop on a virtual clock; the beat itself goes through
//! [`HeartbeatDeps`], which mm-core's `MinimuxerCore` implements
//! over the real gateway/endpoint/proxy.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mm_common::constants::{
    HEARTBEAT_INITIAL_INTERVAL_SECONDS, HEARTBEAT_MAX_INTERVAL_SECONDS,
    HEARTBEAT_MIN_INTERVAL_SECONDS,
};
use mm_common::logging::{debug_log, verbose_log};
use mm_common::MmErrorInfo;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

pub type HeartbeatFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The environment the heartbeat loop runs against (Swift: the
/// service's `deviceProvider`/`gateway`, `proxyServer` and
/// `endpoint`, read fresh on every beat — a backend switch takes
/// effect on the next beat, exactly like Swift).
pub trait HeartbeatDeps: Send + Sync {
    /// Swift `gateway.requiresUsbmuxd`.
    fn requires_usbmuxd(&self) -> bool;
    /// Swift `proxyServer.isListening`.
    fn proxy_listening(&self) -> bool;
    /// Swift `endpoint.ip()` — `None` while uninitialized.
    fn endpoint_ip(&self) -> Option<String>;
    /// Swift `NetworkUtils.testTCP(ip:port:)` against the
    /// gateway's service port, with NetworkUtils' own default
    /// timeout (the constant 250 ms — *not* the configurable
    /// deviceProbeTimeout, which only flows through
    /// `MinimuxerImpl.testDeviceConnection`).
    fn probe(&self, ip: &str) -> bool;
    /// Swift `gateway.performHeartbeat(interval:)`.
    fn perform_heartbeat<'a>(
        &'a self,
        interval: u64,
    ) -> HeartbeatFuture<'a, Result<u64, MmErrorInfo>>;
}

/// Sleep seam: production sleeps on tokio time; tests drive a
/// virtual clock.
pub trait HeartbeatClock: Send + Sync {
    fn sleep<'a>(&'a self, dur: Duration) -> HeartbeatFuture<'a, ()>;
}

/// The production clock.
pub struct TokioClock;

impl HeartbeatClock for TokioClock {
    fn sleep<'a>(&'a self, dur: Duration) -> HeartbeatFuture<'a, ()> {
        Box::pin(async move {
            tokio::time::sleep(dur).await;
        })
    }
}

/// Swift `clampInterval`: the device-reported interval clamped
/// to [5, 60] seconds.
pub fn clamp_interval(interval: u64) -> u64 {
    interval.clamp(
        HEARTBEAT_MIN_INTERVAL_SECONDS,
        HEARTBEAT_MAX_INTERVAL_SECONDS,
    )
}

/// Swift `pacingSleep(for:)`: 75% of the clamped interval, at
/// least 4 seconds. Applied on both the success and the
/// heartbeat-failure paths.
pub fn pacing_duration(interval: u64) -> Duration {
    let seconds = (clamp_interval(interval) as f64 * 0.75).max(4.0);
    Duration::from_secs_f64(seconds)
}

/// The d044c752 backoff curve merged into the unreachable paths
/// (D-M4-1): 5 s, doubling, capped at 60 s.
pub fn next_unreachable_backoff(current: Duration) -> Duration {
    (current * 2).min(Duration::from_secs(HEARTBEAT_MAX_INTERVAL_SECONDS))
}

/// Swift's fixed 5 s unreachable retry — the initial value of
/// the D-M4-1 backoff curve.
pub const UNREACHABLE_RETRY_INITIAL: Duration =
    Duration::from_secs(HEARTBEAT_INITIAL_INTERVAL_SECONDS);

/// Swift `MutableState` actor.
#[derive(Default)]
struct MutableState {
    running: bool,
    task_active: bool,
}

/// Swift `HeartbeatService`.
pub struct HeartbeatService {
    deps: Arc<dyn HeartbeatDeps>,
    clock: Arc<dyn HeartbeatClock>,
    state: Arc<Mutex<MutableState>>,
    task: Mutex<Option<JoinHandle<()>>>,
    /// Wake-up channel for the *current* task's sleeps. Replaced
    /// on every fresh start so a stale stop permit can never
    /// shorten a later task's sleep (Swift's per-Task
    /// cancellation has the same isolation).
    stop_notify: Mutex<Arc<Notify>>,
    last_beat_successful: Arc<AtomicBool>,
}

impl HeartbeatService {
    pub fn new(deps: Arc<dyn HeartbeatDeps>, clock: Arc<dyn HeartbeatClock>) -> Self {
        Self {
            deps,
            clock,
            state: Arc::new(Mutex::new(MutableState::default())),
            task: Mutex::new(None),
            stop_notify: Mutex::new(Arc::new(Notify::new())),
            last_beat_successful: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Swift `lastBeatSuccessful` (internal state + log dedup in
    /// Swift; the port also surfaces it for the FFI/tests — it is
    /// *not* an isReady input, matching Swift).
    pub fn last_beat_successful(&self) -> bool {
        self.last_beat_successful.load(Ordering::SeqCst)
    }

    /// Whether the loop task is alive (Swift `taskActive`).
    pub fn is_task_active(&self) -> bool {
        self.state.lock().unwrap().task_active
    }

    /// Swift `start()`: ignored (beyond setting `running`) while
    /// a task is already active. Must be called from within a
    /// tokio runtime (the loop task is spawned on it).
    pub fn start(&self) {
        {
            let mut st = self.state.lock().unwrap();
            // Swift `MutableState.tryStart()`.
            if st.task_active {
                st.running = true;
                return;
            }
            st.running = true;
            st.task_active = true;
        }
        verbose_log("[minimuxer] Starting heartbeat task...");
        let notify = Arc::new(Notify::new());
        *self.stop_notify.lock().unwrap() = notify.clone();
        let deps = self.deps.clone();
        let clock = self.clock.clone();
        let state = self.state.clone();
        let last_beat = self.last_beat_successful.clone();
        let handle = tokio::spawn(async move {
            verbose_log("[minimuxer] heartbeat-task: started");
            heartbeat_loop(deps, clock, state.clone(), notify, last_beat.clone()).await;
            // Swift: `state.terminate()` + lastBeatSuccessful=false
            // on task exit.
            {
                let mut st = state.lock().unwrap();
                st.task_active = false;
                st.running = false;
            }
            last_beat.store(false, Ordering::SeqCst);
            verbose_log("[minimuxer] heartbeat-task: stopped");
        });
        *self.task.lock().unwrap() = Some(handle);
    }

    /// Swift `stop()`: flags the loop down, wakes its sleeps,
    /// and waits for the task to exit. An in-flight beat is not
    /// interrupted (Swift cancels the Task, which only cuts the
    /// sleeps); the wait is bounded by that beat's Marco timeout.
    pub async fn stop(&self) {
        {
            self.state.lock().unwrap().running = false;
        }
        self.last_beat_successful.store(false, Ordering::SeqCst);
        let handle = self.task.lock().unwrap().take();
        // notify_one stores a permit, so a sleep the task enters
        // *after* this call still returns immediately and the
        // loop observes `running == false` at its head.
        self.stop_notify.lock().unwrap().notify_one();
        if let Some(handle) = handle {
            let _ = handle.await;
        }
        verbose_log("[minimuxer] HeartbeatService stop requested");
    }
}

fn is_running(state: &Mutex<MutableState>) -> bool {
    state.lock().unwrap().running
}

/// A sleep that `stop()` cuts short. The running flag remains
/// the source of truth; the notify only bounds how long stop()
/// waits.
async fn interruptible_sleep(
    clock: &Arc<dyn HeartbeatClock>,
    notify: &Arc<Notify>,
    dur: Duration,
) {
    tokio::select! {
        _ = clock.sleep(dur) => {},
        _ = notify.notified() => {},
    }
}

/// Swift `logIfNeeded`: consecutive duplicates are logged once.
fn log_if_needed(last: &mut Option<String>, message: &str, verbose: bool) {
    if last.as_deref() != Some(message) {
        if verbose {
            verbose_log(&format!("[minimuxer] heartbeat-task: {message}"));
        } else {
            debug_log(&format!("[minimuxer] heartbeat-task: {message}"));
        }
        *last = Some(message.to_string());
    }
}

async fn heartbeat_loop(
    deps: Arc<dyn HeartbeatDeps>,
    clock: Arc<dyn HeartbeatClock>,
    state: Arc<Mutex<MutableState>>,
    stop_notify: Arc<Notify>,
    last_beat: Arc<AtomicBool>,
) {
    // Swift: while the usbmuxd proxy is not yet listening, poll
    // at a 1 s cadence before entering the main loop.
    if deps.requires_usbmuxd() {
        let mut last_error: Option<String> = None;
        while !deps.proxy_listening() {
            if !is_running(&state) {
                return;
            }
            log_if_needed(
                &mut last_error,
                "Waiting for usbmuxd to be ready...",
                true,
            );
            interruptible_sleep(&clock, &stop_notify, Duration::from_secs(1)).await;
        }
        verbose_log("[minimuxer] heartbeat-task: usbmuxd is ready");
    }

    let mut current_interval: u64 = HEARTBEAT_INITIAL_INTERVAL_SECONDS;
    let mut backoff = UNREACHABLE_RETRY_INITIAL;
    // Swift's `lastErrorDescription` lives on the service but is
    // only touched from inside the loop; it is loop-local here.
    let mut last_error: Option<String> = None;

    while is_running(&state) {
        let Some(tunnel_peer_ip) = deps.endpoint_ip() else {
            log_if_needed(&mut last_error, "device IP unavailable", true);
            last_beat.store(false, Ordering::SeqCst);
            interruptible_sleep(&clock, &stop_notify, backoff).await;
            backoff = next_unreachable_backoff(backoff);
            continue;
        };

        if !deps.probe(&tunnel_peer_ip) {
            log_if_needed(&mut last_error, "device IP not reachable, waiting...", true);
            last_beat.store(false, Ordering::SeqCst);
            interruptible_sleep(&clock, &stop_notify, backoff).await;
            backoff = next_unreachable_backoff(backoff);
            continue;
        }

        // Reachable: the D-M4-1 backoff restarts from 5 s.
        backoff = UNREACHABLE_RETRY_INITIAL;

        match deps.perform_heartbeat(clamp_interval(current_interval)).await {
            Ok(new_interval) => {
                current_interval = new_interval;
                last_beat.store(true, Ordering::SeqCst);
                last_error = None;
            }
            Err(err) => {
                log_if_needed(
                    &mut last_error,
                    &format!("Heartbeat failed: {}", err.description()),
                    false,
                );
                last_beat.store(false, Ordering::SeqCst);
                current_interval = HEARTBEAT_INITIAL_INTERVAL_SECONDS;
            }
        }

        interruptible_sleep(&clock, &stop_notify, pacing_duration(current_interval)).await;
    }
}
