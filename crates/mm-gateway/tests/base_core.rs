//! GatewayCore (Swift `BaseDeviceGateway`) state semantics.

use mm_gateway::GatewayCore;
use mm_common::PairingProtocol;

fn fixture_pairing_bytes() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/pairing.plist"
    ))
    .unwrap()
}

#[test]
fn default_ports_match_swift() {
    let core = GatewayCore::new();
    assert_eq!(core.get_port(PairingProtocol::Lockdown), 62078);
    assert_eq!(core.get_port(PairingProtocol::RpPairing), 49152);
    // .unknown on a pre-iOS-17 system resolves to the lockdown port.
    assert_eq!(core.get_port(PairingProtocol::Unknown), 62078);
}

#[test]
fn set_port_invalidates_only_on_change() {
    let core = GatewayCore::new();
    let g0 = core.generation();
    assert!(core.set_port(49152, PairingProtocol::RpPairing));
    assert!(core.generation() > g0);
    let g1 = core.generation();
    // Same value again: Swift's guard makes it a no-op.
    assert!(!core.set_port(49152, PairingProtocol::RpPairing));
    assert_eq!(core.generation(), g1);
    assert_eq!(core.get_port(PairingProtocol::RpPairing), 49152);
}

#[test]
fn set_endpoint_ip_invalidates_only_on_change() {
    let core = GatewayCore::new();
    assert!(core.set_device_endpoint_ip(Some("10.7.0.1")));
    let g = core.generation();
    assert!(!core.set_device_endpoint_ip(Some("10.7.0.1")));
    assert_eq!(core.generation(), g);
    assert!(core.set_device_endpoint_ip(Some("10.7.0.2")));
    assert!(core.set_device_endpoint_ip(None));
    assert_eq!(core.device_endpoint_ip(), None);
}

#[test]
fn pairing_data_derives_dict_like_swift_didset() {
    let core = GatewayCore::new();
    core.set_pairing_file_data(Some(fixture_pairing_bytes()));
    let dict = core.pairing_data_dict().expect("dict derived");
    let buid = dict
        .get("SystemBUID")
        .and_then(|v| v.as_string())
        .unwrap_or("");
    assert_eq!(buid, "12345678-1234-1234-1234-1234567890AB");

    // Garbage data parses to no dict (Swift `try?` -> nil).
    core.set_pairing_file_data(Some(b"not a plist".to_vec()));
    assert!(core.pairing_data_dict().is_none());

    core.set_pairing_file_data(None);
    assert!(core.pairing_file_data().is_none());
    assert!(core.pairing_data_dict().is_none());
}

#[test]
fn cleanup_clears_state_but_keeps_ports() {
    let core = GatewayCore::new();
    core.set_port(50000, PairingProtocol::RpPairing);
    core.set_pairing_file_data(Some(fixture_pairing_bytes()));
    core.set_pairing_file_type(PairingProtocol::Lockdown);
    core.set_initialized(true);
    let g = core.generation();

    core.cleanup();

    assert!(!core.is_initialized());
    assert_eq!(core.pairing_file_type(), PairingProtocol::Unknown);
    assert!(core.pairing_file_data().is_none());
    assert!(core.generation() > g, "cleanup invalidates");
    // Swift cleanup() does not clear the port table.
    assert_eq!(core.get_port(PairingProtocol::RpPairing), 50000);
}
