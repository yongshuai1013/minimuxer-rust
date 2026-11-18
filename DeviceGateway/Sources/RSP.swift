//
//  RSP.swift
//  DeviceGatewayAPI
//
//  RSP reply value types, carried over verbatim from the Swift
//  package's BaseDeviceGateway.swift (public module surface).
//

import Foundation

public struct RSPStopReply: Codable, Sendable {
    public let rawPacket: String
    public let signal: UInt8?
    public let threadId: String?
    public let registers: [String: String]
    public let metadata: [String: String]

    public init?(rawPacket: String) {
        guard rawPacket.hasPrefix("T") else { return nil }
        self.rawPacket = rawPacket

        let hexSig = rawPacket.dropFirst().prefix(2)
        self.signal = UInt8(hexSig, radix: 16)

        var parsedThreadId: String?
        var parsedRegs: [String: String] = [:]
        var parsedMeta: [String: String] = [:]

        let body = rawPacket.dropFirst(1 + hexSig.count)
        let pairs = body.split(separator: ";")
        for pair in pairs {
            let parts = pair.split(separator: ":", maxSplits: 1)
            guard parts.count == 2 else { continue }
            let key = String(parts[0])
            let val = String(parts[1])

            if key == "thread" {
                parsedThreadId = val
            } else if key.allSatisfy({ $0.isHexDigit }) || ["pc", "sp", "fp", "lr", "cpsr"].contains(key) {
                parsedRegs[key] = val
            } else {
                parsedMeta[key] = val
            }
        }

        self.threadId = parsedThreadId
        self.registers = parsedRegs
        self.metadata = parsedMeta
    }

    public var jsonPrettyPrinted: String? {
        guard let data = try? JSONEncoder().encode(self),
              let obj = try? JSONSerialization.jsonObject(with: data),
              let prettyData = try? JSONSerialization.data(withJSONObject: obj, options: [.prettyPrinted, .sortedKeys]) else {
            return nil
        }
        return String(data: prettyData, encoding: .utf8)
    }
}

public struct RSPProcessInfo: Sendable, Equatable, Hashable {
    public let pid: UInt32
    public let name: String
    public let executable: String

    public init?(rawResponse: String) {
        guard !rawResponse.isEmpty, !rawResponse.hasPrefix("E") else { return nil }

        var parsedPid: UInt32?
        var parsedName = ""
        var parsedExecutable = ""

        let pairs = rawResponse.split(separator: ";")
        for pair in pairs {
            let parts = pair.split(separator: ":", maxSplits: 1)
            guard parts.count == 2 else { continue }
            let key = parts[0].trimmingCharacters(in: .whitespaces)
            let val = String(parts[1]).trimmingCharacters(in: .whitespaces)

            switch key {
            case "pid":
                parsedPid = UInt32(val, radix: 16) ?? UInt32(val)
            case "name":
                parsedName = decodeHexASCII(val) ?? val
            case "executable":
                parsedExecutable = decodeHexASCII(val) ?? val
            default:
                break
            }
        }

        guard let pid = parsedPid else { return nil }
        self.pid = pid
        self.name = parsedName
        self.executable = parsedExecutable
    }

    public func matches(term: String) -> Bool {
        name.localizedCaseInsensitiveContains(term) ||
        (!executable.isEmpty && executable.localizedCaseInsensitiveContains(term))
    }
}

// Hex-ASCII decode identical to the Swift original's
// BaseDeviceGateway.decodeHexASCII (kept file-local: the shell has no
// BaseDeviceGateway class; gateway state lives in the Rust core).
func decodeHexASCII(_ hex: String) -> String? {
    let utf8 = hex.utf8
    guard utf8.count % 2 == 0 else { return nil }
    var data = Data(capacity: utf8.count / 2)
    var byte: UInt8 = 0

    for (index, char) in utf8.enumerated() {
        let val: UInt8
        switch char {
        case 48...57:  val = char - 48       // '0'...'9'
        case 65...70:  val = char - 65 + 10  // 'A'...'F'
        case 97...102: val = char - 97 + 10  // 'a'...'f'
        default: return nil
        }

        if index.isMultiple(of: 2) {
            byte = val << 4
        } else {
            data.append(byte | val)
        }
    }
    return String(data: data, encoding: .utf8) ?? String(data: data, encoding: .ascii)
}
