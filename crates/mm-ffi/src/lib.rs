//! mm-ffi — the swift-bridge boundary of the minimuxer Rust rewrite.
//!
//! M1 skeleton: global tokio runtime ownership, EventSink registration,
//! and the error/type conversion bus, exposing the smallest useful
//! surface — version query, `MmErrorInfo` crossing, the mm-common
//! pairing-parse entry, the TCP probe, and an async ping proving the
//! async bridge. Subsystem APIs land here milestone by milestone.
//!
//! M2 adds the mm-net surface: a global `NetStack` (connection
//! manager + endpoint + polling observer over the live scan source),
//! scan/snapshot queries, endpoint and connection-mode queries, an
//! async refresh entry, observer start/stop, and the
//! `MmConnectionConfig` Swift object presented to the manager as its
//! `ConnectionConfigBinding` — including the async service-port
//! resolver, which crosses as a request on the config object plus an
//! `mm_net_complete_service_port` completion call (10 s timeout,
//! treated as "port unchanged", matching Swift's always-answering
//! resolver contract as closely as the boundary allows).
//!
//! M3 adds the mm-gateway basic chain and the mm-core usbmuxd
//! proxy: a global `GatewayHub` owns the current gateway
//! (backend selection with Swift's switch semantics), the
//! manager's port source now resolves against the real gateway
//! (the M2 `TablePortSource` stand-in and its
//! `mm_net_set_pairing_protocol` driver are retired), endpoint
//! updates flow into the gateway through the hub, and the proxy
//! is wired to the hub and to the NetStack's attach feed.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

mod gateway_hub;

use gateway_hub::GatewayHub;
use mm_gateway::GatewayProvider as _;

/// The single multi-threaded tokio runtime every bridge call spawns on
/// (design §3.3). Held for the App's lifetime.
pub fn global_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("mm-ffi")
            .build()
            .expect("failed to build mm-ffi tokio runtime")
    })
}

#[swift_bridge::bridge]
mod ffi {
    // FFI twin of `mm_common::MmErrorInfo` — the only error shape that
    // crosses the boundary (domain = Swift error type name, code =
    // Swift case name verbatim, reason = payload string).
    #[swift_bridge(swift_repr = "struct")]
    struct MmErrorInfo {
        domain: String,
        code: String,
        reason: String,
    }

    // FFI twin of `mm_common::PairingProtocol`.
    enum MmPairingProtocol {
        RpPairing,
        Lockdown,
        Unknown,
    }

    // Result of a pairing-file parse: on failure `error` carries the
    // PairingError as MmErrorInfo and `protocol` is `Unknown`.
    #[swift_bridge(swift_repr = "struct")]
    struct MmPairingOutcome {
        success: bool,
        pairing_protocol: MmPairingProtocol,
        error: MmErrorInfo,
    }

    // FFI twin of `mm_net::DeviceConnectionMode`.
    enum MmConnectionMode {
        LocalVpn,
        RemoteServer,
        NotConfigured,
    }

    // One row of `mm_net::active_interfaces` (Swift
    // `LocalInterfaceInfo`), flattened for the bridge. `ipv6` uses
    // the empty string for "no IPv6 address" (swift-bridge does not
    // support Option<String> struct fields).
    #[swift_bridge(swift_repr = "struct")]
    struct MmInterfaceInfo {
        name: String,
        ip: String,
        ipv6: String,
        subnet: String,
        interface_type: String,
        is_vpn: bool,
    }

    // A fresh interface scan plus the satisfaction predicates
    // (Swift `NetworkObserverService` getters).
    #[swift_bridge(swift_repr = "struct")]
    struct MmNetSnapshot {
        interfaces: Vec<MmInterfaceInfo>,
        wifi_satisfied: bool,
        wired_satisfied: bool,
        usb_satisfied: bool,
        bridge_satisfied: bool,
        utun_available: bool,
        ipsec_available: bool,
    }

    // Result of reading the device endpoint: on failure `error`
    // carries the InternalError as MmErrorInfo (e.g.
    // deviceEndpointNotInitialized).
    #[swift_bridge(swift_repr = "struct")]
    struct MmEndpointOutcome {
        success: bool,
        ip: String,
        error: MmErrorInfo,
    }

    // FFI twin of `mm_gateway::GatewayBackend` (Swift
    // `GatewayBackend`): the App-visible backend choice.
    enum MmGatewayBackend {
        Idevice,
        Libimobiledevice,
    }

    // Outcome of a gateway operation with no payload.
    #[swift_bridge(swift_repr = "struct")]
    struct MmOutcome {
        success: bool,
        error: MmErrorInfo,
    }

    // Outcome of a gateway operation returning a string
    // (fetchUDID / getLockdownValue).
    #[swift_bridge(swift_repr = "struct")]
    struct MmStringOutcome {
        success: bool,
        value: String,
        error: MmErrorInfo,
    }

    // Outcome of opening a service channel: the service's
    // coordinates on success (the connection itself stays on
    // the Rust side, held by the gateway).
    #[swift_bridge(swift_repr = "struct")]
    struct MmServiceOutcome {
        success: bool,
        service_name: String,
        port: u16,
        enable_ssl: bool,
        error: MmErrorInfo,
    }

    // Outcome of a readiness query (isReady / isDDIMounted /
    // mountDDI): `ready` carries the Bool on success — the one
    // Bool the EventSink status shape cannot carry (diff list
    // D-M4-3).
    #[swift_bridge(swift_repr = "struct")]
    struct MmReadyOutcome {
        success: bool,
        ready: bool,
        error: MmErrorInfo,
    }

    // Outcome of an AFC directory listing.
    #[swift_bridge(swift_repr = "struct")]
    struct MmListOutcome {
        success: bool,
        entries: Vec<String>,
        error: MmErrorInfo,
    }

    // Outcome of an AFC file read.
    #[swift_bridge(swift_repr = "struct")]
    struct MmDataOutcome {
        success: bool,
        data: Vec<u8>,
        error: MmErrorInfo,
    }

    // Outcome of an AFC file-info query (FFI twin of
    // `mm_gateway::AfcFileInfo`).
    #[swift_bridge(swift_repr = "struct")]
    struct MmAfcFileInfoOutcome {
        success: bool,
        is_directory: bool,
        file_size: i64,
        error: MmErrorInfo,
    }

    // Outcome of a wireless pairing operation (M8): the
    // PairedDeviceRecord flattened for the bridge; the
    // pairing file crosses as its raw plist text (both
    // pairing formats are XML plists).
    #[swift_bridge(swift_repr = "struct")]
    struct MmPairedDeviceOutcome {
        success: bool,
        name: String,
        model: String,
        pairing_file_path: String,
        pairing_file_content: String,
        error: MmErrorInfo,
    }

    // Return of the sink's resolve_file_name: swift-bridge
    // has no Option<String> return across an extern Swift
    // fn, so "no resolver / no answer" is the flag.
    #[swift_bridge(swift_repr = "struct")]
    struct MmResolveOutcome {
        has_resolver: bool,
        file_name: String,
    }

    // Wireless pairing service state (M8 status query).
    #[swift_bridge(swift_repr = "struct")]
    struct MmWirelessState {
        start_active: bool,
        active_triggers: i32,
        advertising: bool,
    }

    // FFI twin of `mm_core::ProfileDumpMode`.
    enum MmProfileDumpMode {
        Raw,
        Zip,
    }

    // FFI twin of `mm_core::CoreStatus` (Swift
    // `MinimuxerStatus`).
    enum MmCoreStatus {
        Started,
        InProgress,
        Stopped,
    }

