// GENERATED from crates/mm-common/error-map.toml by
// Scripts/generate-error-table.py -- do not edit by hand.
// Regenerate: python3 Scripts/generate-error-table.py

import Foundation
import MinimuxerRustBindings

/// Rebuild a `PairingError` from its FFI representation.
public func mmPairingError(from info: MmErrorInfo) -> PairingError {
    let code = info.code.toString()
    let reason = info.reason.toString()
    switch code {
    case "unreadable":
        return .unreadable(reason)
    case "invalidPlist":
        return .invalidPlist(reason)
    case "incomplete":
        let parts = reason.components(separatedBy: "\u{1}")
        let proto = PairingProtocol(rawValue: parts.first ?? "") ?? .unknown
        let keys = parts.count > 1 && !parts[1].isEmpty ? parts[1].components(separatedBy: ",") : []
        return .incomplete(protocolType: proto, missingKeys: keys)
    case "ambiguous":
        return .ambiguous(reason)
    default:
        return .invalidPlist(reason)
    }
}
