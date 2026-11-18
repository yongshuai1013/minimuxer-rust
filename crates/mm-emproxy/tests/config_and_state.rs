//! Behavior list §A (wrapper surface) + §C6 + B1 dispatch table.

mod common;

use std::net::UdpSocket;
use std::time::{Duration, Instant};

use mm_emproxy::{forward_emproxy_log, map_start_code, map_stop_code, probe_plan, EmProxy, HandshakeConfig, ProbePlan};
use mm_common::EmProxyError;

fn disabled(proxy: &EmProxy) {
    proxy.set_handshake_client("", 0, false);
}

#[test]
fn a1_config_store_and_replace() {
    let proxy = EmProxy::new();
    assert_eq!(proxy.handshake_config(), None);
    proxy.set_handshake_client("10.7.0.1", 62078, true);
    assert_eq!(
        proxy.handshake_config(),
        Some(HandshakeConfig {
            host: "10.7.0.1".into(),
            port: 62078,
            enabled: true,
        })
    );
    proxy.set_handshake_client("", 0, false);
    assert_eq!(
        proxy.handshake_config(),
        Some(HandshakeConfig {
            host: String::new(),
            port: 0,
            enabled: false,
        })
    );
}

#[tokio::test]
async fn a3_start_without_config_throws() {
    let proxy = EmProxy::new();
    let err = proxy.start("127.0.0.1", 26101).await.unwrap_err();
    assert_eq!(err, EmProxyError::HandshakeClientNotConfigured);
    assert!(!proxy.is_running());
}

#[test]
fn a4_start_code_mapping_table() {
    assert_eq!(map_start_code(0, "h:1"), Ok(()));
    assert_eq!(map_start_code(-1, "h:1"), Err(EmProxyError::InvalidBindAddressPointer));
    assert_eq!(map_start_code(-2, "h:1"), Err(EmProxyError::InvalidUtf8String));
    assert_eq!(
        map_start_code(-3, "h:1"),
        Err(EmProxyError::InvalidSocketAddress("h:1".into()))
    );
    assert_eq!(map_start_code(-4, "h:1"), Err(EmProxyError::SocketBindFailed));
    assert_eq!(map_start_code(-5, "h:1"), Err(EmProxyError::CryptoInitFailed));
    assert_eq!(map_start_code(7, "h:1"), Err(EmProxyError::UnknownError(7)));
    assert_eq!(map_start_code(-99, "h:1"), Err(EmProxyError::UnknownError(-99)));
}

#[test]
fn a6_stop_code_mapping_table() {
    assert_eq!(map_stop_code(0), Ok(()));
    assert_eq!(map_stop_code(-1), Err(EmProxyError::ServerNotRunning));
    assert_eq!(map_stop_code(-2), Err(EmProxyError::StopSignalFailed));
    assert_eq!(map_stop_code(-3), Err(EmProxyError::ThreadJoinFailed));
    assert_eq!(map_stop_code(42), Err(EmProxyError::UnknownError(42)));
}

#[tokio::test]
async fn a4_start_ipv6_host_is_invalid_socket_address() {
    let proxy = EmProxy::new();
    disabled(&proxy);
    let err = proxy.start("::1", 51820).await.unwrap_err();
    assert_eq!(
        err,
        EmProxyError::InvalidSocketAddress("::1:51820".into())
    );
    assert!(!proxy.is_running());
}

#[tokio::test]
async fn a4_start_occupied_port_is_bind_failed() {
    let _guard = common::lock_server();
    let blocker = UdpSocket::bind("127.0.0.1:26102").unwrap();
    let proxy = EmProxy::new();
    disabled(&proxy);
    let err = proxy.start("127.0.0.1", 26102).await.unwrap_err();
    assert_eq!(err, EmProxyError::SocketBindFailed);
    assert!(!proxy.is_running());
    drop(blocker);
}

#[tokio::test]
async fn a2_default_start_binds_51820() {
    let _guard = common::lock_server();
    let proxy = EmProxy::new();
    disabled(&proxy);
    proxy.start_default().await.unwrap();
    assert!(proxy.is_running());
    // The default port is now occupied by the server.
    assert!(UdpSocket::bind("127.0.0.1:51820").is_err());
    proxy.stop().unwrap();
    assert!(!proxy.is_running());
    // And released afterwards.
    let s = UdpSocket::bind("127.0.0.1:51820").unwrap();
    drop(s);
}