    extern "Rust" {
        fn mm_version() -> String;
        fn mm_parse_pairing_file(
            content: String,
            preferred: Option<MmPairingProtocol>,
        ) -> MmPairingOutcome;
        fn mm_describe_error(info: MmErrorInfo) -> String;
        fn mm_test_tcp(ip: String, port: u16, timeout_ms: i32) -> bool;
        async fn mm_async_ping() -> String;
        fn mm_register_event_sink(sink: MmEventSink);
        fn mm_clear_event_sink();
        fn mm_emit_log(message: String);
        fn mm_emit_status(success: bool, domain: String, code: String, reason: String);

        // mm-net (M2).
        fn mm_net_scan_interfaces() -> Vec<MmInterfaceInfo>;
        fn mm_net_snapshot() -> MmNetSnapshot;
        fn mm_net_endpoint() -> MmEndpointOutcome;
        fn mm_net_connection_mode() -> MmConnectionMode;
        async fn mm_net_refresh() -> bool;
        fn mm_net_observer_start() -> bool;
        fn mm_net_observer_stop() -> bool;
        fn mm_net_bind_connection_config(config: MmConnectionConfig);
        fn mm_net_clear_connection_config();
        fn mm_net_set_probe_timeout(timeout_ms: i32);
        fn mm_net_complete_service_port(
            request_id: u64,
            pairing_protocol: MmPairingProtocol,
            port: u16,
        ) -> bool;

        // mm-gateway (M3): the device-gateway basic chain —
        // backend selection, start/stop, endpoint feed, port
        // surface, UDID / lockdown values, session, services.
        fn mm_gateway_backend() -> MmGatewayBackend;
        fn mm_gateway_set_backend(backend: MmGatewayBackend);
        fn mm_gateway_requires_usbmuxd() -> bool;
        async fn mm_gateway_start(
            content: String,
            preferred: Option<MmPairingProtocol>,
        ) -> MmOutcome;
        async fn mm_gateway_stop() -> MmOutcome;
        fn mm_gateway_set_endpoint_ip(ip: Option<String>);
        fn mm_gateway_pairing_protocol() -> MmPairingProtocol;
        fn mm_gateway_service_port(pairing_protocol: MmPairingProtocol) -> u16;
        fn mm_gateway_set_port(port: u16, pairing_protocol: MmPairingProtocol);
        async fn mm_gateway_fetch_udid() -> MmStringOutcome;
        async fn mm_gateway_get_lockdown_value(key: String) -> MmStringOutcome;
        async fn mm_gateway_start_session() -> MmOutcome;
        async fn mm_gateway_stop_session() -> MmOutcome;
        async fn mm_gateway_open_service(service_name: String) -> MmServiceOutcome;
        fn mm_gateway_set_logging(enabled: bool);

        // mm-core usbmuxd proxy (M3): the in-App usbmuxd the
        // libimobiledevice backend is retargeted to.
        fn mm_usbmuxd_retarget();
        async fn mm_proxy_start() -> MmOutcome;
        async fn mm_proxy_stop();
        fn mm_proxy_is_listening() -> bool;
        fn mm_proxy_notify_attached(peer_ip: String);
        fn mm_proxy_notify_detached();

        // mm-core (M4): the MinimuxerImpl lifecycle face —
        // start/stop/restart, the nine-gate isReady, the facade
        // parameter set, and the status/heartbeat readouts. The
        // status bus itself crosses via the EventSink's
        // on_status (subscribed inside core_globals).
        async fn mm_core_start(
            content: String,
            docs_path: String,
            preferred: Option<MmPairingProtocol>,
        ) -> MmOutcome;
        async fn mm_core_stop() -> MmOutcome;
        async fn mm_core_restart() -> MmOutcome;
        async fn mm_core_reinitialize_pairing_data(content: String) -> MmOutcome;
        fn mm_core_bind_connection_config(config: MmConnectionConfig) -> MmOutcome;
        async fn mm_core_is_ready(
            with_network_check: bool,
            with_ddi_mount_check: bool,
        ) -> MmReadyOutcome;
        async fn mm_core_fetch_udid() -> MmStringOutcome;
        async fn mm_core_is_ddi_mounted() -> MmReadyOutcome;
        async fn mm_core_mount_ddi(docs_path: String) -> MmReadyOutcome;
        async fn mm_core_send_ipa_afc(bundle_id: String, ipa: Vec<u8>) -> MmOutcome;
        async fn mm_core_send_app_bundle_afc(
            bundle_id: String,
            app_path: String,
        ) -> MmOutcome;
        async fn mm_core_install_ipa(bundle_id: String) -> MmOutcome;
        async fn mm_core_install_app_bundle(
            bundle_id: String,
            app_name: String,
        ) -> MmOutcome;
        async fn mm_core_remove_app(bundle_id: String) -> MmOutcome;
        async fn mm_core_wipe_container(bundle_id: String) -> MmOutcome;
        async fn mm_core_debug_app(bundle_id: String) -> MmOutcome;
        async fn mm_core_attach_debugger(pid: u32) -> MmOutcome;
        async fn mm_core_install_profile(profile: Vec<u8>) -> MmOutcome;
        async fn mm_core_remove_profile(profile_id: String) -> MmOutcome;
        async fn mm_core_dump_profiles(
            docs_path: String,
            mode: MmProfileDumpMode,
        ) -> MmStringOutcome;
        async fn mm_core_afc_list_directory(
            bundle_id: String,
            path: String,
        ) -> MmListOutcome;
        async fn mm_core_afc_read_file(
            bundle_id: String,
            path: String,
        ) -> MmDataOutcome;
        async fn mm_core_afc_get_file_info(
            bundle_id: String,
            path: String,
        ) -> MmAfcFileInfoOutcome;
        // mm-emproxy (M6): the EMProxyAPI surface over the
        // instance held by the core.
        fn mm_emproxy_set_handshake_client(host: String, port: u16, enabled: bool);
        async fn mm_emproxy_start(host: String, port: u16) -> MmOutcome;
        async fn mm_emproxy_start_default() -> MmOutcome;
        async fn mm_emproxy_stop() -> MmOutcome;
        fn mm_emproxy_is_running() -> bool;
        // Wireless pairing (M8): the WirelessPairService
        // face of the core. The sink carries the App-side
        // handlers (PIN delivery, readiness, PIN requests
        // by id, pairing-file-name resolution).
        fn mm_wireless_bind_sink(sink: MmWirelessPairSink);
        fn mm_wireless_clear_sink();
        async fn mm_wireless_start(
            host_name: String,
            host_model: String,
            out_path: String,
        ) -> MmPairedDeviceOutcome;
        async fn mm_wireless_trigger(
            target_ip: String,
            target_port: u16,
            host_name: String,
            host_model: String,
            out_path: String,
        ) -> MmPairedDeviceOutcome;
        fn mm_wireless_stop();
        fn mm_wireless_complete_pin(request_id: u64, pin: String) -> bool;
        fn mm_wireless_state() -> MmWirelessState;
        fn mm_core_set_params(
            backend: Option<MmGatewayBackend>,
            remote_pairing_port: Option<u16>,
            device_probe_timeout: Option<i32>,
        );
        fn mm_core_status() -> MmCoreStatus;
        fn mm_core_set_logging(enabled: bool);
        fn mm_core_is_pairing_file_loaded() -> bool;
        fn mm_core_last_beat_successful() -> bool;
    }

    extern "Swift" {
        type MmEventSink;
        fn on_log(self: &MmEventSink, message: String);
        fn on_status(
            self: &MmEventSink,
            seq: u64,
            success: bool,
            domain: String,
            code: String,
            reason: String,
        );

        // The App-side ConnectionConfig presented to the Rust
        // connection manager as its ConnectionConfigBinding
        // (7 setters / 3 getters synchronously; the async
        // resolveServicePort crosses as request + completion).
        type MmConnectionConfig;
        fn set_tunnel_iface_ip(self: &MmConnectionConfig, ip: Option<String>);
        fn set_tunnel_peer_ip(self: &MmConnectionConfig, ip: Option<String>);
        fn set_tunnel_peer_subnet_mask(self: &MmConnectionConfig, mask: Option<String>);
        fn set_tunnel_peer_reachable(self: &MmConnectionConfig, reachable: bool);
        fn set_tunnel_iface_subnet_mask(self: &MmConnectionConfig, mask: Option<String>);
        fn set_remote_reachable(self: &MmConnectionConfig, reachable: bool);
        fn set_override_tunnel_peer_reachable(
            self: &MmConnectionConfig,
            reachable: bool,
        );
        fn get_connection_mode(self: &MmConnectionConfig) -> MmConnectionMode;
        fn get_override_tunnel_peer_ip(self: &MmConnectionConfig) -> String;
        fn get_remote_server_ip(self: &MmConnectionConfig) -> String;
        fn request_service_port(
            self: &MmConnectionConfig,
            request_id: u64,
            pairing_protocol: MmPairingProtocol,
            port: u16,
        );

        // The App-side wireless pairing handlers (M8):
        // Swift WirelessPairService's callback properties
        // as one object. request_pin returns whether the
        // App has a PIN handler and issued the request;
        // the answer returns via mm_wireless_complete_pin.
        type MmWirelessPairSink;
        fn on_pin_received(self: &MmWirelessPairSink, pin: String);
        fn on_ready_to_pair(
            self: &MmWirelessPairSink,
            service_id: String,
            port: i32,
        );
        fn request_pin(self: &MmWirelessPairSink, request_id: u64) -> bool;
        fn resolve_file_name(
            self: &MmWirelessPairSink,
            name: String,
            model: String,
        ) -> MmResolveOutcome;
    }
}

// ---------------------------------------------------------------------------
// Conversions between the bridge types and the mm-common domain types.
// ---------------------------------------------------------------------------

impl From<mm_common::MmErrorInfo> for ffi::MmErrorInfo {
    fn from(info: mm_common::MmErrorInfo) -> Self {
        Self {
            domain: info.domain,
            code: info.code,
            reason: info.reason,
        }
    }
}

impl From<ffi::MmErrorInfo> for mm_common::MmErrorInfo {
    fn from(info: ffi::MmErrorInfo) -> Self {
        Self {
            domain: info.domain,
            code: info.code,
            reason: info.reason,
        }
    }
}

impl ffi::MmPairingProtocol {
    fn to_common(self) -> mm_common::PairingProtocol {
        match self {
            ffi::MmPairingProtocol::RpPairing => mm_common::PairingProtocol::RpPairing,
            ffi::MmPairingProtocol::Lockdown => mm_common::PairingProtocol::Lockdown,
            ffi::MmPairingProtocol::Unknown => mm_common::PairingProtocol::Unknown,
        }
    }

    fn from_common(protocol: mm_common::PairingProtocol) -> Self {
        match protocol {
            mm_common::PairingProtocol::RpPairing => ffi::MmPairingProtocol::RpPairing,
            mm_common::PairingProtocol::Lockdown => ffi::MmPairingProtocol::Lockdown,
            mm_common::PairingProtocol::Unknown => ffi::MmPairingProtocol::Unknown,
        }
    }
}

// ---------------------------------------------------------------------------
// EventSink registry (design §1/§3.3): the shell registers one sink; Rust
// only holds the bridge handle and calls it for status/log events.
// ---------------------------------------------------------------------------

struct SinkHolder(ffi::MmEventSink);
// The sink is only ever touched under the registry mutex; the Swift side
// guarantees the object stays alive while registered (shell unregisters
// in deinit), so moving the handle between threads is safe.
unsafe impl Send for SinkHolder {}

static EVENT_SINK: Mutex<Option<SinkHolder>> = Mutex::new(None);
static STATUS_SEQ: AtomicU64 = AtomicU64::new(0);

fn with_sink<R>(f: impl FnOnce(&ffi::MmEventSink) -> R) -> Option<R> {
    let guard = EVENT_SINK.lock().ok()?;
    guard.as_ref().map(|holder| f(&holder.0))
}

