//! MinimuxerCore tests: the nine isReady gates as a table
//! (fake scan source + fake gateway), the lifecycle state
//! machine, the status bus, `set_params`, and the
//! `runWithChecks` mapping. Behaviour references are Swift
//! @714aafa3 via `docs/m4-core-diff.md`.

use std::sync::{Arc, Mutex};

use mm_common::error::{DeviceGatewayCode, DeviceGatewayError};
use mm_common::{MmErrorInfo, MinimuxerError, PairingProtocol};
use mm_core::{
    map_gateway_error, run_with_checks, CoreGatewayProvider, CoreStatus,
    MinimuxerCore, MinimuxerParams, OsVersionProvider,
};
use mm_gateway::{
    DeviceGateway, GatewayBackend, GatewayCore, GatewayFuture, GatewayProvider,
};
use mm_net::{
    ConnectionConfigBinding, DeviceConnectionManager, DeviceConnectionMode,
    DeviceEndpoint, IPAddresses, IPv4Info, NetInfo, NetStack, PollingObserver,
    RouteEntry, RouteTable, ScanSnapshot, ScanSource, TablePortSource,
};

const LOCKDOWN_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n<key>WiFiMACAddress</key>\n<string>AA:BB:CC:DD:EE:FF</string>\n<key>SystemBUID</key>\n<string>BUID-1</string>\n<key>RootPrivateKey</key>\n<data>AAECAw==</data>\n<key>HostPrivateKey</key>\n<data>AAECAw==</data>\n<key>HostID</key>\n<string>H1</string>\n<key>RootCertificate</key>\n<data>AAECAw==</data>\n<key>UDID</key>\n<string>UDID-1</string>\n<key>EscrowBag</key>\n<data>AAECAw==</data>\n<key>HostCertificate</key>\n<data>AAECAw==</data>\n<key>DeviceCertificate</key>\n<data>AAECAw==</data>\n</dict>\n</plist>\n";

/// Serializes the tests in this file: several bind TCP listeners
/// and one mutates the process-wide usbmuxd env var.
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

struct FakeGateway {
    core: GatewayCore,
    backend: GatewayBackend,
    start_error: Mutex<Option<MmErrorInfo>>,
    fetch_result: Mutex<Result<String, MmErrorInfo>>,
    ddi_result: Mutex<Result<bool, MmErrorInfo>>,
    beats: Mutex<Vec<u64>>,
}

impl FakeGateway {
    fn new(backend: GatewayBackend) -> Arc<Self> {
        Arc::new(Self {
            core: GatewayCore::new(),
            backend,
            start_error: Mutex::new(None),
            fetch_result: Mutex::new(Ok("UDID-1".to_string())),
            ddi_result: Mutex::new(Ok(true)),
            beats: Mutex::new(Vec::new()),
        })
    }
}

impl DeviceGateway for FakeGateway {
    fn backend(&self) -> GatewayBackend {
        self.backend
    }

    fn core(&self) -> &GatewayCore {
        &self.core
    }

    fn set_logging(&self, enabled: bool) {
        self.core.set_logging(enabled);
    }

    fn start<'a>(
        &'a self,
        content: &'a str,
        preferred: Option<PairingProtocol>,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.core.cleanup();
            if let Some(err) = self.start_error.lock().unwrap().take() {
                return Err(err);
            }
            // Mirrors the real backends: Swift rethrows the
            // parser's PairingError unwrapped.
            let parsed = mm_common::parse_pairing_content(content, preferred)
                .map_err(|e| e.to_info())?;
            self.core
                .set_pairing_file_data(Some(parsed.raw_data().to_vec()));
            self.core.set_pairing_file_type(parsed.mode());
            self.core.set_initialized(true);
            Ok(())
        })
    }

    fn stop<'a>(&'a self) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.core.cleanup();
            Ok(())
        })
    }

    fn fetch_udid<'a>(&'a self) -> GatewayFuture<'a, String> {
        let result = self.fetch_result.lock().unwrap().clone();
        Box::pin(async move { result })
    }

    fn get_lockdown_value<'a>(&'a self, _key: &'a str) -> GatewayFuture<'a, String> {
        Box::pin(async move { Err(mm_gateway::unsupported("getLockdownValue")) })
    }

    fn start_session<'a>(&'a self) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Ok(()) })
    }

    fn stop_session<'a>(&'a self) -> GatewayFuture<'a, ()> {
        Box::pin(async move { Ok(()) })
    }

    fn open_service<'a>(
        &'a self,
        _name: &'a str,
    ) -> GatewayFuture<'a, mm_gateway::OpenedService> {
        Box::pin(async move { Err(mm_gateway::unsupported("openService")) })
    }

    fn perform_heartbeat<'a>(&'a self, interval: u64) -> GatewayFuture<'a, u64> {
        self.beats.lock().unwrap().push(interval);
        Box::pin(async move { Ok(interval) })
    }

    fn is_ddi_mounted<'a>(&'a self) -> GatewayFuture<'a, bool> {
        let result = self.ddi_result.lock().unwrap().clone();
        Box::pin(async move { result })
    }
}

