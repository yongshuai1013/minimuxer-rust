//! Port of Swift `MinimuxerConstants` (MinimuxerCommon) — every constant,
//! value-for-value. Source: yongshuai1013/minimuxer @714aafa3,
//! `Common/MinimuxerConstants.swift`.

/// lockdown daemon port
pub const LOCKDOWND_PORT: u16 = 62078;
/// RemotePairing daemon port
pub const REMOTE_PAIRING_PORT: u16 = 49152;

pub const APP_NAME: &str = "minimuxer";

pub const USBMUXD_HOST: &str = "127.0.0.1";
/// usbmux daemon port
pub const USBMUXD_PORT: u16 = 27015;
pub const USBMUXD_SOCKET: &str = "127.0.0.1:27015";

pub const EMP_SERVER_HOST: &str = "127.0.0.1";
/// default EMProxy WireGuard loopback port
pub const EMP_SERVER_PORT: u16 = 51820;

pub const VPN_HANDSHAKE_TIMEOUT_NS: u64 = 10_000_000_000;

pub const DEVICE_FETCH_TIMEOUT_MS: u16 = 5000;
/// Swift declares this as a mutable `static var`; it is the *default* value
/// (runtime overrides flow through MinimuxerParams, not by mutating this).
pub const DEFAULT_TCP_PROBE_TIMEOUT_MS: i32 = 250;

pub const PKG_PATH: &str = "PublicStaging";
pub const USBMUXD_ENV_KEY: &str = "USBMUXD_SOCKET_ADDRESS";

pub const PRE17_VERSIONS_URL: &str =
    "https://raw.githubusercontent.com/jkcoxson/JitStreamer/master/versions.json";
pub const DDI_IMAGE_URL: &str = "https://raw.githubusercontent.com/doronz88/DeveloperDiskImage/refs/heads/main/PersonalizedImages/Xcode_iOS_DDI_Personalized/Image.dmg";
pub const DDI_TRUSTCACHE_URL: &str = "https://raw.githubusercontent.com/doronz88/DeveloperDiskImage/refs/heads/main/PersonalizedImages/Xcode_iOS_DDI_Personalized/Image.dmg.trustcache";
pub const DDI_MANIFEST_URL: &str = "https://raw.githubusercontent.com/doronz88/DeveloperDiskImage/refs/heads/main/PersonalizedImages/Xcode_iOS_DDI_Personalized/BuildManifest.plist";

// WirelessPair constants
pub const DEFAULT_HOST_NAME: &str = "SideStore";
pub const DEFAULT_HOST_MODEL: &str = "Mac17,7";
pub const DEFAULT_BIND_PORT: u16 = 0;
pub const DEFAULT_AD_DOMAIN: &str = "";
pub const REMOTE_PAIRING_DAEMON_SERVICE_TYPE: &str = "_remotepairing._tcp.";
pub const REMOTE_PAIRING_PAIRABLE_HOST_SERVICE_TYPE: &str =
    "_remotepairing-pairable-host._tcp.";
pub const REMOTE_PAIRING_MANUAL_PAIRING_SERVICE_TYPE: &str =
    "_remotepairing-manual-pairing._tcp.";
pub const DEFAULT_RP_PAIRING_FILE_NAME: &str = "rp_pairing_file.plist";
pub const RP_PAIRING_FILE_SUFFIX: &str = "_rp.plist";

// UsbmuxdProxyServer constants
pub const DEVICE_ATTACH: &str = "Attached";
pub const DEVICE_DETACH: &str = "Detached";
pub const USBMUX_MAX_PACKET_BUFFER_LENGTH: usize = 4095;
pub const USBMUX_HEADER_LEN: usize = 16;

// Sleep timeouts
pub const HEARTBEAT_INTERVAL: u64 = 1000;

// Heartbeat pacing (seconds). The interval handed to the heartbeat service
// is the timeout in seconds to wait for the device's next Marco; the
// device-returned interval is adopted clamped to [MIN, MAX].
pub const HEARTBEAT_INITIAL_INTERVAL_SECONDS: u64 = 5;
pub const HEARTBEAT_MIN_INTERVAL_SECONDS: u64 = 5;
pub const HEARTBEAT_MAX_INTERVAL_SECONDS: u64 = 60;

// Packet configuration
pub const MAX_PACKET_SIZE: usize = 1024;
pub const PACKET_HEADER_LEN: usize = 16;
