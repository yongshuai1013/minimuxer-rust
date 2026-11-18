//! Behavior list §C: the em_proxy WireGuard server, driven by a
//! Rust transcription of `EMProxyTests`' Test-2 client
//! (`testEMProxyWireGuardServerHandshake_v040`). As in the Swift
//! file, BLAKE2s is implemented locally (RFC 7693) because the
//! platform crypto stack lacks it.

mod common;

use std::net::UdpSocket;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use mm_emproxy::EmProxy;
use x25519_dalek::{PublicKey, StaticSecret};

const CLIENT_PRIV_B64: &str = "AIIeeUDvk3NeAFJ9BWCQvPJize/9WZibMnGJ/0rt5k4=";
const SERVER_PUB_B64: &str = "kHDoekeYhBvfW9a9UQ+UCmpbG423eejTjcjW+DT+JF0=";

// ---------------------------------------------------------------------
// BLAKE2s (RFC 7693), transcribed from EMProxyTests.swift.
// ---------------------------------------------------------------------

const IV: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];
const SIGMA: [[usize; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

struct Blake2s {
    h: [u32; 8],
    t: [u32; 2],
    f: [u32; 2],
    buf: [u8; 64],
    buflen: usize,
    outlen: usize,
}

impl Blake2s {
    fn new(key: &[u8], outlen: usize) -> Self {
        let mut s = Self {
            h: IV,
            t: [0, 0],
            f: [0, 0],
            buf: [0; 64],
            buflen: 0,
            outlen,
        };
        s.h[0] ^= 0x01010000 ^ ((key.len() as u32) << 8) ^ (outlen as u32);
        if !key.is_empty() {
            let mut block = [0u8; 64];
            block[..key.len()].copy_from_slice(key);
            s.update(&block);
        }
        s
    }

    fn increment_counter(&mut self, inc: u32) {
        self.t[0] = self.t[0].wrapping_add(inc);
        if self.t[0] < inc {
            self.t[1] = self.t[1].wrapping_add(1);
        }
    }

    fn compress(&mut self, is_last: bool) {
        if is_last {
            self.f[0] = 0xFFFFFFFF;
        }
        let mut m = [0u32; 16];
        for j in 0..16 {
            m[j] = u32::from_le_bytes([
                self.buf[j * 4],
                self.buf[j * 4 + 1],
                self.buf[j * 4 + 2],
                self.buf[j * 4 + 3],
            ]);
        }
        let mut v = [0u32; 16];
        v[..8].copy_from_slice(&self.h);
        v[8..].copy_from_slice(&IV);
        v[12] ^= self.t[0];
        v[13] ^= self.t[1];
        v[14] ^= self.f[0];
        v[15] ^= self.f[1];
        fn g(v: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, x: u32, y: u32) {
            v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
            v[d] = (v[d] ^ v[a]).rotate_right(16);
            v[c] = v[c].wrapping_add(v[d]);
            v[b] = (v[b] ^ v[c]).rotate_right(12);
            v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
            v[d] = (v[d] ^ v[a]).rotate_right(8);
            v[c] = v[c].wrapping_add(v[d]);
            v[b] = (v[b] ^ v[c]).rotate_right(7);
        }
        for r in 0..10 {
            let s = &SIGMA[r];
            g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
            g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
            g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
            g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
            g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
            g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
            g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
            g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
        }
        for j in 0..8 {
            self.h[j] ^= v[j] ^ v[j + 8];
        }
    }

    fn update(&mut self, data: &[u8]) {
        for &b in data {
            if self.buflen == 64 {
                self.increment_counter(64);
                self.compress(false);
                self.buf = [0; 64];
                self.buflen = 0;
            }
            self.buf[self.buflen] = b;
            self.buflen += 1;
        }
    }

    fn finalize(mut self) -> Vec<u8> {
        self.increment_counter(self.buflen as u32);
        while self.buflen < 64 {
            self.buf[self.buflen] = 0;
            self.buflen += 1;
        }
        self.compress(true);
        let mut result = Vec::with_capacity(32);
        for word in self.h {
            result.extend_from_slice(&word.to_le_bytes());
        }
        result.truncate(self.outlen);
        result
    }
}

fn blake2s_hash(data: &[u8]) -> Vec<u8> {
    let mut s = Blake2s::new(&[], 32);
    s.update(data);
    s.finalize()
}

fn blake2s_keyed(key: &[u8], data: &[u8], outlen: usize) -> Vec<u8> {
    let mut s = Blake2s::new(key, outlen);
    s.update(data);
    s.finalize()
}

/// Swift `BLAKE2s.hmac(key:data1:data2:)` — HMAC over the
/// concatenation of the data parts.
fn blake2s_hmac(key: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    let mut k = key.to_vec();
    if k.len() > 64 {
        k = blake2s_hash(&k);
    }
    k.resize(64, 0);
    let ikey: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let okey: Vec<u8> = k.iter().map(|b| b ^ 0x5C).collect();
    let mut inner = ikey;
    for p in parts {
        inner.extend_from_slice(p);
    }
    let inner_hash = blake2s_hash(&inner);
    let mut outer = okey;
    outer.extend_from_slice(&inner_hash);
    blake2s_hash(&outer)
}

/// Swift `BLAKE2s.hkdf2(ck:input:)`.
fn hkdf2(ck: &[u8], input: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let temp = blake2s_hmac(ck, &[input]);
    let new_ck = blake2s_hmac(&temp, &[&[0x01]]);
    let new_key = blake2s_hmac(&temp, &[&new_ck, &[0x02]]);
    (new_ck, new_key)
}

fn seal(key: &[u8], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .encrypt(Nonce::from_slice(&[0u8; 12]), Payload { msg: plaintext, aad })
        .expect("seal")
}

fn open(key: &[u8], aad: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(Nonce::from_slice(&[0u8; 12]), Payload { msg: ciphertext, aad })
        .ok()
}

// ---------------------------------------------------------------------
// WireGuard client (Test-2 transcription).
// ---------------------------------------------------------------------

const INITIAL_CHAIN_KEY: [u8; 32] = [
    96, 226, 109, 174, 243, 39, 239, 192, 46, 195, 53, 226, 160, 37, 210, 208, 22, 235, 66, 6, 248,
    114, 119, 245, 45, 56, 209, 152, 139, 120, 205, 54,
];
const INITIAL_CHAIN_HASH: [u8; 32] = [
    34, 17, 179, 97, 8, 26, 197, 102, 105, 18, 67, 219, 69, 138, 213, 50, 45, 156, 108, 102, 34,
    147, 232, 183, 14, 225, 156, 101, 186, 7, 158, 243,
];

fn b64(s: &str) -> Vec<u8> {
    B64.decode(s).unwrap()
}

fn client_priv() -> StaticSecret {
    StaticSecret::from(<[u8; 32]>::try_from(b64(CLIENT_PRIV_B64)).unwrap())
}

fn server_pub() -> PublicKey {
    PublicKey::from(<[u8; 32]>::try_from(b64(SERVER_PUB_B64)).unwrap())
}

fn tai64n_now() -> [u8; 12] {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let tai_seconds = now.as_secs().wrapping_add(4611686018427387904);
    let mut out = [0u8; 12];
    out[..8].copy_from_slice(&tai_seconds.to_be_bytes());
    out[8..].copy_from_slice(&now.subsec_nanos().to_be_bytes());
    out
}

struct NoiseState {
    chaining_key: Vec<u8>,
    hash: Vec<u8>,
    ephemeral_priv: StaticSecret,
}

/// Swift `makeWireGuardHandshakeInitiationPacket`. Uses the
/// all-zero ephemeral private key, exactly like the Swift test.
fn make_initiation(client_priv: &StaticSecret) -> (Vec<u8>, NoiseState) {
    let server = server_pub();
    let server_raw = server.as_bytes();
    let mut ck = INITIAL_CHAIN_KEY.to_vec();
    let mut hash_in = INITIAL_CHAIN_HASH.to_vec();
    hash_in.extend_from_slice(server_raw);
    let mut hash = blake2s_hash(&hash_in);

    let ephemeral_priv = StaticSecret::from([0u8; 32]);
    let ephemeral_pub = PublicKey::from(&ephemeral_priv);
    let ephemeral_raw = ephemeral_pub.as_bytes();

    let mut packet = vec![1, 0, 0, 0, 1, 0, 0, 0];
    packet.extend_from_slice(ephemeral_raw);

    let mut h_in = hash.clone();
    h_in.extend_from_slice(ephemeral_raw);
    hash = blake2s_hash(&h_in);

    let temp0 = blake2s_hmac(&ck, &[ephemeral_raw]);
    ck = blake2s_hmac(&temp0, &[&[0x01]]);

    let dh1 = ephemeral_priv.diffie_hellman(&server);
    let (new_ck, key1) = hkdf2(&ck, dh1.as_bytes());
    ck = new_ck;
    let client_pub = PublicKey::from(client_priv);
    let enc_static = seal(&key1, &hash, client_pub.as_bytes());
    packet.extend_from_slice(&enc_static);
    let mut h_in = hash.clone();
    h_in.extend_from_slice(&enc_static);
    hash = blake2s_hash(&h_in);

    let dh2 = client_priv.diffie_hellman(&server);
    let (new_ck, key2) = hkdf2(&ck, dh2.as_bytes());
    ck = new_ck;
    let timestamp = tai64n_now();
    let enc_ts = seal(&key2, &hash, &timestamp);
    packet.extend_from_slice(&enc_ts);
    let mut h_in = hash.clone();
    h_in.extend_from_slice(&enc_ts);
    hash = blake2s_hash(&h_in);

    let mut mac_key_in = b"mac1----".to_vec();
    mac_key_in.extend_from_slice(server_raw);
    let mac1_key = blake2s_hash(&mac_key_in);
    let mac1 = blake2s_keyed(&mac1_key, &packet, 16);
    packet.extend_from_slice(&mac1);
    packet.extend_from_slice(&[0u8; 16]);
    assert_eq!(packet.len(), 148);
    (
        packet,
        NoiseState {
            chaining_key: ck,
            hash,
            ephemeral_priv,
        },
    )
}

struct Session {
    server_index: [u8; 4],
    sending_key: Vec<u8>,
    receiving_key: Vec<u8>,
}

/// Swift `verifyWireGuardHandshakeResponse` (+ session key
/// derivation for transport packets).
fn finish_handshake(resp: &[u8], state: NoiseState, client_priv: &StaticSecret) -> Session {
    assert!(resp.len() >= 60, "response too short: {}", resp.len());
    let server_index = <[u8; 4]>::try_from(&resp[4..8]).unwrap();
    let server_eph = PublicKey::from(<[u8; 32]>::try_from(&resp[12..44]).unwrap());
    let enc_nothing = &resp[44..60];
    let server_eph_raw = server_eph.as_bytes();

    let mut ck = state.chaining_key;
    let mut h_in = state.hash.clone();
    h_in.extend_from_slice(server_eph_raw);
    let mut hash = blake2s_hash(&h_in);

    let temp = blake2s_hmac(&ck, &[server_eph_raw]);
    ck = blake2s_hmac(&temp, &[&[0x01]]);
    let dh_eph = state.ephemeral_priv.diffie_hellman(&server_eph);
    let temp1 = blake2s_hmac(&ck, &[dh_eph.as_bytes()]);
    ck = blake2s_hmac(&temp1, &[&[0x01]]);
    let dh_static = client_priv.diffie_hellman(&server_eph);
    let temp2 = blake2s_hmac(&ck, &[dh_static.as_bytes()]);
    ck = blake2s_hmac(&temp2, &[&[0x01]]);
    let temp_psk = blake2s_hmac(&ck, &[&[0u8; 32]]);
    ck = blake2s_hmac(&temp_psk, &[&[0x01]]);
    let temp3 = blake2s_hmac(&temp_psk, &[&ck, &[0x02]]);
    let key = blake2s_hmac(&temp_psk, &[&temp3, &[0x03]]);
    let mut h_in = hash.clone();
    h_in.extend_from_slice(&temp3);
    hash = blake2s_hash(&h_in);
    let opened = open(&key, &hash, enc_nothing).expect("encrypted-nothing must open");
    assert!(opened.is_empty());

    let temp_session = blake2s_hmac(&ck, &[&[]]);
    let sending_key = blake2s_hmac(&temp_session, &[&[0x01]]);
    let receiving_key = blake2s_hmac(&temp_session, &[&sending_key, &[0x02]]);
    Session {
        server_index,
        sending_key,
        receiving_key,
    }
}

/// Swift `makeWireGuardTransportPacket` shape (type 4).
fn transport_packet(receiver: [u8; 4], counter: u64, key: &[u8], inner: &[u8]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce = [0u8; 12];
    nonce[4..].copy_from_slice(&counter.to_le_bytes());
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: inner, aad: &[] })
        .expect("transport seal");
    let mut packet = vec![4, 0, 0, 0];
    packet.extend_from_slice(&receiver);
    packet.extend_from_slice(&counter.to_le_bytes());
    packet.extend_from_slice(&ct);
    packet
}