struct FakeProvider {
    current: Mutex<Arc<FakeGateway>>,
    backend: Mutex<GatewayBackend>,
    created: Mutex<Vec<Arc<FakeGateway>>>,
}

impl FakeProvider {
    fn new(initial: Arc<FakeGateway>) -> Arc<Self> {
        let backend = initial.backend;
        Arc::new(Self {
            current: Mutex::new(initial),
            backend: Mutex::new(backend),
            created: Mutex::new(Vec::new()),
        })
    }

    fn current_fake(&self) -> Arc<FakeGateway> {
        self.current.lock().unwrap().clone()
    }
}

impl GatewayProvider for FakeProvider {
    fn current_gateway(&self) -> Option<Arc<dyn DeviceGateway>> {
        Some(self.current.lock().unwrap().clone())
    }
}

impl CoreGatewayProvider for FakeProvider {
    fn backend(&self) -> GatewayBackend {
        *self.backend.lock().unwrap()
    }

    fn set_backend(&self, backend: GatewayBackend) -> Arc<dyn DeviceGateway> {
        let gw = FakeGateway::new(backend);
        self.created.lock().unwrap().push(gw.clone());
        *self.current.lock().unwrap() = gw.clone();
        *self.backend.lock().unwrap() = backend;
        gw
    }
}

struct FixedOsVersion(Option<(u32, u32, u32)>);

impl OsVersionProvider for FixedOsVersion {
    fn at_least(&self, major: u32, minor: u32) -> bool {
        match self.0 {
            Some((maj, min, _)) => maj > major || (maj == major && min >= minor),
            None => false,
        }
    }
}

struct FakeScanSource {
    snapshot: Mutex<ScanSnapshot>,
}

impl FakeScanSource {
    fn set(&self, snap: ScanSnapshot) {
        *self.snapshot.lock().unwrap() = snap;
    }
}

