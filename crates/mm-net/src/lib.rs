//! `mm-net` — network observation, connection management and the
//! device endpoint: the Rust port of Swift subsystems 4
//! (NetworkObserverService + NetworkIfaceScanner), 5
//! (DeviceConnectionManager) and 6 (DeviceEndpoint) from
//! yongshuai1013/minimuxer @714aafa3.
//!
//! Behaviour follows the Swift sources line-by-line (design D5);
//! the one structural replacement is NWPathMonitor → a pure-Rust
//! polling observer (design §5.1), whose parameters and named
//! semantic differences are documented in [`observer`].

pub mod endpoint;
pub mod iface;
pub mod manager;
pub mod observer;
pub mod types;

pub use endpoint::{DeviceEndpoint, EndpointIpSink};
pub use iface::{
    build_net_infos, capture_route_entries, format_net_info_list, gather_ifaddrs,
    ipv4_raw, ipv4_string, net_info_sets_equal, parse_route_dump, scan, scan_snapshot,
    IfaRecord, Ip, IPAddresses, IPv4Info, LinkType, LiveScanSource, NetInfo, RouteEntry,
    RouteTable, ScanSnapshot, ScanSource, SubnetRoute, TunnelData, TunnelRoute, TunnelType,
};
pub use manager::{
    resolve_candidate_peers, resolve_candidate_peers_for, resolve_candidate_tunnels,
    CandidatePeer, CandidateSource, ConnectionConfigBinding, DeviceConnectionManager,
    ResolveServicePortFn, ServicePortSource, TablePortSource,
};
pub use observer::{
    active_interfaces, is_bridge_satisfied, is_ikev2_ipsec_available, is_usb_satisfied,
    is_utun_available, is_wifi_satisfied, is_wired_satisfied, AttachListener, NetChangeHandler,
    NetStack, PollingObserver, POLL_INTERVAL, SETTLE,
};
pub use types::{DeviceConnectionMode, LocalInterfaceInfo, LocalInterfaceType, ServicePort};

/// Crate name, for wiring/smoke checks.
pub const CRATE_NAME: &str = "mm-net";