fn mm_register_event_sink(sink: ffi::MmEventSink) {
    if let Ok(mut guard) = EVENT_SINK.lock() {
        *guard = Some(SinkHolder(sink));
    }
    // Route mm-common's logging facade into the same sink (design §2:
    // logging callbacks travel the sink path like status events).
    mm_common::logging::set_log_sink(Some(std::sync::Arc::new(|level, msg| {
        let tag = match level {
            mm_common::logging::LogLevel::Debug => "debug",
            mm_common::logging::LogLevel::Verbose => "verbose",
        };
        mm_emit_log(format!("[{tag}] {msg}"));
    })));
    mm_common::logging::set_logging_enabled(true);
}

fn mm_clear_event_sink() {
    if let Ok(mut guard) = EVENT_SINK.lock() {
        *guard = None;
    }
}

fn mm_emit_log(message: String) {
    with_sink(|sink| sink.on_log(message));
}

fn mm_emit_status(success: bool, domain: String, code: String, reason: String) {
    let seq = STATUS_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    with_sink(|sink| sink.on_status(seq, success, domain, code, reason));
}

// ---------------------------------------------------------------------------
// Bridge entry points.
// ---------------------------------------------------------------------------

fn mm_version() -> String {
    format!(
        "minimuxer-rust {} (mm-ffi {}; error-map {} cases)",
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION"),
        mm_common::ERROR_CASES.len()
    )
}

fn mm_parse_pairing_file(
    content: String,
    preferred: Option<ffi::MmPairingProtocol>,
) -> ffi::MmPairingOutcome {
    let preferred = preferred.map(|p| p.to_common());
    match mm_common::parse_pairing_content(&content, preferred) {
        Ok(file) => ffi::MmPairingOutcome {
            success: true,
            pairing_protocol: ffi::MmPairingProtocol::from_common(file.mode()),
            error: mm_common::MmErrorInfo::new("", "", "").into(),
        },
        Err(err) => ffi::MmPairingOutcome {
            success: false,
            pairing_protocol: ffi::MmPairingProtocol::Unknown,
            error: err.to_info().into(),
        },
    }
}

fn mm_describe_error(info: ffi::MmErrorInfo) -> String {
    mm_common::MmErrorInfo::from(info).description()
}

fn mm_test_tcp(ip: String, port: u16, timeout_ms: i32) -> bool {
    mm_common::test_tcp(&ip, port, timeout_ms)
}

async fn mm_async_ping() -> String {
    // Proves the async bridge end-to-end: the call is driven by the
    // caller's executor, the work itself runs on the global runtime.
    let handle = global_runtime().spawn(async { "pong".to_string() });
    match handle.await {
        Ok(text) => text,
        Err(_) => "join error".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Gateway hub global: the one current gateway for the App's
// lifetime. The connection manager's port source, the endpoint's
// IP sink, and the usbmuxd proxy's gateway provider are all this
// hub (see gateway_hub.rs).
// ---------------------------------------------------------------------------

fn gateway_hub() -> &'static std::sync::Arc<GatewayHub> {
    static HUB: OnceLock<std::sync::Arc<GatewayHub>> = OnceLock::new();
    HUB.get_or_init(|| std::sync::Arc::new(GatewayHub::new()))
}

// ---------------------------------------------------------------------------
// mm-net globals (M2): one NetStack for the App's lifetime — the
// connection manager reads the live scan source, the endpoint is the
// single source of truth, and the polling observer drives refreshes.
// Since M3 the manager's port source is the real gateway (via the
// hub), retiring the M2 TablePortSource stand-in from the wiring.
// ---------------------------------------------------------------------------

struct NetGlobals {
    scan_source: std::sync::Arc<mm_net::LiveScanSource>,
    manager: std::sync::Arc<mm_net::DeviceConnectionManager>,
    endpoint: std::sync::Arc<mm_net::DeviceEndpoint>,
    stack: std::sync::Arc<mm_net::NetStack>,
    observer: std::sync::Arc<mm_net::PollingObserver>,
}

fn net_globals() -> &'static NetGlobals {
    static NET: OnceLock<NetGlobals> = OnceLock::new();
    NET.get_or_init(|| {
        use std::sync::Arc;
        let port_source: Arc<dyn mm_net::ServicePortSource> = gateway_hub().clone();
        let scan_source = Arc::new(mm_net::LiveScanSource);
        let manager = Arc::new(mm_net::DeviceConnectionManager::new(
            port_source,
            scan_source.clone(),
        ));
        let endpoint = Arc::new(mm_net::DeviceEndpoint::new());
        let stack = Arc::new(mm_net::NetStack::new(
            manager.clone(),
            endpoint.clone(),
            scan_source.clone(),
        ));
        // The network-change → status re-probe handler is
        // registered by MinimuxerCore::new (M4) when core_globals
        // initializes; the M2 log-line placeholder that stood in
        // for it is retired. mm_net_observer_start initializes
        // the core globals first so the path is live whenever
        // the observer runs.
        let observer = Arc::new(mm_net::PollingObserver::new(
            stack.clone(),
            scan_source.clone(),
        ));
        NetGlobals {
            scan_source,
            manager,
            endpoint,
            stack,
            observer,
        }
    })
}

// ---------------------------------------------------------------------------
// Gateway globals (M3): the usbmuxd proxy plus the one-time wiring
// between the network layer and the gateway layer — endpoint
// updates flow into the current gateway (hub as EndpointIpSink),
// attach/detach events flow into the proxy (as AttachListener).
// ---------------------------------------------------------------------------

struct GatewayGlobals {
    proxy: std::sync::Arc<mm_core::usbmuxd_proxy::UsbmuxdProxyServer>,
}

fn gateway_globals() -> &'static GatewayGlobals {
    static GW: OnceLock<GatewayGlobals> = OnceLock::new();
    GW.get_or_init(|| {
        use std::sync::Arc;
        let hub = gateway_hub().clone();
        let provider: Arc<dyn mm_gateway::GatewayProvider> = hub.clone();
        let proxy = Arc::new(mm_core::usbmuxd_proxy::UsbmuxdProxyServer::new(provider));
        let sink: Arc<dyn mm_net::EndpointIpSink> = hub.clone();
        net_globals().endpoint.attach_sink(sink);
        let listener: Arc<dyn mm_net::AttachListener> = proxy.clone();
        net_globals().stack.add_attach_listener(listener);
        GatewayGlobals { proxy }
    })
}

impl ffi::MmConnectionMode {
    fn to_net(self) -> mm_net::DeviceConnectionMode {
        match self {
            ffi::MmConnectionMode::LocalVpn => mm_net::DeviceConnectionMode::LocalVpn,
            ffi::MmConnectionMode::RemoteServer => mm_net::DeviceConnectionMode::RemoteServer,
            ffi::MmConnectionMode::NotConfigured => mm_net::DeviceConnectionMode::NotConfigured,
        }
    }

    fn from_net(mode: mm_net::DeviceConnectionMode) -> Self {
        match mode {
            mm_net::DeviceConnectionMode::LocalVpn => ffi::MmConnectionMode::LocalVpn,
            mm_net::DeviceConnectionMode::RemoteServer => ffi::MmConnectionMode::RemoteServer,
            mm_net::DeviceConnectionMode::NotConfigured => ffi::MmConnectionMode::NotConfigured,
        }
    }
}

impl From<mm_net::LocalInterfaceInfo> for ffi::MmInterfaceInfo {
    fn from(info: mm_net::LocalInterfaceInfo) -> Self {
        Self {
            name: info.name,
            ip: info.ip,
            ipv6: info.ipv6.unwrap_or_default(),
            subnet: info.subnet,
            interface_type: info.interface_type.raw_value().to_string(),
            is_vpn: info.interface_type.is_vpn(),
        }
    }
}

// ---------------------------------------------------------------------------
// MmConnectionConfig → ConnectionConfigBinding. Calls into the Swift
// object are serialized on a mutex (the App-side object is not
// documented thread-safe; the manager may call from any runtime
// thread). Same holding pattern as the EventSink registry.
// ---------------------------------------------------------------------------

struct ConfigHolder(ffi::MmConnectionConfig);
// Only ever touched under the holder mutex; the shell keeps the
// object alive while bound and unbinds before releasing it.
unsafe impl Send for ConfigHolder {}

type SharedConfig = std::sync::Arc<Mutex<ConfigHolder>>;

static PORT_REQUESTS: std::sync::LazyLock<
    Mutex<std::collections::HashMap<u64, tokio::sync::oneshot::Sender<mm_net::ServicePort>>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
static NEXT_PORT_REQUEST: AtomicU64 = AtomicU64::new(0);