impl ScanSource for FakeScanSource {
    fn snapshot(&self) -> ScanSnapshot {
        self.snapshot.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// Snapshot / binding / harness builders
// ---------------------------------------------------------------------------

fn net_info(name: &str, index: u16, v4: Option<(&str, &str)>) -> NetInfo {
    NetInfo {
        name: name.to_string(),
        interface_index: index,
        interface_addresses: IPAddresses {
            v4: match v4 {
                Some((host, mask)) => vec![IPv4Info {
                    host: host.to_string(),
                    mask: mask.to_string(),
                    host_raw: 0,
                    mask_raw: 0,
                }],
                None => Vec::new(),
            },
            v6: Vec::new(),
        },
        interface_subnet_routes: Vec::new(),
        flags: 0x41, // IFF_UP | IFF_RUNNING
        tunnel: None,
    }
}

fn default_route(index: u16) -> RouteEntry {
    RouteEntry {
        destination_ipv4: Some("default".to_string()),
        destination_ipv4_raw: Some(0),
        destination_ipv4_mask: None,
        destination_ipv4_mask_raw: None,
        destination_ipv6: None,
        interface_ipv4: None,
        interface_ipv4_raw: None,
        interface_ipv4_mask: None,
        interface_ipv4_mask_raw: None,
        interface_ipv6: None,
        gateway_ipv4: Some("192.168.1.1".to_string()),
        gateway_ipv4_raw: None,
        gateway_ipv6: None,
        interface_index: index,
        flags: 0,
    }
}

/// en0 (Wi-Fi) + utun9 (VPN), default route via en0.
fn snapshot_full() -> ScanSnapshot {
    ScanSnapshot {
        interfaces: vec![
            net_info("en0", 1, Some(("192.168.1.10", "255.255.255.0"))),
            net_info("utun9", 9, Some(("10.7.0.10", "255.255.255.0"))),
        ],
        routes: RouteTable::new(vec![default_route(1)]),
    }
}

fn binding(mode: DeviceConnectionMode) -> ConnectionConfigBinding {
    ConnectionConfigBinding {
        set_tunnel_iface_ip: Box::new(|_| {}),
        set_tunnel_peer_ip: Box::new(|_| {}),
        set_tunnel_peer_subnet_mask: Box::new(|_| {}),
        set_tunnel_peer_reachable: Box::new(|_| {}),
        set_tunnel_iface_subnet_mask: Box::new(|_| {}),
        set_remote_reachable: Box::new(|_| {}),
        set_override_tunnel_peer_reachable: Box::new(|_| {}),
        get_connection_mode: Box::new(move || mode),
        get_override_tunnel_peer_ip: Box::new(String::new),
        get_remote_server_ip: Box::new(String::new),
        resolve_service_port: None,
    }
}

struct Harness {
    core: Arc<MinimuxerCore>,
    provider: Arc<FakeProvider>,
    gateway: Arc<FakeGateway>,
    scan: Arc<FakeScanSource>,
    endpoint: Arc<DeviceEndpoint>,
    stack: Arc<NetStack>,
    proxy: Arc<mm_core::usbmuxd_proxy::UsbmuxdProxyServer>,
}

async fn harness(
    mode: DeviceConnectionMode,
    backend: GatewayBackend,
    os: Option<(u32, u32, u32)>,
) -> Harness {
    let scan = Arc::new(FakeScanSource {
        snapshot: Mutex::new(snapshot_full()),
    });
    let manager = Arc::new(DeviceConnectionManager::new(
        Arc::new(TablePortSource::default()),
        scan.clone(),
    ));
    let endpoint = Arc::new(DeviceEndpoint::new());
    let stack = Arc::new(NetStack::new(
        manager,
        endpoint.clone(),
        scan.clone(),
    ));
    let gateway = FakeGateway::new(backend);
    let provider = FakeProvider::new(gateway.clone());
    let proxy = Arc::new(mm_core::usbmuxd_proxy::UsbmuxdProxyServer::with_bind_addr(
        provider.clone(),
        "127.0.0.1:0",
    ));
    let observer = Arc::new(PollingObserver::new(stack.clone(), scan.clone()));
    let core = MinimuxerCore::new(
        provider.clone(),
        stack.clone(),
        observer,
        proxy.clone(),
        Arc::new(FixedOsVersion(os)),
    );
    core.bind_connection_config(binding(mode)).await;
    Harness {
        core,
        provider,
        gateway,
        scan,
        endpoint,
        stack,
        proxy,
    }
}

/// Starts the core with the lockdown file, then points the
/// readiness chain at a real local listener: endpoint
/// 127.0.0.1 + the gateway's service port set to the listener.
/// Returns the listener (keep it alive for the probe to pass).
async fn start_ready(h: &Harness) -> std::net::TcpListener {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    h.core.start(LOCKDOWN_XML, "/tmp/docs", None).await.unwrap();
    h.gateway
        .core
        .set_port(port, h.gateway.core.pairing_file_type());
    h.endpoint.update("127.0.0.1");
    listener
}

fn err_of(result: Result<bool, MinimuxerError>) -> MinimuxerError {
    result.expect_err("expected an error")
}

// ---------------------------------------------------------------------------
// The nine gates
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gate1_pairing_not_loaded() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;
    let err = err_of(h.core.is_ready(true, false).await);
    assert_eq!(
        err,
        MinimuxerError::PairingNotLoaded(
            "No valid pairing file has been loaded".to_string()
        )
    );
}

#[tokio::test]
async fn gate2_not_started() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;
    // Pairing loaded directly on the gateway; core never started.
    h.gateway.start(LOCKDOWN_XML, None).await.unwrap();
    let err = err_of(h.core.is_ready(true, false).await);
    assert_eq!(
        err,
        MinimuxerError::NotStarted("Minimuxer has not been started".to_string())
    );
}

