//! Connection management — port of Swift
//! `DeviceConnectionManager.swift` (yongshuai1013/minimuxer
//! @714aafa3): connection-mode determination, candidate tunnel/peer
//! derivation with parallel first-win probing, override handling,
//! remote-server mode, and the `ConnectionConfigBinding` write-back
//! semantics (7 setters / 3 getters / 1 async resolver).
//!
//! The Swift actor's gateway dependency (`pairingFileType`,
//! `servicePort`, `setPort`) is abstracted as `ServicePortSource`;
//! mm-gateway (M3) will provide the real one. Until then
//! `TablePortSource` keeps the Swift default port table in memory.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use mm_common::logging::{debug_log, verbose_log};
use mm_common::PairingProtocol;

use crate::iface::{NetInfo, ScanSource, TunnelType};
use crate::types::{DeviceConnectionMode, ServicePort};

// ---------------------------------------------------------------------------
// Gateway port surface (Swift: the gateway reads inside tcpProbe)
// ---------------------------------------------------------------------------

/// The slice of the device gateway the connection manager reads
/// (Swift `DeviceGatewayAPI.pairingFileType` / `.servicePort` /
/// `.setPort(_:for:)`).
pub trait ServicePortSource: Send + Sync {
    fn pairing_file_type(&self) -> PairingProtocol;
    fn service_port(&self) -> u16;
    fn set_port(&self, port: u16, for_protocol: PairingProtocol);
}

/// In-memory port table with the Swift defaults (lockdown 62078,
/// rppairing 49152); stand-in until mm-gateway lands in M3.
pub struct TablePortSource {
    protocol: Mutex<PairingProtocol>,
    ports: Mutex<HashMap<PairingProtocol, u16>>,
}

impl Default for TablePortSource {
    fn default() -> Self {
        let mut ports = HashMap::new();
        ports.insert(PairingProtocol::Lockdown, mm_common::constants::LOCKDOWND_PORT);
        ports.insert(
            PairingProtocol::RpPairing,
            mm_common::constants::REMOTE_PAIRING_PORT,
        );
        Self {
            protocol: Mutex::new(PairingProtocol::Unknown),
            ports: Mutex::new(ports),
        }
    }
}

impl TablePortSource {
    pub fn set_pairing_file_type(&self, protocol: PairingProtocol) {
        *self.protocol.lock().unwrap() = protocol;
    }
}

impl ServicePortSource for TablePortSource {
    fn pairing_file_type(&self) -> PairingProtocol {
        *self.protocol.lock().unwrap()
    }

    fn service_port(&self) -> u16 {
        let protocol = self.pairing_file_type();
        self.ports
            .lock()
            .unwrap()
            .get(&protocol)
            .copied()
            .unwrap_or_else(|| protocol.default_port(false))
    }

    fn set_port(&self, port: u16, for_protocol: PairingProtocol) {
        self.ports.lock().unwrap().insert(for_protocol, port);
    }
}

// ---------------------------------------------------------------------------
// ConnectionConfigBinding (Swift struct of closures)
// ---------------------------------------------------------------------------

/// Async service-port resolver — Swift's
/// `resolveServicePort: (ServicePort) async -> ServicePort`.
pub type ResolveServicePortFn = Arc<
    dyn Fn(ServicePort) -> Pin<Box<dyn Future<Output = ServicePort> + Send>> + Send + Sync,
>;

/// Swift `ConnectionConfigBinding`: the App-facing two-way sync
/// surface. Setters/getters are plain closures here; over the FFI
/// boundary (mm-ffi) they are presented by a registered Swift
/// object, per the port design.
pub struct ConnectionConfigBinding {
    pub set_tunnel_iface_ip: Box<dyn Fn(Option<&str>) + Send + Sync>,
    pub set_tunnel_peer_ip: Box<dyn Fn(Option<&str>) + Send + Sync>,
    pub set_tunnel_peer_subnet_mask: Box<dyn Fn(Option<&str>) + Send + Sync>,
    pub set_tunnel_peer_reachable: Box<dyn Fn(bool) + Send + Sync>,
    pub set_tunnel_iface_subnet_mask: Box<dyn Fn(Option<&str>) + Send + Sync>,
    pub set_remote_reachable: Box<dyn Fn(bool) + Send + Sync>,
    pub set_override_tunnel_peer_reachable: Box<dyn Fn(bool) + Send + Sync>,
    pub get_connection_mode: Box<dyn Fn() -> DeviceConnectionMode + Send + Sync>,
    pub get_override_tunnel_peer_ip: Box<dyn Fn() -> String + Send + Sync>,
    pub get_remote_server_ip: Box<dyn Fn() -> String + Send + Sync>,
    pub resolve_service_port: Option<ResolveServicePortFn>,
}