#[tokio::test]
async fn a5_start_twice_is_idempotent() {
    let _guard = common::lock_server();
    let proxy = EmProxy::new();
    disabled(&proxy);
    proxy.start("127.0.0.1", 26103).await.unwrap();
    proxy.start("127.0.0.1", 26103).await.unwrap();
    assert!(proxy.is_running());
    proxy.stop().unwrap();
}

#[test]
fn a6_stop_when_not_running() {
    let proxy = EmProxy::new();
    assert_eq!(proxy.stop(), Err(EmProxyError::ServerNotRunning));
}

#[tokio::test]
async fn a7_stop_releases_port_and_restart() {
    let _guard = common::lock_server();
    let proxy = EmProxy::new();
    disabled(&proxy);
    proxy.start("127.0.0.1", 26104).await.unwrap();
    proxy.stop().unwrap();
    let s = UdpSocket::bind("127.0.0.1:26104").unwrap();
    drop(s);
    proxy.start("127.0.0.1", 26104).await.unwrap();
    assert!(proxy.is_running());
    proxy.stop().unwrap();
}

#[tokio::test]
async fn c6_stop_returns_promptly() {
    let _guard = common::lock_server();
    let proxy = EmProxy::new();
    disabled(&proxy);
    proxy.start("127.0.0.1", 26105).await.unwrap();
    let t = Instant::now();
    proxy.stop().unwrap();
    assert!(t.elapsed() < Duration::from_secs(2), "stop took {:?}", t.elapsed());
}

#[test]
fn a8_log_forwarding_levels() {
    use mm_common::logging::LogLevel;
    use std::sync::{Arc, Mutex};
    let captured: Arc<Mutex<Vec<(LogLevel, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink_capture = captured.clone();
    mm_common::logging::set_log_sink(Some(Arc::new(move |level, msg| {
        sink_capture.lock().unwrap().push((level, msg.to_string()));
    })));
    mm_common::logging::set_logging_enabled(true);

    assert_eq!(forward_emproxy_log(0, "tok-a8-zero"), LogLevel::Verbose);
    assert_eq!(forward_emproxy_log(1, "tok-a8-one"), LogLevel::Verbose);
    assert_eq!(forward_emproxy_log(2, "tok-a8-two"), LogLevel::Debug);
    assert_eq!(forward_emproxy_log(3, "tok-a8-three"), LogLevel::Debug);

    mm_common::logging::set_logging_enabled(false);
    mm_common::logging::set_log_sink(None);

    let got = captured.lock().unwrap();
    let mine: Vec<&(LogLevel, String)> = got
        .iter()
        .filter(|(_, m)| m.contains("tok-a8"))
        .collect();
    assert_eq!(mine.len(), 4);
    assert!(mine.iter().all(|(_, m)| m.starts_with("[EMProxy] ")));
    assert_eq!(mine[0].0, LogLevel::Verbose);
    assert_eq!(mine[1].0, LogLevel::Verbose);
    assert_eq!(mine[2].0, LogLevel::Debug);
    assert_eq!(mine[3].0, LogLevel::Debug);
}

#[test]
fn b1_dispatch_table() {
    let cfg = |host: &str, port: u16, enabled: bool| HandshakeConfig {
        host: host.into(),
        port,
        enabled,
    };
    assert_eq!(
        probe_plan(&cfg("10.7.0.1", 62078, true)),
        ProbePlan::Trigger {
            host: "10.7.0.1".into(),
            port: 62078,
        }
    );
    // Whitespace-only host does not count as designated.
    assert_eq!(
        probe_plan(&cfg("   ", 62078, true)),
        ProbePlan::Candidates { port: 62078 }
    );
    // Port 0 falls back to lockdownd's port on the scan path.
    assert_eq!(
        probe_plan(&cfg("", 0, true)),
        ProbePlan::Candidates { port: 62078 }
    );
    // Disabled wins over a non-empty host.
    assert_eq!(
        probe_plan(&cfg("10.7.0.1", 39483, false)),
        ProbePlan::Candidates { port: 39483 }
    );
    assert_eq!(
        probe_plan(&cfg("", 0, false)),
        ProbePlan::Candidates { port: 62078 }
    );
    // The trigger path receives the host untrimmed, as Swift does.
    assert_eq!(
        probe_plan(&cfg(" peer ", 1234, true)),
        ProbePlan::Trigger {
            host: " peer ".into(),
            port: 1234,
        }
    );
}
