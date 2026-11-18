//! Interface enumeration + Darwin route-table parsing — port of
//! Swift `NetworkIfaceScanner.swift` (yongshuai1013/minimuxer
//! @714aafa3), including `RouteSnapshot`, `NetInfo`/`TunnelNetInfo`
//! and the scanner's grouping/filtering semantics.
//!
//! The byte-level route parser and the scan builder are pure
//! functions over gathered records so they can be fixture-tested on
//! any host; only the gatherers at the bottom touch libc.

use std::collections::{HashMap, HashSet};
use std::fmt;

// ---------------------------------------------------------------------------
// IPv4 helpers (Swift: ipv4String / sockaddrIPv4 equivalents)
// ---------------------------------------------------------------------------

/// Numeric IPv4 value in the Swift scanner's convention: the raw is
/// the address as a host-order u32 (`10.7.0.1` == `0x0A07_0001`),
/// matching Swift's `sockaddrIPv4` (s_addr.bigEndian) and its
/// `ipv4String` inverse.
pub fn ipv4_string(raw: u32) -> String {
    std::net::Ipv4Addr::from(raw).to_string()
}

/// Parses a dotted-quad into the raw convention above.
pub fn ipv4_raw(s: &str) -> Option<u32> {
    s.parse::<std::net::Ipv4Addr>().ok().map(u32::from)
}

// ---------------------------------------------------------------------------
// IPv4Info / IPAddresses / IP
// ---------------------------------------------------------------------------

/// Swift `IPv4Info`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IPv4Info {
    pub host: String,
    pub mask: String,
    pub host_raw: u32,
    pub mask_raw: u32,
}

impl IPv4Info {
    pub fn new(host: String, mask: String, host_raw: u32, mask_raw: u32) -> Self {
        Self {
            host,
            mask,
            host_raw,
            mask_raw,
        }
    }

    /// Swift `networkBase`.
    pub fn network_base(&self) -> u32 {
        self.host_raw & self.mask_raw
    }

    /// Swift `broadcast`.
    pub fn broadcast(&self) -> u32 {
        self.network_base() | !self.mask_raw
    }
}

/// Swift `IPAddresses`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct IPAddresses {
    pub v4: Vec<IPv4Info>,
    pub v6: Vec<String>,
}

impl fmt::Display for IPAddresses {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        for item in &self.v4 {
            parts.push(format!("ipv4: {} mask: {}", item.host, item.mask));
        }
        for item in &self.v6 {
            parts.push(format!("ipv6: {item}"));
        }
        write!(f, "{}", parts.join(" | "))
    }
}

/// Swift `IP` — one (optional v4, optional v6) pair.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ip {
    pub v4: Option<IPv4Info>,
    pub v6: Option<String>,
}

impl fmt::Display for Ip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v4) = &self.v4 {
            parts.push(format!("ipv4: {} mask: {}", v4.host, v4.mask));
        }
        if let Some(v6) = &self.v6 {
            parts.push(format!("ipv6: {v6}"));
        }
        write!(f, "{}", parts.join(" | "))
    }
}

// ---------------------------------------------------------------------------
// Route table (Swift: RouteEntry / RouteSnapshot)
// ---------------------------------------------------------------------------

pub const RTF_UP: i32 = 0x1;
pub const RTF_GATEWAY: i32 = 0x2;
pub const RTF_HOST: i32 = 0x4;

/// Size of Darwin's `rt_msghdr` as laid out by the Swift scanner's
/// private struct mirror (2+1+1+2 +2 pad +4+4+4+4+4+4+4 +4 pad +
/// rt_metrics 14*4 = 92 bytes). The parser only reads the fields the
/// Swift code reads: msglen@0, index@4, flags@8, addrs@12.
pub const RT_MSGHDR_SIZE: usize = 92;

/// Darwin address families as they appear inside a route dump
/// (AF_INET6 is 30 on Darwin, 10 on Linux — the dump's bytes are
/// always Darwin's).
pub const AF_INET_DARWIN: u8 = 2;
pub const AF_INET6_DARWIN: u8 = 30;

const RTAX_DST: usize = 0;
const RTAX_GATEWAY: usize = 1;
const RTAX_NETMASK: usize = 2;
const RTAX_GENMASK: usize = 3;
#[allow(dead_code)] // completes the RTAX table mirror; Swift skips it too
const RTAX_IFP: usize = 4;
const RTAX_IFA: usize = 5;
const RTAX_MAX: usize = 8;

/// Swift `RouteEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouteEntry {
    pub destination_ipv4: Option<String>,
    pub destination_ipv4_raw: Option<u32>,
    pub destination_ipv4_mask: Option<String>,
    pub destination_ipv4_mask_raw: Option<u32>,
    pub destination_ipv6: Option<String>,
    pub interface_ipv4: Option<String>,
    pub interface_ipv4_raw: Option<u32>,
    pub interface_ipv4_mask: Option<String>,
    pub interface_ipv4_mask_raw: Option<u32>,
    pub interface_ipv6: Option<String>,
    pub gateway_ipv4: Option<String>,
    pub gateway_ipv4_raw: Option<u32>,
    pub gateway_ipv6: Option<String>,
    pub interface_index: u16,
    pub flags: i32,
}