// ---------------------------------------------------------------------------
// Candidate peers (Swift: DeviceConnectionManager.CandidatePeer)
// ---------------------------------------------------------------------------

/// Which derivation priority produced a candidate — recorded in the
/// win log (design risk-11 instrumentation; Swift does not tag the
/// source, the port adds it to the log line only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSource {
    Gateway,
    Destination,
    LinkLayer,
}

impl CandidateSource {
    pub fn log_name(&self) -> &'static str {
        match self {
            CandidateSource::Gateway => "gateway",
            CandidateSource::Destination => "destination",
            CandidateSource::LinkLayer => "link-layer",
        }
    }
}

/// Swift `CandidatePeer` plus the derivation source (log-only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidatePeer {
    pub tunnel_name: String,
    pub ip: String,
    pub mask: Option<String>,
    pub source: CandidateSource,
}

/// Swift `resolveCandidateTunnels(from:)` — utun tunnels with at
/// least one IPv4 address and no IPv6 addresses, sorted by name.
pub fn resolve_candidate_tunnels(interfaces: &[NetInfo]) -> Vec<&NetInfo> {
    let mut tunnels: Vec<&NetInfo> = interfaces
        .iter()
        .filter(|info| {
            info.tunnel.as_ref().map(|t| t.tunnel_type) == Some(TunnelType::Utun)
                && !info.interface_addresses.v4.is_empty()
                && info.interface_addresses.v6.is_empty()
        })
        .collect();
    tunnels.sort_by(|a, b| a.name.cmp(&b.name));
    tunnels
}

/// Swift `isValidCandidatePeer(_:for:)` — the rejection table,
/// verbatim.
fn is_valid_candidate_peer(ip: &str, tunnel: &NetInfo) -> bool {
    if ip.is_empty()
        || ip == "0.0.0.0"
        || ip == "default"
        || ip == "255.255.255.255"
        || ip.starts_with("127.")
        || ip.starts_with("224.")
        || ip.starts_with("239.")
    {
        return false;
    }
    // Reject self-addresses.
    !tunnel.interface_addresses.v4.iter().any(|a| a.host == ip)
}

/// Swift `resolveCandidatePeers(for:)` — priority order:
/// (1) route destination gateways (mask /32), (2) route destination
/// IPs (route mask preserved), (3) point-to-point link-layer
/// destination (/32); deduplicated by IP, order preserved.
pub fn resolve_candidate_peers_for(tunnel: &NetInfo) -> Vec<CandidatePeer> {
    let mut candidates: Vec<CandidatePeer> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    fn add(
        candidates: &mut Vec<CandidatePeer>,
        seen: &mut HashSet<String>,
        tunnel: &NetInfo,
        ip: Option<&str>,
        mask: Option<String>,
        source: CandidateSource,
    ) {
        let Some(ip) = ip else { return };
        if !is_valid_candidate_peer(ip, tunnel) || seen.contains(ip) {
            return;
        }
        seen.insert(ip.to_string());
        candidates.push(CandidatePeer {
            tunnel_name: tunnel.name.clone(),
            ip: ip.to_string(),
            mask,
            source,
        });
    }

    let Some(data) = &tunnel.tunnel else {
        return candidates;
    };

    // Priority 1: destination gateways from the route table.
    for route in &data.destination_routes {
        add(
            &mut candidates,
            &mut seen,
            tunnel,
            route.gateway_ipv4.as_deref(),
            Some("255.255.255.255".to_string()),
            CandidateSource::Gateway,
        );
    }
    // Priority 2: route destination IPs, mask preserved.
    for route in &data.destination_routes {
        add(
            &mut candidates,
            &mut seen,
            tunnel,
            route.destination_ipv4.as_deref(),
            route.destination_ipv4_mask.clone(),
            CandidateSource::Destination,
        );
    }
    // Priority 3: point-to-point link-layer destination.
    add(
        &mut candidates,
        &mut seen,
        tunnel,
        data.link_layer_destination_ip
            .as_ref()
            .and_then(|ip| ip.v4.as_ref())
            .map(|v4| v4.host.as_str()),
        Some("255.255.255.255".to_string()),
        CandidateSource::LinkLayer,
    );

    candidates
}

