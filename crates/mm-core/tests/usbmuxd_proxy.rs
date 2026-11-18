//! UsbmuxdProxyServer + RawPacket tests (INVENTORY 2.7 surface),
//! against a fake gateway — no real device involved.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use mm_core::usbmuxd_proxy::{
    encode_network_address, start_proxy_for_gateway, RawPacket, UsbmuxdProxyServer,
    ZERO_UUID,
};
use mm_gateway::{
    DeviceGateway, GatewayBackend, GatewayCore, GatewayFuture, GatewayProvider, OpenedService,
};
use mm_common::PairingProtocol;
use plist::{Dictionary, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const TEST_UDID: &str = "00008103-000A1B2C3D4E5F6A";
const TEST_BUID: &str = "12345678-1234-1234-1234-1234567890AB";

// MARK: - Fake gateway / provider

struct FakeGateway {
    core: GatewayCore,
}

impl FakeGateway {
    fn with_pairing(bytes: Option<Vec<u8>>) -> Arc<Self> {
        let core = GatewayCore::new();
        core.set_pairing_file_data(bytes);
        Arc::new(Self { core })
    }
}

impl DeviceGateway for FakeGateway {
    fn backend(&self) -> GatewayBackend {
        GatewayBackend::Libimobiledevice
    }
    fn core(&self) -> &GatewayCore {
        &self.core
    }
    fn set_logging(&self, _enabled: bool) {}
    fn start<'a>(
        &'a self,
        _content: &'a str,
        _preferred: Option<PairingProtocol>,
    ) -> GatewayFuture<'a, ()> {
        unimplemented!()
    }
    fn stop<'a>(&'a self) -> GatewayFuture<'a, ()> {
        unimplemented!()
    }
    fn fetch_udid<'a>(&'a self) -> GatewayFuture<'a, String> {
        unimplemented!()
    }
    fn get_lockdown_value<'a>(&'a self, _key: &'a str) -> GatewayFuture<'a, String> {
        unimplemented!()
    }
    fn start_session<'a>(&'a self) -> GatewayFuture<'a, ()> {
        unimplemented!()
    }
    fn stop_session<'a>(&'a self) -> GatewayFuture<'a, ()> {
        unimplemented!()
    }
    fn open_service<'a>(&'a self, _name: &'a str) -> GatewayFuture<'a, OpenedService> {
        unimplemented!()
    }
}

struct FakeProvider {
    gateway: Option<Arc<dyn DeviceGateway>>,
}

impl GatewayProvider for FakeProvider {
    fn current_gateway(&self) -> Option<Arc<dyn DeviceGateway>> {
        self.gateway.clone()
    }
}

fn pairing_bytes(with_udid: bool) -> Vec<u8> {
    let mut d = Dictionary::new();
    d.insert("SystemBUID".into(), Value::String(TEST_BUID.into()));
    if with_udid {
        d.insert("UDID".into(), Value::String(TEST_UDID.into()));
    }
    let mut buf = Vec::new();
    Value::Dictionary(d).to_writer_xml(&mut buf).unwrap();
    buf
}

fn make_proxy(gateway: Option<Arc<dyn DeviceGateway>>) -> UsbmuxdProxyServer {
    UsbmuxdProxyServer::with_bind_addr(
        Arc::new(FakeProvider { gateway }),
        "127.0.0.1:0",
    )
}

// MARK: - Wire helpers

fn request_packet(message_type: &str, tag: u32) -> RawPacket {
    let mut d = Dictionary::new();
    d.insert(
        "MessageType".into(),
        Value::String(message_type.into()),
    );
    let mut buf = Vec::new();
    Value::Dictionary(d).to_writer_xml(&mut buf).unwrap();
    RawPacket::new(buf, tag)
}

async fn send(stream: &mut TcpStream, packet: &RawPacket) {
    stream.write_all(&packet.encode()).await.unwrap();
}

async fn recv(stream: &mut TcpStream) -> RawPacket {
    let mut size_buf = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut size_buf))
        .await
        .expect("response timed out")
        .unwrap();
    let size = u32::from_le_bytes(size_buf) as usize;
    let mut rest = vec![0u8; size - 4];
    stream.read_exact(&mut rest).await.unwrap();
    let mut full = size_buf.to_vec();
    full.extend_from_slice(&rest);
    RawPacket::parse(&full).unwrap()
}

fn packet_dict(packet: &RawPacket) -> Dictionary {
    match Value::from_reader(Cursor::new(packet.payload.as_slice())).unwrap() {
        Value::Dictionary(d) => d,
        other => panic!("expected dict, got {other:?}"),
    }
}

async fn started_proxy(gateway: Option<Arc<dyn DeviceGateway>>) -> (UsbmuxdProxyServer, u16) {
    let proxy = make_proxy(gateway);
    assert!(proxy.start(TEST_UDID).await.unwrap());
    let port = proxy.local_port().unwrap();
    (proxy, port)
}

// MARK: - Codec / layout tests