fn build_binding(holder: SharedConfig) -> mm_net::ConnectionConfigBinding {
    use std::sync::Arc;
    let h = |holder: &SharedConfig| holder.clone();
    let hs = holder.clone();
    let resolver_holder = holder.clone();
    mm_net::ConnectionConfigBinding {
        set_tunnel_iface_ip: Box::new(move |v| {
            hs.lock().unwrap().0.set_tunnel_iface_ip(v.map(str::to_string))
        }),
        set_tunnel_peer_ip: {
            let c = h(&holder);
            Box::new(move |v| c.lock().unwrap().0.set_tunnel_peer_ip(v.map(str::to_string)))
        },
        set_tunnel_peer_subnet_mask: {
            let c = h(&holder);
            Box::new(move |v| {
                c.lock()
                    .unwrap()
                    .0
                    .set_tunnel_peer_subnet_mask(v.map(str::to_string))
            })
        },
        set_tunnel_peer_reachable: {
            let c = h(&holder);
            Box::new(move |v| c.lock().unwrap().0.set_tunnel_peer_reachable(v))
        },
        set_tunnel_iface_subnet_mask: {
            let c = h(&holder);
            Box::new(move |v| {
                c.lock()
                    .unwrap()
                    .0
                    .set_tunnel_iface_subnet_mask(v.map(str::to_string))
            })
        },
        set_remote_reachable: {
            let c = h(&holder);
            Box::new(move |v| c.lock().unwrap().0.set_remote_reachable(v))
        },
        set_override_tunnel_peer_reachable: {
            let c = h(&holder);
            Box::new(move |v| {
                c.lock().unwrap().0.set_override_tunnel_peer_reachable(v)
            })
        },
        get_connection_mode: {
            let c = h(&holder);
            Box::new(move || c.lock().unwrap().0.get_connection_mode().to_net())
        },
        get_override_tunnel_peer_ip: {
            let c = h(&holder);
            Box::new(move || c.lock().unwrap().0.get_override_tunnel_peer_ip())
        },
        get_remote_server_ip: {
            let c = h(&holder);
            Box::new(move || c.lock().unwrap().0.get_remote_server_ip())
        },
        resolve_service_port: Some(Arc::new(move |current: mm_net::ServicePort| {
            let holder = resolver_holder.clone();
            Box::pin(async move {
                let request_id = NEXT_PORT_REQUEST.fetch_add(1, Ordering::Relaxed) + 1;
                let (tx, rx) = tokio::sync::oneshot::channel();
                PORT_REQUESTS.lock().unwrap().insert(request_id, tx);
                holder.lock().unwrap().0.request_service_port(
                    request_id,
                    ffi::MmPairingProtocol::from_common(current.protocol_type),
                    current.port,
                );
                match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
                    Ok(Ok(resolved)) => resolved,
                    _ => {
                        PORT_REQUESTS.lock().unwrap().remove(&request_id);
                        // No completion within 10 s: treat as "port
                        // unchanged" (Swift's resolver always
                        // answers; this is the boundary fallback).
                        current
                    }
                }
            })
        })),
    }
}

// ---------------------------------------------------------------------------
// mm-net bridge entry points.
// ---------------------------------------------------------------------------

fn mm_net_scan_interfaces() -> Vec<ffi::MmInterfaceInfo> {
    net_globals()
        .stack
        .active_interfaces()
        .into_iter()
        .map(ffi::MmInterfaceInfo::from)
        .collect()
}

fn mm_net_snapshot() -> ffi::MmNetSnapshot {
    use mm_net::ScanSource;
    let g = net_globals();
    let snap = g.scan_source.snapshot();
    ffi::MmNetSnapshot {
        interfaces: mm_net::active_interfaces(&snap.interfaces)
            .into_iter()
            .map(ffi::MmInterfaceInfo::from)
            .collect(),
        wifi_satisfied: mm_net::is_wifi_satisfied(&snap),
        wired_satisfied: mm_net::is_wired_satisfied(&snap),
        usb_satisfied: mm_net::is_usb_satisfied(&snap.interfaces),
        bridge_satisfied: mm_net::is_bridge_satisfied(&snap.interfaces),
        utun_available: mm_net::is_utun_available(&snap.interfaces),
        ipsec_available: mm_net::is_ikev2_ipsec_available(&snap.interfaces),
    }
}

fn mm_net_endpoint() -> ffi::MmEndpointOutcome {
    match net_globals().endpoint.ip() {
        Ok(ip) => ffi::MmEndpointOutcome {
            success: true,
            ip,
            error: mm_common::MmErrorInfo::new("", "", "").into(),
        },
        Err(err) => ffi::MmEndpointOutcome {
            success: false,
            ip: String::new(),
            error: err.to_info().into(),
        },
    }
}

fn mm_net_connection_mode() -> ffi::MmConnectionMode {
    ffi::MmConnectionMode::from_net(net_globals().manager.get_preferred_connection_mode())
}

async fn mm_net_refresh() -> bool {
    net_globals().stack.refresh_endpoint().await
}

fn mm_net_observer_start() -> bool {
    // Wire the core (and with it the status re-probe handler)
    // before the observer can fire any network change.
    let _ = core_globals();
    net_globals().observer.start(global_runtime().handle())
}

fn mm_net_observer_stop() -> bool {
    net_globals().observer.stop()
}

fn mm_net_bind_connection_config(config: ffi::MmConnectionConfig) {
    let holder: SharedConfig = std::sync::Arc::new(Mutex::new(ConfigHolder(config)));
    net_globals()
        .manager
        .bind_connection_config(build_binding(holder));
}

fn mm_net_clear_connection_config() {
    net_globals().manager.clear_connection_config();
}

fn mm_net_set_probe_timeout(timeout_ms: i32) {
    net_globals().manager.set_device_probe_timeout(timeout_ms);
}

