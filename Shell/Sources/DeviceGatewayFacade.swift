//
//  DeviceGatewayFacade.swift
//  Minimuxer
//
//  DeviceGatewayAPI conformance for the Rust-backed shell. Calls
//  forward to the gateway hub (mm_gateway_*) where the hub exposes
//  the operation, and to mm_core_* for the operations the Swift
//  original routed through the same gateway object (both reach the
//  one Rust gateway instance). Errors are rebuilt from MmErrorInfo
//  by the generated tables; nothing is mapped or decided here.
//
//  Two protocol members have no FFI counterpart and throw
//  DeviceGatewayError.unsupportedOperation, the same shape the
//  Swift package itself uses for paths a backend does not support
//  (e.g. libimobiledevice startWirelessPair, audit appendix A-a2):
//    - mountDeveloperImage / mountPersonalizedDdi: in the Swift
//      original these were only called by the internal Mounter;
//      that orchestration (download, version split, mount) lives
//      in the Rust core and is reached via MinimuxerAPI.mountDDI.
//    - performHeartbeat: the heartbeat loop is internal to the
//      Rust core (mm-core heartbeat service).
//    - startWirelessPair / triggerWirelessPair: in the Swift
//      original these were the internal plumbing between
//      WirelessPairService and the gateway; in the shell the
//      WirelessPairAPI face drives mm_wireless_* directly.
//  The App reads only `pairingFileType` from this facade
//  (verified against the SideStore tree); all App-reachable
//  operations above are fully forwarded.
//

import Foundation
import MinimuxerCommon
import DeviceGatewayAPI
import MinimuxerRustBindings

final class MinimuxerGatewayFacade: DeviceGatewayAPI {
    private let lock = NSLock()
    private var lastPairingContent: String?

    var requiresUsbmuxd: Bool {
        mm_gateway_requires_usbmuxd()
    }

    var pairingFileType: PairingProtocol {
        mm_gateway_pairing_protocol().pairingProtocol
    }

    var pairingFileData: Data? {
        lock.withLock { lastPairingContent }?.data(using: .utf8)
    }

    var pairingDataDict: [String: any Sendable]? {
        guard let data = pairingFileData,
              let raw = try? PropertyListSerialization.propertyList(from: data, options: [], format: nil) as? [String: Any] else {
            return nil
        }
        return ConcurrencyUtils.toSendableDictionary(raw)
    }

    func getPort(for protocolType: PairingProtocol) -> UInt16 {
        mm_gateway_service_port(protocolType.mmPairingProtocol)
    }

    func setPort(_ port: UInt16, for protocolType: PairingProtocol) {
        mm_gateway_set_port(port, protocolType.mmPairingProtocol)
    }

    func start(pairingFileContent: String, preferred: PairingProtocol?) async throws {
        let outcome = await mm_gateway_start(pairingFileContent, preferred?.mmPairingProtocol)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
        lock.withLock { lastPairingContent = pairingFileContent }
    }

    func stop() async throws {
        let outcome = await mm_gateway_stop()
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func setDeviceEndpointIp(_ ip: String?) {
        mm_gateway_set_endpoint_ip(ip)
    }

    func setLogging(_ enabled: Bool) {
        DeviceGatewayLogging.setLogging(enabled)
        mm_gateway_set_logging(enabled)
    }

    func fetchUDID() async throws -> String {
        let outcome = await mm_gateway_fetch_udid()
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
        return outcome.value.toString()
    }

    func getLockdownValue(key: String) async throws -> String {
        let outcome = await mm_gateway_get_lockdown_value(key)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
        return outcome.value.toString()
    }

    func isDDIMounted() async throws -> Bool {
        let outcome = await mm_core_is_ddi_mounted()
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
        return outcome.ready
    }

    func mountDeveloperImage(image: Data, signature: Data) async throws {
        throw DeviceGatewayError(.unsupportedOperation, reason: "mountDeveloperImage")
    }

    func mountPersonalizedDdi(image: Data, trustcache: Data, manifest: Data) async throws {
        throw DeviceGatewayError(.unsupportedOperation, reason: "mountPersonalizedDdi")
    }

    func installProvisioningProfile(profile: Data) async throws {
        let outcome = await mm_core_install_profile(RustVec<UInt8>(from: profile))
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func removeProvisioningProfile(id: String) async throws {
        let outcome = await mm_core_remove_profile(id)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func dumpProfiles(docsPath: String) async throws -> String {
        let outcome = await mm_core_dump_profiles(docsPath, .Raw)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
        return outcome.value.toString()
    }

    func removeApp(bundleId: String) async throws {
        let outcome = await mm_core_remove_app(bundleId)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
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

    func wipeContainer(identifier: String) async throws {
        let outcome = await mm_core_wipe_container(identifier)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func debugApp(appId: String) async throws {
        let outcome = await mm_core_debug_app(appId)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func debugProcess(pid: UInt32) async throws {
        let outcome = await mm_core_attach_debugger(pid)
        guard outcome.success else { throw mmSwiftError(from: outcome.error) }
    }

    func performHeartbeat(interval: UInt64) async throws -> UInt64 {
        throw DeviceGatewayError(.unsupportedOperation, reason: "performHeartbeat")
    }

    func startWirelessPair(
        hostName: String,
        hostModel: String,
        outPath: String,
        resolveFileName: (@Sendable (String, String) -> String)?,
        onReady: @escaping @Sendable (String, UInt16, [String: String]) -> Void,
        onPin: @escaping @Sendable (String) -> Void
    ) async throws -> PairedDeviceRecord {
        throw DeviceGatewayError(.unsupportedOperation, reason: "startWirelessPair")
    }

    func triggerWirelessPair(
        targetIp: String,
        targetPort: UInt16,
        hostName: String,
        hostModel: String,
        outPath: String,
        resolveFileName: (@Sendable (String, String) -> String)?,
        onRequestPin: @escaping @Sendable (@escaping @Sendable (String) -> Void) -> Void
    ) async throws -> PairedDeviceRecord {
        throw DeviceGatewayError(.unsupportedOperation, reason: "triggerWirelessPair")
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