/// Swift `roundUp` for sockaddr stepping: len>0 rounds up to a
/// 4-byte boundary; len==0 yields 4 (sizeof UInt32).
fn round_up(len: usize) -> usize {
    if len > 0 {
        ((len - 1) | 3) + 1
    } else {
        4
    }
}

/// Swift `sockaddrNetmaskIPv4` — netmask sockaddrs may be truncated;
/// bytes past `sa_len` read as zero. Always returns a value when the
/// slot is present (0 when sa_len == 0).
fn netmask_ipv4(blob: &[u8], cursor: usize, sa_len: usize) -> u32 {
    if sa_len == 0 {
        return 0;
    }
    let mut raw = [0u8; 4];
    for (b, slot) in raw.iter_mut().enumerate() {
        let pos = 4 + b;
        if pos < sa_len && cursor + pos < blob.len() {
            *slot = blob[cursor + pos];
        }
    }
    u32::from_be_bytes(raw)
}

/// Formats a 16-byte IPv6 address the way Swift's `sockaddrIPv6`
/// (getnameinfo NI_NUMERICHOST, scope stripped) would: link-local
/// addresses coming from the kernel carry the scope index embedded
/// in bytes 2..4 (KAME embedding); getnameinfo de-embeds it, so we
/// zero those bytes when they are non-zero for fe80::/10.
fn ipv6_string(bytes: &[u8; 16]) -> String {
    let mut b = *bytes;
    let is_link_local = b[0] == 0xfe && (b[1] & 0xc0) == 0x80;
    if is_link_local && (b[2] != 0 || b[3] != 0) {
        b[2] = 0;
        b[3] = 0;
    }
    std::net::Ipv6Addr::from(b).to_string()
}

