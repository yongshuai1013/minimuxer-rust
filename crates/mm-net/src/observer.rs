//! Network observation — port of Swift
//! `NetworkObserverService.swift` (yongshuai1013/minimuxer
//! @714aafa3), with the NWPathMonitor replaced by a pure-Rust
//! polling observer (design §5.1, main route; the E1 NWPathMonitor
//! trigger shell stays disabled unless field measurements demand
//! it).
//!
//! Named semantic differences from the Swift original (design D5
//! deviation log for subsystem 4):
//!  1. Trigger latency: NWPathMonitor calls back ~instantly; the
//!     poller ticks every `POLL_INTERVAL` (1s) plus a `SETTLE`
//!     (150ms) re-scan before emitting, so an endpoint update can
//!     lag a tunnel change by up to ~1.15s.
//!  2. `is_wifi_satisfied`: NWPath's "status == .satisfied &&
//!     usesInterfaceType(.wifi)" is approximated as "an en0
//!     interface is present in the scan and a default route
//!     exists" (design §5.1).
//!  3. `is_wired_satisfied`: approximated as "the default route
//!     leaves via an interface classified Ethernet" (NWPath's
//!     .wiredEthernet has no scan equivalent).
//!  4. `is_bridge_satisfied`: the NWPath ".satisfied &&
//!     usesInterfaceType(.other)" clause has no equivalent and is
//!     dropped; the scan clause (name contains bridge/ap) is kept
//!     verbatim.
//!  5. `pathPublisher` never emits (design D4 named downgrade; the
//!     App does not use it).
//!  6. Sub-`SETTLE` transient flaps may be skipped entirely, where
//!     NWPathMonitor would have delivered the intermediate path.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mm_common::logging::{debug_log, verbose_log};

use crate::endpoint::DeviceEndpoint;
use crate::iface::{NetInfo, ScanSnapshot, ScanSource};
use crate::manager::DeviceConnectionManager;
use crate::types::{DeviceConnectionMode, LocalInterfaceInfo, LocalInterfaceType};

/// Poll tick of the polling observer (design §5.1: 1 second).
pub const POLL_INTERVAL: Duration = Duration::from_millis(1000);
/// Settle delay between first detecting a snapshot change and
/// emitting it (debounce against mid-transition states, e.g. a
/// utun interface whose routes are not installed yet).
pub const SETTLE: Duration = Duration::from_millis(150);

// ---------------------------------------------------------------------------
// Satisfaction + active interfaces (Swift NetworkObserverAPI getters)
// ---------------------------------------------------------------------------

/// Swift `isWifiSatisfied` — approximated per the module doc.
pub fn is_wifi_satisfied(snap: &ScanSnapshot) -> bool {
    let has_en0 = snap
        .interfaces
        .iter()
        .any(|i| i.name.to_lowercase() == "en0");
    has_en0 && snap.routes.default_route(libc::AF_INET).is_some()
}

/// Swift `isWiredSatisfied` — approximated per the module doc.
pub fn is_wired_satisfied(snap: &ScanSnapshot) -> bool {
    let Some(def) = snap.routes.default_route(libc::AF_INET) else {
        return false;
    };
    snap.interfaces.iter().any(|i| {
        i.interface_index == def.interface_index
            && LocalInterfaceType::from_name(
                &i.name,
                i.interface_addresses
                    .v4
                    .first()
                    .map(|v| v.host.starts_with("169.254."))
                    .unwrap_or(false),
            ) == LocalInterfaceType::Ethernet
    })
}

/// Swift `isUsbSatisfied` — scan-derived, verbatim: an `en*`
/// interface other than en0 whose first IPv4 address is
/// link-local (169.254.*).
pub fn is_usb_satisfied(interfaces: &[NetInfo]) -> bool {
    interfaces.iter().any(|info| {
        let name = info.name.to_lowercase();
        name.starts_with("en")
            && name != "en0"
            && info
                .interface_addresses
                .v4
                .first()
                .map(|v| v.host.starts_with("169.254."))
                .unwrap_or(false)
    })
}

