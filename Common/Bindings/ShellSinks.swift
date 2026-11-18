// ShellSinks.swift — MinimuxerRustBindings
//
// The three Swift objects the Rust core calls back into
// (swift-bridge `extern "Swift"` types). They are pure adapters:
// each method converts bridge types to plain Swift values and
// forwards to a closure installed by the shell's Minimuxer module.
// No business logic lives here (DESIGN D3).

import Foundation
import minimuxer_rust

/// Registered once via `mm_register_event_sink`; Rust routes its
/// log lines and status evaluations through this object.
public final class MmEventSink {
    public var onLog: ((String) -> Void)?
    public var onStatus: ((UInt64, Bool, String, String, String) -> Void)?

    public init() {}

    public func on_log(message: RustString) {
        onLog?(message.toString())
    }

    public func on_status(seq: UInt64, success: Bool, domain: RustString, code: RustString, reason: RustString) {
        onStatus?(seq, success, domain.toString(), code.toString(), reason.toString())
    }
}

/// The App-side connection config presented to the Rust connection
/// manager (checklist section 2, shape 2). Setter arguments arrive
/// as Option<String>; the Rust side uses "" as the nil sentinel on
/// some paths, so "" is normalized to nil here.
public final class MmConnectionConfig {
    public var setTunnelIfaceIp: ((String?) -> Void)?
    public var setTunnelPeerIp: ((String?) -> Void)?
    public var setTunnelPeerSubnetMask: ((String?) -> Void)?
    public var setTunnelPeerReachable: ((Bool) -> Void)?
    public var setTunnelIfaceSubnetMask: ((String?) -> Void)?
    public var setRemoteReachable: ((Bool) -> Void)?
    public var setOverrideTunnelPeerReachable: ((Bool) -> Void)?
    public var getConnectionMode: (() -> MmConnectionMode)?
    public var getOverrideTunnelPeerIp: (() -> String)?
    public var getRemoteServerIp: (() -> String)?
    public var requestServicePort: ((UInt64, MmPairingProtocol, UInt16) -> Void)?

    public init() {}

    private func normalized(_ value: RustString?) -> String? {
        guard let value = value else { return nil }
        let s = value.toString()
        return s.isEmpty ? nil : s
    }

    public func set_tunnel_iface_ip(ip: RustString?) {
        setTunnelIfaceIp?(normalized(ip))
    }

    public func set_tunnel_peer_ip(ip: RustString?) {
        setTunnelPeerIp?(normalized(ip))
    }

    public func set_tunnel_peer_subnet_mask(mask: RustString?) {
        setTunnelPeerSubnetMask?(normalized(mask))
    }

    public func set_tunnel_peer_reachable(reachable: Bool) {
        setTunnelPeerReachable?(reachable)
    }

    public func set_tunnel_iface_subnet_mask(mask: RustString?) {
        setTunnelIfaceSubnetMask?(normalized(mask))
    }

    public func set_remote_reachable(reachable: Bool) {
        setRemoteReachable?(reachable)
    }

    public func set_override_tunnel_peer_reachable(reachable: Bool) {
        setOverrideTunnelPeerReachable?(reachable)
    }

    public func get_connection_mode() -> MmConnectionMode {
        getConnectionMode?() ?? .NotConfigured
    }

    public func get_override_tunnel_peer_ip() -> RustString {
        (getOverrideTunnelPeerIp?() ?? "").intoRustString()
    }

    public func get_remote_server_ip() -> RustString {
        (getRemoteServerIp?() ?? "").intoRustString()
    }

    public func request_service_port(request_id: UInt64, pairing_protocol: MmPairingProtocol, port: UInt16) {
        requestServicePort?(request_id, pairing_protocol, port)
    }
}

/// Wireless pairing callbacks (checklist section 2, shape 3).
/// `request_pin` returns whether the App has a PIN handler; the PIN
/// itself returns via `mm_wireless_complete_pin`. When no handler
/// answers, the Rust gateway applies its 60 s "000000" default
/// (audit D-M8-4, same layer as the Swift original).
public final class MmWirelessPairSink {
    public var onPinReceived: ((String) -> Void)?
    public var onReadyToPair: ((String, Int) -> Void)?
    public var requestPin: ((UInt64) -> Bool)?
    public var resolveFileName: ((String, String) -> MmResolveOutcome)?

    public init() {}

    public func on_pin_received(pin: RustString) {
        onPinReceived?(pin.toString())
    }

    public func on_ready_to_pair(service_id: RustString, port: Int32) {
        onReadyToPair?(service_id.toString(), Int(port))
    }

    public func request_pin(request_id: UInt64) -> Bool {
        requestPin?(request_id) ?? false
    }

    public func resolve_file_name(name: RustString, model: RustString) -> MmResolveOutcome {
        resolveFileName?(name.toString(), model.toString())
            ?? MmResolveOutcome(has_resolver: false, file_name: "".intoRustString())
    }
}