fn mm_net_complete_service_port(
    request_id: u64,
    pairing_protocol: ffi::MmPairingProtocol,
    port: u16,
) -> bool {
    let sender = PORT_REQUESTS.lock().unwrap().remove(&request_id);
    match sender {
        Some(tx) => tx
            .send(mm_net::ServicePort::new(pairing_protocol.to_common(), port))
            .is_ok(),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// mm-gateway bridge entry points (M3). Async operations run on
// the global runtime: the gateway's I/O needs the tokio reactor,
// while bridge async functions are driven by the caller's
// executor — the same pattern as mm_async_ping, generalized.
// ---------------------------------------------------------------------------

impl ffi::MmGatewayBackend {
    fn to_gateway(self) -> mm_gateway::GatewayBackend {
        match self {
            ffi::MmGatewayBackend::Idevice => mm_gateway::GatewayBackend::Idevice,
            ffi::MmGatewayBackend::Libimobiledevice => {
                mm_gateway::GatewayBackend::Libimobiledevice
            }
        }
    }

    fn from_gateway(backend: mm_gateway::GatewayBackend) -> Self {
        match backend {
            mm_gateway::GatewayBackend::Idevice => ffi::MmGatewayBackend::Idevice,
            mm_gateway::GatewayBackend::Libimobiledevice => {
                ffi::MmGatewayBackend::Libimobiledevice
            }
        }
    }
}

fn empty_error() -> ffi::MmErrorInfo {
    mm_common::MmErrorInfo::new("", "", "").into()
}

fn outcome_from(result: Result<(), mm_common::MmErrorInfo>) -> ffi::MmOutcome {
    match result {
        Ok(()) => ffi::MmOutcome {
            success: true,
            error: empty_error(),
        },
        Err(e) => ffi::MmOutcome {
            success: false,
            error: e.into(),
        },
    }
}

fn string_outcome_from(
    result: Result<String, mm_common::MmErrorInfo>,
) -> ffi::MmStringOutcome {
    match result {
        Ok(value) => ffi::MmStringOutcome {
            success: true,
            value,
            error: empty_error(),
        },
        Err(e) => ffi::MmStringOutcome {
            success: false,
            value: String::new(),
            error: e.into(),
        },
    }
}

/// Spawn a gateway future on the global runtime and await its
/// result; a join failure becomes an internal error info.
async fn spawn_gateway<T, F>(fut: F) -> Result<T, mm_common::MmErrorInfo>
where
    F: std::future::Future<Output = Result<T, mm_common::MmErrorInfo>> + Send + 'static,
    T: Send + 'static,
{
    match global_runtime().spawn(fut).await {
        Ok(result) => result,
        Err(e) => Err(mm_common::MmErrorInfo::new(
            "MinimuxerInternalError",
            "unknown",
            &format!("gateway task failed: {e}"),
        )),
    }
}

fn mm_gateway_backend() -> ffi::MmGatewayBackend {
    ffi::MmGatewayBackend::from_gateway(gateway_hub().backend())
}

fn mm_gateway_set_backend(backend: ffi::MmGatewayBackend) {
    gateway_hub().set_backend(backend.to_gateway());
}

fn mm_gateway_requires_usbmuxd() -> bool {
    gateway_hub().ensure_gateway().requires_usbmuxd()
}

async fn mm_gateway_start(
    content: String,
    preferred: Option<ffi::MmPairingProtocol>,
) -> ffi::MmOutcome {
    // Ensure the endpoint/attach wiring exists before the
    // gateway can be started.
    let _ = gateway_globals();
    let gw = gateway_hub().ensure_gateway();
    let preferred = preferred.map(|p| p.to_common());
    outcome_from(
        spawn_gateway(async move { gw.start(&content, preferred).await }).await,
    )
}

async fn mm_gateway_stop() -> ffi::MmOutcome {
    let gw = gateway_hub().ensure_gateway();
    outcome_from(spawn_gateway(async move { gw.stop().await }).await)
}

fn mm_gateway_set_endpoint_ip(ip: Option<String>) {
    // The same EndpointIpSink path the network layer's pushes
    // take.
    use mm_net::EndpointIpSink;
    gateway_hub().set_device_endpoint_ip(ip.as_deref());
}

fn mm_gateway_pairing_protocol() -> ffi::MmPairingProtocol {
    use mm_net::ServicePortSource;
    ffi::MmPairingProtocol::from_common(gateway_hub().pairing_file_type())
}

fn mm_gateway_service_port(pairing_protocol: ffi::MmPairingProtocol) -> u16 {
    gateway_hub()
        .ensure_gateway()
        .get_port(pairing_protocol.to_common())
}

fn mm_gateway_set_port(port: u16, pairing_protocol: ffi::MmPairingProtocol) {
    use mm_net::ServicePortSource;
    gateway_hub().set_port(port, pairing_protocol.to_common());
}

async fn mm_gateway_fetch_udid() -> ffi::MmStringOutcome {
    let gw = gateway_hub().ensure_gateway();
    string_outcome_from(spawn_gateway(async move { gw.fetch_udid().await }).await)
}

async fn mm_gateway_get_lockdown_value(key: String) -> ffi::MmStringOutcome {
    let gw = gateway_hub().ensure_gateway();
    string_outcome_from(
        spawn_gateway(async move { gw.get_lockdown_value(&key).await }).await,
    )
}

async fn mm_gateway_start_session() -> ffi::MmOutcome {
    let gw = gateway_hub().ensure_gateway();
    outcome_from(spawn_gateway(async move { gw.start_session().await }).await)
}

async fn mm_gateway_stop_session() -> ffi::MmOutcome {
    let gw = gateway_hub().ensure_gateway();
    outcome_from(spawn_gateway(async move { gw.stop_session().await }).await)
}

async fn mm_gateway_open_service(service_name: String) -> ffi::MmServiceOutcome {
    let gw = gateway_hub().ensure_gateway();
    match spawn_gateway(async move { gw.open_service(&service_name).await }).await {
        Ok(opened) => ffi::MmServiceOutcome {
            success: true,
            service_name: opened.service_name,
            port: opened.port,
            enable_ssl: opened.enable_ssl,
            error: empty_error(),
        },
        Err(e) => ffi::MmServiceOutcome {
            success: false,
            service_name: String::new(),
            port: 0,
            enable_ssl: false,
            error: e.into(),
        },
    }
}

fn mm_gateway_set_logging(enabled: bool) {
    if let Some(gw) = gateway_hub().current_gateway() {
        gw.set_logging(enabled);
    }
}

// ---------------------------------------------------------------------------
// mm-core usbmuxd proxy bridge entry points (M3). The retarget +
// proxy start composition is MinimuxerImpl's start path; it is
// exposed in pieces here and composed by the shell (M4), exactly
// like Swift splits MinimuxerImpl from the gateway itself.
// ---------------------------------------------------------------------------

fn mm_usbmuxd_retarget() {
    mm_core::usbmuxd_proxy::retarget_usbmuxd_address();
}

async fn mm_proxy_start() -> ffi::MmOutcome {
    let proxy = gateway_globals().proxy.clone();
    let gw = gateway_hub().ensure_gateway();
    outcome_from(
        spawn_gateway(async move {
            mm_core::usbmuxd_proxy::start_proxy_for_gateway(&proxy, &gw).await
        })
        .await,
    )
}

async fn mm_proxy_stop() {
    gateway_globals().proxy.stop().await;
}

fn mm_proxy_is_listening() -> bool {
    gateway_globals().proxy.is_listening()
}

fn mm_proxy_notify_attached(peer_ip: String) {
    gateway_globals().proxy.notify_device_attached(&peer_ip);
}

fn mm_proxy_notify_detached() {
    gateway_globals().proxy.notify_device_detached();
}

// ---------------------------------------------------------------------------
// Core globals (M4): the one MinimuxerCore for the App's
// lifetime, composed over the net/gateway globals. Building it
// registers the network-change → isReady re-probe handler on
// the stack (inside MinimuxerCore::new); the status bus is
// subscribed here and forwarded to the EventSink's on_status.
// ---------------------------------------------------------------------------

/// `CoreGatewayProvider` over the hub: the core always reads a
/// materialized gateway (Swift's DeviceProvider starts empty
/// but MinimuxerImpl only ever runs against a live instance),
/// while the proxy keeps the hub's None-able view.
struct CoreProviderAdapter(std::sync::Arc<GatewayHub>);

impl mm_gateway::GatewayProvider for CoreProviderAdapter {
    fn current_gateway(
        &self,
    ) -> Option<std::sync::Arc<dyn mm_gateway::DeviceGateway>> {
        Some(self.0.ensure_gateway())
    }
}

impl mm_core::CoreGatewayProvider for CoreProviderAdapter {
    fn backend(&self) -> mm_gateway::GatewayBackend {
        self.0.backend()
    }

    fn set_backend(
        &self,
        backend: mm_gateway::GatewayBackend,
    ) -> std::sync::Arc<dyn mm_gateway::DeviceGateway> {
        self.0.set_backend(backend)
    }
}

struct CoreGlobals {
    core: std::sync::Arc<mm_core::MinimuxerCore>,
}

fn core_globals() -> &'static CoreGlobals {
    static CORE: OnceLock<CoreGlobals> = OnceLock::new();
    CORE.get_or_init(|| {
        use std::sync::Arc;
        let net = net_globals();
        let gw = gateway_globals();
        let provider: Arc<dyn mm_core::CoreGatewayProvider> =
            Arc::new(CoreProviderAdapter(gateway_hub().clone()));
        let core = mm_core::MinimuxerCore::new(
            provider,
            net.stack.clone(),
            net.observer.clone(),
            gw.proxy.clone(),
            Arc::new(mm_core::SystemOsVersion::new()),
        );
        // Status bus → EventSink. The sink's on_status shape
        // (success + error triple) cannot carry `.success(false)`
        // — the DDI-not-mounted result of the withDDIMountCheck
        // probe, which the App does not use; it collapses to a
        // plain success emission (diff list D-M4-3). The direct
        // mm_core_is_ready query carries the Bool faithfully.
        core.subscribe_status(Arc::new(|result| match result {
            Ok(_) => {
                mm_emit_status(true, String::new(), String::new(), String::new())
            }
            Err(e) => {
                let info = e.to_info();
                mm_emit_status(false, info.domain, info.code, info.reason);
            }
        }));
        CoreGlobals { core }
    })
}

fn ready_outcome_from(
    result: Result<bool, mm_common::MmErrorInfo>,
) -> ffi::MmReadyOutcome {
    match result {
        Ok(ready) => ffi::MmReadyOutcome {
            success: true,
            ready,
            error: empty_error(),
        },
        Err(e) => ffi::MmReadyOutcome {
            success: false,
            ready: false,
            error: e.into(),
        },
    }
}

// Core operations run on the global runtime via spawn_gateway
// (the core spawns its heartbeat/observer tasks there); the
// core's MinimuxerError is flattened to its info shape at the
// boundary, like every other subsystem.

async fn mm_core_start(
    content: String,
    docs_path: String,
    preferred: Option<ffi::MmPairingProtocol>,
) -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    let preferred = preferred.map(|p| p.to_common());
    outcome_from(
        spawn_gateway(async move {
            core.start(&content, &docs_path, preferred)
                .await
                .map_err(|e| e.to_info())
        })
        .await,
    )
}

async fn mm_core_stop() -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(
        spawn_gateway(async move { core.stop().await.map_err(|e| e.to_info()) })
            .await,
    )
}

async fn mm_core_restart() -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(
        spawn_gateway(async move { core.restart().await.map_err(|e| e.to_info()) })
            .await,
    )
}

async fn mm_core_reinitialize_pairing_data(content: String) -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(
        spawn_gateway(async move {
            core.reinitialize_pairing_data(&content)
                .await
                .map_err(|e| e.to_info())
        })
        .await,
    )
}

fn mm_core_bind_connection_config(
    config: ffi::MmConnectionConfig,
) -> ffi::MmOutcome {
    // The Swift-side config object is not Send, so this entry
    // stays synchronous (like mm_net_bind_connection_config):
    // the binding itself is installed synchronously on the
    // shared manager — the same instance the core reads — and
    // the endpoint refresh that completes
    // MinimuxerCore::bind_connection_config is dispatched to
    // the global runtime.
    let holder: SharedConfig = std::sync::Arc::new(Mutex::new(ConfigHolder(config)));
    let binding = build_binding(holder);
    let _ = core_globals();
    let net = net_globals();
    net.manager.bind_connection_config(binding);
    let stack = net.stack.clone();
    global_runtime().spawn(async move {
        stack.refresh_endpoint().await;
    });
    ffi::MmOutcome {
        success: true,
        error: empty_error(),
    }
}

async fn mm_core_is_ready(
    with_network_check: bool,
    with_ddi_mount_check: bool,
) -> ffi::MmReadyOutcome {
    let core = core_globals().core.clone();
    ready_outcome_from(
        spawn_gateway(async move {
            core.is_ready(with_network_check, with_ddi_mount_check)
                .await
                .map_err(|e| e.to_info())
        })
        .await,
    )
}

async fn mm_core_fetch_udid() -> ffi::MmStringOutcome {
    let core = core_globals().core.clone();
    string_outcome_from(
        spawn_gateway(async move {
            core.fetch_udid().await.map_err(|e| e.to_info())
        })
        .await,
    )
}

async fn mm_core_is_ddi_mounted() -> ffi::MmReadyOutcome {
    let core = core_globals().core.clone();
    ready_outcome_from(
        spawn_gateway(async move {
            core.is_ddi_mounted().await.map_err(|e| e.to_info())
        })
        .await,
    )
}

async fn mm_core_mount_ddi(docs_path: String) -> ffi::MmReadyOutcome {
    let core = core_globals().core.clone();
    ready_outcome_from(
        spawn_gateway(async move {
            core.mount_ddi(&docs_path).await.map_err(|e| e.to_info())
        })
        .await,
    )
}


// ---------------------------------------------------------------------------
// mm-core M5 face: install pipeline, profiles, AFC, debug.
// Every operation flattens the core's MinimuxerError to its
// info shape at the boundary, like the M4 face.
// ---------------------------------------------------------------------------

fn list_outcome_from(
    result: Result<Vec<String>, mm_common::MmErrorInfo>,
) -> ffi::MmListOutcome {
    match result {
        Ok(entries) => ffi::MmListOutcome {
            success: true,
            entries,
            error: empty_error(),
        },
        Err(e) => ffi::MmListOutcome {
            success: false,
            entries: Vec::new(),
            error: e.into(),
        },
    }
}

fn data_outcome_from(
    result: Result<Vec<u8>, mm_common::MmErrorInfo>,
) -> ffi::MmDataOutcome {
    match result {
        Ok(data) => ffi::MmDataOutcome {
            success: true,
            data,
            error: empty_error(),
        },
        Err(e) => ffi::MmDataOutcome {
            success: false,
            data: Vec::new(),
            error: e.into(),
        },
    }
}

