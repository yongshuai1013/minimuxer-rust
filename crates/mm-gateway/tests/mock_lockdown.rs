//! Integration tests for the lockdown basic chain against a mock
//! lockdownd listening on 127.0.0.1:62078 (the gateway always dials
//! the constant lockdownd port, like Swift's
//! `performWithTcpService`). The mock speaks the real wire protocol
//! (4-byte big-endian length + XML plist), answers StartSession
//! with `EnableSessionSSL: true` and upgrades the connection to
//! TLS — the idevice crate's client verifier accepts any server
//! certificate, and the fixture pairing file carries real
//! (test-generated) certificates, so the full session path runs
//! end to end.
//!
//! All tests share one mock (bound once) and serialize on
//! `TEST_LOCK`, switching the mock's behavior per test.

use std::collections::HashSet;
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use mm_gateway::{DeviceGateway, IdeviceGateway};
use mm_common::PairingProtocol;
use plist::{Dictionary, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

const FIXTURE_UDID: &str = "00008103-000A1B2C3D4E5F6A";
const LOCKDOWND_ADDR: &str = "127.0.0.1:62078";

#[derive(Clone, Copy, PartialEq, Eq)]
enum SessionBehavior {
    Accept,
    InvalidHostId,
}

struct MockState {
    session: SessionBehavior,
    rejected_services: HashSet<String>,
}

struct Mock {
    state: Arc<Mutex<MockState>>,
    service_port: u16,
    service_accepts: Arc<AtomicUsize>,
}

static MOCK: OnceLock<Mock> = OnceLock::new();
static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn pairing_content() -> String {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/pairing.plist"
    ))
    .unwrap();
    String::from_utf8(bytes).unwrap()
}

fn tls_acceptor() -> TlsAcceptor {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cert_bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/server.crt"
    ))
    .unwrap();
    let key_bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/server.key"
    ))
    .unwrap();
    use rustls::pki_types::pem::PemObject;
    let cert = rustls::pki_types::CertificateDer::from_pem_slice(&cert_bytes).unwrap();
    let key = rustls::pki_types::PrivateKeyDer::from_pem_slice(&key_bytes).unwrap();
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    TlsAcceptor::from(Arc::new(config))
}

/// The connection starts plain and upgrades to TLS after
/// StartSession — an enum keeps both phases on one type.
enum MockStream {
    Plain(TcpStream),
    Tls(tokio_rustls::server::TlsStream<TcpStream>),
}

impl MockStream {
    fn into_plain(self) -> TcpStream {
        match self {
            MockStream::Plain(s) => s,
            MockStream::Tls(_) => panic!("already upgraded"),
        }
    }
}

