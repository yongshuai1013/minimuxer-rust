//! Public value types of the network subsystems — port of the Swift
//! types in `Sources/MinimuxerApi.swift` (yongshuai1013/minimuxer
//! @714aafa3): `LocalInterfaceType`, `LocalInterfaceInfo`,
//! `DeviceConnectionMode`, `ServicePort`. Raw values and the
//! classification/priority tables replicate the Swift source verbatim
//! (design D5: Swift behaviour is the spec).

use mm_common::PairingProtocol;

/// Swift `DeviceConnectionMode` — raw values are the Swift case names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceConnectionMode {
    LocalVpn,
    RemoteServer,
    NotConfigured,
}

impl DeviceConnectionMode {
    pub fn raw_value(&self) -> &'static str {
        match self {
            DeviceConnectionMode::LocalVpn => "localVPN",
            DeviceConnectionMode::RemoteServer => "remoteServer",
            DeviceConnectionMode::NotConfigured => "notConfigured",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<Self> {
        match raw {
            "localVPN" => Some(DeviceConnectionMode::LocalVpn),
            "remoteServer" => Some(DeviceConnectionMode::RemoteServer),
            "notConfigured" => Some(DeviceConnectionMode::NotConfigured),
            _ => None,
        }
    }
}

/// Swift `ServicePort` — a (pairing protocol, port) pair handed to the
/// App-side `resolveServicePort` callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServicePort {
    pub protocol_type: PairingProtocol,
    pub port: u16,
}

impl ServicePort {
    pub fn new(protocol_type: PairingProtocol, port: u16) -> Self {
        Self {
            protocol_type,
            port,
        }
    }
}

/// Swift `LocalInterfaceType` — 12 cases; raw values are the Swift
/// `String` raw values verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalInterfaceType {
    VpnUtun,
    VpnIpsec,
    Wifi,
    UsbLinkLocal,
    Ethernet,
    Cellular,
    Airdrop,
    LowLatencyWlan,
    HotspotBridge,
    Loopback,
    PacketCapture,
    Other,
}

impl LocalInterfaceType {
    pub fn raw_value(&self) -> &'static str {
        match self {
            LocalInterfaceType::VpnUtun => "VPN (uTun)",
            LocalInterfaceType::VpnIpsec => "VPN (IPSec)",
            LocalInterfaceType::Wifi => "Wi-Fi",
            LocalInterfaceType::UsbLinkLocal => "USB / Link-Local",
            LocalInterfaceType::Ethernet => "Ethernet / Adapter",
            LocalInterfaceType::Cellular => "Cellular",
            LocalInterfaceType::Airdrop => "AirDrop (AWDL)",
            LocalInterfaceType::LowLatencyWlan => "Low-Latency WLAN",
            LocalInterfaceType::HotspotBridge => "Personal Hotspot / Bridge",
            LocalInterfaceType::Loopback => "Loopback",
            LocalInterfaceType::PacketCapture => "Packet Capture",
            LocalInterfaceType::Other => "Other",
        }
    }

    /// Swift `priorityOrder` — note this is *not* declaration order:
    /// wifi, vpnUtun, vpnIpsec, usbLinkLocal, cellular, hotspotBridge,
    /// ethernet, airdrop, lowLatencyWLAN, loopback, packetCapture,
    /// other.
    pub fn priority(&self) -> usize {
        use LocalInterfaceType::*;
        match self {
            Wifi => 0,
            VpnUtun => 1,
            VpnIpsec => 2,
            UsbLinkLocal => 3,
            Cellular => 4,
            HotspotBridge => 5,
            Ethernet => 6,
            Airdrop => 7,
            LowLatencyWlan => 8,
            Loopback => 9,
            PacketCapture => 10,
            Other => 11,
        }
    }

    /// Swift `isVPN`.
    pub fn is_vpn(&self) -> bool {
        matches!(
            self,
            LocalInterfaceType::VpnUtun | LocalInterfaceType::VpnIpsec
        )
    }

    /// Swift `symbolName`.
    pub fn symbol_name(&self) -> &'static str {
        use LocalInterfaceType::*;
        match self {
            VpnUtun | VpnIpsec => "lock.shield",
            Wifi | LowLatencyWlan => "wifi",
            UsbLinkLocal | Ethernet => "cable.connector",
            Cellular => "antenna.radiowaves.left.and.right",
            Loopback => "arrow.triangle.2.circlepath",
            Airdrop => "airdrop",
            HotspotBridge => "personalhotspot",
            PacketCapture => "waveform.path.ecg",
            Other => "network",
        }
    }

    /// Swift `init(name:isLinkLocal:)` — the name is lowercased first;
    /// the exact name `en0` classifies as Wi-Fi before the `en` prefix
    /// rule; prefix rules then apply in Swift switch order.
    pub fn from_name(name: &str, is_link_local: bool) -> Self {
        use LocalInterfaceType::*;
        let lower = name.to_lowercase();
        if lower == "en0" {
            return Wifi;
        }
        if lower.starts_with("en") {
            return if is_link_local { UsbLinkLocal } else { Ethernet };
        }
        if lower.starts_with("utun") {
            return VpnUtun;
        }
        if lower.starts_with("ipsec") {
            return VpnIpsec;
        }
        if lower.starts_with("pdp") {
            return Cellular;
        }
        if lower.starts_with("awdl") {
            return Airdrop;
        }
        if lower.starts_with("llw") {
            return LowLatencyWlan;
        }
        if lower.starts_with("bridge") || lower.starts_with("ap") {
            return HotspotBridge;
        }
        if lower.starts_with("lo") {
            return Loopback;
        }
        if lower.starts_with("pktap") {
            return PacketCapture;
        }
        Other
    }

    /// All cases, in Swift declaration order (CaseIterable).
    pub fn all_cases() -> [LocalInterfaceType; 12] {
        use LocalInterfaceType::*;
        [
            VpnUtun,
            VpnIpsec,
            Wifi,
            UsbLinkLocal,
            Ethernet,
            Cellular,
            Airdrop,
            LowLatencyWlan,
            HotspotBridge,
            Loopback,
            PacketCapture,
            Other,
        ]
    }
}

/// Swift `LocalInterfaceInfo` — the App-facing per-interface summary
/// produced by `NetworkObserverService.activeInterfaces`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalInterfaceInfo {
    pub name: String,
    pub ip: String,
    pub ipv6: Option<String>,
    pub subnet: String,
    pub interface_type: LocalInterfaceType,
}

impl LocalInterfaceInfo {
    /// Swift `id` — `name.lowercased() + "-" + ip`.
    pub fn id(&self) -> String {
        format!("{}-{}", self.name.to_lowercase(), self.ip)
    }
}
