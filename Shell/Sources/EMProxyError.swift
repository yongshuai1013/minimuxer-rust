//
//  EMProxyError.swift
//  Minimuxer
//
//  Error type carried over verbatim from the Swift package's
//  EMProxyImpl.swift. EMProxy operations themselves run in the Rust
//  core (mm-emproxy); failures cross as MmErrorInfo and are rebuilt
//  by the generated table in MmErrorInfo+MinimuxerError.swift.
//

import Foundation

public enum EMProxyError: Error, LocalizedError, CustomStringConvertible, Equatable, Sendable {
    case invalidBindAddressPointer
    case invalidUTF8String
    case invalidSocketAddress(String)
    case socketBindFailed
    case cryptoInitFailed
    case serverNotRunning
    case stopSignalFailed
    case threadJoinFailed
    case handshakeClientNotConfigured
    case unknownError(Int32)

    public var description: String {
        switch self {
        case .invalidBindAddressPointer:
            return "Invalid bind address pointer"
        case .invalidUTF8String:
            return "Failed to convert bind address to UTF-8"
        case .invalidSocketAddress(let addr):
            return "Invalid IPv4 socket address format: \(addr)"
        case .socketBindFailed:
            return "Failed to bind to UDP socket address"
        case .cryptoInitFailed:
            return "Failed to initialize EMProxy crypto keys"
        case .serverNotRunning:
            return "EMProxy server is not running"
        case .stopSignalFailed:
            return "Failed to send stop signal to EMProxy server"
        case .threadJoinFailed:
            return "Failed to join EMProxy loopback thread"
        case .handshakeClientNotConfigured:
            return "EMProxy WireGuard VPN handshake client not configured"
        case .unknownError(let code):
            return "EMProxy error code: \(code)"
        }
    }

    public var errorDescription: String? {
        return description
    }
}