impl AsyncRead for MockStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            MockStream::Plain(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            MockStream::Tls(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for MockStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            MockStream::Plain(s) => std::pin::Pin::new(s).poll_write(cx, data),
            MockStream::Tls(s) => std::pin::Pin::new(s).poll_write(cx, data),
        }
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            MockStream::Plain(s) => std::pin::Pin::new(s).poll_flush(cx),
            MockStream::Tls(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            MockStream::Plain(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            MockStream::Tls(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

async fn read_frame(stream: &mut MockStream) -> Option<Dictionary> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await.ok()?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > 1_000_000 {
        return None;
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await.ok()?;
    match Value::from_reader(Cursor::new(body.as_slice())) {
        Ok(Value::Dictionary(d)) => Some(d),
        _ => None,
    }
}

async fn write_frame(stream: &mut MockStream, dict: Dictionary) {
    let mut buf = Vec::new();
    Value::Dictionary(dict)
        .to_writer_xml(&mut buf)
        .unwrap();
    let len = (buf.len() as u32).to_be_bytes();
    let _ = stream.write_all(&len).await;
    let _ = stream.write_all(&buf).await;
    let _ = stream.flush().await;
}

fn dict_of(pairs: &[(&str, Value)]) -> Dictionary {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

async fn handle_conn(
    socket: TcpStream,
    state: Arc<Mutex<MockState>>,
    acceptor: TlsAcceptor,
    service_port: u16,
) {
    let mut stream = Some(MockStream::Plain(socket));
    loop {
        let req = match read_frame(stream.as_mut().unwrap()).await {
            Some(d) => d,
            None => return,
        };
        let request = req
            .get("Request")
            .and_then(|v| v.as_string())
            .unwrap_or("")
            .to_string();
        match request.as_str() {
            "StartSession" => {
                let behavior = state.lock().unwrap().session;
                match behavior {
                    SessionBehavior::InvalidHostId => {
                        write_frame(
                            stream.as_mut().unwrap(),
                            dict_of(&[("Error", Value::String("InvalidHostID".into()))]),
                        )
                        .await;
                    }
                    SessionBehavior::Accept => {
                        write_frame(
                            stream.as_mut().unwrap(),
                            dict_of(&[
                                ("SessionID", Value::String("MOCK-SESSION".into())),
                                ("EnableSessionSSL", Value::Boolean(true)),
                            ]),
                        )
                        .await;
                        let plain = stream.take().unwrap().into_plain();
                        match acceptor.accept(plain).await {
                            Ok(tls) => {
                                stream = Some(MockStream::Tls(tls));
                            }
                            Err(_) => return,
                        }
                    }
                }
            }
            "GetValue" => {
                let key = req
                    .get("Key")
                    .and_then(|v| v.as_string())
                    .unwrap_or("")
                    .to_string();
                let value = match key.as_str() {
                    "UniqueDeviceID" => Some(FIXTURE_UDID),
                    "ProductVersion" => Some("15.8.8"),
                    "DeviceName" => Some("MockPhone"),
                    _ => None,
                };
                match value {
                    Some(v) => {
                        write_frame(
                            stream.as_mut().unwrap(),
                            dict_of(&[("Value", Value::String(v.into()))]),
                        )
                        .await;
                    }
                    None => write_frame(stream.as_mut().unwrap(), Dictionary::new()).await,
                }
            }
            "StartService" => {
                let service = req
                    .get("Service")
                    .and_then(|v| v.as_string())
                    .unwrap_or("")
                    .to_string();
                let rejected = state.lock().unwrap().rejected_services.contains(&service);
                if rejected {
                    write_frame(
                        stream.as_mut().unwrap(),
                        dict_of(&[("Error", Value::String("InvalidService".into()))]),
                    )
                    .await;
                } else {
                    write_frame(
                        stream.as_mut().unwrap(),
                        dict_of(&[
                            ("Port", Value::Integer((service_port as i64).into())),
                            ("EnableServiceSSL", Value::Boolean(false)),
                            ("Service", Value::String(service)),
                        ]),
                    )
                    .await;
                }
            }
            _ => {
                write_frame(stream.as_mut().unwrap(), Dictionary::new()).await;
            }
        }
    }
}

fn mock() -> &'static Mock {
    MOCK.get_or_init(|| {
        let state = Arc::new(Mutex::new(MockState {
            session: SessionBehavior::Accept,
            rejected_services: HashSet::new(),
        }));
        let service_accepts = Arc::new(AtomicUsize::new(0));
        let acceptor = tls_acceptor();
        let (port_tx, port_rx) = std::sync::mpsc::channel();
        let state_clone = state.clone();
        let accepts_clone = service_accepts.clone();
        // The mock runs on its own thread with its own runtime:
        // blocking below for the service port must not starve the
        // runtime the test itself is executing on.
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
            // The "started service" endpoint StartService points at.
            let service_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let service_port = service_listener.local_addr().unwrap().port();
            port_tx.send(service_port).unwrap();
            tokio::spawn(async move {
                loop {
                    match service_listener.accept().await {
                        Ok((socket, _)) => {
                            accepts_clone.fetch_add(1, Ordering::SeqCst);
                            // Hold the connection briefly, then drop.
                            tokio::spawn(async move {
                                tokio::time::sleep(Duration::from_millis(200)).await;
                                drop(socket);
                            });
                        }
                        Err(_) => break,
                    }
                }
            });
                let listener = TcpListener::bind(LOCKDOWND_ADDR).await.unwrap();
                loop {
                    match listener.accept().await {
                        Ok((socket, _)) => {
                            let st = state_clone.clone();
                            let acc = acceptor.clone();
                            tokio::spawn(handle_conn(socket, st, acc, service_port));
                        }
                        Err(_) => break,
                    }
                }
            });
        });
        let service_port = port_rx.recv().unwrap();
        Mock {
            state,
            service_port,
            service_accepts,
        }
    })
}

impl Mock {
    fn set_session(&self, behavior: SessionBehavior) {
        self.state.lock().unwrap().session = behavior;
    }
    fn reject_service(&self, service: &str) {
        self.state
            .lock()
            .unwrap()
            .rejected_services
            .insert(service.to_string());
    }
    fn clear_rejected(&self) {
        self.state.lock().unwrap().rejected_services.clear();
    }
}

