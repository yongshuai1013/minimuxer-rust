//
//  CoreImpl.swift
//  Minimuxer
//
//  MinimuxerAPI conformance for the Rust-backed shell. Every
//  operation forwards to the mm-core / mm-net FFI surface; failures
//  cross as MmErrorInfo and are rebuilt into the Swift error types
//  by the generated conversion tables (DESIGN D3: no mapping
//  decisions in the shell).
//

import Foundation
import Combine
import MinimuxerCommon
import DeviceGatewayAPI
import MinimuxerRustBindings

final class MinimuxerCoreImpl: MinimuxerAPI {
    let statusSubject = PassthroughSubject<Result<Bool, MinimuxerError>, Never>()
    var statusPublisher: AnyPublisher<Result<Bool, MinimuxerError>, Never> {
        statusSubject.eraseToAnyPublisher()
    }

    private let eventSink = MmEventSink()
    private var connectionConfig: MmConnectionConfig?
    private let seqLock = NSLock()
    private var lastStatusSeq: UInt64 = 0
    private let timeoutLock = NSLock()
    private var probeTimeout: Int

    init(deviceProbeTimeout: Int) {
        self.probeTimeout = deviceProbeTimeout

        eventSink.onLog = { message in
            // Rust log lines already carry their [debug]/[verbose]
            // level tag (checklist section 2); route to the matching
            // Swift log channel of this module.
            if message.contains("[verbose]") {
                verboseLog(message)
            } else {
                debugLog(message)
            }
        }
        eventSink.onStatus = { [weak self] seq, success, domain, code, reason in
            guard let self = self else { return }
            let fresh: Bool = self.seqLock.withLock {
                guard seq > self.lastStatusSeq else { return false }
                self.lastStatusSeq = seq
                return true
            }
            guard fresh else { return }
            if success {
                self.statusSubject.send(.success(true))
            } else {
                let info = MmErrorInfo(domain: domain.intoRustString(),
                                       code: code.intoRustString(),
                                       reason: reason.intoRustString())
                self.statusSubject.send(.failure(mmMinimuxerError(from: info)))
            }
        }
        mm_register_event_sink(eventSink)
    }

    // MARK: - State queries

    var pairingFileType: PairingProtocol {
        mm_gateway_pairing_protocol().pairingProtocol
    }

    var isLoggingEnabled: Bool {
        MinimuxerLogging.isLoggingEnabled
    }

    var isPairingFileLoaded: Bool {
        mm_core_is_pairing_file_loaded()
    }

    var deviceProbeTimeout: Int {
        timeoutLock.withLock { probeTimeout }
    }

    func setDeviceProbeTimeout(_ timeoutMs: Int) {
        timeoutLock.withLock { probeTimeout = timeoutMs }
        mm_net_set_probe_timeout(Int32(timeoutMs))
    }

    /// Cache-only update used by Minimuxer.set(_:), which applies
    /// the value to the Rust core via mm_core_set_params.
    func noteDeviceProbeTimeout(_ timeoutMs: Int) {
        timeoutLock.withLock { probeTimeout = timeoutMs }
    }

    // MARK: - Configuration

    func getConnectionMode() async -> DeviceConnectionMode {
        mm_net_connection_mode().deviceConnectionMode
    }

    func bindConnectionConfig(_ binding: ConnectionConfigBinding) async {
        let config = MmConnectionConfig()
        config.setTunnelIfaceIp = { binding.setTunnelIfaceIp($0) }
        config.setTunnelPeerIp = { binding.setTunnelPeerIp($0) }
        config.setTunnelPeerSubnetMask = { binding.setTunnelPeerSubnetMask($0) }
        config.setTunnelPeerReachable = { binding.setTunnelPeerReachable($0) }
        config.setTunnelIfaceSubnetMask = { binding.setTunnelIfaceSubnetMask($0) }
        config.setRemoteReachable = { binding.setRemoteReachable($0) }
        config.setOverrideTunnelPeerReachable = { binding.setOverrideTunnelPeerReachable($0) }
        config.getConnectionMode = { binding.getConnectionMode().mmConnectionMode }
        config.getOverrideTunnelPeerIp = { binding.getOverrideTunnelPeerIp() }
        config.getRemoteServerIp = { binding.getRemoteServerIp() }
        config.requestServicePort = { [weak self] requestId, proto, port in
            guard let self = self else { return }
            let current = ServicePort(protocolType: proto.pairingProtocol, port: port)
            Task {
                let resolved: ServicePort
                if let resolve = binding.resolveServicePort {
                    resolved = await resolve(current)
                } else {
                    resolved = ServicePort(protocolType: current.protocolType,
                                           port: current.protocolType.defaultPort)
                }
                _ = mm_net_complete_service_port(requestId,
                                                 resolved.protocolType.mmPairingProtocol,
                                                 resolved.port)
            }
        }
        self.connectionConfig = config
        _ = mm_core_bind_connection_config(config)
        // The Swift original refreshed the endpoint right after
        // binding (MinimuxerImpl.bindConnectionConfig); mirror it.
        _ = await mm_net_refresh()
    }

