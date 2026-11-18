//! Behavior list §B: the post-start handshake probe, exercised
//! through `EmProxy::start` with shortened `ProbeParams`.

mod common;

use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mm_emproxy::{CandidateProvider, EmProxy, ProbeParams};

fn params(total_ms: u64) -> ProbeParams {
    ProbeParams {
        attempt_timeout: Duration::from_millis(200),
        retry_interval: Duration::from_millis(50),
        total_timeout: Duration::from_millis(total_ms),
    }
}

fn proxy_with(candidates: Option<CandidateProvider>, total_ms: u64) -> EmProxy {
    EmProxy::with_options(candidates, params(total_ms))
}

#[tokio::test]
async fn b2_trigger_succeeds_on_listener() {
    let _guard = common::lock_server();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let target = listener.local_addr().unwrap().port();
    let proxy = proxy_with(None, 5_000);
    proxy.set_handshake_client("127.0.0.1", target, true);
    let t = Instant::now();
    proxy.start("127.0.0.1", 26201).await.unwrap();
    assert!(t.elapsed() < Duration::from_secs(3), "took {:?}", t.elapsed());
    assert!(proxy.is_running());
    proxy.stop().unwrap();
    drop(listener);
}

#[tokio::test]
async fn b2_trigger_refused_counts_as_success() {
    let _guard = common::lock_server();
    let target = common::closed_tcp_port();
    let proxy = proxy_with(None, 5_000);
    proxy.set_handshake_client("127.0.0.1", target, true);
    let t = Instant::now();
    proxy.start("127.0.0.1", 26202).await.unwrap();
    // ECONNREFUSED is a probe success (behavior B4): the first
    // attempt already ends the probe.
    assert!(t.elapsed() < Duration::from_secs(3), "took {:?}", t.elapsed());
    proxy.stop().unwrap();
}

#[tokio::test]
async fn b2_trigger_times_out_without_throwing() {
    let _guard = common::lock_server();
    let proxy = proxy_with(None, 600);
    // Unroutable RFC1918: every attempt fails; the probe must run
    // out its total budget and the start must still succeed.
    proxy.set_handshake_client("10.255.255.1", 62078, true);
    let t = Instant::now();
    proxy.start("127.0.0.1", 26203).await.unwrap();
    let elapsed = t.elapsed();
    assert!(elapsed >= Duration::from_millis(500), "probe gave up early: {elapsed:?}");
    assert!(elapsed < Duration::from_secs(5), "probe overran: {elapsed:?}");
    assert!(proxy.is_running());
    proxy.stop().unwrap();
}

#[tokio::test]
async fn b3_probe_path_uses_injected_candidates() {
    let _guard = common::lock_server();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let target = listener.local_addr().unwrap().port();
    let provider: CandidateProvider = Arc::new(|| vec!["127.0.0.1".to_string()]);
    let proxy = proxy_with(Some(provider), 5_000);
    // Disabled + empty host → candidate path on the configured port.
    proxy.set_handshake_client("", target, false);
    let t = Instant::now();
    proxy.start("127.0.0.1", 26204).await.unwrap();
    assert!(t.elapsed() < Duration::from_secs(3), "took {:?}", t.elapsed());
    proxy.stop().unwrap();
    drop(listener);
}

#[tokio::test]
async fn b3_probe_path_no_candidates_skips_fast() {
    let _guard = common::lock_server();
    let provider: CandidateProvider = Arc::new(Vec::new);
    // A long total budget: if the skip were broken, start would
    // sit in the probe for the full 5 s.
    let proxy = proxy_with(Some(provider), 5_000);
    proxy.set_handshake_client("", 0, false);
    let t = Instant::now();
    proxy.start("127.0.0.1", 26205).await.unwrap();
    assert!(t.elapsed() < Duration::from_secs(2), "skip failed: {:?}", t.elapsed());
    proxy.stop().unwrap();
}

#[tokio::test]
async fn b4_probe_host_outcomes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let open = listener.local_addr().unwrap().port();
    let proxy = proxy_with(None, 1_000);
    assert!(proxy.probe_host("127.0.0.1", open).await, "connected");
    assert!(
        proxy.probe_host("127.0.0.1", common::closed_tcp_port()).await,
        "refused counts as success"
    );
    assert!(
        !proxy.probe_host("10.255.255.1", 62078).await,
        "unroutable fails"
    );
    drop(listener);
}
