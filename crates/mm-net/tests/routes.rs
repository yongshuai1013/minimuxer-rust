//! Route-dump parser fixtures: byte-level rt_msghdr blobs built
//! per the Darwin layout the Swift scanner parses.

mod common;

use common::*;
use mm_net::{parse_route_dump, RouteTable};

#[test]
fn subnet_route_with_truncated_netmask() {
    let blob = rt_msg(
        7,
        0,
        &[
            (RTAX_DST, sa_v4([10, 7, 0, 0])),
            (RTAX_NETMASK, sa_mask(&[255, 255, 255])),
            (RTAX_IFA, sa_v4([10, 7, 0, 10])),
        ],
    );
    let entries = parse_route_dump(&blob);
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.destination_ipv4.as_deref(), Some("10.7.0.0"));
    assert_eq!(e.destination_ipv4_raw, Some(0x0A07_0000));
    assert_eq!(e.destination_ipv4_mask.as_deref(), Some("255.255.255.0"));
    assert_eq!(e.destination_ipv4_mask_raw, Some(0xFFFF_FF00));
    assert_eq!(e.interface_ipv4.as_deref(), Some("10.7.0.10"));
    assert_eq!(e.interface_index, 7);
    assert_eq!(e.gateway_ipv4, None);
}

#[test]
fn default_route_and_gateway() {
    let blob = wireguard_blob(9);
    let entries = parse_route_dump(&blob);
    assert_eq!(entries.len(), 3);
    let table = RouteTable::new(entries);
    let def = table.default_route(libc::AF_INET).expect("default route");
    assert_eq!(def.destination_ipv4.as_deref(), Some("0.0.0.0"));
    assert_eq!(def.destination_ipv4_mask_raw, Some(0));
    assert_eq!(def.gateway_ipv4.as_deref(), Some("10.7.0.1"));
    assert_eq!(def.gateway_ipv4_raw, Some(0x0A07_0001));
    assert_eq!(table.routes_for_index(9).len(), 3);
    assert_eq!(table.routes_for_index(3).len(), 0);
    assert!(table.default_route(libc::AF_INET6).is_none());
}

#[test]
fn host_route_synthesizes_slash32_mask() {
    let blob = rt_msg(
        9,
        RTF_HOST,
        &[
            (RTAX_DST, sa_v4([10, 7, 0, 1])),
            (RTAX_IFA, sa_v4([10, 7, 0, 10])),
        ],
    );
    let entries = parse_route_dump(&blob);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].destination_ipv4_mask_raw, Some(0xFFFF_FFFF));
    assert_eq!(
        entries[0].destination_ipv4_mask.as_deref(),
        Some("255.255.255.255")
    );
}

#[test]
fn one_byte_netmask_zero_fills() {
    // sa_len = 5: only the first mask byte is present.
    let blob = rt_msg(
        4,
        0,
        &[
            (RTAX_DST, sa_v4([10, 0, 0, 0])),
            (RTAX_NETMASK, sa_mask(&[255])),
            (RTAX_IFA, sa_v4([10, 0, 0, 2])),
        ],
    );
    let entries = parse_route_dump(&blob);
    assert_eq!(entries[0].destination_ipv4_mask_raw, Some(0xFF00_0000));
    assert_eq!(entries[0].destination_ipv4_mask.as_deref(), Some("255.0.0.0"));
}

#[test]
fn genmask_lands_in_interface_mask_fields() {
    let blob = rt_msg(
        4,
        0,
        &[
            (RTAX_DST, sa_v4([10, 7, 0, 0])),
            (RTAX_GENMASK, sa_mask(&[255, 255, 0])),
            (RTAX_IFA, sa_v4([10, 7, 0, 10])),
        ],
    );
    let entries = parse_route_dump(&blob);
    assert_eq!(entries[0].interface_ipv4_mask_raw, Some(0xFFFF_0000));
    assert_eq!(entries[0].destination_ipv4_mask_raw, None);
}

#[test]
fn ipv6_destination_route() {
    let mut addr = [0u8; 16];
    addr[0] = 0xfe;
    addr[1] = 0x80;
    let blob = rt_msg(
        5,
        0,
        &[(RTAX_DST, sa_v6(addr)), (RTAX_IFA, sa_v6(addr))],
    );
    let entries = parse_route_dump(&blob);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].destination_ipv6.as_deref(), Some("fe80::"));
    assert_eq!(entries[0].interface_ipv6.as_deref(), Some("fe80::"));
    let table = RouteTable::new(entries);
    assert!(table.default_route(libc::AF_INET6).is_none());
}

#[test]
fn zero_msglen_terminates_dump() {
    let mut blob = wireguard_blob(9);
    blob.extend(vec![0u8; RT_MSGHDR]); // msglen == 0 header
    blob.extend(rt_msg(9, 0, &[(RTAX_DST, sa_v4([1, 2, 3, 0]))]));
    let entries = parse_route_dump(&blob);
    assert_eq!(entries.len(), 3, "entries after a zero msglen are dropped");
}

#[test]
fn truncated_tail_is_ignored() {
    let mut blob = wireguard_blob(9);
    blob.truncate(blob.len() - 10); // cut into the last message body
    // The first two messages remain whole; the third header is
    // incomplete or its body is short — parser must not panic and
    // must keep the whole entries.
    let entries = parse_route_dump(&blob);
    assert!(entries.len() >= 2);
}

#[test]
fn empty_blob_yields_no_entries() {
    assert!(parse_route_dump(&[]).is_empty());
    assert!(parse_route_dump(&[0u8; 10]).is_empty());
}