fn open_transport(packet: &[u8], key: &[u8]) -> Vec<u8> {
    assert_eq!(&packet[..4], &[4, 0, 0, 0]);
    let counter = u64::from_le_bytes(<[u8; 8]>::try_from(&packet[8..16]).unwrap());
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce = [0u8; 12];
    nonce[4..].copy_from_slice(&counter.to_le_bytes());
    cipher
        .decrypt(Nonce::from_slice(&nonce), Payload { msg: &packet[16..], aad: &[] })
        .expect("transport open")
}

// ---------------------------------------------------------------------
// Server / UDP plumbing and the §C tests.
// ---------------------------------------------------------------------

fn start_server(port: u16) -> EmProxy {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let proxy = EmProxy::new();
    proxy.set_handshake_client("", 0, false);
    rt.block_on(proxy.start("127.0.0.1", port)).unwrap();
    proxy
}

fn udp_client(port: u16, timeout: Duration) -> UdpSocket {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    sock.connect(("127.0.0.1", port)).unwrap();
    sock.set_read_timeout(Some(timeout)).unwrap();
    sock
}

fn recv_packet(sock: &UdpSocket) -> Option<Vec<u8>> {
    let mut buf = [0u8; 2048];
    match sock.recv(&mut buf) {
        Ok(n) => Some(buf[..n].to_vec()),
        Err(_) => None,
    }
}