/// Parses a Darwin `NET_RT_DUMP` blob into route entries — a pure
/// port of `RouteSnapshot.dumpRoutes()`. Parsing stops (keeping the
/// entries gathered so far) when a message header is truncated or a
/// message length is zero, exactly like the Swift loop.
pub fn parse_route_dump(blob: &[u8]) -> Vec<RouteEntry> {
    let mut entries = Vec::new();
    let mut offset = 0usize;

    while offset + RT_MSGHDR_SIZE <= blob.len() {
        let msglen = u16::from_le_bytes([blob[offset], blob[offset + 1]]) as usize;
        if msglen == 0 {
            break;
        }
        let index = u16::from_le_bytes([blob[offset + 4], blob[offset + 5]]);
        let flags = i32::from_le_bytes([
            blob[offset + 8],
            blob[offset + 9],
            blob[offset + 10],
            blob[offset + 11],
        ]);
        let addrs = i32::from_le_bytes([
            blob[offset + 12],
            blob[offset + 13],
            blob[offset + 14],
            blob[offset + 15],
        ]);

        let mut dest_ipv4: Option<String> = None;
        let mut dest_ipv4_raw: Option<u32> = None;
        let mut dest_ipv4_mask: Option<String> = None;
        let mut dest_ipv4_mask_raw: Option<u32> = None;
        let mut dest_ipv6: Option<String> = None;
        let mut iface_ipv4: Option<String> = None;
        let mut iface_ipv4_raw: Option<u32> = None;
        let mut iface_ipv4_mask: Option<String> = None;
        let mut iface_ipv4_mask_raw: Option<u32> = None;
        let mut iface_ipv6: Option<String> = None;
        let mut gw_ipv4: Option<String> = None;
        let mut gw_ipv4_raw: Option<u32> = None;
        let mut gw_ipv6: Option<String> = None;

        let mut cursor = offset + RT_MSGHDR_SIZE;
        for i in 0..RTAX_MAX {
            if (addrs & (1 << i)) == 0 {
                continue;
            }
            if cursor >= blob.len() {
                break;
            }
            let sa_len_byte = blob[cursor];
            let sa_len = if sa_len_byte == 0 {
                16 // sizeof(sockaddr) in the Swift fallback
            } else {
                sa_len_byte as usize
            };

            match i {
                RTAX_NETMASK => {
                    let raw = netmask_ipv4(blob, cursor, sa_len);
                    dest_ipv4_mask_raw = Some(raw);
                    dest_ipv4_mask = Some(ipv4_string(raw));
                }
                RTAX_GENMASK => {
                    let raw = netmask_ipv4(blob, cursor, sa_len);
                    iface_ipv4_mask_raw = Some(raw);
                    iface_ipv4_mask = Some(ipv4_string(raw));
                }
                RTAX_DST | RTAX_IFA | RTAX_GATEWAY => {
                    if cursor + 1 < blob.len() {
                        let family = blob[cursor + 1];
                        // The dump is a Darwin kernel structure: its
                        // sockaddr family bytes are Darwin values
                        // (AF_INET6 = 30) regardless of the host
                        // running this parser, so they must not be
                        // compared against the host's libc constants
                        // (Linux AF_INET6 = 10).
                        if family == AF_INET_DARWIN {
                            if cursor + 8 <= blob.len() {
                                let raw = u32::from_be_bytes([
                                    blob[cursor + 4],
                                    blob[cursor + 5],
                                    blob[cursor + 6],
                                    blob[cursor + 7],
                                ]);
                                let s = ipv4_string(raw);
                                match i {
                                    RTAX_DST => {
                                        dest_ipv4 = Some(s);
                                        dest_ipv4_raw = Some(raw);
                                    }
                                    RTAX_IFA => {
                                        iface_ipv4 = Some(s);
                                        iface_ipv4_raw = Some(raw);
                                    }
                                    _ => {
                                        gw_ipv4 = Some(s);
                                        gw_ipv4_raw = Some(raw);
                                    }
                                }
                            }
                        } else if family == AF_INET6_DARWIN {
                            if cursor + 24 <= blob.len() {
                                let mut bytes = [0u8; 16];
                                bytes.copy_from_slice(&blob[cursor + 8..cursor + 24]);
                                let s = ipv6_string(&bytes);
                                match i {
                                    RTAX_DST => dest_ipv6 = Some(s),
                                    RTAX_IFA => iface_ipv6 = Some(s),
                                    _ => gw_ipv6 = Some(s),
                                }
                            }
                        }
                    }
                }
                _ => {}
            }

            cursor += round_up(sa_len);
        }

        if (flags & RTF_HOST) != 0 && dest_ipv4_mask_raw.is_none() {
            dest_ipv4_mask_raw = Some(0xFFFF_FFFF);
            dest_ipv4_mask = Some("255.255.255.255".to_string());
        }

        entries.push(RouteEntry {
            destination_ipv4: dest_ipv4,
            destination_ipv4_raw: dest_ipv4_raw,
            destination_ipv4_mask: dest_ipv4_mask,
            destination_ipv4_mask_raw: dest_ipv4_mask_raw,
            destination_ipv6: dest_ipv6,
            interface_ipv4: iface_ipv4,
            interface_ipv4_raw: iface_ipv4_raw,
            interface_ipv4_mask: iface_ipv4_mask,
            interface_ipv4_mask_raw: iface_ipv4_mask_raw,
            interface_ipv6: iface_ipv6,
            gateway_ipv4: gw_ipv4,
            gateway_ipv4_raw: gw_ipv4_raw,
            gateway_ipv6: gw_ipv6,
            interface_index: index,
            flags,
        });
        offset += msglen;
    }

    entries
}

/// Swift `RouteSnapshot` query surface over parsed entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteTable {
    pub entries: Vec<RouteEntry>,
}

impl RouteTable {
    pub fn new(entries: Vec<RouteEntry>) -> Self {
        Self { entries }
    }

    pub fn routes_for_index(&self, index: u16) -> Vec<RouteEntry> {
        self.entries
            .iter()
            .filter(|e| e.interface_index == index)
            .cloned()
            .collect()
    }

