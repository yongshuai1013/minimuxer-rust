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

use std::collections::{HashMap, HashSet};
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
    /// FIFO of services handed out by StartService; the service
    /// endpoint pops one name per accepted connection so it can
    /// speak the right protocol (heartbeat vs. generic).
    service_queue: std::collections::VecDeque<String>,
    /// M5: CopyDevices reports a mounted personalized DDI.
    ddi_mounted: bool,
    /// M5: instproxy Install/Uninstall fail with this Error.
    instproxy_error: Option<String>,
    /// M5: house_arrest vend fails with this Error.
    vend_error: Option<String>,
    /// M5: misagent CopyAll payloads.
    misagent_payloads: Vec<Vec<u8>>,
    /// M5: instproxy Lookup entries by bundle id.
    apps: HashMap<String, Dictionary>,
}

/// M5: the in-memory AFC filesystem (shared across service
/// connections; keys are AFC paths without a leading slash,
/// "" is the container root for house arrest, "PublicStaging"
/// style paths for the plain service).
#[derive(Default)]
struct AfcStore {
    files: HashMap<String, Vec<u8>>,
    dirs: HashSet<String>,
}

/// Everything the mock's service handlers share.
struct MockServices {
    state: Arc<Mutex<MockState>>,
    afc: Arc<Mutex<AfcStore>>,
    /// (service, request) pairs in arrival order, for tests
    /// that assert on what the gateway sent.
    requests: Arc<Mutex<Vec<(String, Dictionary)>>>,
    polo_frames: Arc<Mutex<Vec<Dictionary>>>,
}

struct Mock {
    state: Arc<Mutex<MockState>>,
    service_port: u16,
    /// M5: the mock usbmuxd listener port (debug proxy tests).
    mux_port: u16,
    services: Arc<MockServices>,
    service_accepts: Arc<AtomicUsize>,
    /// Polo frames the mock heartbeat service received.
    polo_frames: Arc<Mutex<Vec<Dictionary>>>,
}

