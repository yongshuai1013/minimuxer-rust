// GENERATED from crates/mm-common/error-map.toml by
// Scripts/generate-error-table.py -- do not edit by hand.
// Regenerate: python3 Scripts/generate-error-table.py

import Foundation
import MinimuxerCommon
import DeviceGatewayAPI
import MinimuxerRustBindings

/// Rebuild a `MinimuxerError` from its FFI representation.
public func mmMinimuxerError(from info: MmErrorInfo) -> MinimuxerError {
    let code = info.code.toString()
    let reason = info.reason.toString()
    switch code {
    case "noDevice":
        return .noDevice(reason)
    case "noConnection":
        return .noConnection(reason)
    case "notReachable":
        return .notReachable(reason)
    case "noVPN":
        return .noVPN(reason)
    case "notStarted":
        return .notStarted(reason)
    case "pairingNotLoaded":
        return .pairingNotLoaded(reason)
    case "connectionModeNotConfigured":
        return .connectionModeNotConfigured(reason)
    case "restartAlreadyInProgressError":
        return .restartAlreadyInProgressError(reason)
    case "invalidVPN":
        return .invalidVPN(reason)
    case "invalidPairing":
        let parts = reason.components(separatedBy: "\u{1}")
        let proto = PairingProtocol(rawValue: parts.first ?? "") ?? .unknown
        let rest = parts.count > 1 ? parts[1] : ""
        return .invalidPairing(protocol: proto, reason: rest)
    case "muxerNotListening":
        return .muxerNotListening(reason)
    case "createDebug":
        return .createDebug(reason)
    case "createInstproxy":
        return .createInstproxy(reason)
    case "createLockdown":
        return .createLockdown(reason)
    case "createCoreDevice":
        return .createCoreDevice(reason)
    case "createSoftwareTunnel":
        return .createSoftwareTunnel(reason)
    case "createRemoteServer":
        return .createRemoteServer(reason)
    case "createProcessControl":
        return .createProcessControl(reason)
    case "getLockdownValue":
        return .getLockdownValue(reason)
    case "fetchUDID":
        return .fetchUDID(reason)
    case "connect":
        return .connect(reason)
    case "close":
        return .close(reason)
    case "xpcHandshake":
        return .xpcHandshake(reason)
    case "noService":
        return .noService(reason)
    case "invalidProductVersion":
        return .invalidProductVersion(reason)
    case "lookupApps":
        return .lookupApps(reason)
    case "findApp":
        return .findApp(reason)
    case "bundlePath":
        return .bundlePath(reason)
    case "maxPacket":
        return .maxPacket(reason)
    case "workingDirectory":
        return .workingDirectory(reason)
    case "argv":
        return .argv(reason)
    case "launchSuccess":
        return .launchSuccess(reason)
    case "detach":
        return .detach(reason)
    case "attach":
        return .attach(reason)
    case "createAfc":
        return .createAfc(reason)
    case "rwAfc":
        return .rwAfc(reason)
    case "installApp":
        return .installApp(reason)
    case "uninstallApp":
        return .uninstallApp(reason)
    case "createMisagent":
        return .createMisagent(reason)
    case "profileInstall":
        return .profileInstall(reason)
    case "profileRemove":
        return .profileRemove(reason)
    case "createFolder":
        return .createFolder(reason)
    case "downloadImage":
        return .downloadImage(reason)
    case "imageLookup":
        return .imageLookup(reason)
    case "imageRead":
        return .imageRead(reason)
    case "mount":
        let parts = reason.components(separatedBy: "\u{1}")
        let proto = PairingProtocol(rawValue: parts.first ?? "") ?? .unknown
        let rest = parts.count > 1 ? parts[1] : ""
        return .mount(protocol: proto, reason: rest)
    default:
        return .noConnection(reason)
    }
}

/// Rebuild an `EMProxyError` from its FFI representation.
public func mmEMProxyError(from info: MmErrorInfo) -> EMProxyError {
    let code = info.code.toString()
    let reason = info.reason.toString()
    switch code {
    case "invalidBindAddressPointer":
        return .invalidBindAddressPointer
    case "invalidUTF8String":
        return .invalidUTF8String
    case "invalidSocketAddress":
        return .invalidSocketAddress(reason)
    case "socketBindFailed":
        return .socketBindFailed
    case "cryptoInitFailed":
        return .cryptoInitFailed
    case "serverNotRunning":
        return .serverNotRunning
    case "stopSignalFailed":
        return .stopSignalFailed
    case "threadJoinFailed":
        return .threadJoinFailed
    case "handshakeClientNotConfigured":
        return .handshakeClientNotConfigured
    case "unknownError":
        return .unknownError(Int32(reason) ?? 0)
    default:
        return .unknownError(0)
    }
}

/// Rebuild a `MinimuxerInternalError` from its FFI representation.
func mmInternalError(from info: MmErrorInfo) -> MinimuxerInternalError {
    let code = info.code.toString()
    let reason = info.reason.toString()
    switch code {
    case "deviceEndpointNotInitialized":
        return .deviceEndpointNotInitialized
    case "networkIfaceNotRefreshed":
        return .networkIfaceNotRefreshed
    case "pairingFailed":
        return .pairingFailed(reason)
    default:
        return .pairingFailed(reason)
    }
}

// MinimuxerServiceError: MmErrorInfo.code carries the component raw
// value (heartbeat, mounter); reason carries the inner error's description.
/// Rebuild a `MinimuxerServiceError` from its FFI representation.
public func mmServiceError(from info: MmErrorInfo) -> MinimuxerServiceError {
    let component = MinimuxerComponent(rawValue: info.code.toString()) ?? .heartbeat
    let inner = NSError(domain: "MinimuxerServiceError.inner", code: 0,
                        userInfo: [NSLocalizedDescriptionKey: info.reason.toString()])
    return MinimuxerServiceError(component: component, error: inner)
}

/// Full-domain dispatcher used by the shell's Minimuxer module:
/// rebuilds the Swift error type named by `MmErrorInfo.domain`.
/// Unknown domains keep their Rust-rendered description
/// (`mm_describe_error`) inside an NSError instead of inventing a case.
public func mmSwiftError(from info: MmErrorInfo) -> Error {
    switch info.domain.toString() {
    case "MinimuxerError": return mmMinimuxerError(from: info)
    case "DeviceGatewayError": return mmDeviceGatewayError(from: info)
    case "PairingError": return mmPairingError(from: info)
    case "EMProxyError": return mmEMProxyError(from: info)
    case "MinimuxerInternalError": return mmInternalError(from: info)
    case "MinimuxerServiceError": return mmServiceError(from: info)
    default:
        return NSError(domain: info.domain.toString(), code: 0,
                       userInfo: [NSLocalizedDescriptionKey: mm_describe_error(info).toString()])
    }
}
