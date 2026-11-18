//! Scan-builder fixtures: IfaRecord sets + route blobs → NetInfo
//! collections, mirroring NetworkIfaceScanner.scan semantics.

mod common;

use common::*;
use mm_net::{LinkType, RouteTable, TunnelType};
use std::collections::HashMap;

const UTUN: u16 = 9;
const EN0: u16 = 4;
const LO0: u16 = 1;

fn base_records() -> Vec<mm_net::IfaRecord> {
    vec![
        v4_rec("lo0", IFF_UP | IFF_LOOPBACK | IFF_RUNNING, "127.0.0.1", "255.0.0.0"),
        v6_rec("lo0", IFF_UP | IFF_LOOPBACK | IFF_RUNNING, "::1"),
        v4_rec("en0", IFF_UP | IFF_RUNNING, "192.168.1.5", "255.255.255.0"),
        v4_rec("utun0", IFF_UP | IFF_RUNNING, "10.7.0.10", "255.255.255.0"),
    ]
}

#[test]
fn wireguard_shape_builds_tunnel_with_subnet_route() {
    // The 2026-11-18 drift shape: utun with a /24 subnet route to
    // 10.7.0.0 and a default route via 10.7.0.1.
    let snap = build(
        &base_records(),
        &wireguard_blob(UTUN),
        &[("lo0", LO0), ("en0", EN0), ("utun0", UTUN)],
    );
    assert_eq!(snap.interfaces.len(), 3);

    let tun = iface(&snap, "utun0");
    let data = tun.tunnel.as_ref().expect("utun0 is a tunnel");
    assert_eq!(data.tunnel_type, TunnelType::Utun);
    assert_eq!(data.destination_routes.len(), 3);
    // The subnet route is an interface subnet route...
    assert_eq!(tun.interface_subnet_routes.len(), 1);
    assert_eq!(
        tun.interface_subnet_routes[0].destination_ipv4.as_deref(),
        Some("10.7.0.0")
    );
    assert_eq!(tun.link_type(), LinkType::Subnet);
    // ...and shows up in destinationIPs with its /24 mask preserved
    // (this is the value the settings page later displayed).
    let dest_ips = data.destination_ips();
    let subnet_ip = dest_ips
        .iter()
        .find(|ip| ip.v4.as_ref().map(|v| v.host.as_str()) == Some("10.7.0.0"))
        .expect("10.7.0.0 among destination IPs");
    assert_eq!(subnet_ip.v4.as_ref().unwrap().mask, "255.255.255.0");
    // Gateway of the default route is a destination gateway IP.
    let gw_ips = data.destination_gateway_ips();
    assert!(gw_ips
        .iter()
        .any(|ip| ip.v4.as_ref().map(|v| v.host.as_str()) == Some("10.7.0.1")));
}

#[test]
fn point_to_point_tunnel_gets_link_layer_destination() {
    let records = vec![v4_p2p_rec("utun1", "10.8.0.2", "255.255.255.0", "10.8.0.1")];
    let snap = build(&records, &[], &[("utun1", 11)]);
    let tun = iface(&snap, "utun1");
    let data = tun.tunnel.as_ref().unwrap();
    let dst = data.link_layer_destination_ip.as_ref().unwrap();
    assert_eq!(dst.v4.as_ref().unwrap().host, "10.8.0.1");
    // Link-layer destination inherits the interface mask (Swift).
    assert_eq!(dst.v4.as_ref().unwrap().mask, "255.255.255.0");
    assert_eq!(tun.link_type(), LinkType::PointToPoint);
}

#[test]
fn down_and_addressless_and_indexless_interfaces_are_skipped() {
    let mut records = base_records();
    // en1 is DOWN.
    records.push(v4_rec("en1", IFF_RUNNING, "192.168.1.6", "255.255.255.0"));
    // en2 is UP but has no address record (family 0 entry only).
    records.push(mm_net::IfaRecord {
        name: "en2".to_string(),
        flags: IFF_UP,
        family: 0,
        addr_v4: None,
        mask_v4: None,
        addr_v6: None,
        dst_v4: None,
        dst_v6: None,
    });
    // en3 is UP with an address but resolves to index 0.
    records.push(v4_rec("en3", IFF_UP, "192.168.1.7", "255.255.255.0"));
    let snap = build(
        &records,
        &wireguard_blob(UTUN),
        &[("lo0", LO0), ("en0", EN0), ("utun0", UTUN), ("en1", 5), ("en2", 6), ("en3", 0)],
    );
    let names: Vec<&str> = snap.interfaces.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, vec!["lo0", "en0", "utun0"]);
}