    /// Swift `defaultRoute(family:)` — first entry whose destination
    /// is "default"/"0.0.0.0" (v4) or "default"/"::" (v6). The
    /// family argument is interpreted leniently (Darwin AF_INET6 =
    /// 30, Linux = 10) because callers pass their platform's libc
    /// value while entries are classified by the Darwin parser.
    pub fn default_route(&self, family: i32) -> Option<&RouteEntry> {
        if family == AF_INET_DARWIN as i32 {
            self.entries.iter().find(|e| {
                e.destination_ipv4.as_deref() == Some("default")
                    || e.destination_ipv4.as_deref() == Some("0.0.0.0")
            })
        } else if family == AF_INET6_DARWIN as i32 || family == libc::AF_INET6 {
            self.entries.iter().find(|e| {
                e.destination_ipv6.as_deref() == Some("default")
                    || e.destination_ipv6.as_deref() == Some("::")
            })
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// SubnetRoute / TunnelRoute / TunnelType
// ---------------------------------------------------------------------------

/// Swift `SubnetRoute` — the destination half of a route entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubnetRoute {
    pub destination_ipv4: Option<String>,
    pub destination_ipv4_raw: Option<u32>,
    pub destination_ipv4_mask: Option<String>,
    pub destination_ipv4_mask_raw: Option<u32>,
    pub destination_ipv6: Option<String>,
}

impl From<&RouteEntry> for SubnetRoute {
    fn from(e: &RouteEntry) -> Self {
        Self {
            destination_ipv4: e.destination_ipv4.clone(),
            destination_ipv4_raw: e.destination_ipv4_raw,
            destination_ipv4_mask: e.destination_ipv4_mask.clone(),
            destination_ipv4_mask_raw: e.destination_ipv4_mask_raw,
            destination_ipv6: e.destination_ipv6.clone(),
        }
    }
}

/// Swift `TunnelRoute` — destination + gateway of a route entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TunnelRoute {
    pub destination_ipv4: Option<String>,
    pub destination_ipv4_raw: Option<u32>,
    pub destination_ipv4_mask: Option<String>,
    pub destination_ipv4_mask_raw: Option<u32>,
    pub destination_ipv6: Option<String>,
    pub gateway_ipv4: Option<String>,
    pub gateway_ipv4_raw: Option<u32>,
    pub gateway_ipv6: Option<String>,
}

impl From<&RouteEntry> for TunnelRoute {
    fn from(e: &RouteEntry) -> Self {
        Self {
            destination_ipv4: e.destination_ipv4.clone(),
            destination_ipv4_raw: e.destination_ipv4_raw,
            destination_ipv4_mask: e.destination_ipv4_mask.clone(),
            destination_ipv4_mask_raw: e.destination_ipv4_mask_raw,
            destination_ipv6: e.destination_ipv6.clone(),
            gateway_ipv4: e.gateway_ipv4.clone(),
            gateway_ipv4_raw: e.gateway_ipv4_raw,
            gateway_ipv6: e.gateway_ipv6.clone(),
        }
    }
}

/// Swift `TunnelType` — prefix classification of tunnel interfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TunnelType {
    Utun,
    Ipsec,
}

impl TunnelType {
    /// Swift `TunnelType(interfaceName:)` — throws for non-tunnel
    /// names; here returns `None`.
    pub fn from_name(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        if lower.starts_with("utun") {
            Some(TunnelType::Utun)
        } else if lower.starts_with("ipsec") {
            Some(TunnelType::Ipsec)
        } else {
            None
        }
    }

    pub fn raw_value(&self) -> &'static str {
        match self {
            TunnelType::Utun => "utun",
            TunnelType::Ipsec => "ipsec",
        }
    }
}

/// Swift `LinkType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinkType {
    PointToPoint,
    Subnet,
    Loopback,
    None,
}

impl LinkType {
    pub fn raw_value(&self) -> &'static str {
        match self {
            LinkType::PointToPoint => "p2p",
            LinkType::Subnet => "subnet",
            LinkType::Loopback => "loopback",
            LinkType::None => "none",
        }
    }
}

impl fmt::Display for LinkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw_value())
    }
}

// ---------------------------------------------------------------------------
// NetInfo / TunnelNetInfo
// ---------------------------------------------------------------------------

/// Tunnel-specific payload of a `NetInfo` (Swift `TunnelNetInfo`
/// subclass). A `NetInfo` carrying this payload never equals one
/// without it (Swift's `==` requires identical dynamic type).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TunnelData {
    pub tunnel_type: TunnelType,
    pub link_layer_destination_ip: Option<Ip>,
    /// All raw route entries for this interface, unfiltered
    /// (Swift `destinationRoutes`).
    pub destination_routes: Vec<TunnelRoute>,
}

impl TunnelData {
    /// Swift `TunnelNetInfo.destinationIPs`.
    pub fn destination_ips(&self) -> Vec<Ip> {
        self.destination_routes
            .iter()
            .filter(|r| r.destination_ipv4.is_some() || r.destination_ipv6.is_some())
            .map(|r| {
                let v4 = r.destination_ipv4.as_ref().map(|host| IPv4Info {
                    host: host.clone(),
                    mask: r
                        .destination_ipv4_mask
                        .clone()
                        .unwrap_or_else(|| "255.255.255.255".to_string()),
                    host_raw: r.destination_ipv4_raw.unwrap_or(0),
                    mask_raw: r.destination_ipv4_mask_raw.unwrap_or(0xFFFF_FFFF),
                });
                Ip {
                    v4,
                    v6: r.destination_ipv6.clone(),
                }
            })
            .collect()
    }

    /// Swift `TunnelNetInfo.destinationGatewayIPs`.
    pub fn destination_gateway_ips(&self) -> Vec<Ip> {
        self.destination_routes
            .iter()
            .filter(|r| r.gateway_ipv4.is_some() || r.gateway_ipv6.is_some())
            .map(|r| {
                let v4 = r.gateway_ipv4.as_ref().map(|host| IPv4Info {
                    host: host.clone(),
                    mask: "255.255.255.255".to_string(),
                    host_raw: r.gateway_ipv4_raw.unwrap_or(0),
                    mask_raw: 0xFFFF_FFFF,
                });
                Ip {
                    v4,
                    v6: r.gateway_ipv6.clone(),
                }
            })
            .collect()
    }
}