fn afc_info_outcome_from(
    result: Result<mm_gateway::AfcFileInfo, mm_common::MmErrorInfo>,
) -> ffi::MmAfcFileInfoOutcome {
    match result {
        Ok(info) => ffi::MmAfcFileInfoOutcome {
            success: true,
            is_directory: info.is_directory,
            file_size: info.file_size,
            error: empty_error(),
        },
        Err(e) => ffi::MmAfcFileInfoOutcome {
            success: false,
            is_directory: false,
            file_size: 0,
            error: e.into(),
        },
    }
}

impl ffi::MmProfileDumpMode {
    fn to_core(self) -> mm_core::ProfileDumpMode {
        match self {
            ffi::MmProfileDumpMode::Raw => mm_core::ProfileDumpMode::Raw,
            ffi::MmProfileDumpMode::Zip => mm_core::ProfileDumpMode::Zip,
        }
    }
}

/// Runs one core M5 operation on the global runtime.
async fn spawn_core<T, F>(fut: F) -> Result<T, mm_common::MmErrorInfo>
where
    F: std::future::Future<Output = Result<T, mm_common::MinimuxerError>>
        + Send
        + 'static,
    T: Send + 'static,
{
    spawn_gateway(async move { fut.await.map_err(|e| e.to_info()) }).await
}

async fn mm_core_send_ipa_afc(bundle_id: String, ipa: Vec<u8>) -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(
        spawn_core(async move { core.send_ipa_afc(&bundle_id, &ipa).await }).await,
    )
}

async fn mm_core_send_app_bundle_afc(
    bundle_id: String,
    app_path: String,
) -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(
        spawn_core(async move { core.send_app_bundle_afc(&bundle_id, &app_path).await })
            .await,
    )
}

async fn mm_core_install_ipa(bundle_id: String) -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(spawn_core(async move { core.install_ipa(&bundle_id).await }).await)
}

async fn mm_core_install_app_bundle(
    bundle_id: String,
    app_name: String,
) -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(
        spawn_core(async move { core.install_app_bundle(&bundle_id, &app_name).await })
            .await,
    )
}

async fn mm_core_remove_app(bundle_id: String) -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(spawn_core(async move { core.remove_app(&bundle_id).await }).await)
}

async fn mm_core_wipe_container(bundle_id: String) -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(
        spawn_core(async move { core.wipe_container(&bundle_id).await }).await,
    )
}

async fn mm_core_debug_app(bundle_id: String) -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(spawn_core(async move { core.debug_app(&bundle_id).await }).await)
}

async fn mm_core_attach_debugger(pid: u32) -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(spawn_core(async move { core.attach_debugger(pid).await }).await)
}

async fn mm_core_install_profile(profile: Vec<u8>) -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(
        spawn_core(async move { core.install_provisioning_profile(&profile).await })
            .await,
    )
}

async fn mm_core_remove_profile(profile_id: String) -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(
        spawn_core(async move { core.remove_provisioning_profile(&profile_id).await })
            .await,
    )
}

async fn mm_core_dump_profiles(
    docs_path: String,
    mode: ffi::MmProfileDumpMode,
) -> ffi::MmStringOutcome {
    let core = core_globals().core.clone();
    string_outcome_from(
        spawn_core(async move { core.dump_profiles(&docs_path, mode.to_core()).await })
            .await,
    )
}

async fn mm_core_afc_list_directory(
    bundle_id: String,
    path: String,
) -> ffi::MmListOutcome {
    let core = core_globals().core.clone();
    list_outcome_from(
        spawn_core(async move { core.afc_list_directory(&bundle_id, &path).await })
            .await,
    )
}

async fn mm_core_afc_read_file(
    bundle_id: String,
    path: String,
) -> ffi::MmDataOutcome {
    let core = core_globals().core.clone();
    data_outcome_from(
        spawn_core(async move { core.afc_read_file(&bundle_id, &path).await }).await,
    )
}

async fn mm_core_afc_get_file_info(
    bundle_id: String,
    path: String,
) -> ffi::MmAfcFileInfoOutcome {
    let core = core_globals().core.clone();
    afc_info_outcome_from(
        spawn_core(async move { core.afc_get_file_info(&bundle_id, &path).await })
            .await,
    )
}

fn mm_core_set_params(
    backend: Option<ffi::MmGatewayBackend>,
    remote_pairing_port: Option<u16>,
    device_probe_timeout: Option<i32>,
) {
    core_globals().core.set_params(mm_core::MinimuxerParams {
        backend: backend.map(|b| b.to_gateway()),
        remote_pairing_port,
        device_probe_timeout,
    });
}

fn mm_core_status() -> ffi::MmCoreStatus {
    match core_globals().core.status() {
        mm_core::CoreStatus::Started => ffi::MmCoreStatus::Started,
        mm_core::CoreStatus::InProgress => ffi::MmCoreStatus::InProgress,
        mm_core::CoreStatus::Stopped => ffi::MmCoreStatus::Stopped,
    }
}

fn mm_core_set_logging(enabled: bool) {
    core_globals().core.set_logging(enabled);
}

fn mm_core_is_pairing_file_loaded() -> bool {
    core_globals().core.is_pairing_file_loaded()
}

fn mm_core_last_beat_successful() -> bool {
    core_globals().core.heartbeat().last_beat_successful()
}

// ---------------------------------------------------------------------------
// mm-emproxy (M6): the EMProxyAPI surface. The instance lives on
// the core (Swift `MinimuxerImpl.emproxy`); the App layer drives
// it — configure the handshake client, start (explicit bind or
// the 127.0.0.1:51820 default), stop. Errors cross as the
// EMProxyError MmErrorInfo shape inside MmOutcome.
// ---------------------------------------------------------------------------

fn mm_emproxy_set_handshake_client(host: String, port: u16, enabled: bool) {
    core_globals()
        .core
        .emproxy()
        .set_handshake_client(&host, port, enabled);
}

async fn mm_emproxy_start(host: String, port: u16) -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(
        spawn_gateway(async move {
            core.emproxy()
                .start(&host, port)
                .await
                .map_err(|e| e.to_info())
        })
        .await,
    )
}

async fn mm_emproxy_start_default() -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(
        spawn_gateway(async move {
            core.emproxy().start_default().await.map_err(|e| e.to_info())
        })
        .await,
    )
}

async fn mm_emproxy_stop() -> ffi::MmOutcome {
    let core = core_globals().core.clone();
    outcome_from(
        spawn_gateway(async move { core.emproxy().stop().map_err(|e| e.to_info()) }).await,
    )
}

fn mm_emproxy_is_running() -> bool {
    core_globals().core.emproxy().is_running()
}

// ---------------------------------------------------------------------------
// mm-core M8 face: wireless pairing. The App-side
// handlers live on one Swift object (MmWirelessPairSink)
// held in a registry like the EventSink; an adapter
// presents it to the core service as its
// WirelessPairAppEvents. Calls into the Swift object
// are serialized on the registry mutex (the App-side
// object is not documented thread-safe).
// ---------------------------------------------------------------------------

struct WirelessSinkHolder(ffi::MmWirelessPairSink);
// Only ever touched under the registry mutex; the shell
// keeps the object alive while bound and unbinds before
// releasing it.
unsafe impl Send for WirelessSinkHolder {}

static WIRELESS_SINK: Mutex<Option<WirelessSinkHolder>> = Mutex::new(None);

fn with_wireless_sink<R>(
    f: impl FnOnce(&ffi::MmWirelessPairSink) -> R,
) -> Option<R> {
    let guard = WIRELESS_SINK.lock().ok()?;
    guard.as_ref().map(|holder| f(&holder.0))
}

/// The sink presented to the core's wireless pairing
/// service as its App events.
struct WirelessSinkEvents;

impl mm_core::WirelessPairAppEvents for WirelessSinkEvents {
    fn on_pin_received(&self, pin: &str) {
        let pin = pin.to_string();
        let _ = with_wireless_sink(|sink| sink.on_pin_received(pin));
    }

    fn on_ready_to_pair(&self, service_id: &str, port: i32) {
        let service_id = service_id.to_string();
        let _ = with_wireless_sink(|sink| {
            sink.on_ready_to_pair(service_id, port)
        });
    }

    fn request_pin(&self, request_id: u64) -> bool {
        with_wireless_sink(|sink| sink.request_pin(request_id))
            .unwrap_or(false)
    }

    fn resolve_file_name(&self, name: &str, model: &str) -> Option<String> {
        let outcome = with_wireless_sink(|sink| {
            sink.resolve_file_name(name.to_string(), model.to_string())
        })?;
        if outcome.has_resolver {
            Some(outcome.file_name)
        } else {
            None
        }
    }
}

fn mm_wireless_bind_sink(sink: ffi::MmWirelessPairSink) {
    if let Ok(mut guard) = WIRELESS_SINK.lock() {
        *guard = Some(WirelessSinkHolder(sink));
    }
    core_globals()
        .core
        .wireless_pair()
        .set_app_events(Some(std::sync::Arc::new(WirelessSinkEvents)));
}

fn mm_wireless_clear_sink() {
    if let Ok(mut guard) = WIRELESS_SINK.lock() {
        *guard = None;
    }
    core_globals().core.wireless_pair().set_app_events(None);
}

fn paired_outcome_from(
    result: Result<mm_gateway::PairedDeviceRecord, mm_common::MmErrorInfo>,
) -> ffi::MmPairedDeviceOutcome {
    match result {
        Ok(record) => ffi::MmPairedDeviceOutcome {
            success: true,
            name: record.name,
            model: record.model,
            pairing_file_path: record.pairing_file_path,
            pairing_file_content: record.pairing_file.raw_content().to_string(),
            error: empty_error(),
        },
        Err(e) => ffi::MmPairedDeviceOutcome {
            success: false,
            name: String::new(),
            model: String::new(),
            pairing_file_path: String::new(),
            pairing_file_content: String::new(),
            error: e.into(),
        },
    }
}

