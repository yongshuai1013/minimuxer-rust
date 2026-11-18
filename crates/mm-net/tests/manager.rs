//! DeviceConnectionManager + NetStack.refreshEndpoint behaviour
//! (INVENTORY 2.5/2.6): mode determination, candidate derivation,
//! parallel first-win probing against local listeners, override
//! rules, resolver retry, and binding write-back payloads.

mod common;

use common::*;
use mm_common::PairingProtocol;
use mm_net::{
    ConnectionConfigBinding, DeviceConnectionManager, DeviceConnectionMode, DeviceEndpoint,
    NetStack, ResolveServicePortFn, ScanSnapshot, ScanSource, ServicePort, ServicePortSource,
    TablePortSource,
};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------

struct FixedSource {
    snap: Mutex<ScanSnapshot>,
}

impl FixedSource {
    fn new(snap: ScanSnapshot) -> Self {
        Self {
            snap: Mutex::new(snap),
        }
    }
}

impl ScanSource for FixedSource {
    fn snapshot(&self) -> ScanSnapshot {
        self.snap.lock().unwrap().clone()
    }
}

struct BindingScript {
    mode: Mutex<DeviceConnectionMode>,
    override_ip: Mutex<String>,
    remote_ip: Mutex<String>,
    calls: Mutex<Vec<String>>,
}

