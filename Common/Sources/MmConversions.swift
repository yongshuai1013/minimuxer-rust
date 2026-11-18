// MmConversions.swift — MinimuxerCommon
//
// Bridge-enum <-> Swift-enum conversions shared by all shell modules.
// Pure type conversion (DESIGN D3); no decisions live here.

import Foundation
import MinimuxerRustBindings

public extension PairingProtocol {
    var mmPairingProtocol: MmPairingProtocol {
        switch self {
        case .rppairing: return .RpPairing
        case .lockdown: return .Lockdown
        case .unknown: return .Unknown
        }
    }
}

public extension MmPairingProtocol {
    var pairingProtocol: PairingProtocol {
        switch self {
        case .RpPairing: return .rppairing
        case .Lockdown: return .lockdown
        case .Unknown: return .unknown
        }
    }
}