async fn mm_wireless_start(
    host_name: String,
    host_model: String,
    out_path: String,
) -> ffi::MmPairedDeviceOutcome {
    let service = core_globals().core.wireless_pair().clone();
    paired_outcome_from(
        spawn_gateway(async move {
            service.start(&host_name, &host_model, &out_path).await
        })
        .await,
    )
}

async fn mm_wireless_trigger(
    target_ip: String,
    target_port: u16,
    host_name: String,
    host_model: String,
    out_path: String,
) -> ffi::MmPairedDeviceOutcome {
    let service = core_globals().core.wireless_pair().clone();
    paired_outcome_from(
        spawn_gateway(async move {
            service
                .trigger(&target_ip, target_port, &host_name, &host_model, &out_path)
                .await
        })
        .await,
    )
}

fn mm_wireless_stop() {
    core_globals().core.wireless_pair().stop();
}

fn mm_wireless_complete_pin(request_id: u64, pin: String) -> bool {
    core_globals()
        .core
        .wireless_pair()
        .complete_pin(request_id, pin)
}

fn mm_wireless_state() -> ffi::MmWirelessState {
    let service = core_globals().core.wireless_pair();
    let (start_active, triggers) = service.state();
    ffi::MmWirelessState {
        start_active,
        active_triggers: triggers as i32,
        advertising: service.is_advertising(),
    }
}