impl BindingScript {
    fn new(mode: DeviceConnectionMode) -> Arc<Self> {
        Arc::new(Self {
            mode: Mutex::new(mode),
            override_ip: Mutex::new(String::new()),
            remote_ip: Mutex::new(String::new()),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn binding(self: &Arc<Self>) -> ConnectionConfigBinding {
        let opt = |v: Option<&str>| v.unwrap_or("nil").to_string();
        let s1 = self.clone();
        let s2 = self.clone();
        let s3 = self.clone();
        let s4 = self.clone();
        let s5 = self.clone();
        let s6 = self.clone();
        let s7 = self.clone();
        let s8 = self.clone();
        let s9 = self.clone();
        let s10 = self.clone();
        ConnectionConfigBinding {
            set_tunnel_iface_ip: Box::new(move |v| {
                s1.calls
                    .lock()
                    .unwrap()
                    .push(format!("setTunnelIfaceIp({})", opt(v)))
            }),
            set_tunnel_peer_ip: Box::new(move |v| {
                s2.calls
                    .lock()
                    .unwrap()
                    .push(format!("setTunnelPeerIp({})", opt(v)))
            }),
            set_tunnel_peer_subnet_mask: Box::new(move |v| {
                s3.calls.lock().unwrap().push(format!(
                    "setTunnelPeerSubnetMask({})",
                    opt(v)
                ))
            }),
            set_tunnel_peer_reachable: Box::new(move |v| {
                s4.calls
                    .lock()
                    .unwrap()
                    .push(format!("setTunnelPeerReachable({v})"))
            }),
            set_tunnel_iface_subnet_mask: Box::new(move |v| {
                s5.calls.lock().unwrap().push(format!(
                    "setTunnelIfaceSubnetMask({})",
                    opt(v)
                ))
            }),
            set_remote_reachable: Box::new(move |v| {
                s6.calls
                    .lock()
                    .unwrap()
                    .push(format!("setRemoteReachable({v})"))
            }),
            set_override_tunnel_peer_reachable: Box::new(move |v| {
                s7.calls.lock().unwrap().push(format!(
                    "setOverrideTunnelPeerReachable({v})"
                ))
            }),
            get_connection_mode: Box::new(move || *s8.mode.lock().unwrap()),
            get_override_tunnel_peer_ip: Box::new(move || {
                s9.override_ip.lock().unwrap().clone()
            }),
            get_remote_server_ip: Box::new(move || s10.remote_ip.lock().unwrap().clone()),
            resolve_service_port: None,
        }
    }
}

/// Builds a snapshot whose utun0 default-route gateway is
/// `gateway_ip` (plus the usual /24 subnet route).
fn tunnel_snapshot(gateway_ip: &str) -> ScanSnapshot {
    let gw = mm_net::ipv4_raw(gateway_ip).unwrap().to_be_bytes();
    let records = vec![
        v4_rec("en0", IFF_UP | IFF_RUNNING, "192.168.1.5", "255.255.255.0"),
        v4_rec("utun0", IFF_UP | IFF_RUNNING, "10.7.0.10", "255.255.255.0"),
    ];
    let mut blob = Vec::new();
    blob.extend(rt_msg(
        9,
        0,
        &[
            (RTAX_DST, sa_v4([10, 7, 0, 0])),
            (RTAX_NETMASK, sa_mask(&[255, 255, 255])),
            (RTAX_IFA, sa_v4([10, 7, 0, 10])),
        ],
    ));
    blob.extend(rt_msg(
        9,
        RTF_GATEWAY,
        &[
            (RTAX_DST, sa_v4([0, 0, 0, 0])),
            (RTAX_GATEWAY, sa_v4(gw)),
            (RTAX_NETMASK, sa_mask(&[])),
            (RTAX_IFA, sa_v4([10, 7, 0, 10])),
        ],
    ));
    build(&records, &blob, &[("en0", 4), ("utun0", 9)])
}

fn port_source(port: u16) -> Arc<TablePortSource> {
    let ps = Arc::new(TablePortSource::default());
    ps.set_pairing_file_type(PairingProtocol::Lockdown);
    ps.set_port(port, PairingProtocol::Lockdown);
    ps
}

fn listen_on(ip: &str) -> (std::net::TcpListener, u16) {
    let listener = std::net::TcpListener::bind((ip, 0)).expect("bind listener");
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

// ---------------------------------------------------------------------------
// Candidate derivation (pure)
// ---------------------------------------------------------------------------

#[test]
fn candidate_priority_and_filter_table() {
    let snap = tunnel_snapshot("10.7.0.1");
    let tun = iface(&snap, "utun0");
    let candidates = mm_net::resolve_candidate_peers_for(tun);
    // Priority 1: gateway 10.7.0.1 /32. Priority 2: destinations —
    // 10.7.0.0 (subnet, /24 mask kept), then 10.7.0.1 again (dup,
    // skipped). No link-layer destination in this fixture.
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].ip, "10.7.0.1");
    assert_eq!(candidates[0].mask.as_deref(), Some("255.255.255.255"));
    assert_eq!(candidates[0].source, mm_net::CandidateSource::Gateway);
    assert_eq!(candidates[1].ip, "10.7.0.0");
    assert_eq!(candidates[1].mask.as_deref(), Some("255.255.255.0"));
    assert_eq!(candidates[1].source, mm_net::CandidateSource::Destination);
}

#[test]
fn candidate_filters_reject_non_unicast_and_self() {
    // Tunnel whose routes carry every rejected shape plus its own
    // address as a gateway.
    let records = vec![v4_rec("utun0", IFF_UP, "10.7.0.10", "255.255.255.0")];
    let mut blob = Vec::new();
    for gw in [
        [0, 0, 0, 0],
        [255, 255, 255, 255],
        [127, 0, 0, 1],
        [224, 0, 0, 1],
        [239, 1, 2, 3],
        [10, 7, 0, 10], // self
    ] {
        blob.extend(rt_msg(
            9,
            RTF_GATEWAY,
            &[
                (RTAX_DST, sa_v4([0, 0, 0, 0])),
                (RTAX_GATEWAY, sa_v4(gw)),
                (RTAX_IFA, sa_v4([10, 7, 0, 10])),
            ],
        ));
    }
    let snap = build(&records, &blob, &[("utun0", 9)]);
    let tun = iface(&snap, "utun0");
    assert!(mm_net::resolve_candidate_peers_for(tun).is_empty());
}

#[test]
fn candidate_tunnels_require_utun_v4_only() {
    let records = vec![
        v4_rec("utun0", IFF_UP, "10.7.0.10", "255.255.255.0"),
        v4_rec("utun1", IFF_UP, "10.8.0.2", "255.255.255.0"),
        v6_rec("utun1", IFF_UP, "fe80::5"),
        v4_rec("ipsec0", IFF_UP, "10.9.0.2", "255.255.255.0"),
        v4_rec("en0", IFF_UP, "192.168.1.5", "255.255.255.0"),
    ];
    let snap = build(
        &records,
        &[],
        &[("utun0", 9), ("utun1", 10), ("ipsec0", 11), ("en0", 4)],
    );
    let tunnels = mm_net::resolve_candidate_tunnels(&snap.interfaces);
    // utun1 has a v6 address → excluded; ipsec0 is not utun.
    assert_eq!(tunnels.len(), 1);
    assert_eq!(tunnels[0].name, "utun0");
}

// ---------------------------------------------------------------------------
// Refresh behaviour with live listeners
// ---------------------------------------------------------------------------

#[tokio::test]
async fn derived_peer_wins_via_gateway_and_writes_back() {
    let Some(host) = host_ipv4() else {
        eprintln!("skip: no non-loopback IPv4 on this host");
        return;
    };
    let (_listener, port) = listen_on(&host);
    let source = Arc::new(FixedSource::new(tunnel_snapshot(&host)));
    let manager = Arc::new(DeviceConnectionManager::new(
        port_source(port),
        source.clone(),
    ));
    let script = BindingScript::new(DeviceConnectionMode::LocalVpn);
    manager.bind_connection_config(script.binding());

    let endpoint = Arc::new(DeviceEndpoint::new());
    let stack = NetStack::new(manager.clone(), endpoint.clone(), source.clone());
    stack.refresh_endpoint().await;

    assert_eq!(manager.derived_peer_ip().as_deref(), Some(host.as_str()));
    assert_eq!(
        manager.derived_peer_subnet_mask().as_deref(),
        Some("255.255.255.255")
    );
    assert!(manager.is_derived_peer_ip_reachable());
    assert_eq!(endpoint.ip().unwrap(), host);

    let calls = script.calls();
    assert!(calls.contains(&format!("setTunnelPeerIp({host})")), "{calls:?}");
    assert!(calls.contains(&"setTunnelPeerReachable(true)".to_string()));
    assert!(calls.contains(&"setRemoteReachable(false)".to_string()));
    assert!(calls.contains(&"setTunnelIfaceIp(10.7.0.10)".to_string()));

    // Second refresh with no changes → false, no new write-backs.
    let before = script.calls().len();
    assert!(!manager.refresh(false).await);
    assert_eq!(script.calls().len(), before);
}

#[tokio::test]
async fn unreachable_derived_peer_leaves_endpoint_unset() {
    let Some(host) = host_ipv4() else {
        return;
    };
    // The gateway is a non-local RFC1918 address with no route:
    // the probe fails in every environment (EPERM under sandboxed
    // NAT, timeout/no-route on a plain host). A merely-closed port
    // on the host's own address is NOT a reliable negative — some
    // sandboxes accept all TCP to the host NAT address.
    let (_l, port) = listen_on(&host);
    drop(_l);
    let source = Arc::new(FixedSource::new(tunnel_snapshot("10.255.255.1")));
    let manager = Arc::new(DeviceConnectionManager::new(
        port_source(port),
        source.clone(),
    ));
    let script = BindingScript::new(DeviceConnectionMode::LocalVpn);
    manager.bind_connection_config(script.binding());
    let endpoint = Arc::new(DeviceEndpoint::new());
    let stack = NetStack::new(manager.clone(), endpoint.clone(), source.clone());
    stack.refresh_endpoint().await;

    assert_eq!(manager.derived_peer_ip(), None);
    assert!(!manager.is_derived_peer_ip_reachable());
    assert!(endpoint.ip().is_err());
    assert!(script
        .calls()
        .contains(&"setTunnelPeerIp(nil)".to_string()));
}

#[tokio::test]
async fn override_reachable_beats_derived() {
    let Some(host) = host_ipv4() else {
        return;
    };
    // Swift semantics (refreshEndpoint): the override is only
    // consulted when a derived peer won (vpnIface != nil); then a
    // reachable override replaces the derived peer as the endpoint
    // ("when override active, we don't question user intent").
    // Both probes use the same service port, so the two listeners
    // share one port on different addresses.
    let (_listener, port) = listen_on(&host);
    let _loop_listener = std::net::TcpListener::bind(("127.0.0.1", port)).expect("bind loopback");
    let source = Arc::new(FixedSource::new(tunnel_snapshot(&host)));
    let manager = Arc::new(DeviceConnectionManager::new(
        port_source(port),
        source.clone(),
    ));
    let script = BindingScript::new(DeviceConnectionMode::LocalVpn);
    *script.override_ip.lock().unwrap() = "127.0.0.1".to_string();
    manager.bind_connection_config(script.binding());
    let endpoint = Arc::new(DeviceEndpoint::new());
    let stack = NetStack::new(manager.clone(), endpoint.clone(), source.clone());
    stack.refresh_endpoint().await;

    assert_eq!(manager.derived_peer_ip().as_deref(), Some(host.as_str()));
    assert!(manager.is_override_peer_ip_reachable());
    assert_eq!(
        endpoint.ip().unwrap(),
        "127.0.0.1",
        "override wins over the derived peer"
    );
}

#[tokio::test]
async fn override_alone_cannot_create_endpoint_when_derived_loses() {
    let Some(host) = host_ipv4() else {
        return;
    };
    // Companion case pinned from the Swift source: with no winning
    // derived candidate, vpnIface is nil and refreshEndpoint clears
    // the endpoint WITHOUT consulting the override — even when the
    // override itself is reachable.
    let (_listener, port) = listen_on(&host);
    let source = Arc::new(FixedSource::new(tunnel_snapshot("10.7.0.99")));
    let manager = Arc::new(DeviceConnectionManager::new(
        port_source(port),
        source.clone(),
    ));
    let script = BindingScript::new(DeviceConnectionMode::LocalVpn);
    *script.override_ip.lock().unwrap() = host.clone();
    manager.bind_connection_config(script.binding());
    let endpoint = Arc::new(DeviceEndpoint::new());
    let stack = NetStack::new(manager.clone(), endpoint.clone(), source.clone());
    stack.refresh_endpoint().await;

    assert!(manager.is_override_peer_ip_reachable());
    assert_eq!(manager.derived_peer_ip(), None);
    assert!(endpoint.ip().is_err(), "vpnIface nil ⇒ endpoint cleared");
}

#[tokio::test]
async fn override_unreachable_clears_endpoint_without_fallback() {
    let Some(host) = host_ipv4() else {
        return;
    };
    let (_listener, port) = listen_on(&host);
    // Derived IS reachable (gateway = host with listener)...
    let source = Arc::new(FixedSource::new(tunnel_snapshot(&host)));
    let manager = Arc::new(DeviceConnectionManager::new(
        port_source(port),
        source.clone(),
    ));
    let script = BindingScript::new(DeviceConnectionMode::LocalVpn);
    // ...but the override points at an unroutable address
    // (non-local RFC1918, no route): Swift semantics — override
    // unreachable ⇒ endpoint cleared, derived NOT used.
    *script.override_ip.lock().unwrap() = "10.255.255.1".to_string();
    manager.bind_connection_config(script.binding());
    let endpoint = Arc::new(DeviceEndpoint::new());
    let stack = NetStack::new(manager.clone(), endpoint.clone(), source.clone());
    stack.refresh_endpoint().await;

    assert!(manager.is_derived_peer_ip_reachable());
    assert!(!manager.is_override_peer_ip_reachable());
    assert!(endpoint.ip().is_err(), "no fallback to derived");
}

#[tokio::test]
async fn resolver_supplies_working_port_and_sets_it() {
    // tcp_probe itself applies no candidate filtering, so loopback
    // is usable here — and loopback is the one address family whose
    // closed-port refusal behaves conventionally in every sandbox.
    let (_listener, good_port) = listen_on("127.0.0.1");
    let (_l2, bad_port) = listen_on("127.0.0.1");
    drop(_l2); // bad_port is closed
    let source = Arc::new(FixedSource::new(tunnel_snapshot("192.0.2.1")));
    let ps = port_source(bad_port);
    let manager = Arc::new(DeviceConnectionManager::new(ps.clone(), source.clone()));
    let script = BindingScript::new(DeviceConnectionMode::LocalVpn);
    let mut binding = script.binding();
    let resolver: ResolveServicePortFn = Arc::new(move |current: ServicePort| {
        Box::pin(async move { ServicePort::new(current.protocol_type, good_port) })
    });
    binding.resolve_service_port = Some(resolver);
    manager.bind_connection_config(binding);

    assert!(manager.tcp_probe(Some("127.0.0.1")).await);
    // The gateway port table now holds the resolved port.
    assert_eq!(
        ps.service_port(),
        good_port,
        "setPort must be called with the resolved port"
    );
}

#[tokio::test]
async fn not_configured_without_binding() {
    let source = Arc::new(FixedSource::new(tunnel_snapshot("10.7.0.1")));
    let manager = DeviceConnectionManager::new(port_source(62078), source);
    assert_eq!(
        manager.get_preferred_connection_mode(),
        DeviceConnectionMode::NotConfigured
    );
    assert!(!manager.refresh(false).await);
}

#[tokio::test]
async fn remote_server_mode_probes_and_clears_local_state() {
    let Some(host) = host_ipv4() else {
        return;
    };
    let (_listener, port) = listen_on(&host);
    let source = Arc::new(FixedSource::new(tunnel_snapshot("10.7.0.1")));
    let manager = Arc::new(DeviceConnectionManager::new(
        port_source(port),
        source.clone(),
    ));
    let script = BindingScript::new(DeviceConnectionMode::RemoteServer);
    *script.remote_ip.lock().unwrap() = host.clone();
    manager.bind_connection_config(script.binding());
    let endpoint = Arc::new(DeviceEndpoint::new());
    let stack = NetStack::new(manager.clone(), endpoint.clone(), source.clone());
    stack.refresh_endpoint().await;

    assert!(manager.is_remote_server_ip_reachable());
    assert_eq!(endpoint.ip().unwrap(), host);
    let calls = script.calls();
    assert!(calls.contains(&"setRemoteReachable(true)".to_string()));
    assert!(calls.contains(&"setTunnelPeerIp(nil)".to_string()));
    assert!(calls.contains(&"setTunnelPeerReachable(false)".to_string()));
    // No-change second refresh.
    assert!(!manager.refresh(false).await);
}
