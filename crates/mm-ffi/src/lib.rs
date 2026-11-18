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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

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
        protocol: MmPairingProtocol,
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
        fn mm_net_set_pairing_protocol(protocol: MmPairingProtocol);
        fn mm_net_complete_service_port(
            request_id: u64,
            protocol: MmPairingProtocol,
            port: u16,
        ) -> bool;
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
            protocol: MmPairingProtocol,
            port: u16,
        );
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
        "minimuxer-rust rewrite skeleton (mm-ffi {}, mm-common {})",
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
            protocol: ffi::MmPairingProtocol::from_common(file.mode()),
            error: mm_common::MmErrorInfo::new("", "", "").into(),
        },
        Err(err) => ffi::MmPairingOutcome {
            success: false,
            protocol: ffi::MmPairingProtocol::Unknown,
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
// mm-net globals (M2): one NetStack for the App's lifetime — the
// connection manager reads the live scan source, the endpoint is the
// single source of truth, and the polling observer drives refreshes.
// The port table stands in for the gateway's port surface until
// mm-gateway lands (M3).
// ---------------------------------------------------------------------------

struct NetGlobals {
    port_source: std::sync::Arc<mm_net::TablePortSource>,
    scan_source: std::sync::Arc<mm_net::LiveScanSource>,
    manager: std::sync::Arc<mm_net::DeviceConnectionManager>,
    endpoint: std::sync::Arc<mm_net::DeviceEndpoint>,
    stack: std::sync::Arc<mm_net::NetStack>,
    observer: mm_net::PollingObserver,
}

fn net_globals() -> &'static NetGlobals {
    static NET: OnceLock<NetGlobals> = OnceLock::new();
    NET.get_or_init(|| {
        use std::sync::Arc;
        let port_source = Arc::new(mm_net::TablePortSource::default());
        let scan_source = Arc::new(mm_net::LiveScanSource);
        let manager = Arc::new(mm_net::DeviceConnectionManager::new(
            port_source.clone(),
            scan_source.clone(),
        ));
        let endpoint = Arc::new(mm_net::DeviceEndpoint::new());
        let stack = Arc::new(mm_net::NetStack::new(
            manager.clone(),
            endpoint.clone(),
            scan_source.clone(),
        ));
        // Until mm-core (M4) wires the status re-probe, network
        // changes surface as log lines through the event sink so the
        // path is observable end to end.
        stack.add_change_handler(Arc::new(|| {
            Box::pin(async {
                mm_emit_log("[net] network changed".to_string());
            })
        }));
        let observer = mm_net::PollingObserver::new(stack.clone(), scan_source.clone());
        NetGlobals {
            port_source,
            scan_source,
            manager,
            endpoint,
            stack,
            observer,
        }
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

fn mm_net_set_pairing_protocol(protocol: ffi::MmPairingProtocol) {
    // Drives the stand-in port table; the real source of this value
    // is the gateway's pairingFileType (M3).
    net_globals()
        .port_source
        .set_pairing_file_type(protocol.to_common());
}

fn mm_net_complete_service_port(
    request_id: u64,
    protocol: ffi::MmPairingProtocol,
    port: u16,
) -> bool {
    let sender = PORT_REQUESTS.lock().unwrap().remove(&request_id);
    match sender {
        Some(tx) => tx
            .send(mm_net::ServicePort::new(protocol.to_common(), port))
            .is_ok(),
        None => false,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCKDOWN_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n<key>WiFiMACAddress</key>\n<string>AA:BB:CC:DD:EE:FF</string>\n<key>SystemBUID</key>\n<string>BUID-1</string>\n<key>RootPrivateKey</key>\n<data>AAECAw==</data>\n<key>HostPrivateKey</key>\n<data>AAECAw==</data>\n<key>HostID</key>\n<string>H1</string>\n<key>RootCertificate</key>\n<data>AAECAw==</data>\n<key>UDID</key>\n<string>UDID-1</string>\n<key>EscrowBag</key>\n<data>AAECAw==</data>\n<key>HostCertificate</key>\n<data>AAECAw==</data>\n<key>DeviceCertificate</key>\n<data>AAECAw==</data>\n</dict>\n</plist>\n";

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
}