async fn started_gateway() -> IdeviceGateway {
    let gw = IdeviceGateway::new();
    gw.start(&pairing_content(), None).await.unwrap();
    gw.set_device_endpoint_ip(Some("127.0.0.1"));
    gw
}

async fn within<T>(
    fut: impl std::future::Future<Output = T>,
) -> T {
    tokio::time::timeout(Duration::from_secs(20), fut)
        .await
        .expect("gateway operation timed out")
}


#[tokio::test]
async fn start_rejects_garbage_with_pairing_error_domain() {
    let _guard = TEST_LOCK.lock().await;
    let gw = IdeviceGateway::new();
    let err = gw.start("this is not a plist", None).await.unwrap_err();
    assert_eq!(err.domain, "PairingError");
    assert_eq!(err.code, "invalidPlist");
    assert_eq!(gw.pairing_file_type(), PairingProtocol::Unknown);
}

#[tokio::test]
async fn start_rejects_bad_certificates_as_invalid_pairing_file() {
    let _guard = TEST_LOCK.lock().await;
    // mm-common's parser is lenient about the optional UDID's type
    // (falls back to empty), but the idevice pairing-file
    // construction (Swift's idevice_pairing_file_from_bytes) is
    // strict serde: an integer UDID fails construction.
    let mut value = Value::from_reader(Cursor::new(pairing_content().as_bytes())).unwrap();
    let dict = value.as_dictionary_mut().unwrap();
    dict.insert("UDID".to_string(), Value::Integer(42.into()));
    let mut buf = Vec::new();
    value.to_writer_xml(&mut buf).unwrap();
    let content = String::from_utf8(buf).unwrap();

    let gw = IdeviceGateway::new();
    let err = gw.start(&content, None).await.unwrap_err();
    assert_eq!(err.domain, "DeviceGatewayError");
    assert_eq!(err.code, "invalidPairingFile");
    assert_eq!(err.reason, "idevice_pairing_file_from_bytes failed");
}

#[tokio::test]
async fn operations_require_start() {
    let _guard = TEST_LOCK.lock().await;
    let gw = IdeviceGateway::new();
    let err = within(gw.fetch_udid()).await.unwrap_err();
    assert_eq!(err.domain, "DeviceGatewayError");
    assert_eq!(err.code, "notInitialized");
}

#[tokio::test]
async fn fetch_udid_and_values_over_tls_session() {
    let _guard = TEST_LOCK.lock().await;
    mock().set_session(SessionBehavior::Accept);
    let gw = started_gateway().await;

    let udid = within(gw.fetch_udid()).await.unwrap();
    assert_eq!(udid, FIXTURE_UDID);

    let version = within(gw.get_lockdown_value("ProductVersion"))
        .await
        .unwrap();
    assert_eq!(version, "15.8.8");

    // A key the device does not report: no Value in the response.
    let err = within(gw.get_lockdown_value("NoSuchKey")).await.unwrap_err();
    assert_eq!(err.domain, "DeviceGatewayError");
    assert_eq!(err.code, "serviceError");
}

#[tokio::test]
async fn session_positive_cached_and_stopped() {
    let _guard = TEST_LOCK.lock().await;
    mock().set_session(SessionBehavior::Accept);
    let gw = started_gateway().await;

    within(gw.start_session()).await.unwrap();
    // Second call reuses the cached session (same generation).
    within(gw.start_session()).await.unwrap();
    within(gw.stop_session()).await.unwrap();
    // Idempotent.
    within(gw.stop_session()).await.unwrap();
}

#[tokio::test]
async fn session_invalid_host_id_is_invalid_pairing_file() {
    let _guard = TEST_LOCK.lock().await;
    mock().set_session(SessionBehavior::InvalidHostId);
    let gw = started_gateway().await;

    let err = within(gw.start_session()).await.unwrap_err();
    assert_eq!(err.domain, "DeviceGatewayError");
    assert_eq!(err.code, "invalidPairingFile");
    assert!(
        err.reason
            .starts_with("Lockdown session failed (pairing invalid):"),
        "unexpected reason: {}",
        err.reason
    );

    // get_lockdown_value hits the same classification at its
    // session step.
    let err = within(gw.get_lockdown_value("ProductVersion"))
        .await
        .unwrap_err();
    assert_eq!(err.code, "invalidPairingFile");
    mock().set_session(SessionBehavior::Accept);
}