/// The Marco interval the mock heartbeat service reports.
const MOCK_MARCO_INTERVAL: i64 = 15;

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
                    state
                        .lock()
                        .unwrap()
                        .service_queue
                        .push_back(service.clone());
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
            service_queue: std::collections::VecDeque::new(),
            ddi_mounted: false,
            instproxy_error: None,
            vend_error: None,
            misagent_payloads: Vec::new(),
            apps: HashMap::new(),
        }));
        let services = Arc::new(MockServices {
            state: state.clone(),
            afc: Arc::new(Mutex::new(AfcStore::default())),
            requests: Arc::new(Mutex::new(Vec::new())),
            polo_frames: Arc::new(Mutex::new(Vec::new())),
        });
        let service_accepts = Arc::new(AtomicUsize::new(0));
        let polo_frames: Arc<Mutex<Vec<Dictionary>>> = services.polo_frames.clone();
        let acceptor = tls_acceptor();
        let (port_tx, port_rx) = std::sync::mpsc::channel();
        let (mux_tx, mux_rx) = std::sync::mpsc::channel();
        let state_clone = state.clone();
        let accepts_clone = service_accepts.clone();
        let polo_clone = polo_frames.clone();
        let services_clone = services.clone();
        let services_mux = services.clone();
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
            let service_state = state_clone.clone();
            tokio::spawn(async move {
                loop {
                    match service_listener.accept().await {
                        Ok((socket, _)) => {
                            accepts_clone.fetch_add(1, Ordering::SeqCst);
                            let service = service_state
                                .lock()
                                .unwrap()
                                .service_queue
                                .pop_front()
                                .unwrap_or_default();
                            let svc = services_clone.clone();
                            if service == "com.apple.mobile.heartbeat" {
                                let polo = polo_clone.clone();
                                tokio::spawn(async move {
                                    handle_heartbeat_conn(socket, polo).await;
                                });
                            } else if M5_SERVICES.contains(&service.as_str()) {
                                tokio::spawn(async move {
                                    handle_m5_service_conn(socket, service, svc).await;
                                });
                            } else {
                                // Hold the connection briefly, then drop.
                                tokio::spawn(async move {
                                    tokio::time::sleep(Duration::from_millis(200)).await;
                                    drop(socket);
                                });
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
                // The mock usbmuxd (M5 debug proxy tests):
                // ListDevices / Connect, then pipe to the
                // service endpoint (which pops the queued
                // service name — "com.apple.debugserver").
                let mux_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                mux_tx.send(mux_listener.local_addr().unwrap().port()).unwrap();
                tokio::spawn(async move {
                    loop {
                        match mux_listener.accept().await {
                            Ok((socket, _)) => {
                                let svc = services_mux.clone();
                                tokio::spawn(async move {
                                    handle_mux_conn(socket, svc, service_port).await;
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
        let mux_port = mux_rx.recv().unwrap();
        Mock {
            state,
            service_port,
            mux_port,
            services,
            service_accepts,
            polo_frames,
        }
    })
}

/// The mock heartbeat service: send a Marco carrying
/// `MOCK_MARCO_INTERVAL`, then record whatever the client
/// answers (expected: the Polo frame) and close.
async fn handle_heartbeat_conn(socket: TcpStream, polo_frames: Arc<Mutex<Vec<Dictionary>>>) {
    let mut stream = MockStream::Plain(socket);
    write_frame(
        &mut stream,
        dict_of(&[
            ("Command", Value::String("Marco".into())),
            ("Interval", Value::Integer(MOCK_MARCO_INTERVAL.into())),
        ]),
    )
    .await;
    if let Some(frame) = read_frame(&mut stream).await {
        polo_frames.lock().unwrap().push(frame);
    }
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
    fn take_polo_frames(&self) -> Vec<Dictionary> {
        std::mem::take(&mut *self.polo_frames.lock().unwrap())
    }
    fn polo_count(&self) -> usize {
        self.polo_frames.lock().unwrap().len()
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

// ---------------------------------------------------------------------------
// M4: performHeartbeat against the mock heartbeat service.
// ---------------------------------------------------------------------------

fn rp_pairing_content() -> String {
    // Minimal valid rppairing file (mm-common's required keys).
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\
     <key>private_key</key>\n<data>AAECAw==</data>\n\
     <key>public_key</key>\n<data>AAECAw==</data>\n\
     <key>identifier</key>\n<string>rp-device-1</string>\n\
     </dict>\n</plist>\n"
        .to_string()
}

#[tokio::test]
async fn perform_heartbeat_marco_polo_round_trip() {
    let _guard = TEST_LOCK.lock().await;
    mock().set_session(SessionBehavior::Accept);
    let _ = mock().take_polo_frames();
    let gw = started_gateway().await;

    let new_interval = within(gw.perform_heartbeat(5)).await.unwrap();
    // The mock's Marco carries Interval=15; the gateway returns it
    // verbatim (Swift: `newInterval > 0 ? newInterval : interval`).
    assert_eq!(new_interval, MOCK_MARCO_INTERVAL as u64);

    // The client must have answered the Marco with a Polo (the
    // mock records it asynchronously; wait for it to land).
    let mut polos = Vec::new();
    for _ in 0..50 {
        if mock().polo_count() > 0 {
            polos = mock().take_polo_frames();
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(polos.len(), 1);
    assert_eq!(
        polos[0].get("Command").and_then(|v| v.as_string()),
        Some("Polo")
    );
}

#[tokio::test]
async fn perform_heartbeat_requires_start() {
    let _guard = TEST_LOCK.lock().await;
    let gw = IdeviceGateway::new();
    let err = within(gw.perform_heartbeat(5)).await.unwrap_err();
    assert_eq!(err.domain, "DeviceGatewayError");
    assert_eq!(err.code, "notInitialized");
}

#[tokio::test]
async fn perform_heartbeat_rp_is_unsupported_until_m7() {
    let _guard = TEST_LOCK.lock().await;
    let gw = IdeviceGateway::new();
    gw.start(&rp_pairing_content(), None).await.unwrap();
    assert_eq!(gw.pairing_file_type(), PairingProtocol::RpPairing);
    let err = within(gw.perform_heartbeat(5)).await.unwrap_err();
    assert_eq!(err.domain, "DeviceGatewayError");
    assert_eq!(err.code, "unsupportedOperation");
    assert_eq!(err.reason, "performHeartbeat");
}

#[tokio::test]
async fn is_ddi_mounted_reflects_copy_devices() {
    let _guard = TEST_LOCK.lock().await;
    mock().reset_m5();
    let gw = started_gateway().await;
    let mounted = within(gw.is_ddi_mounted()).await.unwrap();
    assert!(!mounted);
    mock().set_ddi_mounted(true);
    let mounted = within(gw.is_ddi_mounted()).await.unwrap();
    assert!(mounted);
    mock().reset_m5();
}

// ---------------------------------------------------------------------------
// M5 mock services: instproxy / misagent / image mounter /
// AFC / house arrest / debugserver, plus a mock usbmuxd.
// ---------------------------------------------------------------------------

const M5_SERVICES: [&str; 6] = [
    "com.apple.mobile.installation_proxy",
    "com.apple.misagent",
    "com.apple.mobile.mobile_image_mounter",
    "com.apple.afc",
    "com.apple.mobile.house_arrest",
    "com.apple.debugserver",
];

fn record(services: &MockServices, service: &str, req: &Dictionary) {
    services
        .requests
        .lock()
        .unwrap()
        .push((service.to_string(), req.clone()));
}

async fn handle_m5_service_conn(
    socket: TcpStream,
    service: String,
    services: Arc<MockServices>,
) {
    match service.as_str() {
        "com.apple.mobile.installation_proxy" => {
            handle_instproxy_conn(socket, services).await
        }
        "com.apple.misagent" => handle_misagent_conn(socket, services).await,
        "com.apple.mobile.mobile_image_mounter" => {
            handle_mounter_conn(socket, services).await
        }
        "com.apple.afc" => handle_afc_conn(socket, services, None).await,
        "com.apple.mobile.house_arrest" => {
            handle_house_arrest_conn(socket, services).await
        }
        "com.apple.debugserver" => handle_debugserver_conn(socket).await,
        _ => {}
    }
}

async fn handle_instproxy_conn(socket: TcpStream, services: Arc<MockServices>) {
    let mut stream = MockStream::Plain(socket);
    while let Some(req) = read_frame(&mut stream).await {
        record(&services, "com.apple.mobile.installation_proxy", &req);
        let command = req
            .get("Command")
            .and_then(|v| v.as_string())
            .unwrap_or("")
            .to_string();
        match command.as_str() {
            "Lookup" => {
                let ids: Vec<String> = match req.get("ClientOptions") {
                    Some(Value::Dictionary(opts)) => match opts.get("BundleIDs") {
                        Some(Value::Array(a)) => a
                            .iter()
                            .filter_map(|v| v.as_string().map(|s| s.to_string()))
                            .collect(),
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                };
                let mut result = Dictionary::new();
                {
                    let state = services.state.lock().unwrap();
                    for id in &ids {
                        if let Some(entry) = state.apps.get(id) {
                            result.insert(id.clone(), Value::Dictionary(entry.clone()));
                        }
                    }
                }
                write_frame(
                    &mut stream,
                    dict_of(&[("LookupResult", Value::Dictionary(result))]),
                )
                .await;
            }
            "Install" => {
                // A progress frame, then the terminal frame.
                write_frame(
                    &mut stream,
                    dict_of(&[
                        ("Status", Value::String("Installing".into())),
                        ("PercentComplete", Value::Integer(50.into())),
                    ]),
                )
                .await;
                let error = services.state.lock().unwrap().instproxy_error.clone();
                match error {
                    Some(e) => {
                        write_frame(
                            &mut stream,
                            dict_of(&[("Error", Value::String(e))]),
                        )
                        .await;
                    }
                    None => {
                        write_frame(
                            &mut stream,
                            dict_of(&[("Status", Value::String("Complete".into()))]),
                        )
                        .await;
                    }
                }
            }
            "Uninstall" => {
                let error = services.state.lock().unwrap().instproxy_error.clone();
                match error {
                    Some(e) => {
                        write_frame(
                            &mut stream,
                            dict_of(&[("Error", Value::String(e))]),
                        )
                        .await;
                    }
                    None => {
                        write_frame(
                            &mut stream,
                            dict_of(&[("Status", Value::String("Complete".into()))]),
                        )
                        .await;
                    }
                }
            }
            _ => {
                write_frame(&mut stream, Dictionary::new()).await;
            }
        }
    }
}

async fn handle_misagent_conn(socket: TcpStream, services: Arc<MockServices>) {
    let mut stream = MockStream::Plain(socket);
    while let Some(req) = read_frame(&mut stream).await {
        record(&services, "com.apple.misagent", &req);
        let message_type = req
            .get("MessageType")
            .and_then(|v| v.as_string())
            .unwrap_or("")
            .to_string();
        match message_type.as_str() {
            "Install" | "Remove" => {
                write_frame(
                    &mut stream,
                    dict_of(&[("Status", Value::Integer(1.into()))]),
                )
                .await;
            }
            "CopyAll" => {
                let payloads = services.state.lock().unwrap().misagent_payloads.clone();
                let array: Vec<Value> =
                    payloads.into_iter().map(Value::Data).collect();
                write_frame(
                    &mut stream,
                    dict_of(&[("Payload", Value::Array(array))]),
                )
                .await;
            }
            _ => {
                write_frame(&mut stream, Dictionary::new()).await;
            }
        }
    }
}

async fn handle_mounter_conn(socket: TcpStream, services: Arc<MockServices>) {
    let mut stream = MockStream::Plain(socket);
    while let Some(req) = read_frame(&mut stream).await {
        record(&services, "com.apple.mobile.mobile_image_mounter", &req);
        let command = req
            .get("Command")
            .and_then(|v| v.as_string())
            .unwrap_or("")
            .to_string();
        match command.as_str() {
            "CopyDevices" => {
                let mounted = services.state.lock().unwrap().ddi_mounted;
                let entries: Vec<Value> = if mounted {
                    vec![Value::Dictionary(dict_of(&[
                        ("MountPath", Value::String("/System/Developer".into())),
                        (
                            "PersonalizedImageType",
                            Value::String("DeveloperDiskImage".into()),
                        ),
                        ("DiskImageType", Value::String("Personalized".into())),
                    ]))]
                } else {
                    Vec::new()
                };
                write_frame(
                    &mut stream,
                    dict_of(&[("EntryList", Value::Array(entries))]),
                )
                .await;
            }
            "ReceiveBytes" => {
                let size = match req.get("ImageSize") {
                    Some(Value::Integer(i)) => i.as_signed().unwrap_or(0) as usize,
                    _ => 0,
                };
                write_frame(
                    &mut stream,
                    dict_of(&[("Status", Value::String("ReceiveBytesAck".into()))]),
                )
                .await;
                // Consume the raw image bytes.
                let mut remaining = size;
                let mut buf = vec![0u8; 65536];
                while remaining > 0 {
                    let want = remaining.min(buf.len());
                    match stream.read_exact(&mut buf[..want]).await {
                        Ok(_) => remaining -= want,
                        Err(_) => return,
                    }
                }
                write_frame(
                    &mut stream,
                    dict_of(&[("Status", Value::String("Complete".into()))]),
                )
                .await;
            }
            "MountImage" => {
                services.state.lock().unwrap().ddi_mounted = true;
                write_frame(
                    &mut stream,
                    dict_of(&[("Status", Value::String("Complete".into()))]),
                )
                .await;
            }
            "LookupImage" => {
                write_frame(&mut stream, Dictionary::new()).await;
            }
            _ => {
                write_frame(&mut stream, Dictionary::new()).await;
            }
        }
    }
}

// --- AFC packet plumbing ---------------------------------------------------

const AFC_MAGIC: u64 = 0x4141504C36414643;

struct AfcRequest {
    opcode: u64,
    packet_num: u64,
    header_payload: Vec<u8>,
    payload: Vec<u8>,
}

async fn read_afc_packet(stream: &mut MockStream) -> Option<AfcRequest> {
    let mut header = [0u8; 40];
    stream.read_exact(&mut header).await.ok()?;
    let magic = u64::from_le_bytes(header[0..8].try_into().unwrap());
    if magic != AFC_MAGIC {
        return None;
    }
    let entire_len = u64::from_le_bytes(header[8..16].try_into().unwrap()) as usize;
    let header_payload_len =
        u64::from_le_bytes(header[16..24].try_into().unwrap()) as usize;
    let packet_num = u64::from_le_bytes(header[24..32].try_into().unwrap());
    let opcode = u64::from_le_bytes(header[32..40].try_into().unwrap());
    if entire_len < header_payload_len || header_payload_len < 40 {
        return None;
    }
    let mut header_payload = vec![0u8; header_payload_len - 40];
    stream.read_exact(&mut header_payload).await.ok()?;
    let mut payload = vec![0u8; entire_len - header_payload_len];
    stream.read_exact(&mut payload).await.ok()?;
    Some(AfcRequest {
        opcode,
        packet_num,
        header_payload,
        payload,
    })
}

async fn write_afc_packet(
    stream: &mut MockStream,
    opcode: u64,
    packet_num: u64,
    header_payload: &[u8],
    payload: &[u8],
) {
    let header_payload_len = 40 + header_payload.len() as u64;
    let entire_len = header_payload_len + payload.len() as u64;
    let mut out = Vec::with_capacity(entire_len as usize);
    out.extend_from_slice(&AFC_MAGIC.to_le_bytes());
    out.extend_from_slice(&entire_len.to_le_bytes());
    out.extend_from_slice(&header_payload_len.to_le_bytes());
    out.extend_from_slice(&packet_num.to_le_bytes());
    out.extend_from_slice(&opcode.to_le_bytes());
    out.extend_from_slice(header_payload);
    out.extend_from_slice(payload);
    let _ = stream.write_all(&out).await;
    let _ = stream.flush().await;
}

async fn afc_status(stream: &mut MockStream, packet_num: u64, code: u64) {
    write_afc_packet(stream, 1, packet_num, &code.to_le_bytes(), &[]).await;
}

fn afc_path(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Normalizes an AFC path for the store: strips the leading
/// slash (house-arrest paths are container-relative with "/"
/// as root).
fn store_key(path: &str) -> String {
    path.trim_start_matches('/').to_string()
}

/// One AFC connection. `vend_prefix` is `Some(root)` for a
/// house-arrest connection scoped to a container (the store
/// keys get the prefix); plain AFC passes `None`.
async fn handle_afc_conn(
    socket: TcpStream,
    services: Arc<MockServices>,
    vend_prefix: Option<String>,
) {
    let mut stream = MockStream::Plain(socket);
    let mut handles: HashMap<u64, (String, usize)> = HashMap::new();
    let mut next_handle: u64 = 42;
    let prefix = vend_prefix.unwrap_or_default();
    let qualify = |path: &str| -> String {
        let key = store_key(path);
        if prefix.is_empty() {
            key
        } else if key.is_empty() {
            prefix.clone()
        } else {
            format!("{prefix}/{key}")
        }
    };
    while let Some(req) = read_afc_packet(&mut stream).await {
        match req.opcode {
            // MakeDir
            9 => {
                let key = qualify(&afc_path(&req.header_payload));
                services.afc.lock().unwrap().dirs.insert(key);
                afc_status(&mut stream, req.packet_num, 0).await;
            }
            // ReadDir
            3 => {
                let key = qualify(&afc_path(&req.header_payload));
                let entries: Vec<String> = {
                    let store = services.afc.lock().unwrap();
                    let mut entries: Vec<String> = vec![".".into(), "..".into()];
                    let mut seen: HashSet<String> = HashSet::new();
                    for k in store.dirs.iter().chain(store.files.keys()) {
                        if k == &key {
                            continue;
                        }
                        let rest = if key.is_empty() {
                            Some(k.as_str())
                        } else {
                            k.strip_prefix(&format!("{key}/"))
                        };
                        if let Some(rest) = rest {
                            let first = rest.split('/').next().unwrap_or("");
                            if !first.is_empty() && seen.insert(first.to_string()) {
                                entries.push(first.to_string());
                            }
                        }
                    }
                    entries
                };
                let mut payload = Vec::new();
                for e in &entries {
                    payload.extend_from_slice(e.as_bytes());
                    payload.push(0);
                }
                payload.push(0);
                write_afc_packet(&mut stream, 2, req.packet_num, &[], &payload).await;
            }
            // GetFileInfo
            10 => {
                let key = qualify(&afc_path(&req.header_payload));
                let info: Option<Vec<(String, String)>> = {
                    let store = services.afc.lock().unwrap();
                    let has_children = store
                        .files
                        .keys()
                        .chain(store.dirs.iter())
                        .any(|k| k.starts_with(&format!("{key}/")));
                    if store.dirs.contains(&key) || key.is_empty() || key == prefix || has_children {
                        Some(vec![
                            ("st_ifmt".into(), "S_IFDIR".into()),
                            ("st_size".into(), "0".into()),
                        ])
                    } else if let Some(data) = store.files.get(&key) {
                        Some(vec![
                            ("st_ifmt".into(), "S_IFREG".into()),
                            ("st_size".into(), data.len().to_string()),
                        ])
                    } else {
                        None
                    }
                };
                match info {
                    Some(pairs) => {
                        let mut payload = Vec::new();
                        for (k, v) in pairs {
                            payload.extend_from_slice(k.as_bytes());
                            payload.push(0);
                            payload.extend_from_slice(v.as_bytes());
                            payload.push(0);
                        }
                        write_afc_packet(&mut stream, 2, req.packet_num, &[], &payload)
                            .await;
                    }
                    // ObjectNotFound
                    None => afc_status(&mut stream, req.packet_num, 8).await,
                }
            }
            // FileOpen
            13 => {
                if req.header_payload.len() < 8 {
                    afc_status(&mut stream, req.packet_num, 7).await;
                    continue;
                }
                let path = afc_path(&req.header_payload[8..]);
                let key = qualify(&path);
                let handle = next_handle;
                next_handle += 1;
                handles.insert(handle, (key, 0));
                write_afc_packet(
                    &mut stream,
                    14,
                    req.packet_num,
                    &handle.to_le_bytes(),
                    &[],
                )
                .await;
            }
            // Write
            16 => {
                if req.header_payload.len() < 8 {
                    afc_status(&mut stream, req.packet_num, 7).await;
                    continue;
                }
                let handle =
                    u64::from_le_bytes(req.header_payload[..8].try_into().unwrap());
                match handles.get(&handle) {
                    Some((key, _)) => {
                        let key = key.clone();
                        services
                            .afc
                            .lock()
                            .unwrap()
                            .files
                            .insert(key, req.payload.clone());
                        afc_status(&mut stream, req.packet_num, 0).await;
                    }
                    None => afc_status(&mut stream, req.packet_num, 7).await,
                }
            }
            // Read
            15 => {
                if req.header_payload.len() < 16 {
                    afc_status(&mut stream, req.packet_num, 7).await;
                    continue;
                }
                let handle =
                    u64::from_le_bytes(req.header_payload[..8].try_into().unwrap());
                let count =
                    u64::from_le_bytes(req.header_payload[8..16].try_into().unwrap())
                        as usize;
                let data = handles.get(&handle).and_then(|(key, _)| {
                    services.afc.lock().unwrap().files.get(key).cloned()
                });
                match data {
                    Some(bytes) => {
                        let n = count.min(bytes.len());
                        write_afc_packet(
                            &mut stream,
                            2,
                            req.packet_num,
                            &[],
                            &bytes[..n],
                        )
                        .await;
                    }
                    // EndOfData
                    None => afc_status(&mut stream, req.packet_num, 14).await,
                }
            }
            // FileClose
            20 => {
                if req.header_payload.len() >= 8 {
                    let handle =
                        u64::from_le_bytes(req.header_payload[..8].try_into().unwrap());
                    handles.remove(&handle);
                }
                afc_status(&mut stream, req.packet_num, 0).await;
            }
            // RemovePath
            8 => {
                let key = qualify(&afc_path(&req.header_payload));
                {
                    let mut store = services.afc.lock().unwrap();
                    store.files.remove(&key);
                    store.dirs.remove(&key);
                }
                afc_status(&mut stream, req.packet_num, 0).await;
            }
            _ => {
                afc_status(&mut stream, req.packet_num, 15).await;
            }
        }
    }
}

async fn handle_house_arrest_conn(socket: TcpStream, services: Arc<MockServices>) {
    let mut stream = MockStream::Plain(socket);
    let req = match read_frame(&mut stream).await {
        Some(r) => r,
        None => return,
    };
    record(&services, "com.apple.mobile.house_arrest", &req);
    let vend_error = services.state.lock().unwrap().vend_error.clone();
    if let Some(error) = vend_error {
        write_frame(
            &mut stream,
            dict_of(&[("Error", Value::String(error))]),
        )
        .await;
        return;
    }
    let bundle_id = req
        .get("Identifier")
        .and_then(|v| v.as_string())
        .unwrap_or("")
        .to_string();
    write_frame(&mut stream, Dictionary::new()).await;
    // The connection continues as AFC, scoped to the vend root.
    match stream {
        MockStream::Plain(socket) => {
            handle_afc_conn(socket, services, Some(format!("vend:{bundle_id}"))).await;
        }
        MockStream::Tls(_) => {}
    }
}

/// The scripted debugserver: every framed RSP command gets
/// `+$OK#9a`; the connection closes after `D`.
async fn handle_debugserver_conn(socket: TcpStream) {
    let mut stream = MockStream::Plain(socket);
    let mut byte = [0u8; 1];
    loop {
        // Skip acks / stray bytes up to the '$' frame start.
        match stream.read_exact(&mut byte).await {
            Ok(_) => {}
            Err(_) => return,
        }
        if byte[0] != b'$' {
            continue;
        }
        let mut name = Vec::new();
        loop {
            match stream.read_exact(&mut byte).await {
                Ok(_) => {}
                Err(_) => return,
            }
            if byte[0] == b'#' {
                break;
            }
            name.push(byte[0]);
        }
        let mut checksum = [0u8; 2];
        if stream.read_exact(&mut checksum).await.is_err() {
            return;
        }
        if stream.write_all(b"+$OK#9a").await.is_err() {
            return;
        }
        let _ = stream.flush().await;
        if name == b"D" {
            return;
        }
    }
}

// --- Mock usbmuxd ------------------------------------------------------------

async fn read_mux_frame(stream: &mut MockStream) -> Option<Dictionary> {
    let mut header = [0u8; 16];
    stream.read_exact(&mut header).await.ok()?;
    let total = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
    if total < 16 || total > 1_000_000 {
        return None;
    }
    let mut body = vec![0u8; total - 16];
    stream.read_exact(&mut body).await.ok()?;
    match Value::from_reader(Cursor::new(body.as_slice())) {
        Ok(Value::Dictionary(d)) => Some(d),
        _ => None,
    }
}

async fn write_mux_frame(stream: &mut MockStream, dict: Dictionary) {
    let mut body = Vec::new();
    Value::Dictionary(dict).to_writer_xml(&mut body).unwrap();
    let total = (16 + body.len()) as u32;
    let mut frame = Vec::with_capacity(total as usize);
    frame.extend_from_slice(&total.to_le_bytes());
    frame.extend_from_slice(&1u32.to_le_bytes());
    frame.extend_from_slice(&8u32.to_le_bytes());
    frame.extend_from_slice(&0u32.to_le_bytes());
    frame.extend_from_slice(&body);
    let _ = stream.write_all(&frame).await;
    let _ = stream.flush().await;
}

async fn handle_mux_conn(socket: TcpStream, services: Arc<MockServices>, service_port: u16) {
    let mut stream = MockStream::Plain(socket);
    while let Some(req) = read_mux_frame(&mut stream).await {
        record(&services, "usbmuxd", &req);
        let message_type = req
            .get("MessageType")
            .and_then(|v| v.as_string())
            .unwrap_or("")
            .to_string();
        match message_type.as_str() {
            "ListDevices" => {
                let device = dict_of(&[
                    ("DeviceID", Value::Integer(1.into())),
                    (
                        "Properties",
                        Value::Dictionary(dict_of(&[
                            ("SerialNumber", Value::String(FIXTURE_UDID.into())),
                            ("ConnectionType", Value::String("USB".into())),
                            ("DeviceID", Value::Integer(1.into())),
                        ])),
                    ),
                ]);
                write_mux_frame(
                    &mut stream,
                    dict_of(&[("DeviceList", Value::Array(vec![Value::Dictionary(device)]))]),
                )
                .await;
            }
            "Connect" => {
                write_mux_frame(
                    &mut stream,
                    dict_of(&[("Number", Value::Integer(0.into()))]),
                )
                .await;
                // Pipe to the service endpoint; the accept loop
                // pops the queued service name (debugserver).
                match stream {
                    MockStream::Plain(mut client) => {
                        if let Ok(mut upstream) =
                            TcpStream::connect(("127.0.0.1", service_port)).await
                        {
                            let _ = tokio::io::copy_bidirectional(
                                &mut client,
                                &mut upstream,
                            )
                            .await;
                        }
                        return;
                    }
                    MockStream::Tls(_) => return,
                }
            }
            _ => {
                write_mux_frame(
                    &mut stream,
                    dict_of(&[("Number", Value::Integer(0.into()))]),
                )
                .await;
            }
        }
    }
}

impl Mock {
    fn set_ddi_mounted(&self, mounted: bool) {
        self.state.lock().unwrap().ddi_mounted = mounted;
    }
    fn set_instproxy_error(&self, error: Option<&str>) {
        self.state.lock().unwrap().instproxy_error =
            error.map(|e| e.to_string());
    }
    fn set_vend_error(&self, error: Option<&str>) {
        self.state.lock().unwrap().vend_error = error.map(|e| e.to_string());
    }
    fn set_misagent_payloads(&self, payloads: Vec<Vec<u8>>) {
        self.state.lock().unwrap().misagent_payloads = payloads;
    }
    fn add_app(&self, bundle_id: &str, container: &str, path: &str, executable: &str) {
        let entry = dict_of(&[
            ("Container", Value::String(container.into())),
            ("Path", Value::String(path.into())),
            ("CFBundleExecutable", Value::String(executable.into())),
        ]);
        self.state
            .lock()
            .unwrap()
            .apps
            .insert(bundle_id.to_string(), entry);
    }
    fn seed_afc_file(&self, key: &str, data: &[u8]) {
        self.services
            .afc
            .lock()
            .unwrap()
            .files
            .insert(key.to_string(), data.to_vec());
    }
    fn afc_file(&self, key: &str) -> Option<Vec<u8>> {
        self.services.afc.lock().unwrap().files.get(key).cloned()
    }
    fn requests_for(&self, service: &str) -> Vec<Dictionary> {
        self.services
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, _)| s == service)
            .map(|(_, d)| d.clone())
            .collect()
    }
    fn reset_m5(&self) {
        let mut state = self.state.lock().unwrap();
        state.ddi_mounted = false;
        state.instproxy_error = None;
        state.vend_error = None;
        state.misagent_payloads = Vec::new();
        state.apps.clear();
        drop(state);
        self.services.afc.lock().unwrap().files.clear();
        self.services.afc.lock().unwrap().dirs.clear();
        self.services.requests.lock().unwrap().clear();
    }
}

// ---------------------------------------------------------------------------
// M5 integration tests (gw-idevice against the mock services)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mount_developer_image_uploads_then_mounts() {
    let _guard = TEST_LOCK.lock().await;
    mock().reset_m5();
    let gw = started_gateway().await;
    within(gw.mount_developer_image(b"DMGDATA", b"SIGBYTES"))
        .await
        .unwrap();
    let reqs = mock().requests_for("com.apple.mobile.mobile_image_mounter");
    let commands: Vec<&str> = reqs
        .iter()
        .filter_map(|r| r.get("Command").and_then(|v| v.as_string()))
        .collect();
    assert_eq!(commands, ["ReceiveBytes", "MountImage"]);
    // The mock flips its mounted flag on MountImage.
    let mounted = within(gw.is_ddi_mounted()).await.unwrap();
    assert!(mounted);
    mock().reset_m5();
}

#[tokio::test]
async fn send_ipa_then_install_ipa_round_trip() {
    let _guard = TEST_LOCK.lock().await;
    mock().reset_m5();
    let gw = started_gateway().await;
    let ipa = b"PK\x03\x04-fake-ipa-bytes";
    within(gw.send_ipa_afc("com.example.app", ipa)).await.unwrap();
    assert_eq!(
        mock().afc_file("PublicStaging/com.example.app/app.ipa"),
        Some(ipa.to_vec())
    );
    within(gw.install_ipa("com.example.app")).await.unwrap();
    let reqs = mock().requests_for("com.apple.mobile.installation_proxy");
    let install = reqs
        .iter()
        .find(|r| r.get("Command").and_then(|v| v.as_string()) == Some("Install"))
        .expect("Install request recorded");
    assert_eq!(
        install.get("PackagePath").and_then(|v| v.as_string()),
        Some("PublicStaging/com.example.app/app.ipa")
    );
    assert!(install.get("ClientOptions").is_none());
    mock().reset_m5();
}

#[tokio::test]
async fn install_ipa_error_maps_to_swift_reason() {
    let _guard = TEST_LOCK.lock().await;
    mock().reset_m5();
    mock().set_instproxy_error(Some("ApplicationVerificationFailed"));
    let gw = started_gateway().await;
    let err = within(gw.install_ipa("com.example.app")).await.unwrap_err();
    assert_eq!(err.domain, "DeviceGatewayError");
    assert_eq!(err.code, "serviceError");
    assert_eq!(
        err.reason,
        "Failed to install IPA, error: (ApplicationVerificationFailed)"
    );
    mock().reset_m5();
}

#[tokio::test]
async fn remove_app_uninstalls_and_maps_errors() {
    let _guard = TEST_LOCK.lock().await;
    mock().reset_m5();
    let gw = started_gateway().await;
    within(gw.remove_app("com.example.app")).await.unwrap();
    let reqs = mock().requests_for("com.apple.mobile.installation_proxy");
    let uninstall = reqs
        .iter()
        .find(|r| r.get("Command").and_then(|v| v.as_string()) == Some("Uninstall"))
        .expect("Uninstall request recorded");
    assert_eq!(
        uninstall
            .get("ApplicationIdentifier")
            .and_then(|v| v.as_string()),
        Some("com.example.app")
    );
    mock().set_instproxy_error(Some("ApplicationNotFound"));
    let err = within(gw.remove_app("com.example.app")).await.unwrap_err();
    assert_eq!(
        err.reason,
        "Failed to uninstall app, error: (ApplicationNotFound)"
    );
    mock().reset_m5();
}

#[tokio::test]
async fn profiles_install_remove_and_dump() {
    let _guard = TEST_LOCK.lock().await;
    mock().reset_m5();
    let gw = started_gateway().await;
    within(gw.install_provisioning_profile(b"profile-bytes"))
        .await
        .unwrap();
    within(gw.remove_provisioning_profile("SOME-UUID"))
        .await
        .unwrap();
    let reqs = mock().requests_for("com.apple.misagent");
    let types: Vec<&str> = reqs
        .iter()
        .filter_map(|r| r.get("MessageType").and_then(|v| v.as_string()))
        .collect();
    assert_eq!(types, ["Install", "Remove"]);

    // Dump: payload 0 carries a plist with a UUID between
    // binary junk; payload 1 has no plist section (skipped);
    // payload 2's plist has no UUID (unknown_<i>).
    let plist_with_uuid = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>UUID</key><string>AAAA-UUID-1</string></dict></plist>"#;
    let mut payload0 = b"\x00\x01binary".to_vec();
    payload0.extend_from_slice(plist_with_uuid);
    payload0.extend_from_slice(b"\xff\xfe");
    let payload1 = b"no plist here".to_vec();
    let plist_no_uuid = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>Name</key><string>x</string></dict></plist>"#;
    let mut payload2 = b"junk".to_vec();
    payload2.extend_from_slice(plist_no_uuid);
    mock().set_misagent_payloads(vec![payload0.clone(), payload1, payload2]);

    let dir = std::env::temp_dir().join(format!("mm-mock-dump-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let docs = dir.to_string_lossy().into_owned();
    let dump_dir = within(gw.dump_profiles(&docs)).await.unwrap();
    assert_eq!(dump_dir, format!("{docs}/Profiles"));
    let full = std::fs::read(format!("{dump_dir}/AAAA-UUID-1.mobileprovision")).unwrap();
    assert_eq!(full, payload0);
    let plist_part =
        std::fs::read(format!("{dump_dir}/AAAA-UUID-1.plist")).unwrap();
    assert_eq!(plist_part, plist_with_uuid);
    assert!(std::fs::read(format!("{dump_dir}/unknown_2.mobileprovision")).is_ok());
    assert_eq!(
        std::fs::read_dir(&dump_dir).unwrap().count(),
        3,
        "exactly the three expected files"
    );
    let _ = std::fs::remove_dir_all(&dir);
    mock().reset_m5();
}

#[tokio::test]
async fn send_app_bundle_then_install_with_developer_options() {
    let _guard = TEST_LOCK.lock().await;
    mock().reset_m5();
    let gw = started_gateway().await;
    let root = std::env::temp_dir().join(format!("mm-mock-app-{}", std::process::id()));
    let app = root.join("Mock.app");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(app.join("Sub")).unwrap();
    std::fs::write(app.join("Info.plist"), b"info-bytes").unwrap();
    std::fs::write(app.join("Mock"), b"exec-bytes").unwrap();
    std::fs::write(app.join("Sub/data.bin"), b"data-bytes").unwrap();
    std::fs::write(app.join("empty.txt"), b"").unwrap();

    let app_str = app.to_string_lossy().into_owned();
    within(gw.send_app_bundle_afc("com.example.app", &app_str))
        .await
        .unwrap();
    assert_eq!(
        mock().afc_file("PublicStaging/com.example.app/Mock.app/Info.plist"),
        Some(b"info-bytes".to_vec())
    );
    assert_eq!(
        mock().afc_file("PublicStaging/com.example.app/Mock.app/Sub/data.bin"),
        Some(b"data-bytes".to_vec())
    );

    within(gw.install_app_bundle("com.example.app", "Mock.app"))
        .await
        .unwrap();
    let reqs = mock().requests_for("com.apple.mobile.installation_proxy");
    let install = reqs
        .iter()
        .find(|r| r.get("Command").and_then(|v| v.as_string()) == Some("Install"))
        .expect("Install request recorded");
    assert_eq!(
        install.get("PackagePath").and_then(|v| v.as_string()),
        Some("PublicStaging/com.example.app/Mock.app")
    );
    let package_type = install
        .get("ClientOptions")
        .and_then(|v| v.as_dictionary())
        .and_then(|d| d.get("PackageType"))
        .and_then(|v| v.as_string());
    assert_eq!(package_type, Some("Developer"));
    let _ = std::fs::remove_dir_all(&root);
    mock().reset_m5();
}

#[tokio::test]
async fn afc_queries_and_wipe_via_house_arrest() {
    let _guard = TEST_LOCK.lock().await;
    mock().reset_m5();
    mock().seed_afc_file("vend:com.example.app/Documents/note.txt", b"hello-afc");
    mock().seed_afc_file("vend:com.example.app/Documents/sub/deep.txt", b"deep");
    let gw = started_gateway().await;

    let entries = within(gw.afc_list_directory("com.example.app", "/Documents"))
        .await
        .unwrap();
    assert!(entries.iter().any(|e| e == "note.txt"), "entries: {entries:?}");
    assert!(entries.iter().any(|e| e == "sub"), "entries: {entries:?}");

    let data = within(gw.afc_read_file("com.example.app", "/Documents/note.txt"))
        .await
        .unwrap();
    assert_eq!(data, b"hello-afc");

    let info = within(gw.afc_get_file_info("com.example.app", "/Documents/note.txt"))
        .await
        .unwrap();
    assert!(!info.is_directory);
    assert_eq!(info.file_size, 9);
    let info = within(gw.afc_get_file_info("com.example.app", "/Documents"))
        .await
        .unwrap();
    assert!(info.is_directory);

    within(gw.wipe_container("com.example.app")).await.unwrap();
    assert_eq!(
        mock().afc_file("vend:com.example.app/Documents/note.txt"),
        None
    );
    assert_eq!(
        mock().afc_file("vend:com.example.app/Documents/sub/deep.txt"),
        None
    );
    mock().reset_m5();
}

#[tokio::test]
async fn house_arrest_vend_error_maps_to_swift_reason() {
    let _guard = TEST_LOCK.lock().await;
    mock().reset_m5();
    mock().set_vend_error(Some("VendFailed"));
    let gw = started_gateway().await;
    let err = within(gw.afc_list_directory("com.example.app", "/"))
        .await
        .unwrap_err();
    assert_eq!(
        err.reason,
        "Failed to vend container for com.example.app, error: (VendFailed)"
    );
    mock().reset_m5();
}

#[tokio::test]
async fn debug_app_lockdown_flow_via_mock_mux() {
    let _guard = TEST_LOCK.lock().await;
    mock().reset_m5();
    mock().add_app(
        "com.example.app",
        "/var/mobile/Containers/Data/Application/UUID",
        "/var/containers/Bundle/Application/UUID/Mock.app",
        "Mock",
    );
    std::env::set_var(
        "USBMUXD_SOCKET_ADDRESS",
        format!("127.0.0.1:{}", mock().mux_port),
    );
    let gw = started_gateway().await;
    let result = within(gw.debug_app("com.example.app")).await;
    std::env::remove_var("USBMUXD_SOCKET_ADDRESS");
    result.unwrap();
    mock().reset_m5();
}

#[tokio::test]
async fn debug_process_idevice_is_unsupported_with_swift_reason() {
    let _guard = TEST_LOCK.lock().await;
    let gw = started_gateway().await;
    let err = within(gw.debug_process(4242)).await.unwrap_err();
    assert_eq!(err.domain, "DeviceGatewayError");
    assert_eq!(err.code, "unsupportedOperation");
    assert_eq!(
        err.reason,
        "Attaching to process by PID is only supported in.rppairing mode"
    );
}