/// Swift `isBridgeSatisfied` — scan clause verbatim (name contains
/// "bridge" or "ap"); the NWPath clause is dropped (module doc).
pub fn is_bridge_satisfied(interfaces: &[NetInfo]) -> bool {
    interfaces.iter().any(|info| {
        let name = info.name.to_lowercase();
        name.contains("bridge") || name.contains("ap")
    })
}

/// Swift `isUTunAvailable` — scan contains a `utun*` interface.
pub fn is_utun_available(interfaces: &[NetInfo]) -> bool {
    interfaces
        .iter()
        .any(|i| i.name.to_lowercase().starts_with("utun"))
}

/// Swift `isIKEv2IPSecAvailable` — scan contains an `ipsec*`
/// interface.
pub fn is_ikev2_ipsec_available(interfaces: &[NetInfo]) -> bool {
    interfaces
        .iter()
        .any(|i| i.name.to_lowercase().starts_with("ipsec"))
}

/// Swift `activeInterfaces` — maps a scan to App-facing summaries,
/// sorted by interface-type priority then by (lowercased) name.
/// (Swift uses `localizedCompare` for the name ordering; for the
/// ASCII interface names involved this is plain case-insensitive
/// lexicographic order.)
pub fn active_interfaces(interfaces: &[NetInfo]) -> Vec<LocalInterfaceInfo> {
    let mut out: Vec<LocalInterfaceInfo> = interfaces
        .iter()
        .map(|info| {
            let is_link_local = info
                .interface_addresses
                .v4
                .first()
                .map(|v| v.host.starts_with("169.254."))
                .unwrap_or(false);
            let ty = LocalInterfaceType::from_name(&info.name, is_link_local);
            let v4 = info.interface_addresses.v4.first();
            let v6 = info.interface_addresses.v6.first();
            LocalInterfaceInfo {
                name: info.name.clone(),
                ip: v4
                    .map(|v| v.host.clone())
                    .unwrap_or_else(|| v6.cloned().unwrap_or_default()),
                ipv6: v6.cloned(),
                subnet: v4.map(|v| v.mask.clone()).unwrap_or_default(),
                interface_type: ty,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        a.interface_type
            .priority()
            .cmp(&b.interface_type.priority())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

// ---------------------------------------------------------------------------
// Attach/detach notification (Swift: UsbmuxdProxyServer notify calls
// inside refreshEndpoint; the proxy itself lands in M3)
// ---------------------------------------------------------------------------

/// Receives device attach/detach notifications from
/// `refresh_endpoint` (Swift: `UsbmuxdProxyServer`).
pub trait AttachListener: Send + Sync {
    fn device_attached(&self, tunnel_peer_ip: &str);
    fn device_detached(&self);
}

/// Async network-changed handler — Swift's `onNetworkChanged`
/// (wired to the core's status re-probe in M4).
pub type NetChangeHandler =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

// ---------------------------------------------------------------------------
// NetStack — the composition the Swift NetworkObserverService
// performs (connectionManager + endpoint + proxy notifications),
// plus the polling observer driving it.
// ---------------------------------------------------------------------------

pub struct NetStack {
    pub manager: Arc<DeviceConnectionManager>,
    pub endpoint: Arc<DeviceEndpoint>,
    scan_source: Arc<dyn ScanSource>,
    attach_listeners: Mutex<Vec<Arc<dyn AttachListener>>>,
    change_handlers: Mutex<Vec<NetChangeHandler>>,
}

impl NetStack {
    pub fn new(
        manager: Arc<DeviceConnectionManager>,
        endpoint: Arc<DeviceEndpoint>,
        scan_source: Arc<dyn ScanSource>,
    ) -> Self {
        Self {
            manager,
            endpoint,
            scan_source,
            attach_listeners: Mutex::new(Vec::new()),
            change_handlers: Mutex::new(Vec::new()),
        }
    }

    pub fn add_attach_listener(&self, listener: Arc<dyn AttachListener>) {
        self.attach_listeners.lock().unwrap().push(listener);
    }

    pub fn add_change_handler(&self, handler: NetChangeHandler) {
        self.change_handlers.lock().unwrap().push(handler);
    }

    fn notify_attached(&self, peer: &str) {
        for l in self.attach_listeners.lock().unwrap().iter() {
            l.device_attached(peer);
        }
    }

    fn notify_detached(&self) {
        for l in self.attach_listeners.lock().unwrap().iter() {
            l.device_detached();
        }
    }

    /// Swift `NetworkObserverService.refreshEndpoint()` — runs the
    /// connection manager's refresh and, only when it reports a
    /// change, applies the mode-specific endpoint/attach updates.
    /// Returns the manager's changed flag (Swift returns Void; the
    /// flag is surfaced for the FFI refresh entry).
    pub async fn refresh_endpoint(&self) -> bool {
        let manager = &self.manager;
        verbose_log("[minimuxer] [net] refreshing interfaces list and peers");
        let ifaces_changed = manager.refresh(false).await;
        if !ifaces_changed {
            return false;
        }

        match manager.get_preferred_connection_mode() {
            DeviceConnectionMode::NotConfigured => {
                debug_log(
                    "[minimuxer] [net] connection mode not configured. skipping endpoint update...",
                );
            }
            DeviceConnectionMode::LocalVpn => {
                if let Some(info) = manager.vpn_iface() {
                    verbose_log(&format!(
                        "[minimuxer] [net] vpn interface detected\n  • name: {}\n  • linkType: {}",
                        info.name,
                        info.link_type()
                    ));
                    let override_ip = manager.override_peer_ip();
                    let is_overridden = override_ip
                        .as_ref()
                        .map(|s| !s.is_empty())
                        .unwrap_or(false);
                    // When an override is active it is used iff it
                    // is reachable (no fallback to the derived
                    // peer); otherwise the derived peer is used iff
                    // reachable (Swift refreshEndpoint, verbatim).
                    let effective_ip = if is_overridden {
                        if manager.is_override_peer_ip_reachable() {
                            override_ip.clone()
                        } else {
                            None
                        }
                    } else if manager.is_derived_peer_ip_reachable() {
                        manager.derived_peer_ip()
                    } else {
                        None
                    };

                    if let Some(peer) = effective_ip {
                        self.endpoint.update(&peer);
                        self.notify_attached(&peer);
                    } else {
                        self.endpoint.clear();
                        self.notify_detached();
                    }
                } else {
                    verbose_log("[minimuxer] [net] no local VPN interface detected");
                    self.endpoint.clear();
                    self.notify_detached();
                }
            }
            DeviceConnectionMode::RemoteServer => {
                let is_reachable = manager.is_remote_server_ip_reachable();
                if let Some(remote_ip) = manager.remote_server_ip() {
                    if is_reachable {
                        self.endpoint.update(&remote_ip);
                        self.notify_attached(&remote_ip);
                    } else {
                        self.endpoint.clear();
                        self.notify_detached();
                    }
                } else {
                    verbose_log("[minimuxer] [net] remote server endpoint unreachable");
                    self.endpoint.clear();
                    self.notify_detached();
                }
            }
        }
        true
    }

    /// Swift `handleNetworkChange()` — refresh the endpoint, then
    /// unconditionally notify network-changed subscribers.
    pub async fn handle_network_change(&self) {
        self.refresh_endpoint().await;
        debug_log("[minimuxer] [net] dispatching status update to subscribers");
        let handlers = self.change_handlers.lock().unwrap().clone();
        for h in handlers {
            h().await;
        }
    }

    /// Fresh-scan satisfaction/accessor surface (Swift's
    /// NetworkObserverAPI getters scan on every call).
    pub fn snapshot(&self) -> ScanSnapshot {
        self.scan_source.snapshot()
    }

    pub fn active_interfaces(&self) -> Vec<LocalInterfaceInfo> {
        active_interfaces(&self.snapshot().interfaces)
    }
}

// ---------------------------------------------------------------------------
// Polling observer (design §5.1 main route)
// ---------------------------------------------------------------------------

/// The change signature the poller diffs: the interface set, the
/// default routes, and (redundantly, per design) the utun set —
/// any difference counts as a path change.
#[derive(Debug, Clone, PartialEq)]
struct Signature {
    interfaces: Vec<NetInfo>,
    default_v4: Option<(Option<String>, Option<String>, u16)>,
    default_v6: Option<(Option<String>, Option<String>, u16)>,
    utun_names: Vec<String>,
}

impl Signature {
    fn of(snap: &ScanSnapshot) -> Self {
        let route_sig = |e: Option<&crate::iface::RouteEntry>| {
            e.map(|r| {
                (
                    r.destination_ipv4
                        .clone()
                        .or_else(|| r.destination_ipv6.clone()),
                    r.gateway_ipv4.clone().or_else(|| r.gateway_ipv6.clone()),
                    r.interface_index,
                )
            })
        };
        let mut utun_names: Vec<String> = snap
            .interfaces
            .iter()
            .filter(|i| i.name.to_lowercase().starts_with("utun"))
            .map(|i| i.name.clone())
            .collect();
        utun_names.sort();
        Self {
            interfaces: snap.interfaces.clone(),
            default_v4: route_sig(snap.routes.default_route(libc::AF_INET)),
            default_v6: route_sig(snap.routes.default_route(libc::AF_INET6)),
            utun_names,
        }
    }

    fn differs_from(&self, other: &Signature) -> bool {
        !crate::iface::net_info_sets_equal(&self.interfaces, &other.interfaces)
            || self.default_v4 != other.default_v4
            || self.default_v6 != other.default_v6
            || self.utun_names != other.utun_names
    }
}

/// Pure-Rust polling replacement for NWPathMonitor. See the module
/// doc for parameters and the named semantic differences.
pub struct PollingObserver {
    stack: Arc<NetStack>,
    scan_source: Arc<dyn ScanSource>,
    poll_interval: Duration,
    settle: Duration,
    started: Mutex<bool>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl PollingObserver {
    pub fn new(stack: Arc<NetStack>, scan_source: Arc<dyn ScanSource>) -> Self {
        Self::with_timing(stack, scan_source, POLL_INTERVAL, SETTLE)
    }

    pub fn with_timing(
        stack: Arc<NetStack>,
        scan_source: Arc<dyn ScanSource>,
        poll_interval: Duration,
        settle: Duration,
    ) -> Self {
        Self {
            stack,
            scan_source,
            poll_interval,
            settle,
            started: Mutex::new(false),
            task: Mutex::new(None),
        }
    }

    pub fn is_started(&self) -> bool {
        *self.started.lock().unwrap()
    }

    /// Swift `NetworkObserverService.start()` — idempotent;
    /// returns false when already started.
    pub fn start(&self, handle: &tokio::runtime::Handle) -> bool {
        {
            let mut started = self.started.lock().unwrap();
            if *started {
                verbose_log("[minimuxer] [net] monitor already started");
                return false;
            }
            *started = true;
        }
        verbose_log("[minimuxer] [net] monitor started");

        let stack = self.stack.clone();
        let source = self.scan_source.clone();
        let poll = self.poll_interval;
        let settle = self.settle;
        let task = handle.spawn(async move {
            let mut last_emitted = Signature::of(&source.snapshot());
            loop {
                tokio::time::sleep(poll).await;
                let now = Signature::of(&source.snapshot());
                if !now.differs_from(&last_emitted) {
                    continue;
                }
                // Settle: re-scan once after a short delay and
                // emit only the settled state.
                if !settle.is_zero() {
                    tokio::time::sleep(settle).await;
                }
                let settled = Signature::of(&source.snapshot());
                if settled.differs_from(&last_emitted) {
                    last_emitted = settled;
                    verbose_log("[minimuxer] [net] path changed (poll)");
                    stack.handle_network_change().await;
                }
            }
        });
        *self.task.lock().unwrap() = Some(task);
        true
    }

    /// Swift `NetworkObserverService.stop()` — idempotent;
    /// returns false when not started.
    pub fn stop(&self) -> bool {
        {
            let mut started = self.started.lock().unwrap();
            if !*started {
                verbose_log("[minimuxer] [net] monitor already stopped");
                return false;
            }
            *started = false;
        }
        if let Some(task) = self.task.lock().unwrap().take() {
            task.abort();
        }
        verbose_log("[minimuxer] [net] monitor stopped");
        true
    }
}