/// Swift `resolveCandidatePeers(from:)` — candidates of all
/// candidate tunnels, in tunnel-name order.
pub fn resolve_candidate_peers(interfaces: &[NetInfo]) -> Vec<CandidatePeer> {
    resolve_candidate_tunnels(interfaces)
        .into_iter()
        .flat_map(resolve_candidate_peers_for)
        .collect()
}

// ---------------------------------------------------------------------------
// DeviceConnectionManager
// ---------------------------------------------------------------------------

/// The probe used for every candidate/override/remote check —
/// Swift `DeviceConnectionManager.tcpProbe(_:)`: probe the
/// gateway's current service port; on failure, if the binding has
/// a resolver, ask it for a new port and retry once; a reachable
/// resolved port is written back via `setPort`.
async fn probe_with_source(
    port_source: &Arc<dyn ServicePortSource>,
    binding: &Option<Arc<ConnectionConfigBinding>>,
    device_probe_timeout: i32,
    ip: Option<&str>,
) -> bool {
    let Some(ip) = ip.filter(|s| !s.is_empty()) else {
        debug_log("[minimuxer] [iface] tcpProbe skipped — IP is nil or empty");
        return false;
    };
    let ip_owned = ip.to_string();
    let current_protocol = port_source.pairing_file_type();
    let current_port = port_source.service_port();

    let probe_ip = ip_owned.clone();
    let mut reachable = tokio::task::spawn_blocking(move || {
        mm_common::test_tcp(&probe_ip, current_port, device_probe_timeout)
    })
    .await
    .unwrap_or(false);

    if !reachable {
        if let Some(resolver) = binding.as_ref().and_then(|b| b.resolve_service_port.clone()) {
            let current = ServicePort::new(current_protocol, current_port);
            let resolved = resolver(current).await;
            if resolved.port != current_port {
                let probe_ip = ip_owned.clone();
                let new_port = resolved.port;
                let new_reachable = tokio::task::spawn_blocking(move || {
                    mm_common::test_tcp(&probe_ip, new_port, device_probe_timeout)
                })
                .await
                .unwrap_or(false);
                if new_reachable {
                    port_source.set_port(resolved.port, current_protocol);
                    reachable = true;
                }
            }
        }
    }

    debug_log(&format!(
        "[minimuxer] [iface] tcpProbe {}:{} (protocol: .{}) -> {}",
        ip_owned,
        port_source.service_port(),
        port_source.pairing_file_type().raw_value(),
        if reachable { "reachable" } else { "unreachable" }
    ));
    reachable
}

#[derive(Default)]
struct ManagerState {
    interfaces_cache: Vec<NetInfo>,
    last_connection_mode: Option<DeviceConnectionMode>,
    // local vpn params
    vpn_iface: Option<NetInfo>,
    reported_peer_ip: Option<String>,
    derived_peer_ip: Option<String>,
    derived_peer_subnet_mask: Option<String>,
    is_derived_peer_ip_reachable: bool,
    override_peer_ip: Option<String>,
    is_override_peer_ip_reachable: bool,
    // remote server params
    remote_server_ip: Option<String>,
    is_remote_server_ip_reachable: bool,
}

/// Swift `DeviceConnectionManager` (actor → internally locked
/// state; async methods never hold the lock across an await).
pub struct DeviceConnectionManager {
    port_source: Arc<dyn ServicePortSource>,
    scan_source: Arc<dyn ScanSource>,
    binding: Mutex<Option<Arc<ConnectionConfigBinding>>>,
    state: Mutex<ManagerState>,
    device_probe_timeout: Mutex<i32>,
}

impl DeviceConnectionManager {
    pub fn new(
        port_source: Arc<dyn ServicePortSource>,
        scan_source: Arc<dyn ScanSource>,
    ) -> Self {
        Self {
            port_source,
            scan_source,
            binding: Mutex::new(None),
            state: Mutex::new(ManagerState::default()),
            device_probe_timeout: Mutex::new(mm_common::constants::DEFAULT_TCP_PROBE_TIMEOUT_MS),
        }
    }