/// Swift `NetInfo` (with the `TunnelNetInfo` subclass folded in as
/// `tunnel: Option<TunnelData>`).
#[derive(Debug, Clone)]
pub struct NetInfo {
    pub name: String,
    pub interface_index: u16,
    pub interface_addresses: IPAddresses,
    pub interface_subnet_routes: Vec<SubnetRoute>,
    pub flags: i32,
    pub tunnel: Option<TunnelData>,
}

pub const IFF_UP: i32 = 0x1;
pub const IFF_LOOPBACK: i32 = 0x8;
pub const IFF_POINTOPOINT: i32 = 0x10;
pub const IFF_RUNNING: i32 = 0x40;

impl NetInfo {
    /// Swift `NetInfo.isSubnetRoute(_:for:)` — a route belongs to
    /// the interface's subnets when its v4 destination raw equals
    /// one of the interface's network bases (addresses with a /32
    /// mask excluded); v6 destinations match on the fe80: prefix or
    /// a prefix relation with a local v6 address.
    pub fn is_subnet_route(entry: &RouteEntry, addresses: &IPAddresses) -> bool {
        let local_v4_bases: HashSet<u32> = addresses
            .v4
            .iter()
            .filter(|a| a.mask != "255.255.255.255")
            .map(|a| a.network_base())
            .collect();
        if let Some(raw_v4) = entry.destination_ipv4_raw {
            return local_v4_bases.contains(&raw_v4);
        }
        if let Some(dst_v6) = entry.destination_ipv6.as_ref().map(|s| s.to_lowercase()) {
            if dst_v6.starts_with("fe80:") {
                return true;
            }
            let local_v6: Vec<String> = addresses.v6.iter().map(|s| s.to_lowercase()).collect();
            return local_v6
                .iter()
                .any(|local| dst_v6.starts_with(local) || local.starts_with(&dst_v6));
        }
        false
    }

    pub fn is_up(&self) -> bool {
        (self.flags & IFF_UP) != 0
    }

    pub fn is_running(&self) -> bool {
        (self.flags & IFF_RUNNING) != 0
    }

    pub fn is_loopback(&self) -> bool {
        (self.flags & IFF_LOOPBACK) != 0
    }

    pub fn is_point_to_point(&self) -> bool {
        (self.flags & IFF_POINTOPOINT) != 0
    }

    /// Swift `linkType`, including the `TunnelNetInfo` override
    /// (point-to-point wins for tunnels before the loopback check).
    pub fn link_type(&self) -> LinkType {
        if let Some(t) = &self.tunnel {
            if self.is_point_to_point() || t.link_layer_destination_ip.is_some() {
                return LinkType::PointToPoint;
            }
        }
        if self.is_loopback() {
            return LinkType::Loopback;
        }
        if self.is_point_to_point() {
            return LinkType::PointToPoint;
        }
        if !self.interface_subnet_routes.is_empty() {
            return LinkType::Subnet;
        }
        LinkType::None
    }

    /// Swift `isEqual(to:)` — dynamic-type equality first (tunnel
    /// vs non-tunnel), then name/index/addresses/subnetRoutes/flags,
    /// plus the tunnel payload for tunnels.
    pub fn is_equal(&self, other: &NetInfo) -> bool {
        match (&self.tunnel, &other.tunnel) {
            (None, None) => {}
            (Some(a), Some(b)) => {
                if a.link_layer_destination_ip != b.link_layer_destination_ip
                    || a.destination_routes != b.destination_routes
                {
                    return false;
                }
            }
            _ => return false,
        }
        self.name == other.name
            && self.interface_index == other.interface_index
            && self.interface_addresses == other.interface_addresses
            && self.interface_subnet_routes == other.interface_subnet_routes
            && self.flags == other.flags
    }
}

impl PartialEq for NetInfo {
    fn eq(&self, other: &Self) -> bool {
        self.is_equal(other)
    }
}
impl Eq for NetInfo {}

impl fmt::Display for NetInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut desc = format!(
            "{} (idx: {}) [{}] | {}",
            self.name,
            self.interface_index,
            self.link_type(),
            self.interface_addresses
        );
        if !self.interface_subnet_routes.is_empty() {
            let subs: Vec<String> = self
                .interface_subnet_routes
                .iter()
                .filter_map(|r| r.destination_ipv4.clone().or_else(|| r.destination_ipv6.clone()))
                .collect();
            desc += &format!(" subnets: [{}]", subs.join(", "));
        }
        if let Some(t) = &self.tunnel {
            if let Some(dst) = &t.link_layer_destination_ip {
                desc += &format!(" linkLayerDest: {dst}");
            }
            if !t.destination_routes.is_empty() {
                let dsts: Vec<String> = t
                    .destination_routes
                    .iter()
                    .filter_map(|r| {
                        r.destination_ipv4.clone().or_else(|| r.destination_ipv6.clone())
                    })
                    .collect();
                desc += &format!(" destinationRoutes: [{}]", dsts.join(", "));
            }
        }
        write!(f, "{desc}")
    }
}