fn do_handshake(sock: &UdpSocket) -> Session {
    let priv_key = client_priv();
    let (initiation, state) = make_initiation(&priv_key);
    sock.send(&initiation).unwrap();
    let resp = recv_packet(sock).expect("handshake response");
    finish_handshake(&resp, state, &priv_key)
}

#[test]
fn c1_wireguard_handshake_completes() {
    let _guard = common::lock_server();
    if !common::udp_replies_work() {
        eprintln!("SKIP: this host blocks unconnected UDP replies (sandbox EPERM); the full assertions run on macOS CI");
        return;
    }
    let proxy = start_server(26301);
    let sock = udp_client(26301, Duration::from_secs(2));
    let session = do_handshake(&sock);
    assert_ne!(session.server_index, [0, 0, 0, 0]);
    proxy.stop().unwrap();
}

#[test]
fn c2_keepalive_accepted_and_server_stays_up() {
    let _guard = common::lock_server();
    if !common::udp_replies_work() {
        eprintln!("SKIP: this host blocks unconnected UDP replies (sandbox EPERM); the full assertions run on macOS CI");
        return;
    }
    let proxy = start_server(26302);
    let sock = udp_client(26302, Duration::from_secs(2));
    let session = do_handshake(&sock);
    let keepalive = transport_packet(session.server_index, 0, &session.sending_key, &[]);
    assert_eq!(keepalive.len(), 32);
    sock.send(&keepalive).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(proxy.is_running());
    proxy.stop().unwrap();
}

