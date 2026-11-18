//
//  WirelessPairImpl.swift
//  Minimuxer
//
//  WirelessPairAPI conformance for the Rust-backed shell. Pairing
//  (SRP, RSD, Bonjour advertising via dns-sd) runs in the Rust core
//  (mm-rp / mm-core wireless_pair); this class owns the App-facing
//  callback properties and the MmWirelessPairSink registration,
//  delivering callbacks on the main actor like the Swift original.
//

import Foundation
import MinimuxerCommon
import DeviceGatewayAPI
import MinimuxerRustBindings

final class MinimuxerWirelessPairImpl: WirelessPairAPI {
    var onPinReceived: ((String) -> Void)?
    var onReadyToPair: ((String, Int) -> Void)?
    var onRequestPin: ((@escaping (String) -> Void) -> Void)?

    private let sink = MmWirelessPairSink()
    private let resolverLock = NSLock()
    private var currentResolver: (@Sendable (String, String) -> String)?

    init() {
        sink.onPinReceived = { [weak self] pin in
            Task { @MainActor in self?.onPinReceived?(pin) }
        }
        sink.onReadyToPair = { [weak self] serviceId, port in
            Task { @MainActor in self?.onReadyToPair?(serviceId, port) }
        }
        sink.requestPin = { [weak self] requestId in
            guard let self = self, self.onRequestPin != nil else { return false }
            Task { @MainActor in
                self.onRequestPin? { pin in
                    _ = mm_wireless_complete_pin(requestId, pin)
                }
            }
            return true
        }
        sink.resolveFileName = { [weak self] name, model in
            guard let self = self,
                  let resolver = self.resolverLock.withLock({ self.currentResolver }) else {
                return MmResolveOutcome(has_resolver: false, file_name: "".intoRustString())
            }
            return MmResolveOutcome(has_resolver: true,
                                    file_name: resolver(name, model).intoRustString())
        }
        mm_wireless_bind_sink(sink)
    }

    private func setResolver(_ resolver: (@Sendable (String, String) -> String)?) {
        resolverLock.withLock { currentResolver = resolver }
    }

    func start(
        hostName: String,
        hostModel: String,
        outPath: String,
        resolveFileName: (@Sendable (String, String) -> String)?,
        completion: @escaping (Result<PairedDeviceRecord, Swift.Error>) -> Void
    ) {
        setResolver(resolveFileName)
        Task.detached {
            let outcome = await mm_wireless_start(hostName, hostModel, outPath)
            let result = Self.result(from: outcome)
            await MainActor.run { completion(result) }
        }
    }

    func trigger(
        targetIp: String,
        targetPort: UInt16,
        hostName: String,
        hostModel: String,
        outPath: String,
        resolveFileName: (@Sendable (String, String) -> String)?,
        completion: @escaping (Result<PairedDeviceRecord, Swift.Error>) -> Void
    ) {
        setResolver(resolveFileName)
        Task.detached {
            let outcome = await mm_wireless_trigger(targetIp, targetPort, hostName, hostModel, outPath)
            let result = Self.result(from: outcome)
            await MainActor.run { completion(result) }
        }
    }

    func stop() {
        mm_wireless_stop()
        setResolver(nil)
    }

    private static func result(from outcome: MmPairedDeviceOutcome) -> Result<PairedDeviceRecord, Swift.Error> {
        guard outcome.success else {
            return .failure(mmSwiftError(from: outcome.error))
        }
        do {
            // The pairing file crosses as its serialized content
            // (audit I7); materialize the Swift PairingFile here.
            let parsed = try PairingFileParser.parse(content: outcome.pairing_file_content.toString())
            return .success(PairedDeviceRecord(
                name: outcome.name.toString(),
                model: outcome.model.toString(),
                pairingFilePath: outcome.pairing_file_path.toString(),
                pairingFile: parsed
            ))
        } catch {
            return .failure(error)
        }
    }
}
