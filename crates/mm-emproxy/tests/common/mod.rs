//! Shared helpers for mm-emproxy integration tests.

use std::net::TcpListener;
use std::sync::Mutex;

/// Serializes tests that run a real em_proxy server inside one
/// test binary (behavior list §F). Ports are still distinct per
/// test so parallel test *binaries* never collide.
pub static SERVER_LOCK: Mutex<()> = Mutex::new(());

pub fn lock_server() -> std::sync::MutexGuard<'static, ()> {
    SERVER_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether this host lets an unconnected UDP socket reply to a
/// loopback peer (`send_to` from a bound server socket). The
/// Linux sandbox used for local runs denies unconnected UDP
/// sends with EPERM, which makes the em_proxy server's response
/// path unreachable there; macOS CI has no such restriction.
/// Probed once with a real socket pair.
pub fn udp_replies_work() -> bool {
    use std::net::UdpSocket;
    use std::sync::OnceLock;
    static WORKS: OnceLock<bool> = OnceLock::new();
    *WORKS.get_or_init(|| {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        server
            .set_read_timeout(Some(std::time::Duration::from_millis(500)))
            .unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_millis(500)))
            .unwrap();
        client.connect(server.local_addr().unwrap()).unwrap();
        if client.send(b"ping").is_err() {
            return false;
        }
        let mut buf = [0u8; 16];
        match server.recv_from(&mut buf) {
            Ok((_, ep)) => {
                if server.send_to(b"pong", ep).is_err() {
                    return false;
                }
            }
            Err(_) => return false,
        }
        client.recv(&mut buf).is_ok()
    })
}

/// A TCP port that is closed by the time this returns (bound,
/// read, dropped): loopback connects to it refuse immediately.
pub fn closed_tcp_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let p = l.local_addr().unwrap().port();
    drop(l);
    p
}