#[test]
fn c3_wrong_client_key_gets_no_response() {
    let _guard = common::lock_server();
    let proxy = start_server(26303);
    let sock = udp_client(26303, Duration::from_millis(1500));
    let wrong_priv = StaticSecret::from([9u8; 32]);
    let (initiation, _state) = make_initiation(&wrong_priv);
    sock.send(&initiation).unwrap();
    assert!(
        recv_packet(&sock).is_none(),
        "server must not answer an initiation for an unknown client key"
    );
    proxy.stop().unwrap();
}

#[test]
fn c5_garbage_datagram_then_valid_initiation() {
    let _guard = common::lock_server();
    if !common::udp_replies_work() {
        eprintln!("SKIP: this host blocks unconnected UDP replies (sandbox EPERM); the full assertions run on macOS CI");
        return;
    }
    let proxy = start_server(26305);
    let sock = udp_client(26305, Duration::from_secs(2));
    sock.send(&[0xABu8; 64]).unwrap();
    do_handshake(&sock);
    proxy.stop().unwrap();
}

fn ipv4_packet(src: [u8; 4], dst: [u8; 4], payload: &[u8]) -> Vec<u8> {
    let total = (20 + payload.len()) as u16;
    let mut p = vec![
        0x45, 0, // version/IHL, DSCP
    ];
    p.extend_from_slice(&total.to_be_bytes());
    p.extend_from_slice(&[0, 0, 0, 0]); // id, flags/fragment
    p.extend_from_slice(&[64, 253]); // TTL, protocol (experimental)
    p.extend_from_slice(&[0, 0]); // checksum, filled below
    p.extend_from_slice(&src);
    p.extend_from_slice(&dst);
    // One's-complement header checksum.
    let mut sum: u32 = 0;
    for pair in p[..20].chunks_exact(2) {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let csum = !(sum as u16);
    p[10..12].copy_from_slice(&csum.to_be_bytes());
    p.extend_from_slice(payload);
    p
}

#[test]
fn c4_inner_ipv4_is_looped_back_with_swapped_addresses() {
    let _guard = common::lock_server();
    if !common::udp_replies_work() {
        eprintln!("SKIP: this host blocks unconnected UDP replies (sandbox EPERM); the full assertions run on macOS CI");
        return;
    }
    let proxy = start_server(26304);
    let sock = udp_client(26304, Duration::from_secs(2));
    let session = do_handshake(&sock);
    let payload = b"mm-m6-c4";
    let inner = ipv4_packet([10, 7, 0, 10], [10, 7, 0, 1], payload);
    let packet = transport_packet(session.server_index, 0, &session.sending_key, &inner);
    sock.send(&packet).unwrap();
    let resp = recv_packet(&sock).expect("looped-back transport packet");
    // The reply is addressed to our sender index (1).
    assert_eq!(&resp[4..8], &[1, 0, 0, 0]);
    let opened = open_transport(&resp, &session.receiving_key);
    // WriteToTunnelV4 swaps the IPv4 source and destination.
    assert_eq!(&opened[12..16], &[10, 7, 0, 1], "src after swap");
    assert_eq!(&opened[16..20], &[10, 7, 0, 10], "dst after swap");
    assert_eq!(&opened[20..], payload);
    proxy.stop().unwrap();
}
