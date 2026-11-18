//! Heartbeat rhythm tests with a virtual clock and fake deps:
//! Marco-interval pacing (Swift 714aafa3), failure reset, and the
//! D-M4-1 unreachable backoff curve (the one adopted deviation
//! from Swift's fixed 5 s).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mm_common::{MmErrorInfo, MinimuxerError};
use mm_core::heartbeat::{
    clamp_interval, next_unreachable_backoff, pacing_duration, HeartbeatClock,
    HeartbeatDeps, HeartbeatFuture, HeartbeatService,
};
use tokio::sync::Notify;

// ---------------------------------------------------------------------------
// Virtual clock
// ---------------------------------------------------------------------------

struct ClockState {
    now: Duration,
    log: Vec<Duration>,
}

struct ClockInner {
    state: Mutex<ClockState>,
    notify: Notify,
}

#[derive(Clone)]
struct FakeClock {
    inner: Arc<ClockInner>,
}

impl FakeClock {
    fn new() -> Self {
        Self {
            inner: Arc::new(ClockInner {
                state: Mutex::new(ClockState {
                    now: Duration::ZERO,
                    log: Vec::new(),
                }),
                notify: Notify::new(),
            }),
        }
    }

    fn advance(&self, dur: Duration) {
        self.inner.state.lock().unwrap().now += dur;
        self.inner.notify.notify_waiters();
    }

    fn sleeps(&self) -> Vec<Duration> {
        self.inner.state.lock().unwrap().log.clone()
    }
}

