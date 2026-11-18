// GENERATED from crates/mm-common/error-map.toml by
// Scripts/generate-error-table.py -- do not edit by hand.
// Regenerate: python3 Scripts/generate-error-table.py

import Foundation
import MinimuxerRustBindings

// Domain cases (from error-map.toml): invalidPairingFile, connectionFailed, serviceError, noConnection, notInitialized, deviceEndpointIpNotAvailable, invalidTargetEndpoint, unsupportedOperation
/// Rebuild a `DeviceGatewayError` from its FFI representation.
public func mmDeviceGatewayError(from info: MmErrorInfo) -> DeviceGatewayError {
    DeviceGatewayError(DeviceGatewayError.Code(info.code.toString()), reason: info.reason.toString())
}