#[test]
fn raw_packet_roundtrip() {
    let packet = RawPacket::new(b"<plist/>".to_vec(), 42);
    assert_eq!(packet.version, 1);
    assert_eq!(packet.message, 8);
    assert_eq!(packet.size, 16 + 8);
    let parsed = RawPacket::parse(&packet.encode()).unwrap();
    assert_eq!(parsed, packet);
}

#[test]
fn raw_packet_rejects_bad_buffers() {
    assert!(RawPacket::parse(&[0u8; 8]).is_none());
    // Size below the 16-byte header.
    let mut buf = vec![0u8; 16];
    buf[0..4].copy_from_slice(&10u32.to_le_bytes());
    assert!(RawPacket::parse(&buf).is_none());
    // Size above the 4095 cap.
    let mut buf = vec![0u8; 16];
    buf[0..4].copy_from_slice(&5000u32.to_le_bytes());
    assert!(RawPacket::parse(&buf).is_none());
}

#[test]
fn network_address_v4_layout() {
    let data = encode_network_address("10.7.0.1");
    assert_eq!(data.len(), 152);
    assert_eq!(data[0], 16, "sin_len");
    assert_eq!(data[1], 2, "AF_INET");
    assert_eq!(&data[4..8], &[10, 7, 0, 1]);
    assert!(data[8..].iter().all(|b| *b == 0));
}

#[test]
fn network_address_v6_layout() {
    // Global address without a scope suffix: scope id is 0 on
    // every platform (Swift only fills it for %iface or fe80:).
    let data = encode_network_address("2001:db8::1");
    assert_eq!(data.len(), 152);
    assert_eq!(data[0], 28, "sin6_len");
    assert_eq!(data[1], 30, "AF_INET6 (Darwin)");
    let mut expected = [0u8; 16];
    expected[0..4].copy_from_slice(&[0x20, 0x01, 0x0d, 0xb8]);
    expected[15] = 1;
    assert_eq!(&data[8..24], &expected);
    assert_eq!(&data[24..28], &[0, 0, 0, 0]);
}

#[test]
fn network_address_unparseable_is_zero_buffer() {
    let data = encode_network_address("not-an-ip");
    assert_eq!(data.len(), 152);
    assert!(data.iter().all(|b| *b == 0));
}

// MARK: - Server behavior tests

#[tokio::test]
async fn proxy_flow_list_devices_listen_buid_pair_record() {
    let gateway = FakeGateway::with_pairing(Some(pairing_bytes(true)));
    let (proxy, port) = started_proxy(Some(gateway)).await;
    assert!(proxy.is_listening());
    // Second start reports false, like Swift.
    assert!(!proxy.start(TEST_UDID).await.unwrap());

    let mut conn = TcpStream::connect(("127.0.0.1", port)).await.unwrap();

    // No attach yet: empty device list.
    send(&mut conn, &request_packet("ListDevices", 7)).await;
    let resp = recv(&mut conn).await;
    assert_eq!(resp.tag, 7, "response echoes the request tag");
    assert_eq!(resp.version, 1);
    assert_eq!(resp.message, 8);
    let body = packet_dict(&resp);
    let list = body
        .get("DeviceList")
        .and_then(|v| v.as_array())
        .unwrap();
    assert!(list.is_empty());

    // Attach: one device with Swift's exact entry shape.
    proxy.notify_device_attached("10.7.0.1");
    send(&mut conn, &request_packet("ListDevices", 8)).await;
    let resp = recv(&mut conn).await;
    let body = packet_dict(&resp);
    let list = body
        .get("DeviceList")
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(list.len(), 1);
    let entry = list[0].as_dictionary().unwrap();
    assert_eq!(
        entry.get("DeviceID").and_then(|v| v.as_signed_integer()),
        Some(1)
    );
    let props = entry
        .get("Properties")
        .and_then(|v| v.as_dictionary())
        .unwrap();
    assert_eq!(
        props.get("ConnectionType").and_then(|v| v.as_string()),
        Some("Network")
    );
    assert_eq!(
        props.get("EscapedFullServiceName").and_then(|v| v.as_string()),
        Some("00008103-000A1B2C3D4E5F6A._apple-mobdev2._tcp.local")
    );
    assert_eq!(
        props.get("InterfaceIndex").and_then(|v| v.as_signed_integer()),
        Some(0)
    );
    assert_eq!(
        props.get("SerialNumber").and_then(|v| v.as_string()),
        Some(TEST_UDID)
    );
    let addr = props
        .get("NetworkAddress")
        .and_then(|v| v.as_data())
        .unwrap();
    assert_eq!(addr, encode_network_address("10.7.0.1").as_slice());

    // Listen.
    send(&mut conn, &request_packet("Listen", 9)).await;
    let resp = recv(&mut conn).await;
    let body = packet_dict(&resp);
    assert_eq!(
        body.get("MessageType").and_then(|v| v.as_string()),
        Some("Result")
    );
    assert_eq!(body.get("Number").and_then(|v| v.as_signed_integer()), Some(0));

    // ReadBUID reads the gateway's pairing dict.
    send(&mut conn, &request_packet("ReadBUID", 10)).await;
    let resp = recv(&mut conn).await;
    let body = packet_dict(&resp);
    assert_eq!(
        body.get("BUID").and_then(|v| v.as_string()),
        Some(TEST_BUID)
    );

    // ReadPairRecord returns the raw pairing bytes.
    send(&mut conn, &request_packet("ReadPairRecord", 11)).await;
    let resp = recv(&mut conn).await;
    let body = packet_dict(&resp);
    assert_eq!(
        body.get("PairRecordData").and_then(|v| v.as_data()),
        Some(pairing_bytes(true).as_slice())
    );

    // Unknown message type: swallowed — no response, and the
    // connection stays usable (Swift do/catch semantics).
    send(&mut conn, &request_packet("Frobnicate", 12)).await;
    let mut probe = [0u8; 1];
    let silent = tokio::time::timeout(
        Duration::from_millis(300),
        conn.peek(&mut probe),
    )
    .await;
    assert!(silent.is_err(), "unknown type must not be answered");
    send(&mut conn, &request_packet("Listen", 13)).await;
    let resp = recv(&mut conn).await;
    assert_eq!(resp.tag, 13);

    proxy.stop().await;
    assert!(!proxy.is_listening());
}

