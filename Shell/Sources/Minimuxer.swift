//
//  Minimuxer.swift
//  Minimuxer
//
//  Facade for the Rust-backed shell. Structure mirrors the Swift
//  original (singleton, current* statics, set(_:) semantics); the
//  per-face objects forward to the Rust core, and backend/port/
//  timeout changes are applied through mm_core_set_params, whose
//  Rust implementation performs the gateway swap and port
//  carry-over (equivalence audit A6/A8).
//

import Foundation
import MinimuxerCommon
import DeviceGatewayAPI
import MinimuxerRustBindings

public final class Minimuxer: MinimuxerFacade, @unchecked Sendable {
    public let core: any MinimuxerAPI
    public let network: any NetworkObserverAPI
    public let wirelessPair: any WirelessPairAPI
    public let emproxy: any EMProxyAPI
    public let gateway: any DeviceGatewayAPI

    private let coreImpl: MinimuxerCoreImpl

    // singleton
    public static let shared: Minimuxer = {
        createInstance(
            backend: currentBackend,
            deviceProbeTimeout: currentDeviceProbeTimeout,
            remotePairingPort: currentRemotePairingPort
        )
    }()

    private static var currentDeviceProbeTimeout: Int = MinimuxerConstants.defaultTCPProbeTimeoutMs
    private static var currentBackend: GatewayBackend = .idevice
    private static var currentRemotePairingPort: UInt16 = MinimuxerConstants.remotePairingPort
    private static let lock = NSLock()

    private init(
        coreImpl: MinimuxerCoreImpl,
        network: any NetworkObserverAPI,
        wirelessPair: any WirelessPairAPI,
        emproxy: any EMProxyAPI,
        gateway: any DeviceGatewayAPI
    ) {
        self.coreImpl = coreImpl
        self.core = coreImpl
        self.network = network
        self.wirelessPair = wirelessPair
        self.emproxy = emproxy
        self.gateway = gateway
    }

    @discardableResult
    public func set(_ params: MinimuxerParams) -> any MinimuxerFacade {
        Self.lock.lock()
        defer { Self.lock.unlock() }

        if let newBackend = params.backend {
            Self.currentBackend = newBackend
        }
        if let newPort = params.remotePairingPort {
            Self.currentRemotePairingPort = newPort
        }
        if let newTimeout = params.deviceProbeTimeout {
            Self.currentDeviceProbeTimeout = newTimeout
            coreImpl.noteDeviceProbeTimeout(newTimeout)
        }

        mm_core_set_params(
            params.backend?.mmGatewayBackend,
            params.remotePairingPort,
            params.deviceProbeTimeout.map { Int32($0) }
        )

        return self
    }

    private static func createInstance(
        backend: GatewayBackend,
        deviceProbeTimeout: Int,
        remotePairingPort: UInt16
    ) -> Minimuxer {
        // Apply the initial parameters to the Rust core, mirroring
        // the Swift original's createInstance (gateway built for the
        // backend, RP port set on it, probe timeout handed to the
        // connection manager).
        mm_core_set_params(
            backend.mmGatewayBackend,
            remotePairingPort,
            Int32(deviceProbeTimeout)
        )

        let coreImpl = MinimuxerCoreImpl(deviceProbeTimeout: deviceProbeTimeout)
        return Minimuxer(
            coreImpl: coreImpl,
            network: MinimuxerNetworkImpl(),
            wirelessPair: MinimuxerWirelessPairImpl(),
            emproxy: MinimuxerEMProxyImpl(),
            gateway: MinimuxerGatewayFacade()
        )
    }
}

extension GatewayBackend {
    var mmGatewayBackend: MmGatewayBackend {
        switch self {
        case .idevice: return .Idevice
        case .libimobiledevice: return .Libimobiledevice
        }
    }
}