/// Order-insensitive equality of two interface collections —
/// Swift compares `Set<NetInfo>` values.
pub fn net_info_sets_equal(a: &[NetInfo], b: &[NetInfo]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().all(|x| b.iter().any(|y| x.is_equal(y)))
}

// ---------------------------------------------------------------------------
// Scanner (Swift: NetworkIfaceScanner.scan)
// ---------------------------------------------------------------------------

/// One `getifaddrs` list entry, pre-extracted into plain data so the
/// scan builder is fixture-testable. Address strings follow the
/// Swift extraction semantics (getnameinfo NI_NUMERICHOST; IPv6
/// scope suffix stripped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfaRecord {
    pub name: String,
    pub flags: i32,
    /// `sa_family` of `ifa_addr` (0 when the entry has no address).
    pub family: i32,
    /// AF_INET: (host raw, host string) of `ifa_addr`.
    pub addr_v4: Option<(u32, String)>,
    /// AF_INET: (mask raw, mask string) of `ifa_netmask`.
    pub mask_v4: Option<(u32, String)>,
    /// AF_INET6: host string of `ifa_addr`.
    pub addr_v6: Option<String>,
    /// Point-to-point `ifa_dstaddr`, v4: (host raw, host string).
    pub dst_v4: Option<(u32, String)>,
    /// Point-to-point `ifa_dstaddr`, v6 host string.
    pub dst_v6: Option<String>,
}

/// The result of one scan pass: built interfaces plus the route
/// table they were built against (kept for satisfaction checks that
/// need the default route).
#[derive(Debug, Clone, Default)]
pub struct ScanSnapshot {
    pub interfaces: Vec<NetInfo>,
    pub routes: RouteTable,
}

/// Builds the `NetInfo` collection from gathered records — a pure
/// port of the grouping/building half of
/// `NetworkIfaceScanner.scan`. `index_of` resolves interface names
/// to kernel indices (libc `if_nametoindex` live; a fixture map in
/// tests). `recapture_routes` implements the Swift "Case Y": a live
/// tunnel interface with no routes in the current snapshot triggers
/// one route re-capture, and the fresh table is used from then on.
pub fn build_net_infos(
    records: &[IfaRecord],
    initial_routes: RouteTable,
    index_of: &dyn Fn(&str) -> u16,
    recapture_routes: &mut dyn FnMut() -> RouteTable,
) -> ScanSnapshot {
    let mut routes = initial_routes;

    // 1. Group addresses by (lowercased) interface name.
    let mut addresses_map: HashMap<String, IPAddresses> = HashMap::new();
    let mut link_dest_map: HashMap<String, Ip> = HashMap::new();
    let mut flags_map: HashMap<String, i32> = HashMap::new();
    let mut ordered_names: Vec<String> = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();

    for rec in records {
        let is_up = (rec.flags & IFF_UP) != 0;
        if !is_up {
            continue;
        }
        let lower = rec.name.to_lowercase();
        if !seen_names.contains(&lower) {
            seen_names.insert(lower.clone());
            ordered_names.push(rec.name.clone());
            flags_map.insert(lower.clone(), rec.flags);
        }

        if rec.family == libc::AF_INET {
            if let (Some((host_raw, host_s)), Some((mask_raw, mask_s))) =
                (rec.addr_v4.clone(), rec.mask_v4.clone())
            {
                let info = IPv4Info::new(host_s, mask_s, host_raw, mask_raw);
                let entry = addresses_map.entry(lower.clone()).or_default();
                entry.v4.push(info.clone());

                if (rec.flags & IFF_POINTOPOINT) != 0 {
                    if let Some((dst_raw, dst_s)) = rec.dst_v4.clone() {
                        let dst_v4 = IPv4Info::new(dst_s, info.mask.clone(), dst_raw, info.mask_raw);
                        let prev_v6 = link_dest_map.get(&lower).and_then(|ip| ip.v6.clone());
                        link_dest_map.insert(
                            lower.clone(),
                            Ip {
                                v4: Some(dst_v4),
                                v6: prev_v6,
                            },
                        );
                    }
                }
            }
        } else if rec.family == libc::AF_INET6 {
            if let Some(ip6) = &rec.addr_v6 {
                let entry = addresses_map.entry(lower.clone()).or_default();
                entry.v6.push(ip6.clone());
            }
            if (rec.flags & IFF_POINTOPOINT) != 0 {
                if let Some(dst6) = &rec.dst_v6 {
                    let prev_v4 = link_dest_map.get(&lower).and_then(|ip| ip.v4.clone());
                    link_dest_map.insert(
                        lower.clone(),
                        Ip {
                            v4: prev_v4,
                            v6: Some(dst6.clone()),
                        },
                    );
                }
            }
        }
    }

    // 2. Build NetInfo / TunnelNetInfo objects.
    let mut result: Vec<NetInfo> = Vec::new();
    for name in &ordered_names {
        let lower = name.to_lowercase();
        let if_index = index_of(name) & 0xFFFF;
        if if_index == 0 {
            continue;
        }
        let Some(iface_addresses) = addresses_map.get(&lower) else {
            continue;
        };
        if iface_addresses.v4.is_empty() && iface_addresses.v6.is_empty() {
            continue;
        }
        let link_dest = link_dest_map.get(&lower).cloned();
        let if_flags = flags_map.get(&lower).copied().unwrap_or(0);

        // Case Y: tunnel interface alive but routeless in the
        // current snapshot — re-capture the route table once.
        if TunnelType::from_name(name).is_some()
            && !routes.entries.iter().any(|e| e.interface_index == if_index)
        {
            routes = recapture_routes();
        }

        let raw_routes = routes.routes_for_index(if_index);
        let subnet_routes: Vec<SubnetRoute> = raw_routes
            .iter()
            .filter(|e| NetInfo::is_subnet_route(e, iface_addresses))
            .map(SubnetRoute::from)
            .collect();

        let tunnel = TunnelType::from_name(name).map(|tt| TunnelData {
            tunnel_type: tt,
            link_layer_destination_ip: link_dest,
            destination_routes: raw_routes.iter().map(TunnelRoute::from).collect(),
        });

        result.push(NetInfo {
            name: name.clone(),
            interface_index: if_index,
            interface_addresses: iface_addresses.clone(),
            interface_subnet_routes: subnet_routes,
            flags: if_flags,
            tunnel,
        });
    }

    ScanSnapshot {
        interfaces: result,
        routes,
    }
}

