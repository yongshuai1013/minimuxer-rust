//! M8 host tests for the wireless pair-setup wiring
//! (behavior list `docs/m8-wireless-behavior.md` G2/G3
//! and the error shapes of G5/G6). The SRP wire flow
//! itself is idevice68's (verdict D-M8-1) and is
//! real-device territory; what is verified here is the
//! preparation content, the acceptor, and the failure
//! shapes of the wiring layer.

use mm_rp::idevice68::remote_pairing::PeerDevice;
use mm_rp::{pair_network, PairHostPreparation, TcpAcceptor};
use tokio::net::TcpStream;

/// Behavior G2: prepare's service id is the pairing
/// file identifier — UUIDv3(DNS, host name), the same
/// value the FFI's `pairable_host_prepare` returns.
#[test]
fn prepare_service_id_is_uuidv3_of_host_name() {
    let prep = PairHostPreparation::prepare("SideStore", "Mac17,7");
    assert_eq!(prep.service_id(), "01c72866-bd42-3c7d-bf6c-fe55dc1e5ae9");
    // Deterministic across preparations (the identity
    // is a pure function of the host name).
    let again = PairHostPreparation::prepare("SideStore", "Mac17,7");
    assert_eq!(again.service_id(), prep.service_id());
}

/// Behavior G2: the TXT record set and values, and the
/// authTag's consistency with the host alt-IRK (the
/// pairing file's own validation entry point).
#[test]
fn prepare_txt_records_match_ffi_shape() {
    let prep = PairHostPreparation::prepare("SideStore", "Mac17,7");
    let txt = prep.txt_records();
    let get = |k: &str| {
        txt.iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(txt.len(), 7);
    assert_eq!(get("name"), Some("SideStore"));
    assert_eq!(get("model"), Some("Mac17,7"));
    assert_eq!(get("identifier"), Some(prep.service_id()));
    assert_eq!(get("flags"), Some("1"));
    assert_eq!(get("ver"), Some("26"));
    assert_eq!(get("minVer"), Some("17"));
    let auth_tag = get("authTag").expect("authTag present");
    assert!(
        PeerDevice::validate_auth_tag(&prep.host_alt_irk(), prep.service_id(), auth_tag),
        "authTag must validate against the host alt-IRK"
    );
    // The alt-IRK is random per preparation (a fresh
    // identity every call, FFI semantics).
    let other = PairHostPreparation::prepare("SideStore", "Mac17,7");
    assert_ne!(other.host_alt_irk(), prep.host_alt_irk());
    assert_eq!(prep.host_alt_irk().len(), 16);
}

/// Behavior G3: the acceptor binds an ephemeral port
/// and a loopback connection is accepted.
#[tokio::test]
async fn acceptor_binds_ephemeral_and_accepts_loopback() {
    let acceptor = TcpAcceptor::bind().await.expect("bind");
    assert_ne!(acceptor.port(), 0);
    let client = TcpStream::connect(("127.0.0.1", acceptor.port()))
        .await
        .expect("connect");
    let accepted = acceptor.accept().await.expect("accept");
    drop(accepted);
    drop(client);
}

/// Behavior G5 (wiring shape): a peer that connects and
/// immediately vanishes makes the responder handshake
/// fail — the gateway maps any such failure to
/// "Pairing failed or cancelled".
#[tokio::test]
async fn accept_fails_when_peer_vanishes() {
    let acceptor = TcpAcceptor::bind().await.expect("bind");
    let client = TcpStream::connect(("127.0.0.1", acceptor.port()))
        .await
        .expect("connect");
    let stream = acceptor.accept().await.expect("accept");
    drop(client);
    let prep = PairHostPreparation::prepare("SideStore", "Mac17,7");
    let result = prep.accept(stream, |_pin| async {}).await;
    assert!(result.is_err(), "dead peer must fail the handshake");
}

/// Behavior G6 (wiring shape): pair_network against a
/// port with no listener fails at the connect stage.
#[tokio::test]
async fn pair_network_refused_is_connect_stage() {
    // Bind then drop a listener to obtain a closed port.
    let acceptor = TcpAcceptor::bind().await.expect("bind");
    let port = acceptor.port();
    drop(acceptor);
    let mut file =
        mm_rp::idevice68::remote_pairing::RpPairingFile::generate("SideStore");
    let target = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let err = pair_network(target, "SideStore", &mut file, || async {
        "000000".to_string()
    })
    .await
    .expect_err("no listener must fail");
    assert_eq!(err.stage, "connect");
}