    /// Swift `deviceProbeTimeout` (set via
    /// `MinimuxerImpl.setDeviceProbeTimeout`).
    pub fn device_probe_timeout(&self) -> i32 {
        *self.device_probe_timeout.lock().unwrap()
    }

    pub fn set_device_probe_timeout(&self, timeout_ms: i32) {
        *self.device_probe_timeout.lock().unwrap() = timeout_ms;
    }

    /// Swift `bindConnectionConfig(_:)`.
    pub fn bind_connection_config(&self, binding: ConnectionConfigBinding) {
        let mode = (binding.get_connection_mode)();
        verbose_log(&format!(
            "[minimuxer] [iface] preferred connection mode set in binding\n  • mode: .{} \n  • overrideTunnelPeerIp: {} \n  • remoteServerIp: {} \n",
            mode.raw_value(),
            (binding.get_override_tunnel_peer_ip)(),
            (binding.get_remote_server_ip)(),
        ));
        *self.binding.lock().unwrap() = Some(Arc::new(binding));
    }

    pub fn clear_connection_config(&self) {
        *self.binding.lock().unwrap() = None;
    }

    pub fn has_binding(&self) -> bool {
        self.binding.lock().unwrap().is_some()
    }

    /// Swift `getPreferredConnectionMode()` — `.notConfigured`
    /// while unbound.
    pub fn get_preferred_connection_mode(&self) -> DeviceConnectionMode {
        match self.binding.lock().unwrap().as_ref() {
            Some(b) => (b.get_connection_mode)(),
            None => DeviceConnectionMode::NotConfigured,
        }
    }

    fn binding_snapshot(&self) -> Option<Arc<ConnectionConfigBinding>> {
        self.binding.lock().unwrap().clone()
    }

    /// Swift `tcpProbe(_:)`.
    pub async fn tcp_probe(&self, ip: Option<&str>) -> bool {
        probe_with_source(
            &self.port_source,
            &self.binding_snapshot(),
            self.device_probe_timeout(),
            ip,
        )
        .await
    }

    /// Swift `resolveLocalVPNTunnel(from:)` — probes all candidate
    /// peers in parallel; the first reachable one wins and the rest
    /// are abandoned (Swift task-group `cancelAll` semantics; the
    /// blocking probe itself is bounded by the probe timeout).
    async fn resolve_local_vpn_tunnel(
        &self,
        interfaces: &[NetInfo],
    ) -> (Option<NetInfo>, Option<CandidatePeer>, bool) {
        let candidates = resolve_candidate_peers(interfaces);
        if candidates.is_empty() {
            return (None, None, false);
        }

        let mut set = tokio::task::JoinSet::new();
        for candidate in candidates {
            let port_source = self.port_source.clone();
            let binding = self.binding_snapshot();
            let timeout = self.device_probe_timeout();
            set.spawn(async move {
                let ok =
                    probe_with_source(&port_source, &binding, timeout, Some(&candidate.ip)).await;
                if ok {
                    Some(candidate)
                } else {
                    None
                }
            });
        }

        while let Some(joined) = set.join_next().await {
            if let Ok(Some(candidate)) = joined {
                set.abort_all();
                debug_log(&format!(
                    "[minimuxer] [iface] candidate peer {} won via {} (mask: {})",
                    candidate.ip,
                    candidate.source.log_name(),
                    candidate.mask.as_deref().unwrap_or("nil")
                ));
                let tunnel = interfaces
                    .iter()
                    .find(|i| i.name == candidate.tunnel_name)
                    .cloned();
                return (tunnel, Some(candidate), true);
            }
        }

        (None, None, false)
    }

    /// Swift `refresh(quietScan:)` — returns true when any tracked
    /// state changed (and the binding write-backs were emitted).
    pub async fn refresh(&self, quiet_scan: bool) -> bool {
        let connection_mode = self.get_preferred_connection_mode();
        let changed = match connection_mode {
            DeviceConnectionMode::NotConfigured => {
                debug_log(
                    "[minimuxer] [iface] connection mode not configured. skipping refresh...",
                );
                false
            }
            DeviceConnectionMode::LocalVpn => self.refresh_local_vpn(quiet_scan).await,
            DeviceConnectionMode::RemoteServer => self.refresh_remote_server().await,
        };
        self.state.lock().unwrap().last_connection_mode = Some(connection_mode);
        changed
    }