/// Swift `formatNetInfoList` (verbose log rendering).
pub fn format_net_info_list(list: &[NetInfo]) -> String {
    let max_name = list.iter().map(|i| i.name.chars().count()).max().unwrap_or(0);
    let mut lines: Vec<String> = list
        .iter()
        .map(|info| {
            let pad = max_name.saturating_sub(info.name.chars().count());
            format!("  • {}{} {}", info.name, " ".repeat(pad), info)
        })
        .collect();
    lines.sort();
    format!(
        "[minimuxer] [iface] local interfaces list:\n---------------------------------------------------\n{}\n---------------------------------------------------",
        lines.join("\n")
    )
}

/// Source of scan snapshots — the live implementation reads the
/// kernel; tests inject scripted snapshots.
pub trait ScanSource: Send + Sync {
    fn snapshot(&self) -> ScanSnapshot;
}

/// Live scan source: getifaddrs + sysctl route dump.
pub struct LiveScanSource;

impl ScanSource for LiveScanSource {
    fn snapshot(&self) -> ScanSnapshot {
        scan_snapshot()
    }
}

/// One full live scan (Swift `NetworkIfaceScanner.scan`).
pub fn scan(quiet: bool) -> Vec<NetInfo> {
    if !quiet {
        mm_common::logging::debug_log("[minimuxer] [iface] scan requested...");
    }
    let snap = scan_snapshot();
    if !quiet {
        mm_common::logging::verbose_log(&format_net_info_list(&snap.interfaces));
        mm_common::logging::debug_log(&format!(
            "[minimuxer] [iface] total: {}",
            snap.interfaces.len()
        ));
    }
    snap.interfaces
}

/// Live scan returning interfaces + the route table used.
pub fn scan_snapshot() -> ScanSnapshot {
    let records = gather_ifaddrs();
    let routes = RouteTable::new(capture_route_entries());
    build_net_infos(&records, routes, &live_index_of, &mut || {
        RouteTable::new(capture_route_entries())
    })
}

fn live_index_of(name: &str) -> u16 {
    match std::ffi::CString::new(name) {
        Ok(c) => unsafe { (libc::if_nametoindex(c.as_ptr()) & 0xFFFF) as u16 },
        Err(_) => 0,
    }
}

// ---------------------------------------------------------------------------
// Live gatherers (libc)
// ---------------------------------------------------------------------------

/// Extracts an IPv4 (raw, string) pair from a `sockaddr_in` pointer.
unsafe fn sockaddr_in_pair(sa: *const libc::sockaddr) -> Option<(u32, String)> {
    if sa.is_null() {
        return None;
    }
    let sin = &*(sa as *const libc::sockaddr_in);
    let bytes = sin.sin_addr.s_addr.to_ne_bytes();
    // s_addr is stored in network byte order; its memory image is
    // the dotted-quad byte sequence on any host endianness.
    let raw = u32::from_be_bytes(bytes);
    Some((raw, ipv4_string(raw)))
}