impl HeartbeatClock for FakeClock {
    fn sleep<'a>(&'a self, dur: Duration) -> HeartbeatFuture<'a, ()> {
        let inner = self.inner.clone();
        let deadline = {
            let mut st = inner.state.lock().unwrap();
            st.log.push(dur);
            st.now + dur
        };
        Box::pin(async move {
            loop {
                let notified = inner.notify.notified();
                tokio::pin!(notified);
                let _ = notified.as_mut().enable();
                if inner.state.lock().unwrap().now >= deadline {
                    return;
                }
                notified.await;
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Fake deps
// ---------------------------------------------------------------------------

struct FakeDeps {
    requires: bool,
    proxy_listening: AtomicBool,
    endpoint: Mutex<Option<String>>,
    probe_ok: AtomicBool,
    beats: Mutex<Vec<u64>>,
    script: Mutex<VecDeque<Result<u64, MmErrorInfo>>>,
}

impl FakeDeps {
    fn new() -> Arc<Self> {
        Self::with_proxy(false, true)
    }

    fn with_proxy(requires: bool, listening: bool) -> Arc<Self> {
        Arc::new(Self {
            requires,
            proxy_listening: AtomicBool::new(listening),
            endpoint: Mutex::new(Some("10.7.0.1".to_string())),
            probe_ok: AtomicBool::new(true),
            beats: Mutex::new(Vec::new()),
            script: Mutex::new(VecDeque::new()),
        })
    }

    fn beat_args(&self) -> Vec<u64> {
        self.beats.lock().unwrap().clone()
    }
}

impl HeartbeatDeps for FakeDeps {
    fn requires_usbmuxd(&self) -> bool {
        self.requires
    }

    fn proxy_listening(&self) -> bool {
        self.proxy_listening.load(Ordering::SeqCst)
    }

    fn endpoint_ip(&self) -> Option<String> {
        self.endpoint.lock().unwrap().clone()
    }

    fn probe(&self, _ip: &str) -> bool {
        self.probe_ok.load(Ordering::SeqCst)
    }

    fn perform_heartbeat<'a>(
        &'a self,
        interval: u64,
    ) -> HeartbeatFuture<'a, Result<u64, MmErrorInfo>> {
        self.beats.lock().unwrap().push(interval);
        let scripted = self.script.lock().unwrap().pop_front();
        Box::pin(async move {
            match scripted {
                Some(r) => r,
                None => Ok(interval), // device echoes the interval
            }
        })
    }
}

fn make_svc(deps: Arc<FakeDeps>, clock: &FakeClock) -> HeartbeatService {
    HeartbeatService::new(deps, Arc::new(clock.clone()))
}

async fn wait_for_sleeps(clock: &FakeClock, n: usize) {
    for _ in 0..2000 {
        if clock.sleeps().len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    panic!("timed out waiting for {n} sleeps; got {:?}", clock.sleeps());
}

// ---------------------------------------------------------------------------
// Pure pacing math
// ---------------------------------------------------------------------------

#[test]
fn pacing_math_matches_swift() {
    assert_eq!(clamp_interval(0), 5);
    assert_eq!(clamp_interval(3), 5);
    assert_eq!(clamp_interval(15), 15);
    assert_eq!(clamp_interval(60), 60);
    assert_eq!(clamp_interval(600), 60);
    // max(clamp * 0.75, 4s)
    assert_eq!(pacing_duration(5), Duration::from_secs(4));
    assert_eq!(pacing_duration(8), Duration::from_secs(6));
    assert_eq!(pacing_duration(15), Duration::from_millis(11_250));
    assert_eq!(pacing_duration(60), Duration::from_secs(45));
    assert_eq!(pacing_duration(0), Duration::from_secs(4));
}

#[test]
fn backoff_curve_matches_d_m4_1() {
    let mut cur = Duration::from_secs(5);
    let mut curve = vec![cur.as_secs()];
    for _ in 0..6 {
        cur = next_unreachable_backoff(cur);
        curve.push(cur.as_secs());
    }
    assert_eq!(curve, vec![5, 10, 20, 40, 60, 60, 60]);
}

// ---------------------------------------------------------------------------
// Rhythm tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn success_rhythm_follows_device_interval() {
    let clock = FakeClock::new();
    let deps = FakeDeps::new();
    deps.script.lock().unwrap().extend([Ok(15), Ok(15), Ok(15)]);
    let svc = make_svc(deps.clone(), &clock);
    svc.start();

    // First beat uses clamp(5) = 5, then pacing(15) = 11.25s.
    wait_for_sleeps(&clock, 1).await;
    assert_eq!(deps.beat_args(), vec![5]);
    assert_eq!(clock.sleeps(), vec![Duration::from_millis(11_250)]);
    assert!(svc.last_beat_successful());

    clock.advance(Duration::from_millis(11_250));
    wait_for_sleeps(&clock, 2).await;
    assert_eq!(deps.beat_args(), vec![5, 15]);
    assert_eq!(
        clock.sleeps(),
        vec![Duration::from_millis(11_250), Duration::from_millis(11_250)]
    );

    svc.stop().await;
    assert!(!svc.is_task_active());
    assert!(!svc.last_beat_successful());
}

#[tokio::test]
async fn failure_resets_interval_and_paces_at_floor() {
    let clock = FakeClock::new();
    let deps = FakeDeps::new();
    let err = MinimuxerError::FetchUDID("boom".to_string()).to_info();
    deps.script.lock().unwrap().extend([Ok(30), Err(err), Ok(10)]);
    let svc = make_svc(deps.clone(), &clock);
    svc.start();

    wait_for_sleeps(&clock, 1).await;
    // Beat(5) -> Ok(30) -> pacing(30) = 22.5s.
    assert_eq!(clock.sleeps(), vec![Duration::from_millis(22_500)]);

    clock.advance(Duration::from_millis(22_500));
    wait_for_sleeps(&clock, 2).await;
    // Beat(clamp(30)) -> Err -> interval reset to 5 -> pacing(5) = 4s.
    assert_eq!(deps.beat_args(), vec![5, 30]);
    assert_eq!(
        clock.sleeps(),
        vec![Duration::from_millis(22_500), Duration::from_secs(4)]
    );
    assert!(!svc.last_beat_successful());

    clock.advance(Duration::from_secs(4));
    wait_for_sleeps(&clock, 3).await;
    // Next beat starts from the reset interval (5), not 30.
    assert_eq!(deps.beat_args(), vec![5, 30, 5]);
    assert_eq!(clock.sleeps()[2], Duration::from_millis(7_500));

    svc.stop().await;
}

#[tokio::test]
async fn unreachable_uses_backoff_and_resets_on_reachable() {
    let clock = FakeClock::new();
    let deps = FakeDeps::new();
    *deps.endpoint.lock().unwrap() = None; // no IP at all
    let svc = make_svc(deps.clone(), &clock);
    svc.start();

    // 5, 10, 20, 40, 60, 60 …
    let expected = [5u64, 10, 20, 40, 60, 60];
    for (i, secs) in expected.iter().enumerate() {
        wait_for_sleeps(&clock, i + 1).await;
        assert_eq!(clock.sleeps()[i], Duration::from_secs(*secs));
        clock.advance(Duration::from_secs(*secs));
    }
    assert!(deps.beat_args().is_empty());

    // Endpoint appears. Depending on scheduling, the loop either
    // wakes straight from the last backoff sleep into a beat, or
    // first parks in one more 60 s backoff sleep that the advance
    // below wakes. Either way it must end with one beat and a
    // pacing(5) = 4 s sleep as the latest entry.
    *deps.endpoint.lock().unwrap() = Some("10.7.0.1".to_string());
    clock.advance(Duration::from_secs(60));
    for _ in 0..2000 {
        if deps.beat_args() == vec![5]
            && clock.sleeps().last() == Some(&Duration::from_secs(4))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert_eq!(deps.beat_args(), vec![5]);
    assert_eq!(*clock.sleeps().last().unwrap(), Duration::from_secs(4));

    // Endpoint disappears again: backoff restarts at 5.
    let n = clock.sleeps().len();
    *deps.endpoint.lock().unwrap() = None;
    clock.advance(Duration::from_secs(4));
    wait_for_sleeps(&clock, n + 1).await;
    assert_eq!(clock.sleeps()[n], Duration::from_secs(5));

    svc.stop().await;
}

#[tokio::test]
async fn probe_failure_also_uses_backoff() {
    let clock = FakeClock::new();
    let deps = FakeDeps::new();
    deps.probe_ok.store(false, Ordering::SeqCst);
    let svc = make_svc(deps.clone(), &clock);
    svc.start();

    wait_for_sleeps(&clock, 1).await;
    assert_eq!(clock.sleeps(), vec![Duration::from_secs(5)]);
    clock.advance(Duration::from_secs(5));
    wait_for_sleeps(&clock, 2).await;
    assert_eq!(clock.sleeps()[1], Duration::from_secs(10));
    assert!(deps.beat_args().is_empty());

    deps.probe_ok.store(true, Ordering::SeqCst);
    clock.advance(Duration::from_secs(10));
    wait_for_sleeps(&clock, 3).await;
    assert_eq!(deps.beat_args(), vec![5]);

    svc.stop().await;
}

#[tokio::test]
async fn proxy_wait_uses_one_second_cadence() {
    let clock = FakeClock::new();
    let deps = FakeDeps::with_proxy(true, false);
    let svc = make_svc(deps.clone(), &clock);
    svc.start();

    wait_for_sleeps(&clock, 1).await;
    assert_eq!(clock.sleeps(), vec![Duration::from_secs(1)]);
    clock.advance(Duration::from_secs(1));
    wait_for_sleeps(&clock, 2).await;
    assert_eq!(clock.sleeps()[1], Duration::from_secs(1));
    assert!(deps.beat_args().is_empty());

    deps.proxy_listening.store(true, Ordering::SeqCst);
    clock.advance(Duration::from_secs(1));
    wait_for_sleeps(&clock, 3).await;
    assert_eq!(deps.beat_args(), vec![5]);

    svc.stop().await;
}

#[tokio::test]
async fn stop_interrupts_pacing_sleep_without_clock_advance() {
    let clock = FakeClock::new();
    let deps = FakeDeps::new();
    deps.script.lock().unwrap().push_back(Ok(60)); // pacing = 45s
    let svc = make_svc(deps.clone(), &clock);
    svc.start();
    wait_for_sleeps(&clock, 1).await;
    assert_eq!(clock.sleeps(), vec![Duration::from_secs(45)]);

    // The virtual clock never advances; stop must still return.
    tokio::time::timeout(Duration::from_secs(2), svc.stop())
        .await
        .expect("stop must not wait out the pacing sleep");
    assert!(!svc.is_task_active());
}

#[tokio::test]
async fn start_is_idempotent_while_task_active() {
    let clock = FakeClock::new();
    let deps = FakeDeps::new();
    deps.script.lock().unwrap().extend([Ok(15), Ok(15)]);
    let svc = make_svc(deps.clone(), &clock);
    svc.start();
    svc.start();
    svc.start();

    wait_for_sleeps(&clock, 1).await;
    clock.advance(Duration::from_millis(11_250));
    wait_for_sleeps(&clock, 2).await;
    // Exactly one beat per pacing period despite three starts.
    assert_eq!(deps.beat_args(), vec![5, 15]);

    svc.stop().await;
}