#[tokio::test]
async fn start_rejects_not_configured_mode() {
    let _g = lock();
    let h = harness(
        DeviceConnectionMode::NotConfigured,
        GatewayBackend::Idevice,
        None,
    )
    .await;
    let err = h
        .core
        .start(LOCKDOWN_XML, "/tmp/docs", None)
        .await
        .expect_err("start must fail");
    assert_eq!(
        err,
        MinimuxerError::ConnectionModeNotConfigured(
            "Connection mode not configured. Supported modes: [localVPN, remoteServer]"
                .to_string()
        )
    );
}

#[tokio::test]
async fn gate3_wifi_only_when_network_check_on() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;
    let _listener = start_ready(&h).await;
    // Drop en0: no wifi interface remains.
    h.scan.set(ScanSnapshot {
        interfaces: vec![net_info(
            "utun9",
            9,
            Some(("10.7.0.10", "255.255.255.0")),
        )],
        routes: RouteTable::new(vec![]),
    });
    let err = err_of(h.core.is_ready(true, false).await);
    assert_eq!(
        err,
        MinimuxerError::NoConnection("No wifi interface satisfied".to_string())
    );
    // With the network check off, the same state is ready.
    assert_eq!(h.core.is_ready(false, false).await, Ok(true));
    h.core.stop().await.unwrap();
}

#[tokio::test]
async fn gate4_mode_not_configured_after_rebind() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;
    let _listener = start_ready(&h).await;
    h.core
        .bind_connection_config(binding(DeviceConnectionMode::NotConfigured))
        .await;
    let err = err_of(h.core.is_ready(true, false).await);
    assert_eq!(
        err,
        MinimuxerError::ConnectionModeNotConfigured(
            "Connection mode not configured. Supported modes: [localVPN, remoteServer]"
                .to_string()
        )
    );
    h.core.stop().await.unwrap();
}

#[tokio::test]
async fn gate4_no_utun() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;
    let _listener = start_ready(&h).await;
    h.scan.set(ScanSnapshot {
        interfaces: vec![net_info(
            "en0",
            1,
            Some(("192.168.1.10", "255.255.255.0")),
        )],
        routes: RouteTable::new(vec![default_route(1)]),
    });
    let err = err_of(h.core.is_ready(true, false).await);
    assert_eq!(
        err,
        MinimuxerError::NoVPN(
            "No utun interface detected — LocalDevVPN is not connected".to_string()
        )
    );
    h.core.stop().await.unwrap();
}

#[tokio::test]
async fn gate4_ipsec_required_only_lockdown_on_ios26_4() {
    let _g = lock();
    // iOS 26.4 + lockdown + no ipsec -> invalidVPN.
    let h = harness(
        DeviceConnectionMode::LocalVpn,
        GatewayBackend::Idevice,
        Some((26, 4, 0)),
    )
    .await;
    let _l = start_ready(&h).await;
    let err = err_of(h.core.is_ready(true, false).await);
    assert_eq!(
        err,
        MinimuxerError::InvalidVPN(
            "utun is present but no ipsec/IKEv2 interface found — LocalDevVPN may not support the lockdown protocol on iOS 26.4+".to_string()
        )
    );
    h.core.stop().await.unwrap();

    // iOS 26.4 + an ipsec interface -> gate passes.
    let h = harness(
        DeviceConnectionMode::LocalVpn,
        GatewayBackend::Idevice,
        Some((26, 4, 0)),
    )
    .await;
    let _l = start_ready(&h).await;
    let mut snap = snapshot_full();
    snap.interfaces
        .push(net_info("ipsec0", 12, Some(("10.8.0.2", "255.255.255.0"))));
    h.scan.set(snap);
    assert_eq!(h.core.is_ready(true, false).await, Ok(true));
    h.core.stop().await.unwrap();

    // iOS 15 (below 26.4) + no ipsec -> gate passes.
    let h = harness(
        DeviceConnectionMode::LocalVpn,
        GatewayBackend::Idevice,
        Some((15, 8, 8)),
    )
    .await;
    let _l = start_ready(&h).await;
    assert_eq!(h.core.is_ready(true, false).await, Ok(true));
    h.core.stop().await.unwrap();

    // iOS 26.4 + rppairing -> the ipsec clause does not apply.
    let h = harness(
        DeviceConnectionMode::LocalVpn,
        GatewayBackend::Idevice,
        Some((26, 4, 0)),
    )
    .await;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    h.core.start(LOCKDOWN_XML, "/tmp/docs", None).await.unwrap();
    h.gateway
        .core
        .set_pairing_file_type(PairingProtocol::RpPairing);
    h.gateway.core.set_port(port, PairingProtocol::RpPairing);
    h.endpoint.update("127.0.0.1");
    assert_eq!(h.core.is_ready(true, false).await, Ok(true));
    h.core.stop().await.unwrap();
}

