//! Shared fixture builders for mm-net tests: Darwin `NET_RT_DUMP`
//! blob construction (byte-exact per the layout the Swift scanner
//! parses) and `IfaRecord` sets modelling real interface states.

#![allow(dead_code)]

use mm_net::{IfaRecord, NetInfo, RouteTable, ScanSnapshot};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Route blob builder
// ---------------------------------------------------------------------------

pub const RT_MSGHDR: usize = 92;

fn round_up(len: usize) -> usize {
    if len > 0 {
        ((len - 1) | 3) + 1
    } else {
        4
    }
}

/// A 16-byte AF_INET sockaddr (len, family, port, addr, padding).
pub fn sa_v4(ip: [u8; 4]) -> Vec<u8> {
    let mut v = vec![16u8, 2, 0, 0];
    v.extend_from_slice(&ip);
    v.extend_from_slice(&[0; 8]);
    v
}

/// A 28-byte AF_INET6 sockaddr.
pub fn sa_v6(addr: [u8; 16]) -> Vec<u8> {
    let mut v = vec![28u8, 30, 0, 0, 0, 0, 0, 0];
    v.extend_from_slice(&addr);
    v.extend_from_slice(&[0; 4]);
    v
}

/// A netmask sockaddr carrying only `significant` mask bytes
/// (Darwin truncates trailing zero bytes: sa_len = 4 + n).
pub fn sa_mask(significant: &[u8]) -> Vec<u8> {
    let mut v = vec![(4 + significant.len()) as u8, 0, 0, 0];
    v.extend_from_slice(significant);
    v
}

/// One rt_msghdr + sockaddr area. `addrs` maps RTAX slot → sockaddr
/// bytes (already at natural length; padded to roundup here).
pub fn rt_msg(index: u16, flags: i32, addrs: &[(usize, Vec<u8>)]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    let mut addrs_bits: i32 = 0;
    let mut slots: Vec<Option<&Vec<u8>>> = vec![None; 8];
    for (slot, bytes) in addrs {
        addrs_bits |= 1 << slot;
        slots[*slot] = Some(bytes);
    }
    for slot in slots.iter().flatten() {
        let mut b = (*slot).clone();
        let target = round_up(b.len());
        b.resize(target, 0);
        body.extend_from_slice(&b);
    }
    let msglen = (RT_MSGHDR + body.len()) as u16;
    let mut hdr = vec![0u8; RT_MSGHDR];
    hdr[0..2].copy_from_slice(&msglen.to_le_bytes());
    hdr[2] = 5; // RTM_VERSION
    hdr[3] = 1; // type (ignored by the parser)
    hdr[4..6].copy_from_slice(&index.to_le_bytes());
    hdr[8..12].copy_from_slice(&flags.to_le_bytes());
    hdr[12..16].copy_from_slice(&addrs_bits.to_le_bytes());
    hdr.extend_from_slice(&body);
    hdr
}

pub const RTAX_DST: usize = 0;
pub const RTAX_GATEWAY: usize = 1;
pub const RTAX_NETMASK: usize = 2;
pub const RTAX_GENMASK: usize = 3;
pub const RTAX_IFA: usize = 5;

pub const RTF_HOST: i32 = 0x4;
pub const RTF_GATEWAY: i32 = 0x2;

// ---------------------------------------------------------------------------
// IfaRecord fixtures
// ---------------------------------------------------------------------------

pub const IFF_UP: i32 = 0x1;
pub const IFF_LOOPBACK: i32 = 0x8;
pub const IFF_P2P: i32 = 0x10;
pub const IFF_RUNNING: i32 = 0x40;

pub fn v4_rec(name: &str, flags: i32, ip: &str, mask: &str) -> IfaRecord {
    IfaRecord {
        name: name.to_string(),
        flags,
        family: libc::AF_INET,
        addr_v4: Some((mm_net::ipv4_raw(ip).unwrap(), ip.to_string())),
        mask_v4: Some((mm_net::ipv4_raw(mask).unwrap(), mask.to_string())),
        addr_v6: None,
        dst_v4: None,
        dst_v6: None,
    }
}

pub fn v4_p2p_rec(name: &str, ip: &str, mask: &str, dst: &str) -> IfaRecord {
    let mut rec = v4_rec(name, IFF_UP | IFF_P2P | IFF_RUNNING, ip, mask);
    rec.dst_v4 = Some((mm_net::ipv4_raw(dst).unwrap(), dst.to_string()));
    rec
}

pub fn v6_rec(name: &str, flags: i32, ip6: &str) -> IfaRecord {
    IfaRecord {
        name: name.to_string(),
        flags,
        family: libc::AF_INET6,
        addr_v4: None,
        mask_v4: None,
        addr_v6: Some(ip6.to_string()),
        dst_v4: None,
        dst_v6: None,
    }
}

/// Builds a ScanSnapshot from records + a route blob, with a
/// name→index map as the index resolver and a no-op recapture.
pub fn build(
    records: &[IfaRecord],
    blob: &[u8],
    indices: &[(&str, u16)],
) -> ScanSnapshot {
    let map: HashMap<String, u16> = indices
        .iter()
        .map(|(n, i)| (n.to_string(), *i))
        .collect();
    let routes = RouteTable::new(mm_net::parse_route_dump(blob));
    mm_net::build_net_infos(records, routes, &move |name| {
        map.get(name).copied().unwrap_or(0)
    }, &mut || RouteTable::new(vec![]))
}

/// The route blob for a WireGuard-style utun at index `idx`:
/// subnet route 10.7.0.0/24, default via gateway 10.7.0.1, host
/// route to 10.7.0.1 — the shape behind the 2026-11-18 settings
/// page (derived peer shown as 10.7.0.0/24).
pub fn wireguard_blob(idx: u16) -> Vec<u8> {
    let mut blob = Vec::new();
    // subnet route: dst 10.7.0.0, netmask /24 (3 significant bytes), ifa 10.7.0.10
    blob.extend(rt_msg(
        idx,
        0,
        &[
            (RTAX_DST, sa_v4([10, 7, 0, 0])),
            (RTAX_NETMASK, sa_mask(&[255, 255, 255])),
            (RTAX_IFA, sa_v4([10, 7, 0, 10])),
        ],
    ));
    // default route: dst 0.0.0.0, netmask empty, gateway 10.7.0.1
    blob.extend(rt_msg(
        idx,
        RTF_GATEWAY,
        &[
            (RTAX_DST, sa_v4([0, 0, 0, 0])),
            (RTAX_GATEWAY, sa_v4([10, 7, 0, 1])),
            (RTAX_NETMASK, sa_mask(&[])),
            (RTAX_IFA, sa_v4([10, 7, 0, 10])),
        ],
    ));
    // host route: dst 10.7.0.1, RTF_HOST (no netmask slot)
    blob.extend(rt_msg(
        idx,
        RTF_HOST,
        &[(RTAX_DST, sa_v4([10, 7, 0, 1])), (RTAX_IFA, sa_v4([10, 7, 0, 10]))],
    ));
    blob
}

/// Finds an interface by name in a snapshot.
pub fn iface<'a>(snap: &'a ScanSnapshot, name: &str) -> &'a NetInfo {
    snap.interfaces
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("interface {name} not in snapshot"))
}

/// The host's primary non-loopback IPv4 (for listener-based probe
/// tests), or None when unavailable.
pub fn host_ipv4() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    match sock.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(v4) if !v4.is_loopback() => Some(v4.to_string()),
        _ => None,
    }
}
