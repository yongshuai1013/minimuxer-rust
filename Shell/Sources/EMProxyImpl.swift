//
//  EMProxyImpl.swift
//  Minimuxer
//
//  EMProxyAPI conformance for the Rust-backed shell. The proxy and
//  its WireGuard handshake probing run in the Rust core
//  (mm-emproxy, em_proxy crate); an unset handshake client is
//  reported by the core as EMProxyError.handshakeClientNotConfigured
//  (equivalence audit H3), so this is a pure forward.
//

import Foundation
import MinimuxerRustBindings

final class MinimuxerEMProxyImpl: EMProxyAPI {
    func setHandshakeClient(host: String, port: UInt16, enabled: Bool) {
        mm_emproxy_set_handshake_client(host, port, enabled)
    }

    func start(host: String, port: UInt16) async throws {
        let outcome = await mm_emproxy_start(host, port)
        guard outcome.success else { throw mmEMProxyError(from: outcome.error) }
    }

    func stop() async throws {
        let outcome = await mm_emproxy_stop()
        guard outcome.success else { throw mmEMProxyError(from: outcome.error) }
    }
}