#[tokio::test]
async fn gate6_no_endpoint() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;
    h.core.start(LOCKDOWN_XML, "/tmp/docs", None).await.unwrap();
    h.endpoint.clear();
    let err = err_of(h.core.is_ready(true, false).await);
    match err {
        MinimuxerError::NoDevice(reason) => {
            assert!(
                reason.starts_with(
                    "VPN tunnel iface is up but tunnel peer IP is not yet reachable"
                ),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected NoDevice, got {other:?}"),
    }
    h.core.stop().await.unwrap();
}

#[tokio::test]
async fn gate7_poll_failed_localvpn() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;
    h.core.start(LOCKDOWN_XML, "/tmp/docs", None).await.unwrap();
    // A port nothing listens on.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    h.gateway.core.set_port(port, PairingProtocol::Lockdown);
    h.endpoint.update("127.0.0.1");
    let err = err_of(h.core.is_ready(true, false).await);
    assert_eq!(
        err,
        MinimuxerError::InvalidVPN(
            "VPN tunnel iface is up and tunnel peer IP 127.0.0.1 is known, but TCP port poll failed — device may be unreachable on this interface"
                .to_string()
        )
    );
    h.core.stop().await.unwrap();
}

#[tokio::test]
async fn gate7_remote_server_not_reachable() {
    let _g = lock();
    let h = harness(
        DeviceConnectionMode::RemoteServer,
        GatewayBackend::Idevice,
        None,
    )
    .await;
    h.core.start(LOCKDOWN_XML, "/tmp/docs", None).await.unwrap();
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    h.gateway.core.set_port(port, PairingProtocol::Lockdown);
    h.endpoint.update("127.0.0.1");
    let err = err_of(h.core.is_ready(true, false).await);
    assert_eq!(
        err,
        MinimuxerError::NotReachable(
            "Remote endpoint 127.0.0.1 is configured, but TCP port poll failed — target device is unreachable"
                .to_string()
        )
    );
    h.core.stop().await.unwrap();
}

#[tokio::test]
async fn gate8_fetch_errors_map_through_run_with_checks() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;
    let _listener = start_ready(&h).await;

    // A VPN-drop reason maps to invalidVPN, not fetchUDID.
    *h.gateway.fetch_result.lock().unwrap() = Err(DeviceGatewayError::new(
        DeviceGatewayCode::ConnectionFailed,
        "Broken pipe (os error 32)",
    )
    .to_info());
    let err = err_of(h.core.is_ready(true, false).await);
    assert_eq!(
        err,
        MinimuxerError::InvalidVPN("Broken pipe (os error 32)".to_string())
    );

    // A plain service error lands in the fetchUDID catch-all.
    *h.gateway.fetch_result.lock().unwrap() = Err(DeviceGatewayError::new(
        DeviceGatewayCode::ServiceError,
        "plain failure",
    )
    .to_info());
    let err = err_of(h.core.is_ready(true, false).await);
    assert_eq!(
        err,
        MinimuxerError::FetchUDID("plain failure".to_string())
    );
    h.core.stop().await.unwrap();
}