/// Extracts an IPv6 host string via getnameinfo NI_NUMERICHOST,
/// mirroring Swift's `sockaddrIPv6` (scope suffix stripped).
unsafe fn sockaddr_in6_string(sa: *const libc::sockaddr) -> Option<String> {
    if sa.is_null() {
        return None;
    }
    let mut buf = [0 as libc::c_char; libc::NI_MAXHOST as usize];
    let len = std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t;
    let rc = libc::getnameinfo(
        sa,
        len,
        buf.as_mut_ptr(),
        buf.len() as libc::socklen_t,
        std::ptr::null_mut(),
        0,
        libc::NI_NUMERICHOST,
    );
    if rc != 0 {
        return None;
    }
    let cstr = std::ffi::CStr::from_ptr(buf.as_ptr());
    let s = cstr.to_string_lossy().into_owned();
    Some(s.split('%').next().unwrap_or(&s).to_string())
}

/// `ifa_dstaddr` accessor — Darwin exposes the field directly;
/// glibc wraps it in the `ifa_ifu` union.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "tvos", target_os = "watchos"))]
unsafe fn ifa_dstaddr(e: &libc::ifaddrs) -> *mut libc::sockaddr {
    e.ifa_dstaddr
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "tvos", target_os = "watchos")))]
unsafe fn ifa_dstaddr(e: &libc::ifaddrs) -> *mut libc::sockaddr {
    // The libc crate models glibc's ifa_ifu union as the pointer
    // itself (broadaddr/dstaddr share the slot).
    e.ifa_ifu
}

/// Walks `getifaddrs` and flattens the list into `IfaRecord`s.
pub fn gather_ifaddrs() -> Vec<IfaRecord> {
    let mut records = Vec::new();
    unsafe {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut head) != 0 || head.is_null() {
            return records;
        }
        let mut cur = head;
        while !cur.is_null() {
            let e = &*cur;
            let name = if e.ifa_name.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(e.ifa_name)
                    .to_string_lossy()
                    .into_owned()
            };
            let flags = e.ifa_flags as i32;
            let (family, addr_v4, mask_v4, addr_v6, dst_v4, dst_v6) = if e.ifa_addr.is_null() {
                (0, None, None, None, None, None)
            } else {
                let fam = (*e.ifa_addr).sa_family as i32;
                if fam == libc::AF_INET {
                    let addr = sockaddr_in_pair(e.ifa_addr);
                    let mask = if e.ifa_netmask.is_null() {
                        None
                    } else {
                        sockaddr_in_pair(e.ifa_netmask)
                    };
                    let dst = if (flags & IFF_POINTOPOINT) != 0 && !ifa_dstaddr(e).is_null() {
                        sockaddr_in_pair(ifa_dstaddr(e))
                    } else {
                        None
                    };
                    (fam, addr, mask, None, dst, None)
                } else if fam == libc::AF_INET6 {
                    let addr = sockaddr_in6_string(e.ifa_addr);
                    let dst = if (flags & IFF_POINTOPOINT) != 0 && !ifa_dstaddr(e).is_null() {
                        sockaddr_in6_string(ifa_dstaddr(e))
                    } else {
                        None
                    };
                    (fam, None, None, addr, None, dst)
                } else {
                    (fam, None, None, None, None, None)
                }
            };
            records.push(IfaRecord {
                name,
                flags,
                family,
                addr_v4,
                mask_v4,
                addr_v6,
                dst_v4,
                dst_v6,
            });
            cur = e.ifa_next;
        }
        libc::freeifaddrs(head);
    }
    records
}

/// Captures the kernel route table. On Darwin this is the sysctl
/// `NET_RT_DUMP` blob the Swift scanner parses; other platforms
/// have no equivalent dump here and yield no entries (the parser
/// itself is platform-independent and fixture-tested).
pub fn capture_route_entries() -> Vec<RouteEntry> {
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "tvos", target_os = "watchos"))]
    {
        dump_routes_sysctl()
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "tvos", target_os = "watchos")))]
    {
        Vec::new()
    }
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "tvos", target_os = "watchos"))]
fn dump_routes_sysctl() -> Vec<RouteEntry> {
    // mib: [CTL_NET, PF_ROUTE, 0, AF_UNSPEC, NET_RT_DUMP, 0] — the
    // exact vector the Swift scanner uses.
    let mut mib: [libc::c_int; 6] = [
        libc::CTL_NET,
        libc::PF_ROUTE,
        0,
        libc::AF_UNSPEC,
        libc::NET_RT_DUMP,
        0,
    ];
    unsafe {
        let mut needed: libc::size_t = 0;
        if libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            std::ptr::null_mut(),
            &mut needed,
            std::ptr::null_mut(),
            0,
        ) != 0
            || needed == 0
        {
            return Vec::new();
        }
        let mut buffer = vec![0u8; needed];
        if libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            buffer.as_mut_ptr() as *mut libc::c_void,
            &mut needed,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return Vec::new();
        }
        buffer.truncate(needed);
        parse_route_dump(&buffer)
    }
}
