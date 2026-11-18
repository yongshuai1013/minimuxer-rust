//! Satisfaction predicates + activeInterfaces mapping, table
//! tests over fixture snapshots (INVENTORY 2.4 branches).

mod common;

use common::*;
use mm_net::LocalInterfaceType;

fn snap_with_default() -> mm_net::ScanSnapshot {
    let records = vec![
        v4_rec("lo0", IFF_UP | IFF_LOOPBACK, "127.0.0.1", "255.0.0.0"),
        v4_rec("en0", IFF_UP | IFF_RUNNING, "192.168.1.5", "255.255.255.0"),
        v4_rec("utun0", IFF_UP | IFF_RUNNING, "10.7.0.10", "255.255.255.0"),
    ];
    // Default route leaves via en0 (index 4) here.
    let mut blob = rt_msg(
        4,
        RTF_GATEWAY,
        &[
            (RTAX_DST, sa_v4([0, 0, 0, 0])),
            (RTAX_GATEWAY, sa_v4([192, 168, 1, 1])),
            (RTAX_NETMASK, sa_mask(&[])),
            (RTAX_IFA, sa_v4([192, 168, 1, 5])),
        ],
    );
    blob.extend(wireguard_blob(9));
    build(&records, &blob, &[("lo0", 1), ("en0", 4), ("utun0", 9)])
}

#[test]
fn wifi_satisfied_needs_en0_and_default_route() {
    let snap = snap_with_default();
    assert!(mm_net::is_wifi_satisfied(&snap));

    // en0 present but no routes at all → not satisfied.
    let records = vec![v4_rec("en0", IFF_UP, "192.168.1.5", "255.255.255.0")];
    let snap = build(&records, &[], &[("en0", 4)]);
    assert!(!mm_net::is_wifi_satisfied(&snap));

    // Default route but no en0 → not satisfied.
    let records = vec![v4_rec("utun0", IFF_UP, "10.7.0.10", "255.255.255.0")];
    let snap = build(&records, &wireguard_blob(9), &[("utun0", 9)]);
    assert!(!mm_net::is_wifi_satisfied(&snap));
}

#[test]
fn wired_satisfied_follows_default_route_interface() {
    // Default via en0 (Wi-Fi) → not wired.
    let snap = snap_with_default();
    assert!(!mm_net::is_wired_satisfied(&snap));

    // Default via en1 (Ethernet class) → wired.
    let records = vec![
        v4_rec("en0", IFF_UP, "192.168.1.5", "255.255.255.0"),
        v4_rec("en1", IFF_UP, "10.0.0.5", "255.255.255.0"),
    ];
    let blob = rt_msg(
        5,
        RTF_GATEWAY,
        &[
            (RTAX_DST, sa_v4([0, 0, 0, 0])),
            (RTAX_GATEWAY, sa_v4([10, 0, 0, 1])),
            (RTAX_NETMASK, sa_mask(&[])),
            (RTAX_IFA, sa_v4([10, 0, 0, 5])),
        ],
    );
    let snap = build(&records, &blob, &[("en0", 4), ("en1", 5)]);
    assert!(mm_net::is_wired_satisfied(&snap));
}

#[test]
fn usb_satisfied_is_link_local_en_not_en0() {
    let records = vec![
        v4_rec("en0", IFF_UP, "169.254.1.1", "255.255.0.0"),
        v4_rec("en1", IFF_UP, "192.168.1.9", "255.255.255.0"),
    ];
    let snap = build(&records, &[], &[("en0", 4), ("en1", 5)]);
    assert!(!mm_net::is_usb_satisfied(&snap.interfaces));

    let records = vec![
        v4_rec("en0", IFF_UP, "192.168.1.5", "255.255.255.0"),
        v4_rec("en2", IFF_UP, "169.254.9.9", "255.255.0.0"),
    ];
    let snap = build(&records, &[], &[("en0", 4), ("en2", 6)]);
    assert!(mm_net::is_usb_satisfied(&snap.interfaces));
}

#[test]
fn bridge_satisfied_matches_bridge_or_ap_substring() {
    let records = vec![v4_rec("bridge100", IFF_UP, "192.168.2.1", "255.255.255.0")];
    let snap = build(&records, &[], &[("bridge100", 12)]);
    assert!(mm_net::is_bridge_satisfied(&snap.interfaces));

    let records = vec![v4_rec("ap1", IFF_UP, "192.168.2.1", "255.255.255.0")];
    let snap = build(&records, &[], &[("ap1", 13)]);
    assert!(mm_net::is_bridge_satisfied(&snap.interfaces));

    let records = vec![v4_rec("en0", IFF_UP, "192.168.1.5", "255.255.255.0")];
    let snap = build(&records, &[], &[("en0", 4)]);
    assert!(!mm_net::is_bridge_satisfied(&snap.interfaces));
}

#[test]
fn tunnel_availability_prefixes() {
    let snap = snap_with_default();
    assert!(mm_net::is_utun_available(&snap.interfaces));
    assert!(!mm_net::is_ikev2_ipsec_available(&snap.interfaces));

    let records = vec![v4_rec("ipsec0", IFF_UP, "10.9.0.2", "255.255.255.0")];
    let snap = build(&records, &[], &[("ipsec0", 14)]);
    assert!(mm_net::is_ikev2_ipsec_available(&snap.interfaces));
    assert!(!mm_net::is_utun_available(&snap.interfaces));
}

#[test]
fn active_interfaces_mapping_and_sort() {
    let records = vec![
        v4_rec("lo0", IFF_UP | IFF_LOOPBACK, "127.0.0.1", "255.0.0.0"),
        v4_rec("utun0", IFF_UP, "10.7.0.10", "255.255.255.0"),
        v4_rec("en0", IFF_UP, "192.168.1.5", "255.255.255.0"),
        v4_rec("pdp_ip0", IFF_UP, "10.20.0.2", "255.255.255.255"),
        v6_rec("en0", IFF_UP, "fe80::abcd"),
    ];
    let snap = build(
        &records,
        &[],
        &[("lo0", 1), ("utun0", 9), ("en0", 4), ("pdp_ip0", 3)],
    );
    let infos = mm_net::active_interfaces(&snap.interfaces);
    let names: Vec<&str> = infos.iter().map(|i| i.name.as_str()).collect();
    // Priority: wifi(en0) < vpnUtun < cellular < loopback.
    assert_eq!(names, vec!["en0", "utun0", "pdp_ip0", "lo0"]);

    let en0 = &infos[0];
    assert_eq!(en0.ip, "192.168.1.5");
    assert_eq!(en0.ipv6.as_deref(), Some("fe80::abcd"));
    assert_eq!(en0.subnet, "255.255.255.0");
    assert_eq!(en0.interface_type, LocalInterfaceType::Wifi);

    let cell = &infos[2];
    assert_eq!(cell.interface_type, LocalInterfaceType::Cellular);
    assert_eq!(cell.ipv6, None);
}

#[test]
fn active_interfaces_ip_falls_back_to_v6() {
    let records = vec![v6_rec("en0", IFF_UP, "fe80::1234")];
    let snap = build(&records, &[], &[("en0", 4)]);
    let infos = mm_net::active_interfaces(&snap.interfaces);
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].ip, "fe80::1234");
    assert_eq!(infos[0].subnet, "");
}