#[tokio::test]
async fn gate9_muxer_not_listening() {
    let _g = lock();
    let h = harness(
        DeviceConnectionMode::LocalVpn,
        GatewayBackend::Libimobiledevice,
        None,
    )
    .await;
    let _listener = start_ready(&h).await;
    // start() brought the proxy up for the limd backend.
    assert!(h.proxy.is_listening());
    assert_eq!(h.core.is_ready(true, false).await, Ok(true));
    // Proxy dies: gate 9 fires.
    h.proxy.stop().await;
    let err = err_of(h.core.is_ready(true, false).await);
    assert_eq!(
        err,
        MinimuxerError::MuxerNotListening(
            "Usbmuxd fake server is not listening".to_string()
        )
    );
    h.core.stop().await.unwrap();
}

#[tokio::test]
async fn all_gates_pass_and_ddi_variants() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;
    let _listener = start_ready(&h).await;
    assert_eq!(h.core.is_ready(true, false).await, Ok(true));

    // Gate 10 (DDI check on demand): the gateway's answer is
    // returned verbatim, true or false.
    *h.gateway.ddi_result.lock().unwrap() = Ok(false);
    assert_eq!(h.core.is_ready(true, true).await, Ok(false));
    *h.gateway.ddi_result.lock().unwrap() = Ok(true);
    assert_eq!(h.core.is_ready(true, true).await, Ok(true));

    // A DDI error maps through the mount catch-all.
    *h.gateway.ddi_result.lock().unwrap() =
        Err(mm_gateway::unsupported("isDDIMounted"));
    let err = err_of(h.core.is_ready(true, true).await);
    match err {
        MinimuxerError::Mount { protocol, reason } => {
            assert_eq!(protocol, PairingProtocol::Lockdown);
            assert!(reason.contains("isDDIMounted"), "reason: {reason}");
        }
        other => panic!("expected Mount, got {other:?}"),
    }
    h.core.stop().await.unwrap();
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_start_stop_and_idempotent_stop() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;
    assert_eq!(h.core.status(), CoreStatus::Stopped);

    h.core.start(LOCKDOWN_XML, "/tmp/docs", None).await.unwrap();
    assert_eq!(h.core.status(), CoreStatus::Started);
    assert_eq!(h.core.pairing_file_type(), PairingProtocol::Lockdown);
    assert!(h.core.is_pairing_file_loaded());
    // The heartbeat lives with start (714aafa3 keep-alive fix).
    for _ in 0..200 {
        if h.core.heartbeat().is_task_active() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(h.core.heartbeat().is_task_active());

    h.core.stop().await.unwrap();
    assert_eq!(h.core.status(), CoreStatus::Stopped);
    assert_eq!(h.core.pairing_file_type(), PairingProtocol::Unknown);
    assert!(!h.core.heartbeat().is_task_active());

    // Stopping a stopped core is a no-op success (Swift parity).
    h.core.stop().await.unwrap();
    assert_eq!(h.core.status(), CoreStatus::Stopped);
}

#[tokio::test]
async fn start_failures_map_and_leave_status_in_progress() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;

    // Garbage content: the parser's PairingError reaches the
    // invalidPairing catch-all with protocol unknown.
    let err = h
        .core
        .start("garbage", "/tmp/docs", None)
        .await
        .expect_err("garbage must fail");
    match err {
        MinimuxerError::InvalidPairing { protocol, .. } => {
            assert_eq!(protocol, PairingProtocol::Unknown);
        }
        other => panic!("expected InvalidPairing, got {other:?}"),
    }
    // Swift has no rollback: status stays inProgress.
    assert_eq!(h.core.status(), CoreStatus::InProgress);

    // A gateway invalidPairingFile error maps verbatim.
    *h.gateway.start_error.lock().unwrap() = Some(
        DeviceGatewayError::new(DeviceGatewayCode::InvalidPairingFile, "bad file")
            .to_info(),
    );
    let err = h
        .core
        .start(LOCKDOWN_XML, "/tmp/docs", None)
        .await
        .expect_err("injected error must fail");
    assert_eq!(
        err,
        MinimuxerError::InvalidPairing {
            protocol: PairingProtocol::Unknown,
            reason: "bad file".to_string(),
        }
    );
}