    async fn refresh_local_vpn(&self, quiet_scan: bool) -> bool {
        // Cache last state in locals (Swift refresh, localVPN case).
        let (
            last_interfaces,
            last_derived_peer,
            last_derived_mask,
            last_override_ip,
            last_derived_reachable,
            last_override_reachable,
            last_reported_peer,
            last_mode,
        ) = {
            let st = self.state.lock().unwrap();
            (
                st.interfaces_cache.clone(),
                st.derived_peer_ip.clone(),
                st.derived_peer_subnet_mask.clone(),
                st.override_peer_ip.clone(),
                st.is_derived_peer_ip_reachable,
                st.is_override_peer_ip_reachable,
                st.reported_peer_ip.clone(),
                st.last_connection_mode,
            )
        };

        let snap = self.scan_source.snapshot();
        if !quiet_scan {
            mm_common::logging::verbose_log(&crate::iface::format_net_info_list(&snap.interfaces));
            debug_log(&format!(
                "[minimuxer] [iface] total: {}",
                snap.interfaces.len()
            ));
        }
        let interfaces = snap.interfaces;

        let (resolved_tunnel, candidate_peer, is_derived_reachable) =
            self.resolve_local_vpn_tunnel(&interfaces).await;
        let new_reported_peer = resolved_tunnel.as_ref().and_then(|t| {
            t.tunnel
                .as_ref()
                .and_then(|d| d.link_layer_destination_ip.as_ref())
                .and_then(|ip| ip.v4.as_ref())
                .map(|v4| v4.host.clone())
        });
        let new_derived_peer = candidate_peer.as_ref().map(|c| c.ip.clone());
        let new_derived_mask = candidate_peer.as_ref().and_then(|c| c.mask.clone());

        let raw_override = self
            .binding_snapshot()
            .map(|b| (b.get_override_tunnel_peer_ip)());
        let new_override_ip = raw_override.filter(|s| !s.is_empty());
        let new_override_reachable = self.tcp_probe(new_override_ip.as_deref()).await;

        {
            let mut st = self.state.lock().unwrap();
            st.interfaces_cache = interfaces.clone();
            st.vpn_iface = resolved_tunnel.clone();
            st.reported_peer_ip = new_reported_peer.clone();
            st.derived_peer_ip = new_derived_peer.clone();
            st.derived_peer_subnet_mask = new_derived_mask.clone();
            st.is_derived_peer_ip_reachable = is_derived_reachable;
            st.override_peer_ip = new_override_ip.clone();
            st.is_override_peer_ip_reachable = new_override_reachable;
        }

        let unchanged = last_mode == Some(DeviceConnectionMode::LocalVpn)
            && crate::iface::net_info_sets_equal(&last_interfaces, &interfaces)
            && last_override_ip == new_override_ip
            && last_derived_peer == new_derived_peer
            && last_derived_mask == new_derived_mask
            && last_reported_peer == new_reported_peer
            && last_derived_reachable == is_derived_reachable
            && last_override_reachable == new_override_reachable;
        if unchanged {
            debug_log("[minimuxer] [iface] no interface state changes detected, skipping refresh");
            return false;
        }

        debug_log("[minimuxer] [iface] using the first uTun vpn interface info");
        if let Some(binding) = self.binding_snapshot() {
            let iface_v4 = resolved_tunnel
                .as_ref()
                .and_then(|t| t.interface_addresses.v4.first());
            (binding.set_tunnel_iface_ip)(iface_v4.map(|v| v.host.as_str()));
            (binding.set_tunnel_iface_subnet_mask)(iface_v4.map(|v| v.mask.as_str()));
            (binding.set_tunnel_peer_ip)(new_derived_peer.as_deref());
            (binding.set_tunnel_peer_subnet_mask)(new_derived_mask.as_deref());
            (binding.set_tunnel_peer_reachable)(is_derived_reachable);
            (binding.set_override_tunnel_peer_reachable)(new_override_reachable);
            // Clear auto-discovered reachability state of the other mode.
            (binding.set_remote_reachable)(false);
        }

        debug_log(&format!(
            "[minimuxer] [iface] refresh - rescan routes\n  • mode: .localVPN\n  • local iface count: {}\n  • probable-vpn host: {}\n  • probable-vpn mask: {}\n  • probable-vpn destination gateway IP: {}\n  • probable-vpn derived peer IP: {}\n  • probable-vpn derived peer mask: {}\n  • override peer IP: {}\n  • override peer reachable: {}",
            interfaces.len(),
            resolved_tunnel
                .as_ref()
                .and_then(|t| t.interface_addresses.v4.first())
                .map(|v| v.host.as_str())
                .unwrap_or("nil"),
            resolved_tunnel
                .as_ref()
                .and_then(|t| t.interface_addresses.v4.first())
                .map(|v| v.mask.as_str())
                .unwrap_or("nil"),
            new_reported_peer.as_deref().unwrap_or("nil"),
            new_derived_peer.as_deref().unwrap_or("nil"),
            new_derived_mask.as_deref().unwrap_or("nil"),
            new_override_ip.as_deref().unwrap_or("nil"),
            new_override_reachable,
        ));
        true
    }

