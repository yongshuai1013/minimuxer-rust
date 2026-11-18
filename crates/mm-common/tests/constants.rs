//! Constant-for-constant check against Swift MinimuxerConstants.

use mm_common::constants::*;
use mm_common::pairing::PairingProtocol;

#[test]
fn ports_and_hosts() {
    assert_eq!(LOCKDOWND_PORT, 62078);
    assert_eq!(REMOTE_PAIRING_PORT, 49152);
    assert_eq!(USBMUXD_HOST, "127.0.0.1");
    assert_eq!(USBMUXD_PORT, 27015);
    assert_eq!(
        USBMUXD_SOCKET,
        format!("{USBMUXD_HOST}:{USBMUXD_PORT}").as_str()
    );
    assert_eq!(EMP_SERVER_HOST, "127.0.0.1");
    assert_eq!(EMP_SERVER_PORT, 51820);
    assert_eq!(USBMUXD_ENV_KEY, "USBMUXD_SOCKET_ADDRESS");
}

#[test]
fn timeouts_and_pacing() {
    assert_eq!(VPN_HANDSHAKE_TIMEOUT_NS, 10_000_000_000);
    assert_eq!(DEVICE_FETCH_TIMEOUT_MS, 5000);
    assert_eq!(DEFAULT_TCP_PROBE_TIMEOUT_MS, 250);
    assert_eq!(HEARTBEAT_INTERVAL, 1000);
    assert_eq!(HEARTBEAT_INITIAL_INTERVAL_SECONDS, 5);
    assert_eq!(HEARTBEAT_MIN_INTERVAL_SECONDS, 5);
    assert_eq!(HEARTBEAT_MAX_INTERVAL_SECONDS, 60);
}

#[test]
fn names_urls_and_service_types() {
    assert_eq!(APP_NAME, "minimuxer");
    assert_eq!(PKG_PATH, "PublicStaging");
    assert_eq!(DEFAULT_HOST_NAME, "SideStore");
    assert_eq!(DEFAULT_HOST_MODEL, "Mac17,7");
    assert_eq!(DEFAULT_BIND_PORT, 0);
    assert_eq!(DEFAULT_AD_DOMAIN, "");
    assert_eq!(REMOTE_PAIRING_DAEMON_SERVICE_TYPE, "_remotepairing._tcp.");
    assert_eq!(
        REMOTE_PAIRING_PAIRABLE_HOST_SERVICE_TYPE,
        "_remotepairing-pairable-host._tcp."
    );
    assert_eq!(
        REMOTE_PAIRING_MANUAL_PAIRING_SERVICE_TYPE,
        "_remotepairing-manual-pairing._tcp."
    );
    assert_eq!(DEFAULT_RP_PAIRING_FILE_NAME, "rp_pairing_file.plist");
    assert_eq!(RP_PAIRING_FILE_SUFFIX, "_rp.plist");
    assert!(PRE17_VERSIONS_URL.ends_with("JitStreamer/master/versions.json"));
    assert!(DDI_IMAGE_URL.ends_with("Image.dmg"));
    assert!(DDI_TRUSTCACHE_URL.ends_with("Image.dmg.trustcache"));
    assert!(DDI_MANIFEST_URL.ends_with("BuildManifest.plist"));
    assert_eq!(DEVICE_ATTACH, "Attached");
    assert_eq!(DEVICE_DETACH, "Detached");
    assert_eq!(USBMUX_MAX_PACKET_BUFFER_LENGTH, 4095);
    assert_eq!(USBMUX_HEADER_LEN, 16);
    assert_eq!(MAX_PACKET_SIZE, 1024);
    assert_eq!(PACKET_HEADER_LEN, 16);
}

#[test]
fn pairing_protocol_raw_values_and_default_ports() {
    assert_eq!(PairingProtocol::RpPairing.raw_value(), "rppairing");
    assert_eq!(PairingProtocol::Lockdown.raw_value(), "lockdown");
    assert_eq!(PairingProtocol::Unknown.raw_value(), "unknown");
    assert_eq!(
        PairingProtocol::from_raw("lockdown"),
        Some(PairingProtocol::Lockdown)
    );
    assert_eq!(PairingProtocol::from_raw("bogus"), None);
    assert_eq!(PairingProtocol::RpPairing.default_port(false), 49152);
    assert_eq!(PairingProtocol::Lockdown.default_port(true), 62078);
    assert_eq!(PairingProtocol::Unknown.default_port(true), 49152);
    assert_eq!(PairingProtocol::Unknown.default_port(false), 62078);
}