#[tokio::test]
async fn restart_flows() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;

    // No pairing anywhere: restart reports the missing file.
    let err = h.core.restart().await.expect_err("restart must fail");
    assert_eq!(
        err,
        MinimuxerError::InvalidPairing {
            protocol: PairingProtocol::Unknown,
            reason: "No existing pairing file found in gateway during restart"
                .to_string(),
        }
    );

    // Pairing loaded but start() never ran: lastDocsPath is nil.
    h.gateway.start(LOCKDOWN_XML, None).await.unwrap();
    let err = h.core.restart().await.expect_err("restart must fail");
    assert_eq!(
        err,
        MinimuxerError::Mount {
            protocol: PairingProtocol::Lockdown,
            reason:
                "start() should be invoked before requesting restart. cause: lastDocsPath is nil"
                    .to_string(),
        }
    );

    // After a real start, restart and reinit both succeed.
    h.core.start(LOCKDOWN_XML, "/tmp/docs", None).await.unwrap();
    h.core.restart().await.unwrap();
    assert_eq!(h.core.status(), CoreStatus::Started);
    h.core
        .reinitialize_pairing_data(LOCKDOWN_XML)
        .await
        .unwrap();
    assert_eq!(h.core.status(), CoreStatus::Started);
    h.core.stop().await.unwrap();
}

// ---------------------------------------------------------------------------
// Status bus
// ---------------------------------------------------------------------------

#[tokio::test]
async fn status_bus_republishes_reprobe_on_network_change() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;
    let _listener = start_ready(&h).await;

    let published: Arc<Mutex<Vec<Result<bool, MinimuxerError>>>> =
        Arc::new(Mutex::new(Vec::new()));
    let sink = published.clone();
    h.core.subscribe_status(Arc::new(move |result| {
        sink.lock().unwrap().push(result);
    }));

    h.stack.handle_network_change().await;
    {
        let log = published.lock().unwrap();
        assert!(!log.is_empty(), "a status must be published");
        assert_eq!(log.last().unwrap(), &Ok(true));
    }

    // Break gate 8: the next network change republishes the
    // re-probe's error, identical to a direct is_ready call.
    *h.gateway.fetch_result.lock().unwrap() = Err(DeviceGatewayError::new(
        DeviceGatewayCode::ServiceError,
        "later failure",
    )
    .to_info());
    h.stack.handle_network_change().await;
    let direct = h.core.is_ready(true, false).await;
    let log = published.lock().unwrap();
    assert_eq!(log.last().unwrap(), &direct);
    assert_eq!(
        log.last().unwrap(),
        &Err(MinimuxerError::FetchUDID("later failure".to_string()))
    );
    h.core.stop().await.unwrap();
}

// ---------------------------------------------------------------------------
// set_params (facade Minimuxer.set)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn set_params_switches_backend_and_carries_stored_port() {
    let _g = lock();
    let h = harness(DeviceConnectionMode::LocalVpn, GatewayBackend::Idevice, None).await;
    assert_eq!(h.provider.created.lock().unwrap().len(), 0);

    // Backend switch: fresh gateway, facade's stored RP port
    // (the 49152 default) applied to it.
    h.core.set_params(MinimuxerParams {
        backend: Some(GatewayBackend::Libimobiledevice),
        ..Default::default()
    });
    assert_eq!(h.provider.backend(), GatewayBackend::Libimobiledevice);
    assert_eq!(h.provider.created.lock().unwrap().len(), 1);
    assert_eq!(
        h.provider
            .current_fake()
            .get_port(PairingProtocol::RpPairing),
        49152
    );

    // Port change applies to the current gateway.
    h.core.set_params(MinimuxerParams {
        remote_pairing_port: Some(50001),
        ..Default::default()
    });
    assert_eq!(
        h.provider
            .current_fake()
            .get_port(PairingProtocol::RpPairing),
        50001
    );

    // Switching back carries the *stored* port (50001).
    h.core.set_params(MinimuxerParams {
        backend: Some(GatewayBackend::Idevice),
        ..Default::default()
    });
    assert_eq!(h.provider.created.lock().unwrap().len(), 2);
    assert_eq!(
        h.provider
            .current_fake()
            .get_port(PairingProtocol::RpPairing),
        50001
    );

    // Unchanged fields are no-ops: no new gateway is built.
    h.core.set_params(MinimuxerParams {
        backend: Some(GatewayBackend::Idevice),
        remote_pairing_port: Some(50001),
        ..Default::default()
    });
    assert_eq!(h.provider.created.lock().unwrap().len(), 2);

    // Probe timeout flows to the connection manager.
    h.core.set_params(MinimuxerParams {
        device_probe_timeout: Some(999),
        ..Default::default()
    });
    assert_eq!(h.core.device_probe_timeout(), 999);
}

