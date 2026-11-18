//! TCP probe against local listeners: positive and negative cases.

use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::thread;

fn spawn_acceptor(listener: TcpListener) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(mut s) => {
                    let mut buf = [0u8; 1];
                    let _ = s.read(&mut buf);
                }
                Err(_) => break,
            }
        }
    })
}

#[test]
fn probe_open_port_is_true() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let _h = spawn_acceptor(listener);
    assert!(mm_common::probe::test_tcp("127.0.0.1", port, 1000));
    assert!(mm_common::probe::test_tcp_default("127.0.0.1", port));
    let _ = TcpStream::connect(("127.0.0.1", port));
}

#[test]
fn probe_closed_port_is_false() {
    // Bind then immediately drop: the port is closed afterwards.
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let p = l.local_addr().unwrap().port();
        drop(l);
        p
    };
    assert!(!mm_common::probe::test_tcp("127.0.0.1", port, 500));
}

#[test]
fn probe_rejects_bad_addresses() {
    // CIDR notation is rejected outright (Swift semantics) -- this is the
    // 10.7.0.0/24 shape seen in the field.
    assert!(!mm_common::probe::test_tcp("10.7.0.0/24", 62078, 100));
    assert!(!mm_common::probe::test_tcp("", 62078, 100));
    assert!(!mm_common::probe::test_tcp("999.1.1.1", 62078, 100));
    assert!(!mm_common::probe::test_tcp("not-an-ip", 62078, 100));
}

#[test]
fn probe_ipv6_loopback() {
    let listener = match TcpListener::bind("[::1]:0") {
        Ok(l) => l,
        Err(_) => return, // IPv6 unavailable on this host; nothing to test.
    };
    let port = listener.local_addr().unwrap().port();
    let _h = spawn_acceptor(listener);
    assert!(mm_common::probe::test_tcp("::1", port, 1000));
}