#[tokio::test]
async fn proxy_read_buid_without_buid_is_zero_uuid() {
    // Pairing dict without SystemBUID -> Swift's zero UUID.
    let mut d = Dictionary::new();
    d.insert("UDID".into(), Value::String(TEST_UDID.into()));
    let mut buf = Vec::new();
    Value::Dictionary(d).to_writer_xml(&mut buf).unwrap();
    let gateway = FakeGateway::with_pairing(Some(buf));
    let (proxy, port) = started_proxy(Some(gateway)).await;

    let mut conn = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    send(&mut conn, &request_packet("ReadBUID", 1)).await;
    let resp = recv(&mut conn).await;
    let body = packet_dict(&resp);
    assert_eq!(
        body.get("BUID").and_then(|v| v.as_string()),
        Some(ZERO_UUID)
    );
    proxy.stop().await;
}

#[tokio::test]
async fn proxy_oversized_packet_closes_connection() {
    let (proxy, port) = started_proxy(None).await;
    let mut conn = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    // Size prefix above the 4095 cap: Swift disconnects.
    conn.write_all(&5000u32.to_le_bytes()).await.unwrap();
    let mut buf = [0u8; 1];
    let n = tokio::time::timeout(Duration::from_secs(5), conn.read(&mut buf))
        .await
        .expect("server should close the connection")
        .unwrap();
    assert_eq!(n, 0, "connection must be closed");
    proxy.stop().await;
}

#[tokio::test]
async fn proxy_stop_and_restart() {
    let (proxy, _port) = started_proxy(None).await;
    proxy.stop().await;
    assert!(!proxy.is_listening());
    assert!(proxy.start(TEST_UDID).await.unwrap());
    assert!(proxy.is_listening());
    proxy.stop().await;
}

#[tokio::test]
async fn orchestration_errors_match_swift() {
    // No pairing dict at all.
    let gateway = FakeGateway::with_pairing(None);
    let proxy = make_proxy(Some(gateway_as_dyn(&gateway)));
    let err = start_proxy_for_gateway(&proxy, &gateway_as_dyn(&gateway))
        .await
        .unwrap_err();
    assert_eq!(err.domain, "MinimuxerError");
    assert_eq!(err.code, "pairingNotLoaded");
    assert_eq!(err.reason, "Pairing dictionary is missing in gateway");

    // Dict present but UDID missing.
    let gateway = FakeGateway::with_pairing(Some(pairing_bytes(false)));
    let proxy = make_proxy(Some(gateway_as_dyn(&gateway)));
    let err = start_proxy_for_gateway(&proxy, &gateway_as_dyn(&gateway))
        .await
        .unwrap_err();
    assert_eq!(err.code, "invalidPairing");
    // The protocol rides inside the reason payload (M1 error-bus
    // encoding for the struct-variant cases).
    assert!(
        err.reason.contains("Pairing file is missing UDID value"),
        "unexpected reason: {}",
        err.reason
    );
}

#[tokio::test]
async fn orchestration_starts_proxy_for_valid_gateway() {
    let gateway = FakeGateway::with_pairing(Some(pairing_bytes(true)));
    let proxy = make_proxy(Some(gateway_as_dyn(&gateway)));
    start_proxy_for_gateway(&proxy, &gateway_as_dyn(&gateway))
        .await
        .unwrap();
    assert!(proxy.is_listening());
    proxy.stop().await;
}

fn gateway_as_dyn(gateway: &Arc<FakeGateway>) -> Arc<dyn DeviceGateway> {
    gateway.clone()
}