// ---------------------------------------------------------------------------
// runWithChecks mapping (free functions)
// ---------------------------------------------------------------------------

#[test]
fn map_gateway_error_table() {
    let catch = |reason: String| MinimuxerError::FetchUDID(reason);

    // invalidPairingFile -> invalidPairing with the active protocol.
    let info = DeviceGatewayError::new(
        DeviceGatewayCode::InvalidPairingFile,
        "bad file",
    )
    .to_info();
    assert_eq!(
        map_gateway_error(info, PairingProtocol::Lockdown, catch),
        MinimuxerError::InvalidPairing {
            protocol: PairingProtocol::Lockdown,
            reason: "bad file".to_string(),
        }
    );

    // VPN-drop reasons -> invalidVPN regardless of catch-all.
    for (code, reason) in [
        (DeviceGatewayCode::ConnectionFailed, "Broken pipe (os error 32)"),
        (DeviceGatewayCode::ServiceError, "early eof"),
        (DeviceGatewayCode::ConnectionFailed, "Connection reset by peer"),
    ] {
        let info = DeviceGatewayError::new(code, reason).to_info();
        assert_eq!(
            map_gateway_error(info, PairingProtocol::Lockdown, catch),
            MinimuxerError::InvalidVPN(reason.to_string()),
            "{reason}"
        );
    }

    // Other gateway errors land in the catch-all.
    let info = DeviceGatewayError::new(
        DeviceGatewayCode::ConnectionFailed,
        "TLS handshake failed",
    )
    .to_info();
    assert_eq!(
        map_gateway_error(info, PairingProtocol::Lockdown, catch),
        MinimuxerError::FetchUDID("TLS handshake failed".to_string())
    );

    // A MinimuxerError passes through untouched.
    let info = MinimuxerError::NoVPN("nope".to_string()).to_info();
    assert_eq!(
        map_gateway_error(info, PairingProtocol::Lockdown, catch),
        MinimuxerError::NoVPN("nope".to_string())
    );

    // Anything else (e.g. PairingError) lands in the catch-all
    // with the ported description.
    let info = MmErrorInfo::new("PairingError", "invalidPlist", "bad plist");
    let expected = info.description();
    assert_eq!(
        map_gateway_error(info, PairingProtocol::Lockdown, catch),
        MinimuxerError::FetchUDID(expected)
    );
}

#[tokio::test]
async fn run_with_checks_free_function() {
    let ok: Result<u32, MinimuxerError> = run_with_checks(
        || PairingProtocol::Lockdown,
        |reason, _| MinimuxerError::FetchUDID(reason),
        async { Ok::<u32, MmErrorInfo>(7) },
    )
    .await;
    assert_eq!(ok, Ok(7));

    let err: Result<u32, MinimuxerError> = run_with_checks(
        || PairingProtocol::Lockdown,
        |reason, _| MinimuxerError::FetchUDID(reason),
        async {
            Err::<u32, MmErrorInfo>(
                DeviceGatewayError::new(
                    DeviceGatewayCode::ConnectionFailed,
                    "Broken pipe",
                )
                .to_info(),
            )
        },
    )
    .await;
    assert_eq!(
        err,
        Err(MinimuxerError::InvalidVPN("Broken pipe".to_string()))
    );
}

#[test]
fn system_os_version_unreadable_is_never_at_least() {
    // Off Apple targets the sysctl read yields nothing, so the
    // 26.4 gate stays closed — the iOS 15 target behaviour.
    let v = mm_core::SystemOsVersion::new();
    #[cfg(not(target_vendor = "apple"))]
    assert!(!v.at_least(26, 4));
    #[cfg(target_vendor = "apple")]
    let _ = v.at_least(0, 0);
}