    func setLogging(_ enabled: Bool) {
        MinimuxerLogging.setLogging(enabled)
        mm_core_set_logging(enabled)
        mm_gateway_set_logging(enabled)
    }

    func describeError(_ error: MinimuxerError) -> String {
        error.description
    }

    // MARK: - Readiness

    func isReady(withNetworkCheck: Bool, withDDIMountCheck: Bool) async -> Result<Bool, MinimuxerError> {
        let outcome = await mm_core_is_ready(withNetworkCheck, withDDIMountCheck)
        if outcome.success {
            return .success(outcome.ready)
        }
        return .failure(mmMinimuxerError(from: outcome.error))
    }

    // MARK: - Lifecycle

    func start(pairingFile: String, mountPath: String, preferred: PairingProtocol?) async throws {
        let outcome = await mm_core_start(pairingFile, mountPath, preferred?.mmPairingProtocol)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func stop() async throws {
        let outcome = await mm_core_stop()
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func restart() async throws {
        let outcome = await mm_core_restart()
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func reinitializePairingData(pairingFile: String) async throws {
        let outcome = await mm_core_reinitialize_pairing_data(pairingFile)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    // MARK: - DDI

    func mountDDI(docsPath: String) async throws -> Bool {
        let outcome = await mm_core_mount_ddi(docsPath)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
        return outcome.ready
    }

    func isDDIMounted() async throws -> Bool {
        let outcome = await mm_core_is_ddi_mounted()
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
        return outcome.ready
    }

    // MARK: - Device operations

    func fetchUDID() async throws -> String {
        let outcome = await mm_core_fetch_udid()
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
        return outcome.value.toString()
    }

    func testDeviceConnection(ifaddr: String, timeout: Int) -> Bool {
        // The Swift original probes the active pairing type's service
        // port (gateway.servicePort), not a fixed port.
        let port = mm_gateway_service_port(mm_gateway_pairing_protocol())
        return mm_test_tcp(ifaddr, port, Int32(timeout))
    }

    func sendIpaAfc(bundleId: String, ipaBytes: Data) async throws {
        let outcome = await mm_core_send_ipa_afc(bundleId, RustVec<UInt8>(from: ipaBytes))
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func sendAppBundleAfc(bundleId: String, appURL: URL) async throws {
        let outcome = await mm_core_send_app_bundle_afc(bundleId, appURL.path)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func installIpa(bundleId: String) async throws {
        let outcome = await mm_core_install_ipa(bundleId)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func installAppBundle(bundleId: String, appName: String) async throws {
        let outcome = await mm_core_install_app_bundle(bundleId, appName)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func removeApp(bundleId: String) async throws {
        let outcome = await mm_core_remove_app(bundleId)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func wipeContainer(identifier: String) async throws {
        let outcome = await mm_core_wipe_container(identifier)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func debugApp(appId: String) async throws {
        let outcome = await mm_core_debug_app(appId)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func attachDebugger(pid: UInt32) async throws {
        let outcome = await mm_core_attach_debugger(pid)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func installProvisioningProfile(profile: Data) async throws {
        let outcome = await mm_core_install_profile(RustVec<UInt8>(from: profile))
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func removeProvisioningProfile(id: String) async throws {
        let outcome = await mm_core_remove_profile(id)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func dumpProfiles(docsPath: String, mode: ProfileDumpMode) async throws -> String {
        let outcome = await mm_core_dump_profiles(docsPath, mode.mmProfileDumpMode)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
        return outcome.value.toString()
    }

    func afcListDirectory(bundleId: String, path: String) async throws -> [String] {
        let outcome = await mm_core_afc_list_directory(bundleId, path)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
        return outcome.entries.map { $0.toString() }
    }

    func afcReadFile(bundleId: String, path: String) async throws -> Data {
        let outcome = await mm_core_afc_read_file(bundleId, path)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
        return outcome.data.toData()
    }

    func afcGetFileInfo(bundleId: String, path: String) async throws -> (isDirectory: Bool, fileSize: Int64) {
        let outcome = await mm_core_afc_get_file_info(bundleId, path)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
        return (isDirectory: outcome.is_directory, fileSize: outcome.file_size)
    }
}

// MARK: - Bridge enum conversions local to this module

extension DeviceConnectionMode {
    var mmConnectionMode: MmConnectionMode {
        switch self {
        case .localVPN: return .LocalVpn
        case .remoteServer: return .RemoteServer
        case .notConfigured: return .NotConfigured
        }
    }
}

extension MmConnectionMode {
    var deviceConnectionMode: DeviceConnectionMode {
        switch self {
        case .LocalVpn: return .localVPN
        case .RemoteServer: return .remoteServer
        case .NotConfigured: return .notConfigured
        }
    }
}

extension ProfileDumpMode {
    var mmProfileDumpMode: MmProfileDumpMode {
        switch self {
        case .zip: return .Zip
        case .raw: return .Raw
        }
    }
}

extension RustVec<UInt8> {
    convenience init(from data: Data) {
        self.init()
        for byte in data { push(value: byte) }
    }

    func toData() -> Data {
        Data((0..<Int(len())).map { self.get(index: UInt($0))! })
    }
}
