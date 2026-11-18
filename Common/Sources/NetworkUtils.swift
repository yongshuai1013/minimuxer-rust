//
//  NetworkUtils.swift
//  MinimuxerCommon
//
//  Shell version: the probe itself (non-blocking connect + poll +
//  SO_ERROR, "/" rejection, IPv6 scope handling) is implemented in
//  the Rust core (mm-common probe.rs; equivalence audit L8) and
//  reached via mm_test_tcp. Signature identical to the Swift
//  original.
//

import Foundation
import MinimuxerRustBindings

public enum NetworkUtils {
    // Probes whether a TCP port is open on IPv4 or IPv6 with a timeout
    public static func testTCP(ip: String, port: UInt16, timeoutMs: Int = MinimuxerConstants.defaultTCPProbeTimeoutMs) -> Bool {
        mm_test_tcp(ip, port, Int32(timeoutMs))
    }
}