#[tokio::test]
async fn endpoint_missing_is_device_endpoint_not_available() {
    let _guard = TEST_LOCK.lock().await;
    mock().set_session(SessionBehavior::Accept);
    let gw = IdeviceGateway::new();
    gw.start(&pairing_content(), None).await.unwrap();
    // No endpoint IP set.
    let err = within(gw.fetch_udid()).await.unwrap_err();
    assert_eq!(err.code, "deviceEndpointIpNotAvailable");
}

#[tokio::test]
async fn connection_refused_is_service_error_at_connect() {
    let _guard = TEST_LOCK.lock().await;
    mock().set_session(SessionBehavior::Accept);
    let gw = IdeviceGateway::new();
    gw.start(&pairing_content(), None).await.unwrap();
    // Nothing listens on [::1]:62078 (the mock is IPv4-only), and
    // the v6 loopback refuses immediately on both Linux and
    // macOS — unlike 127.0.0.2, which macOS does not treat as
    // local and whose connects hang instead of refusing.
    gw.set_device_endpoint_ip(Some("::1"));
    let err = within(gw.fetch_udid()).await.unwrap_err();
    assert_eq!(err.domain, "DeviceGatewayError");
    assert_eq!(err.code, "serviceError");
    assert!(
        err.reason.starts_with("Failed to connect to lockdownd"),
        "unexpected reason: {}",
        err.reason
    );
}

#[tokio::test]
async fn open_service_connects_to_started_port() {
    let _guard = TEST_LOCK.lock().await;
    mock().set_session(SessionBehavior::Accept);
    mock().clear_rejected();
    let gw = started_gateway().await;
    let before = mock().service_accepts.load(Ordering::SeqCst);

    let opened = within(gw.open_service("com.apple.mobile.heartbeat"))
        .await
        .unwrap();
    assert_eq!(opened.port, mock().service_port);
    assert!(!opened.enable_ssl);
    assert_eq!(opened.service_name, "com.apple.mobile.heartbeat");
    assert!(
        mock().service_accepts.load(Ordering::SeqCst) > before,
        "gateway must actually connect to the started service port"
    );

    mock().reject_service("com.apple.no.such.service");
    let err = within(gw.open_service("com.apple.no.such.service"))
        .await
        .unwrap_err();
    assert_eq!(err.code, "serviceError");
    mock().clear_rejected();
}

#[tokio::test]
async fn rppairing_start_ok_but_operations_unsupported_until_m7() {
    let _guard = TEST_LOCK.lock().await;
    let rp_content = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n<key>private_key</key>\n<data>AAECAw==</data>\n<key>public_key</key>\n<data>AAECAw==</data>\n<key>identifier</key>\n<string>RP-ID-1</string>\n</dict>\n</plist>\n";
    let gw = IdeviceGateway::new();
    gw.start(rp_content, None).await.unwrap();
    assert_eq!(gw.pairing_file_type(), PairingProtocol::RpPairing);
    let err = within(gw.get_lockdown_value("ProductVersion"))
        .await
        .unwrap_err();
    assert_eq!(err.domain, "DeviceGatewayError");
    assert_eq!(err.code, "unsupportedOperation");
}

#[test]
fn usbmuxd_addr_env_parsing() {
    use idevice::usbmuxd::UsbmuxdAddr;
    use mm_gateway::gw_idevice::usbmuxd_addr_from_env;
    // Default: the in-App proxy address.
    match usbmuxd_addr_from_env(None) {
        UsbmuxdAddr::TcpSocket(addr) => {
            assert_eq!(addr.to_string(), "127.0.0.1:27015");
        }
        #[allow(unreachable_patterns)]
        _ => panic!("expected TCP addr"),
    }
    match usbmuxd_addr_from_env(Some("10.0.0.5:27015")) {
        UsbmuxdAddr::TcpSocket(addr) => {
            assert_eq!(addr.to_string(), "10.0.0.5:27015");
        }
        #[allow(unreachable_patterns)]
        _ => panic!("expected TCP addr"),
    }
    #[cfg(unix)]
    match usbmuxd_addr_from_env(Some("/tmp/usbmuxd")) {
        UsbmuxdAddr::UnixSocket(path) => assert_eq!(path, "/tmp/usbmuxd"),
        _ => panic!("expected unix addr"),
    }
}