#[test]
fn v4_entry_without_netmask_is_dropped_but_v6_kept() {
    let mut rec = v4_rec("en0", IFF_UP, "192.168.1.5", "255.255.255.0");
    rec.mask_v4 = None;
    let records = vec![rec, v6_rec("en0", IFF_UP, "fe80::1")];
    let snap = build(&records, &[], &[("en0", EN0)]);
    let en0 = iface(&snap, "en0");
    assert!(en0.interface_addresses.v4.is_empty());
    assert_eq!(en0.interface_addresses.v6, vec!["fe80::1".to_string()]);
}

#[test]
fn case_y_recaptures_routes_for_routeless_tunnel() {
    let records = vec![v4_rec("utun0", IFF_UP | IFF_RUNNING, "10.7.0.10", "255.255.255.0")];
    let indices: HashMap<String, u16> = [("utun0".to_string(), UTUN)].into_iter().collect();
    let mut recaptures = 0;
    let snap = mm_net::build_net_infos(
        &records,
        RouteTable::new(vec![]),
        &move |name| indices.get(name).copied().unwrap_or(0),
        &mut || {
            recaptures += 1;
            RouteTable::new(mm_net::parse_route_dump(&wireguard_blob(UTUN)))
        },
    );
    assert_eq!(recaptures, 1);
    let tun = iface(&snap, "utun0");
    assert_eq!(tun.tunnel.as_ref().unwrap().destination_routes.len(), 3);
}

#[test]
fn tunnel_and_plain_info_with_same_fields_are_not_equal() {
    // Constructed directly so every compared field lines up; only
    // the tunnel payload differs (Swift == requires type(of:)
    // equality, so a TunnelNetInfo never equals a plain NetInfo).
    let base = mm_net::NetInfo {
        name: "utun0".to_string(),
        interface_index: 9,
        interface_addresses: mm_net::IPAddresses {
            v4: vec![mm_net::IPv4Info::new(
                "10.7.0.10".to_string(),
                "255.255.255.0".to_string(),
                0x0A07_000A,
                0xFFFF_FF00,
            )],
            v6: vec![],
        },
        interface_subnet_routes: vec![],
        flags: IFF_UP | IFF_RUNNING,
        tunnel: None,
    };
    let mut tun = base.clone();
    tun.tunnel = Some(mm_net::TunnelData {
        tunnel_type: TunnelType::Utun,
        link_layer_destination_ip: None,
        destination_routes: vec![],
    });
    assert!(!tun.is_equal(&base));
    assert!(!base.is_equal(&tun));
    assert!(tun.is_equal(&tun.clone()));
    assert!(base.is_equal(&base.clone()));
}

#[test]
fn identical_scans_compare_equal_as_sets() {
    let a = build(
        &base_records(),
        &wireguard_blob(UTUN),
        &[("lo0", LO0), ("en0", EN0), ("utun0", UTUN)],
    );
    let b = build(
        &base_records(),
        &wireguard_blob(UTUN),
        &[("lo0", LO0), ("en0", EN0), ("utun0", UTUN)],
    );
    assert!(mm_net::net_info_sets_equal(&a.interfaces, &b.interfaces));
    // Removing one interface breaks set equality.
    let mut fewer = b.interfaces.clone();
    fewer.pop();
    assert!(!mm_net::net_info_sets_equal(&a.interfaces, &fewer));
}

#[test]
fn live_scan_smoke() {
    // The live gatherer must not panic on the test host and must
    // find at least a loopback interface.
    let snap = mm_net::scan_snapshot();
    assert!(snap.interfaces.iter().any(|i| i.is_loopback()));
}
