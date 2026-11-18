//
//  NetworkImpl.swift
//  Minimuxer
//
//  NetworkObserverAPI conformance for the Rust-backed shell.
//  Interface scans and satisfaction flags come from the Rust
//  polling observer (mm-net); see the pathPublisher note below.
//

import Foundation
import Network
import Combine
import MinimuxerCommon
import MinimuxerRustBindings

final class MinimuxerNetworkImpl: NetworkObserverAPI {
    private let pathSubject = PassthroughSubject<NWPath, Never>()

    // Named downgrade (DESIGN D4, equivalence audit D7): NWPath has
    // no public constructor and the Rust observer cannot synthesize
    // one; the E1 NWPathMonitor trigger shell is conditional and is
    // NOT enabled by default. This publisher therefore never emits;
    // the property is kept for source compatibility only.
    var pathPublisher: AnyPublisher<NWPath, Never> {
        pathSubject.eraseToAnyPublisher()
    }

    var isWifiSatisfied: Bool { mm_net_snapshot().wifi_satisfied }
    var isWiredSatisfied: Bool { mm_net_snapshot().wired_satisfied }
    var isUsbSatisfied: Bool { mm_net_snapshot().usb_satisfied }
    var isBridgeSatisfied: Bool { mm_net_snapshot().bridge_satisfied }
    var isUTunAvailable: Bool { mm_net_snapshot().utun_available }
    var isIKEv2IPSecAvailable: Bool { mm_net_snapshot().ipsec_available }

    var activeInterfaces: [LocalInterfaceInfo] {
        mm_net_scan_interfaces().map { info in
            LocalInterfaceInfo(
                name: info.name.toString(),
                ip: info.ip.toString(),
                ipv6: info.ipv6.toString().isEmpty ? nil : info.ipv6.toString(),
                subnet: info.subnet.toString(),
                type: LocalInterfaceType(rawValue: info.interface_type.toString()) ?? .other
            )
        }.sorted {
            if $0.type != $1.type {
                return $0.type < $1.type
            }
            return $0.name.lowercased().localizedCompare($1.name.lowercased()) == .orderedAscending
        }
    }

    @discardableResult
    func start() async -> Bool {
        mm_net_observer_start()
    }

    @discardableResult
    func stop() async -> Bool {
        mm_net_observer_stop()
    }

    func refreshEndpoint() async {
        _ = await mm_net_refresh()
    }
}