    async fn refresh_remote_server(&self) -> bool {
        let raw_server_ip = self.binding_snapshot().map(|b| (b.get_remote_server_ip)());
        let server_ip = raw_server_ip.filter(|s| !s.is_empty());
        let reachable = self.tcp_probe(server_ip.as_deref()).await;

        let (last_mode, last_server_ip, last_reachable) = {
            let st = self.state.lock().unwrap();
            (
                st.last_connection_mode,
                st.remote_server_ip.clone(),
                st.is_remote_server_ip_reachable,
            )
        };
        if last_mode == Some(DeviceConnectionMode::RemoteServer)
            && server_ip == last_server_ip
            && reachable == last_reachable
        {
            debug_log("[minimuxer] [iface] no remote server state changes detected, skipping refresh");
            return false;
        }

        {
            let mut st = self.state.lock().unwrap();
            st.remote_server_ip = server_ip.clone();
            st.is_remote_server_ip_reachable = reachable;
            st.reported_peer_ip = None;
        }

        if let Some(binding) = self.binding_snapshot() {
            (binding.set_remote_reachable)(reachable);
            // Clear auto-discovered states but not the explicit override.
            (binding.set_tunnel_iface_ip)(None);
            (binding.set_tunnel_iface_subnet_mask)(None);
            (binding.set_tunnel_peer_ip)(None);
            (binding.set_tunnel_peer_subnet_mask)(None);
            (binding.set_tunnel_peer_reachable)(false);
            (binding.set_override_tunnel_peer_reachable)(false);
        }

        debug_log(&format!(
            "[minimuxer] [iface] refresh\n  • mode: .remoteServer\n  • remote server IP: {}\n  • remote server reachable: {}",
            server_ip.as_deref().unwrap_or("nil"),
            reachable,
        ));
        true
    }

    // -- Read accessors (Swift stored properties, read by the
    //    observer's refreshEndpoint and by isReady in M4) -------------

    pub fn vpn_iface(&self) -> Option<NetInfo> {
        self.state.lock().unwrap().vpn_iface.clone()
    }

    pub fn reported_peer_ip(&self) -> Option<String> {
        self.state.lock().unwrap().reported_peer_ip.clone()
    }

    pub fn derived_peer_ip(&self) -> Option<String> {
        self.state.lock().unwrap().derived_peer_ip.clone()
    }

    pub fn derived_peer_subnet_mask(&self) -> Option<String> {
        self.state.lock().unwrap().derived_peer_subnet_mask.clone()
    }

    pub fn is_derived_peer_ip_reachable(&self) -> bool {
        self.state.lock().unwrap().is_derived_peer_ip_reachable
    }

    pub fn override_peer_ip(&self) -> Option<String> {
        self.state.lock().unwrap().override_peer_ip.clone()
    }

    pub fn is_override_peer_ip_reachable(&self) -> bool {
        self.state.lock().unwrap().is_override_peer_ip_reachable
    }

    pub fn remote_server_ip(&self) -> Option<String> {
        self.state.lock().unwrap().remote_server_ip.clone()
    }

    pub fn is_remote_server_ip_reachable(&self) -> bool {
        self.state.lock().unwrap().is_remote_server_ip_reachable
    }
}
