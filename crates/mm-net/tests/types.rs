//! LocalInterfaceType classification / priority tables and other
//! value-type semantics from MinimuxerApi.swift.

use mm_net::{DeviceConnectionMode, LocalInterfaceInfo, LocalInterfaceType};
use LocalInterfaceType::*;

#[test]
fn classification_table() {
    let cases: &[(&str, bool, LocalInterfaceType)] = &[
        ("en0", false, Wifi),
        ("en0", true, Wifi), // exact en0 wins over the link-local rule
        ("en1", true, UsbLinkLocal),
        ("en1", false, Ethernet),
        ("en12", false, Ethernet),
        ("utun0", false, VpnUtun),
        ("utun", false, VpnUtun),
        ("ipsec0", false, VpnIpsec),
        ("pdp_ip0", false, Cellular),
        ("pdp0", false, Cellular),
        ("awdl0", false, Airdrop),
        ("llw0", false, LowLatencyWlan),
        ("bridge100", false, HotspotBridge),
        ("ap1", false, HotspotBridge),
        ("lo0", false, Loopback),
        ("pktap0", false, PacketCapture),
        ("gif0", false, Other),
        ("stf0", false, Other),
        ("anpi0", false, Other),
        // Case-insensitivity: names are lowercased first.
        ("UTUN3", false, VpnUtun),
        ("EN0", false, Wifi),
    ];
    for (name, link_local, want) in cases {
        assert_eq!(
            LocalInterfaceType::from_name(name, *link_local),
            *want,
            "classify {name} link_local={link_local}"
        );
    }
}

#[test]
fn priority_order_is_the_swift_table() {
    let mut shuffled = vec![
        Other,
        Loopback,
        Ethernet,
        Wifi,
        Cellular,
        PacketCapture,
        VpnIpsec,
        Airdrop,
        UsbLinkLocal,
        HotspotBridge,
        LowLatencyWlan,
        VpnUtun,
    ];
    shuffled.sort_by_key(|t| t.priority());
    assert_eq!(
        shuffled,
        vec![
            Wifi,
            VpnUtun,
            VpnIpsec,
            UsbLinkLocal,
            Cellular,
            HotspotBridge,
            Ethernet,
            Airdrop,
            LowLatencyWlan,
            Loopback,
            PacketCapture,
            Other,
        ]
    );
    // Priorities are a strict total order over all cases.
    let mut prios: Vec<usize> = LocalInterfaceType::all_cases()
        .iter()
        .map(|t| t.priority())
        .collect();
    prios.sort_unstable();
    prios.dedup();
    assert_eq!(prios.len(), 12);
}

#[test]
fn raw_values_and_symbols() {
    let table: &[(LocalInterfaceType, &str, &str, bool)] = &[
        (VpnUtun, "VPN (uTun)", "lock.shield", true),
        (VpnIpsec, "VPN (IPSec)", "lock.shield", true),
        (Wifi, "Wi-Fi", "wifi", false),
        (UsbLinkLocal, "USB / Link-Local", "cable.connector", false),
        (Ethernet, "Ethernet / Adapter", "cable.connector", false),
        (Cellular, "Cellular", "antenna.radiowaves.left.and.right", false),
        (Airdrop, "AirDrop (AWDL)", "airdrop", false),
        (LowLatencyWlan, "Low-Latency WLAN", "wifi", false),
        (HotspotBridge, "Personal Hotspot / Bridge", "personalhotspot", false),
        (Loopback, "Loopback", "arrow.triangle.2.circlepath", false),
        (PacketCapture, "Packet Capture", "waveform.path.ecg", false),
        (Other, "Other", "network", false),
    ];
    for (ty, raw, symbol, is_vpn) in table {
        assert_eq!(ty.raw_value(), *raw);
        assert_eq!(ty.symbol_name(), *symbol);
        assert_eq!(ty.is_vpn(), *is_vpn);
    }
}

#[test]
fn interface_info_id_format() {
    let info = LocalInterfaceInfo {
        name: "UTUN0".to_string(),
        ip: "10.7.0.10".to_string(),
        ipv6: None,
        subnet: "255.255.255.0".to_string(),
        interface_type: VpnUtun,
    };
    assert_eq!(info.id(), "utun0-10.7.0.10");
}

#[test]
fn connection_mode_raw_values() {
    assert_eq!(DeviceConnectionMode::LocalVpn.raw_value(), "localVPN");
    assert_eq!(DeviceConnectionMode::RemoteServer.raw_value(), "remoteServer");
    assert_eq!(DeviceConnectionMode::NotConfigured.raw_value(), "notConfigured");
    for mode in [
        DeviceConnectionMode::LocalVpn,
        DeviceConnectionMode::RemoteServer,
        DeviceConnectionMode::NotConfigured,
    ] {
        assert_eq!(DeviceConnectionMode::from_raw_value(mode.raw_value()), Some(mode));
    }
    assert_eq!(DeviceConnectionMode::from_raw_value("bogus"), None);
}