// ---------------------------------------------------------------------------
// Host-test link stubs: the `__swift_bridge__$...` C symbols for the
// extern Swift types are provided by the generated Swift in the real
// build. Host tests never hold Swift objects (no sink is registered,
// no config is bound), so these are never called — they exist only
// so that objects referencing the bridge types link into the test
// binary.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod swift_symbol_stubs {
    use std::ffi::c_void;

    #[export_name = "__swift_bridge__$MmEventSink$on_log"]
    extern "C" fn sink_on_log(_this: *mut c_void, _message: *mut c_void) {}
    #[export_name = "__swift_bridge__$MmEventSink$on_status"]
    extern "C" fn sink_on_status(
        _this: *mut c_void,
        _seq: u64,
        _success: bool,
        _domain: *mut c_void,
        _code: *mut c_void,
        _reason: *mut c_void,
    ) {
    }
    #[export_name = "__swift_bridge__$MmEventSink$_free"]
    extern "C" fn sink_free(_ptr: *mut c_void) {}

    #[export_name = "__swift_bridge__$MmConnectionConfig$set_tunnel_iface_ip"]
    extern "C" fn cfg_set_tunnel_iface_ip(_this: *mut c_void, _ip: *mut c_void) {}
    #[export_name = "__swift_bridge__$MmConnectionConfig$set_tunnel_peer_ip"]
    extern "C" fn cfg_set_tunnel_peer_ip(_this: *mut c_void, _ip: *mut c_void) {}
    #[export_name = "__swift_bridge__$MmConnectionConfig$set_tunnel_peer_subnet_mask"]
    extern "C" fn cfg_set_tunnel_peer_subnet_mask(_this: *mut c_void, _mask: *mut c_void) {}
    #[export_name = "__swift_bridge__$MmConnectionConfig$set_tunnel_peer_reachable"]
    extern "C" fn cfg_set_tunnel_peer_reachable(_this: *mut c_void, _reachable: bool) {}
    #[export_name = "__swift_bridge__$MmConnectionConfig$set_tunnel_iface_subnet_mask"]
    extern "C" fn cfg_set_tunnel_iface_subnet_mask(_this: *mut c_void, _mask: *mut c_void) {}
    #[export_name = "__swift_bridge__$MmConnectionConfig$set_remote_reachable"]
    extern "C" fn cfg_set_remote_reachable(_this: *mut c_void, _reachable: bool) {}
    #[export_name = "__swift_bridge__$MmConnectionConfig$set_override_tunnel_peer_reachable"]
    extern "C" fn cfg_set_override_reachable(_this: *mut c_void, _reachable: bool) {}
    #[export_name = "__swift_bridge__$MmConnectionConfig$get_connection_mode"]
    extern "C" fn cfg_get_connection_mode(_this: *mut c_void) -> u32 {
        2 // NotConfigured tag; never observed (never called in tests)
    }
    #[export_name = "__swift_bridge__$MmConnectionConfig$get_override_tunnel_peer_ip"]
    extern "C" fn cfg_get_override_tunnel_peer_ip(_this: *mut c_void) -> *mut c_void {
        std::ptr::null_mut()
    }
    #[export_name = "__swift_bridge__$MmConnectionConfig$get_remote_server_ip"]
    extern "C" fn cfg_get_remote_server_ip(_this: *mut c_void) -> *mut c_void {
        std::ptr::null_mut()
    }
    #[export_name = "__swift_bridge__$MmConnectionConfig$request_service_port"]
    extern "C" fn cfg_request_service_port(
        _this: *mut c_void,
        _request_id: u64,
        _protocol: u32,
        _port: u16,
    ) {
    }
    #[export_name = "__swift_bridge__$MmConnectionConfig$_free"]
    extern "C" fn cfg_free(_ptr: *mut c_void) {}

    #[export_name = "__swift_bridge__$MmWirelessPairSink$on_pin_received"]
    extern "C" fn wp_on_pin_received(_this: *mut c_void, _pin: *mut c_void) {}
    #[export_name = "__swift_bridge__$MmWirelessPairSink$on_ready_to_pair"]
    extern "C" fn wp_on_ready_to_pair(
        _this: *mut c_void,
        _service_id: *mut c_void,
        _port: i32,
    ) {
    }
    #[export_name = "__swift_bridge__$MmWirelessPairSink$request_pin"]
    extern "C" fn wp_request_pin(_this: *mut c_void, _request_id: u64) -> bool {
        false
    }
    #[repr(C)]
    struct MmResolveOutcomeStub {
        has_resolver: bool,
        file_name: *mut c_void,
    }
    #[export_name = "__swift_bridge__$MmWirelessPairSink$resolve_file_name"]
    extern "C" fn wp_resolve_file_name(
        _this: *mut c_void,
        _name: *mut c_void,
        _model: *mut c_void,
    ) -> MmResolveOutcomeStub {
        MmResolveOutcomeStub {
            has_resolver: false,
            file_name: std::ptr::null_mut(),
        }
    }
    #[export_name = "__swift_bridge__$MmWirelessPairSink$_free"]
    extern "C" fn wp_free(_ptr: *mut c_void) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCKDOWN_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n<key>WiFiMACAddress</key>\n<string>AA:BB:CC:DD:EE:FF</string>\n<key>SystemBUID</key>\n<string>BUID-1</string>\n<key>RootPrivateKey</key>\n<data>AAECAw==</data>\n<key>HostPrivateKey</key>\n<data>AAECAw==</data>\n<key>HostID</key>\n<string>H1</string>\n<key>RootCertificate</key>\n<data>AAECAw==</data>\n<key>UDID</key>\n<string>UDID-1</string>\n<key>EscrowBag</key>\n<data>AAECAw==</data>\n<key>HostCertificate</key>\n<data>AAECAw==</data>\n<key>DeviceCertificate</key>\n<data>AAECAw==</data>\n</dict>\n</plist>\n";

    #[test]
    fn m8_paired_outcome_carries_record_and_error() {
        let pairing_file =
            mm_common::parse_pairing_content(LOCKDOWN_XML, None).expect("parse");
        let record = mm_gateway::PairedDeviceRecord {
            name: "Peer".to_string(),
            model: "iPhone18,1".to_string(),
            pairing_file_path: "/docs/Peer_iPhone18,1_rp.plist".to_string(),
            pairing_file,
        };
        let ok = paired_outcome_from(Ok(record));
        assert!(ok.success);
        assert_eq!(ok.name, "Peer");
        assert_eq!(ok.model, "iPhone18,1");
        assert_eq!(ok.pairing_file_path, "/docs/Peer_iPhone18,1_rp.plist");
        assert!(ok.pairing_file_content.contains("UDID-1"));
        assert_eq!(ok.error.domain, "");

        let err = paired_outcome_from(Err(mm_common::MmErrorInfo::new(
            "DeviceGatewayError",
            "serviceError",
            "Pairing failed or cancelled",
        )));
        assert!(!err.success);
        assert_eq!(err.error.code, "serviceError");
        assert_eq!(err.error.reason, "Pairing failed or cancelled");
        assert!(err.pairing_file_content.is_empty());
    }

    #[test]
    fn m5_outcome_helpers_carry_payloads_and_errors() {
        let info = mm_common::MmErrorInfo::new("AfcError", "notFound", "gone");

        let list = list_outcome_from(Ok(vec!["a".to_string()]));
        assert!(list.success);
        assert_eq!(list.entries, vec!["a".to_string()]);
        let list_err = list_outcome_from(Err(info.clone()));
        assert!(!list_err.success);
        assert_eq!(list_err.error.code, "notFound");

        let data = data_outcome_from(Ok(vec![1u8, 2, 3]));
        assert!(data.success);
        assert_eq!(data.data, vec![1u8, 2, 3]);
        let data_err = data_outcome_from(Err(info.clone()));
        assert!(!data_err.success);
        assert!(data_err.data.is_empty());

        let afc = afc_info_outcome_from(Ok(mm_gateway::AfcFileInfo {
            is_directory: true,
            file_size: -1,
        }));
        assert!(afc.success && afc.is_directory && afc.file_size == -1);
        let afc_err = afc_info_outcome_from(Err(info));
        assert!(!afc_err.success);
        assert_eq!(afc_err.error.domain, "AfcError");
    }

    #[test]
    fn m5_profile_dump_mode_maps_to_core() {
        assert!(matches!(
            ffi::MmProfileDumpMode::Raw.to_core(),
            mm_core::ProfileDumpMode::Raw
        ));
        assert!(matches!(
            ffi::MmProfileDumpMode::Zip.to_core(),
            mm_core::ProfileDumpMode::Zip
        ));
    }

    #[test]
    fn version_is_nonempty() {
        assert!(mm_version().contains("mm-ffi"));
    }

    #[test]
    fn parse_pairing_crosses_boundary() {
        let ok = mm_parse_pairing_file(LOCKDOWN_XML.to_string(), None);
        assert!(ok.success);
        assert!(matches!(ok.protocol, ffi::MmPairingProtocol::Lockdown));

        let bad = mm_parse_pairing_file("nope".to_string(), None);
        assert!(!bad.success);
        assert_eq!(bad.error.domain, "PairingError");
        assert_eq!(bad.error.code, "invalidPlist");
    }

    #[test]
    fn describe_error_renders_swift_text() {
        let info = ffi::MmErrorInfo {
            domain: "MinimuxerError".to_string(),
            code: "noVPN".to_string(),
            reason: "No utun interface detected".to_string(),
        };
        assert_eq!(
            mm_describe_error(info),
            "NoVPN: No utun interface detected"
        );
    }

    #[test]
    fn async_ping_runs_on_global_runtime() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        assert_eq!(rt.block_on(mm_async_ping()), "pong");
    }

    #[test]
    fn shell_crates_are_linked() {
        assert_eq!(mm_net::CRATE_NAME, "mm-net");
        assert_eq!(mm_rp::CRATE_NAME, "mm-rp");
        assert_eq!(mm_gateway::CRATE_NAME, "mm-gateway");
        assert_eq!(mm_emproxy::CRATE_NAME, "mm-emproxy");
        assert_eq!(mm_core::CRATE_NAME, "mm-core");
    }

    #[test]
    fn ffi_emproxy_face_shapes() {
        // The core-held instance starts unconfigured and stopped.
        assert!(!mm_emproxy_is_running());
        let rt = tokio::runtime::Runtime::new().unwrap();
        // Stop before start: EMProxyError.serverNotRunning.
        let early_stop = rt.block_on(mm_emproxy_stop());
        assert!(!early_stop.success);
        assert_eq!(early_stop.error.domain, "EMProxyError");
        assert_eq!(early_stop.error.code, "serverNotRunning");
        // Configure (disabled, empty host → candidate probe; the
        // host has no utun interfaces, so the probe skips fast).
        mm_emproxy_set_handshake_client(String::new(), 0, false);
        assert_eq!(
            core_globals().core.emproxy().handshake_config(),
            Some(mm_emproxy::HandshakeConfig {
                host: String::new(),
                port: 0,
                enabled: false,
            })
        );
        // Start on a test port, observe, stop.
        let started = rt.block_on(mm_emproxy_start("127.0.0.1".to_string(), 26421));
        assert!(
            started.success,
            "start failed: {} {}",
            started.error.domain, started.error.code
        );
        assert!(mm_emproxy_is_running());
        let stopped = rt.block_on(mm_emproxy_stop());
        assert!(stopped.success);
        assert!(!mm_emproxy_is_running());
    }

    #[test]
    fn net_snapshot_smoke() {
        let snap = mm_net_snapshot();
        assert!(
            !snap.interfaces.is_empty(),
            "the test host has at least a loopback interface"
        );
        let scanned = mm_net_scan_interfaces();
        assert_eq!(scanned.len(), snap.interfaces.len());
    }

    #[test]
    fn net_endpoint_starts_uninitialized() {
        // No binding is ever installed in tests and the mode is
        // notConfigured, so refreshes never set an endpoint.
        let outcome = mm_net_endpoint();
        assert!(!outcome.success);
        assert_eq!(outcome.error.domain, "MinimuxerInternalError");
        assert_eq!(outcome.error.code, "deviceEndpointNotInitialized");
    }

    #[test]
    fn net_mode_unconfigured_without_binding() {
        assert!(matches!(
            mm_net_connection_mode(),
            ffi::MmConnectionMode::NotConfigured
        ));
    }

    #[test]
    fn net_complete_unknown_port_request_is_false() {
        assert!(!mm_net_complete_service_port(
            u64::MAX,
            ffi::MmPairingProtocol::Lockdown,
            62078
        ));
    }

    #[test]
    fn net_observer_start_stop_idempotent() {
        assert!(mm_net_observer_start());
        assert!(!mm_net_observer_start());
        assert!(mm_net_observer_stop());
        assert!(!mm_net_observer_stop());
    }

    #[test]
    fn net_refresh_completes() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        // Value depends on global state history; completion without
        // panic is the assertion here (manager-level behaviour is
        // covered by mm-net's own tests).
        let _changed = rt.block_on(mm_net_refresh());
    }

    #[test]
    fn gateway_hub_defaults_to_idevice_backend() {
        use mm_net::ServicePortSource;
        let hub = GatewayHub::new();
        assert!(matches!(
            hub.backend(),
            mm_gateway::GatewayBackend::Idevice
        ));
        assert!(hub.current_gateway().is_none());
        assert_eq!(
            hub.pairing_file_type(),
            mm_common::PairingProtocol::Unknown
        );
        // Unknown protocol with no gateway: Swift's default port.
        assert_eq!(hub.service_port(), 62078);
        let gw = hub.ensure_gateway();
        assert!(!gw.requires_usbmuxd());
    }

    #[test]
    fn gateway_hub_switch_carries_rp_port() {
        let hub = GatewayHub::new();
        let gw = hub.ensure_gateway();
        gw.set_port(50001, mm_common::PairingProtocol::RpPairing);
        let new_gw = hub.set_backend(mm_gateway::GatewayBackend::Libimobiledevice);
        assert!(matches!(
            hub.backend(),
            mm_gateway::GatewayBackend::Libimobiledevice
        ));
        // Minimuxer.set carries the RP port into the new instance.
        assert_eq!(
            new_gw.get_port(mm_common::PairingProtocol::RpPairing),
            50001
        );
        // The ServicePortSource view follows the new gateway.
        use mm_net::ServicePortSource;
        hub.set_port(50002, mm_common::PairingProtocol::RpPairing);
        assert_eq!(
            new_gw.get_port(mm_common::PairingProtocol::RpPairing),
            50002
        );
    }

    #[test]
    fn gateway_ffi_start_failure_surfaces_pairing_error() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let outcome = rt.block_on(mm_gateway_start("not a plist".to_string(), None));
        assert!(!outcome.success);
        assert_eq!(outcome.error.domain, "PairingError");
        assert_eq!(outcome.error.code, "invalidPlist");
    }

    #[test]
    fn gateway_ffi_fetch_udid_uninitialized() {
        // No test ever starts the global gateway successfully, so
        // the fetch must surface the gateway's NotInitialized.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let outcome = rt.block_on(mm_gateway_fetch_udid());
        assert!(!outcome.success);
        assert_eq!(outcome.error.domain, "DeviceGatewayError");
        assert_eq!(outcome.error.code, "notInitialized");
    }

    #[test]
    fn gateway_ffi_defaults() {
        assert!(matches!(
            mm_gateway_backend(),
            ffi::MmGatewayBackend::Idevice
        ));
        assert_eq!(
            mm_gateway_service_port(ffi::MmPairingProtocol::Lockdown),
            62078
        );
        assert_eq!(
            mm_gateway_service_port(ffi::MmPairingProtocol::RpPairing),
            49152
        );
        assert!(!mm_proxy_is_listening());
    }

    #[test]
    fn proxy_start_without_pairing_is_pairing_not_loaded() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let outcome = rt.block_on(mm_proxy_start());
        assert!(!outcome.success);
        assert_eq!(outcome.error.domain, "MinimuxerError");
        assert_eq!(outcome.error.code, "pairingNotLoaded");
    }

    // -- mm-core (M4) -----------------------------------------------------
    // The global core can never reach Started in host tests (no
    // Swift-side connection config can be bound), which pins the
    // early gates and the not-configured start path.

    #[test]
    fn core_is_ready_fails_at_gate1_without_pairing() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let outcome = rt.block_on(mm_core_is_ready(true, false));
        assert!(!outcome.success);
        assert!(!outcome.ready);
        assert_eq!(outcome.error.domain, "MinimuxerError");
        assert_eq!(outcome.error.code, "pairingNotLoaded");
    }

    #[test]
    fn core_start_without_mode_is_not_configured() {
        // The mode check runs before the gateway is touched, so
        // the global gateway stays unstarted (the invariant the
        // gateway tests above rely on).
        let rt = tokio::runtime::Runtime::new().unwrap();
        let outcome = rt.block_on(mm_core_start(
            LOCKDOWN_XML.to_string(),
            "/tmp/docs".to_string(),
            None,
        ));
        assert!(!outcome.success);
        assert_eq!(outcome.error.domain, "MinimuxerError");
        assert_eq!(outcome.error.code, "connectionModeNotConfigured");
        assert!(matches!(mm_core_status(), ffi::MmCoreStatus::Stopped));
    }

    #[test]
    fn core_defaults_and_fetch_udid_mapping() {
        assert!(!mm_core_is_pairing_file_loaded());
        assert!(!mm_core_last_beat_successful());
        // fetchUDID through the core: the gateway's
        // NotInitialized lands in the fetchUDID catch-all.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let outcome = rt.block_on(mm_core_fetch_udid());
        assert!(!outcome.success);
        assert_eq!(outcome.error.domain, "MinimuxerError");
        assert_eq!(outcome.error.code, "fetchUDID");
    }

    #[test]
    fn core_set_params_accepts_partial_updates() {
        // Timeout-only update: no backend switch, no gateway
        // rebuild; the value lands on the shared manager.
        mm_core_set_params(None, None, Some(4321));
        assert_eq!(net_globals().manager.device_probe_timeout(), 4321);
        mm_core_set_params(None, None, Some(250));
        assert_eq!(net_globals().manager.device_probe_timeout(), 250);
    }
}
