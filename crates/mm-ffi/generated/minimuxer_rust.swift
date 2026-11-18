public func mm_version() -> RustString {
    RustString(ptr: __swift_bridge__$mm_version())
}
public func mm_parse_pairing_file<GenericIntoRustString: IntoRustString>(_ content: GenericIntoRustString, _ preferred: Optional<MmPairingProtocol>) -> MmPairingOutcome {
    __swift_bridge__$mm_parse_pairing_file({ let rustString = content.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), __swift_bridge__$Option$MmPairingProtocol.fromSwiftRepr(preferred)).intoSwiftRepr()
}
public func mm_describe_error(_ info: MmErrorInfo) -> RustString {
    RustString(ptr: __swift_bridge__$mm_describe_error(info.intoFfiRepr()))
}
public func mm_test_tcp<GenericIntoRustString: IntoRustString>(_ ip: GenericIntoRustString, _ port: UInt16, _ timeout_ms: Int32) -> Bool {
    __swift_bridge__$mm_test_tcp({ let rustString = ip.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), port, timeout_ms)
}
public func mm_async_ping() async -> RustString {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: UnsafeMutableRawPointer?) {
        let wrapper = Unmanaged<CbWrapper$mm_async_ping>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(RustString(ptr: rustFnRetVal!)))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<RustString, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_async_ping(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_async_ping(wrapperPtr, onComplete)
    })
}
class CbWrapper$mm_async_ping {
    var cb: (Result<RustString, Never>) -> ()

    public init(cb: @escaping (Result<RustString, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_register_event_sink(_ sink: MmEventSink) {
    __swift_bridge__$mm_register_event_sink(Unmanaged.passRetained(sink).toOpaque())
}
public func mm_clear_event_sink() {
    __swift_bridge__$mm_clear_event_sink()
}
public func mm_emit_log<GenericIntoRustString: IntoRustString>(_ message: GenericIntoRustString) {
    __swift_bridge__$mm_emit_log({ let rustString = message.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
}
public func mm_emit_status<GenericIntoRustString: IntoRustString>(_ success: Bool, _ domain: GenericIntoRustString, _ code: GenericIntoRustString, _ reason: GenericIntoRustString) {
    __swift_bridge__$mm_emit_status(success, { let rustString = domain.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), { let rustString = code.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), { let rustString = reason.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
}
public func mm_net_scan_interfaces() -> RustVec<MmInterfaceInfo> {
    RustVec(ptr: __swift_bridge__$mm_net_scan_interfaces())
}
public func mm_net_snapshot() -> MmNetSnapshot {
    __swift_bridge__$mm_net_snapshot().intoSwiftRepr()
}
public func mm_net_endpoint() -> MmEndpointOutcome {
    __swift_bridge__$mm_net_endpoint().intoSwiftRepr()
}
public func mm_net_connection_mode() -> MmConnectionMode {
    __swift_bridge__$mm_net_connection_mode().intoSwiftRepr()
}
public func mm_net_refresh() async -> Bool {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: Bool) {
        let wrapper = Unmanaged<CbWrapper$mm_net_refresh>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<Bool, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_net_refresh(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_net_refresh(wrapperPtr, onComplete)
    })
}
class CbWrapper$mm_net_refresh {
    var cb: (Result<Bool, Never>) -> ()

    public init(cb: @escaping (Result<Bool, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_net_observer_start() -> Bool {
    __swift_bridge__$mm_net_observer_start()
}
public func mm_net_observer_stop() -> Bool {
    __swift_bridge__$mm_net_observer_stop()
}
public func mm_net_bind_connection_config(_ config: MmConnectionConfig) {
    __swift_bridge__$mm_net_bind_connection_config(Unmanaged.passRetained(config).toOpaque())
}
public func mm_net_clear_connection_config() {
    __swift_bridge__$mm_net_clear_connection_config()
}
public func mm_net_set_probe_timeout(_ timeout_ms: Int32) {
    __swift_bridge__$mm_net_set_probe_timeout(timeout_ms)
}
public func mm_net_complete_service_port(_ request_id: UInt64, _ protocol: MmPairingProtocol, _ port: UInt16) -> Bool {
    __swift_bridge__$mm_net_complete_service_port(request_id, protocol.intoFfiRepr(), port)
}
public func mm_gateway_backend() -> MmGatewayBackend {
    __swift_bridge__$mm_gateway_backend().intoSwiftRepr()
}
public func mm_gateway_set_backend(_ backend: MmGatewayBackend) {
    __swift_bridge__$mm_gateway_set_backend(backend.intoFfiRepr())
}
public func mm_gateway_requires_usbmuxd() -> Bool {
    __swift_bridge__$mm_gateway_requires_usbmuxd()
}
public func mm_gateway_start<GenericIntoRustString: IntoRustString>(_ content: GenericIntoRustString, _ preferred: Optional<MmPairingProtocol>) async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_gateway_start>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_gateway_start(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_gateway_start(wrapperPtr, onComplete, { let rustString = content.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), __swift_bridge__$Option$MmPairingProtocol.fromSwiftRepr(preferred))
    })
}
class CbWrapper$mm_gateway_start {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_gateway_stop() async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_gateway_stop>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_gateway_stop(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_gateway_stop(wrapperPtr, onComplete)
    })
}
class CbWrapper$mm_gateway_stop {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_gateway_set_endpoint_ip<GenericIntoRustString: IntoRustString>(_ ip: Optional<GenericIntoRustString>) {
    __swift_bridge__$mm_gateway_set_endpoint_ip({ if let rustString = optionalStringIntoRustString(ip) { rustString.isOwned = false; return rustString.ptr } else { return nil } }())
}
public func mm_gateway_pairing_protocol() -> MmPairingProtocol {
    __swift_bridge__$mm_gateway_pairing_protocol().intoSwiftRepr()
}
public func mm_gateway_service_port(_ protocol: MmPairingProtocol) -> UInt16 {
    __swift_bridge__$mm_gateway_service_port(protocol.intoFfiRepr())
}
public func mm_gateway_set_port(_ port: UInt16, _ protocol: MmPairingProtocol) {
    __swift_bridge__$mm_gateway_set_port(port, protocol.intoFfiRepr())
}
public func mm_gateway_fetch_udid() async -> MmStringOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmStringOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_gateway_fetch_udid>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmStringOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_gateway_fetch_udid(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_gateway_fetch_udid(wrapperPtr, onComplete)
    })
}
class CbWrapper$mm_gateway_fetch_udid {
    var cb: (Result<MmStringOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmStringOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_gateway_get_lockdown_value<GenericIntoRustString: IntoRustString>(_ key: GenericIntoRustString) async -> MmStringOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmStringOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_gateway_get_lockdown_value>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmStringOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_gateway_get_lockdown_value(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_gateway_get_lockdown_value(wrapperPtr, onComplete, { let rustString = key.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_gateway_get_lockdown_value {
    var cb: (Result<MmStringOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmStringOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_gateway_start_session() async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_gateway_start_session>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_gateway_start_session(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_gateway_start_session(wrapperPtr, onComplete)
    })
}
class CbWrapper$mm_gateway_start_session {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_gateway_stop_session() async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_gateway_stop_session>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_gateway_stop_session(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_gateway_stop_session(wrapperPtr, onComplete)
    })
}
class CbWrapper$mm_gateway_stop_session {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_gateway_open_service<GenericIntoRustString: IntoRustString>(_ service_name: GenericIntoRustString) async -> MmServiceOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmServiceOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_gateway_open_service>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmServiceOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_gateway_open_service(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_gateway_open_service(wrapperPtr, onComplete, { let rustString = service_name.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_gateway_open_service {
    var cb: (Result<MmServiceOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmServiceOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_gateway_set_logging(_ enabled: Bool) {
    __swift_bridge__$mm_gateway_set_logging(enabled)
}
public func mm_usbmuxd_retarget() {
    __swift_bridge__$mm_usbmuxd_retarget()
}
public func mm_proxy_start() async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_proxy_start>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_proxy_start(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_proxy_start(wrapperPtr, onComplete)
    })
}
class CbWrapper$mm_proxy_start {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_proxy_stop() async {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?) {
        let wrapper = Unmanaged<CbWrapper$mm_proxy_stop>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<(), Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_proxy_stop(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_proxy_stop(wrapperPtr, onComplete)
    })
}
class CbWrapper$mm_proxy_stop {
    var cb: (Result<(), Never>) -> ()

    public init(cb: @escaping (Result<(), Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_proxy_is_listening() -> Bool {
    __swift_bridge__$mm_proxy_is_listening()
}
public func mm_proxy_notify_attached<GenericIntoRustString: IntoRustString>(_ peer_ip: GenericIntoRustString) {
    __swift_bridge__$mm_proxy_notify_attached({ let rustString = peer_ip.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
}
public func mm_proxy_notify_detached() {
    __swift_bridge__$mm_proxy_notify_detached()
}
public func mm_core_start<GenericIntoRustString: IntoRustString>(_ content: GenericIntoRustString, _ docs_path: GenericIntoRustString, _ preferred: Optional<MmPairingProtocol>) async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_start>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_start(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_start(wrapperPtr, onComplete, { let rustString = content.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), { let rustString = docs_path.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), __swift_bridge__$Option$MmPairingProtocol.fromSwiftRepr(preferred))
    })
}
class CbWrapper$mm_core_start {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_stop() async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_stop>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_stop(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_stop(wrapperPtr, onComplete)
    })
}
class CbWrapper$mm_core_stop {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_restart() async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_restart>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_restart(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_restart(wrapperPtr, onComplete)
    })
}
class CbWrapper$mm_core_restart {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_reinitialize_pairing_data<GenericIntoRustString: IntoRustString>(_ content: GenericIntoRustString) async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_reinitialize_pairing_data>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_reinitialize_pairing_data(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_reinitialize_pairing_data(wrapperPtr, onComplete, { let rustString = content.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_core_reinitialize_pairing_data {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_bind_connection_config(_ config: MmConnectionConfig) -> MmOutcome {
    __swift_bridge__$mm_core_bind_connection_config(Unmanaged.passRetained(config).toOpaque()).intoSwiftRepr()
}
public func mm_core_is_ready(_ with_network_check: Bool, _ with_ddi_mount_check: Bool) async -> MmReadyOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmReadyOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_is_ready>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmReadyOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_is_ready(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_is_ready(wrapperPtr, onComplete, with_network_check, with_ddi_mount_check)
    })
}
class CbWrapper$mm_core_is_ready {
    var cb: (Result<MmReadyOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmReadyOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_fetch_udid() async -> MmStringOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmStringOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_fetch_udid>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmStringOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_fetch_udid(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_fetch_udid(wrapperPtr, onComplete)
    })
}
class CbWrapper$mm_core_fetch_udid {
    var cb: (Result<MmStringOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmStringOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_is_ddi_mounted() async -> MmReadyOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmReadyOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_is_ddi_mounted>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmReadyOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_is_ddi_mounted(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_is_ddi_mounted(wrapperPtr, onComplete)
    })
}
class CbWrapper$mm_core_is_ddi_mounted {
    var cb: (Result<MmReadyOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmReadyOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_mount_ddi<GenericIntoRustString: IntoRustString>(_ docs_path: GenericIntoRustString) async -> MmReadyOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmReadyOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_mount_ddi>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmReadyOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_mount_ddi(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_mount_ddi(wrapperPtr, onComplete, { let rustString = docs_path.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_core_mount_ddi {
    var cb: (Result<MmReadyOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmReadyOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_send_ipa_afc<GenericIntoRustString: IntoRustString>(_ bundle_id: GenericIntoRustString, _ ipa: RustVec<UInt8>) async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_send_ipa_afc>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_send_ipa_afc(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_send_ipa_afc(wrapperPtr, onComplete, { let rustString = bundle_id.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), { let val = ipa; val.isOwned = false; return val.ptr }())
    })
}
class CbWrapper$mm_core_send_ipa_afc {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_send_app_bundle_afc<GenericIntoRustString: IntoRustString>(_ bundle_id: GenericIntoRustString, _ app_path: GenericIntoRustString) async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_send_app_bundle_afc>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_send_app_bundle_afc(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_send_app_bundle_afc(wrapperPtr, onComplete, { let rustString = bundle_id.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), { let rustString = app_path.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_core_send_app_bundle_afc {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_install_ipa<GenericIntoRustString: IntoRustString>(_ bundle_id: GenericIntoRustString) async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_install_ipa>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_install_ipa(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_install_ipa(wrapperPtr, onComplete, { let rustString = bundle_id.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_core_install_ipa {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_install_app_bundle<GenericIntoRustString: IntoRustString>(_ bundle_id: GenericIntoRustString, _ app_name: GenericIntoRustString) async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_install_app_bundle>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_install_app_bundle(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_install_app_bundle(wrapperPtr, onComplete, { let rustString = bundle_id.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), { let rustString = app_name.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_core_install_app_bundle {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_remove_app<GenericIntoRustString: IntoRustString>(_ bundle_id: GenericIntoRustString) async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_remove_app>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_remove_app(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_remove_app(wrapperPtr, onComplete, { let rustString = bundle_id.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_core_remove_app {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_wipe_container<GenericIntoRustString: IntoRustString>(_ bundle_id: GenericIntoRustString) async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_wipe_container>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_wipe_container(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_wipe_container(wrapperPtr, onComplete, { let rustString = bundle_id.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_core_wipe_container {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_debug_app<GenericIntoRustString: IntoRustString>(_ bundle_id: GenericIntoRustString) async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_debug_app>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_debug_app(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_debug_app(wrapperPtr, onComplete, { let rustString = bundle_id.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_core_debug_app {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_attach_debugger(_ pid: UInt32) async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_attach_debugger>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_attach_debugger(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_attach_debugger(wrapperPtr, onComplete, pid)
    })
}
class CbWrapper$mm_core_attach_debugger {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_install_profile(_ profile: RustVec<UInt8>) async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_install_profile>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_install_profile(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_install_profile(wrapperPtr, onComplete, { let val = profile; val.isOwned = false; return val.ptr }())
    })
}
class CbWrapper$mm_core_install_profile {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_remove_profile<GenericIntoRustString: IntoRustString>(_ profile_id: GenericIntoRustString) async -> MmOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_remove_profile>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_remove_profile(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_remove_profile(wrapperPtr, onComplete, { let rustString = profile_id.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_core_remove_profile {
    var cb: (Result<MmOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_dump_profiles<GenericIntoRustString: IntoRustString>(_ docs_path: GenericIntoRustString, _ mode: MmProfileDumpMode) async -> MmStringOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmStringOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_dump_profiles>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmStringOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_dump_profiles(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_dump_profiles(wrapperPtr, onComplete, { let rustString = docs_path.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), mode.intoFfiRepr())
    })
}
class CbWrapper$mm_core_dump_profiles {
    var cb: (Result<MmStringOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmStringOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_afc_list_directory<GenericIntoRustString: IntoRustString>(_ bundle_id: GenericIntoRustString, _ path: GenericIntoRustString) async -> MmListOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmListOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_afc_list_directory>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmListOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_afc_list_directory(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_afc_list_directory(wrapperPtr, onComplete, { let rustString = bundle_id.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), { let rustString = path.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_core_afc_list_directory {
    var cb: (Result<MmListOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmListOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_afc_read_file<GenericIntoRustString: IntoRustString>(_ bundle_id: GenericIntoRustString, _ path: GenericIntoRustString) async -> MmDataOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmDataOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_afc_read_file>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmDataOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_afc_read_file(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_afc_read_file(wrapperPtr, onComplete, { let rustString = bundle_id.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), { let rustString = path.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_core_afc_read_file {
    var cb: (Result<MmDataOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmDataOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_afc_get_file_info<GenericIntoRustString: IntoRustString>(_ bundle_id: GenericIntoRustString, _ path: GenericIntoRustString) async -> MmAfcFileInfoOutcome {
    func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?, rustFnRetVal: __swift_bridge__$MmAfcFileInfoOutcome) {
        let wrapper = Unmanaged<CbWrapper$mm_core_afc_get_file_info>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
        wrapper.cb(.success(rustFnRetVal.intoSwiftRepr()))
    }

    return await withCheckedContinuation({ (continuation: CheckedContinuation<MmAfcFileInfoOutcome, Never>) in
        let callback = { rustFnRetVal in
            continuation.resume(with: rustFnRetVal)
        }

        let wrapper = CbWrapper$mm_core_afc_get_file_info(cb: callback)
        let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

        __swift_bridge__$mm_core_afc_get_file_info(wrapperPtr, onComplete, { let rustString = bundle_id.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), { let rustString = path.intoRustString(); rustString.isOwned = false; return rustString.ptr }())
    })
}
class CbWrapper$mm_core_afc_get_file_info {
    var cb: (Result<MmAfcFileInfoOutcome, Never>) -> ()

    public init(cb: @escaping (Result<MmAfcFileInfoOutcome, Never>) -> ()) {
        self.cb = cb
    }
}
public func mm_core_set_params(_ backend: Optional<MmGatewayBackend>, _ remote_pairing_port: Optional<UInt16>, _ device_probe_timeout: Optional<Int32>) {
    __swift_bridge__$mm_core_set_params(__swift_bridge__$Option$MmGatewayBackend.fromSwiftRepr(backend), remote_pairing_port.intoFfiRepr(), device_probe_timeout.intoFfiRepr())
}
public func mm_core_status() -> MmCoreStatus {
    __swift_bridge__$mm_core_status().intoSwiftRepr()
}
public func mm_core_set_logging(_ enabled: Bool) {
    __swift_bridge__$mm_core_set_logging(enabled)
}
public func mm_core_is_pairing_file_loaded() -> Bool {
    __swift_bridge__$mm_core_is_pairing_file_loaded()
}
public func mm_core_last_beat_successful() -> Bool {
    __swift_bridge__$mm_core_last_beat_successful()
}
@_cdecl("__swift_bridge__$MmEventSink$on_log")
func __swift_bridge__MmEventSink_on_log (_ this: UnsafeMutableRawPointer, _ message: UnsafeMutableRawPointer) {
    Unmanaged<MmEventSink>.fromOpaque(this).takeUnretainedValue().on_log(message: RustString(ptr: message))
}

@_cdecl("__swift_bridge__$MmEventSink$on_status")
func __swift_bridge__MmEventSink_on_status (_ this: UnsafeMutableRawPointer, _ seq: UInt64, _ success: Bool, _ domain: UnsafeMutableRawPointer, _ code: UnsafeMutableRawPointer, _ reason: UnsafeMutableRawPointer) {
    Unmanaged<MmEventSink>.fromOpaque(this).takeUnretainedValue().on_status(seq: seq, success: success, domain: RustString(ptr: domain), code: RustString(ptr: code), reason: RustString(ptr: reason))
}

@_cdecl("__swift_bridge__$MmConnectionConfig$set_tunnel_iface_ip")
func __swift_bridge__MmConnectionConfig_set_tunnel_iface_ip (_ this: UnsafeMutableRawPointer, _ ip: UnsafeMutableRawPointer?) {
    Unmanaged<MmConnectionConfig>.fromOpaque(this).takeUnretainedValue().set_tunnel_iface_ip(ip: { let val = ip; if val != nil { return RustString(ptr: val!) } else { return nil } }())
}

@_cdecl("__swift_bridge__$MmConnectionConfig$set_tunnel_peer_ip")
func __swift_bridge__MmConnectionConfig_set_tunnel_peer_ip (_ this: UnsafeMutableRawPointer, _ ip: UnsafeMutableRawPointer?) {
    Unmanaged<MmConnectionConfig>.fromOpaque(this).takeUnretainedValue().set_tunnel_peer_ip(ip: { let val = ip; if val != nil { return RustString(ptr: val!) } else { return nil } }())
}

@_cdecl("__swift_bridge__$MmConnectionConfig$set_tunnel_peer_subnet_mask")
func __swift_bridge__MmConnectionConfig_set_tunnel_peer_subnet_mask (_ this: UnsafeMutableRawPointer, _ mask: UnsafeMutableRawPointer?) {
    Unmanaged<MmConnectionConfig>.fromOpaque(this).takeUnretainedValue().set_tunnel_peer_subnet_mask(mask: { let val = mask; if val != nil { return RustString(ptr: val!) } else { return nil } }())
}

@_cdecl("__swift_bridge__$MmConnectionConfig$set_tunnel_peer_reachable")
func __swift_bridge__MmConnectionConfig_set_tunnel_peer_reachable (_ this: UnsafeMutableRawPointer, _ reachable: Bool) {
    Unmanaged<MmConnectionConfig>.fromOpaque(this).takeUnretainedValue().set_tunnel_peer_reachable(reachable: reachable)
}

@_cdecl("__swift_bridge__$MmConnectionConfig$set_tunnel_iface_subnet_mask")
func __swift_bridge__MmConnectionConfig_set_tunnel_iface_subnet_mask (_ this: UnsafeMutableRawPointer, _ mask: UnsafeMutableRawPointer?) {
    Unmanaged<MmConnectionConfig>.fromOpaque(this).takeUnretainedValue().set_tunnel_iface_subnet_mask(mask: { let val = mask; if val != nil { return RustString(ptr: val!) } else { return nil } }())
}

@_cdecl("__swift_bridge__$MmConnectionConfig$set_remote_reachable")
func __swift_bridge__MmConnectionConfig_set_remote_reachable (_ this: UnsafeMutableRawPointer, _ reachable: Bool) {
    Unmanaged<MmConnectionConfig>.fromOpaque(this).takeUnretainedValue().set_remote_reachable(reachable: reachable)
}

@_cdecl("__swift_bridge__$MmConnectionConfig$set_override_tunnel_peer_reachable")
func __swift_bridge__MmConnectionConfig_set_override_tunnel_peer_reachable (_ this: UnsafeMutableRawPointer, _ reachable: Bool) {
    Unmanaged<MmConnectionConfig>.fromOpaque(this).takeUnretainedValue().set_override_tunnel_peer_reachable(reachable: reachable)
}

@_cdecl("__swift_bridge__$MmConnectionConfig$get_connection_mode")
func __swift_bridge__MmConnectionConfig_get_connection_mode (_ this: UnsafeMutableRawPointer) -> __swift_bridge__$MmConnectionMode {
    Unmanaged<MmConnectionConfig>.fromOpaque(this).takeUnretainedValue().get_connection_mode().intoFfiRepr()
}

@_cdecl("__swift_bridge__$MmConnectionConfig$get_override_tunnel_peer_ip")
func __swift_bridge__MmConnectionConfig_get_override_tunnel_peer_ip (_ this: UnsafeMutableRawPointer) -> UnsafeMutableRawPointer {
    { let rustString = Unmanaged<MmConnectionConfig>.fromOpaque(this).takeUnretainedValue().get_override_tunnel_peer_ip().intoRustString(); rustString.isOwned = false; return rustString.ptr }()
}

@_cdecl("__swift_bridge__$MmConnectionConfig$get_remote_server_ip")
func __swift_bridge__MmConnectionConfig_get_remote_server_ip (_ this: UnsafeMutableRawPointer) -> UnsafeMutableRawPointer {
    { let rustString = Unmanaged<MmConnectionConfig>.fromOpaque(this).takeUnretainedValue().get_remote_server_ip().intoRustString(); rustString.isOwned = false; return rustString.ptr }()
}

@_cdecl("__swift_bridge__$MmConnectionConfig$request_service_port")
func __swift_bridge__MmConnectionConfig_request_service_port (_ this: UnsafeMutableRawPointer, _ request_id: UInt64, _ protocol: __swift_bridge__$MmPairingProtocol, _ port: UInt16) {
    Unmanaged<MmConnectionConfig>.fromOpaque(this).takeUnretainedValue().request_service_port(request_id: request_id, protocol: protocol.intoSwiftRepr(), port: port)
}

public struct MmErrorInfo {
    public var domain: RustString
    public var code: RustString
    public var reason: RustString

    public init(domain: RustString,code: RustString,reason: RustString) {
        self.domain = domain
        self.code = code
        self.reason = reason
    }

    @inline(__always)
    func intoFfiRepr() -> __swift_bridge__$MmErrorInfo {
        { let val = self; return __swift_bridge__$MmErrorInfo(domain: { let rustString = val.domain.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), code: { let rustString = val.code.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), reason: { let rustString = val.reason.intoRustString(); rustString.isOwned = false; return rustString.ptr }()); }()
    }
}
extension __swift_bridge__$MmErrorInfo {
    @inline(__always)
    func intoSwiftRepr() -> MmErrorInfo {
        { let val = self; return MmErrorInfo(domain: RustString(ptr: val.domain), code: RustString(ptr: val.code), reason: RustString(ptr: val.reason)); }()
    }
}
extension __swift_bridge__$Option$MmErrorInfo {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmErrorInfo> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }

    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmErrorInfo>) -> __swift_bridge__$Option$MmErrorInfo {
        if let v = val {
            return __swift_bridge__$Option$MmErrorInfo(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmErrorInfo(is_some: false, val: __swift_bridge__$MmErrorInfo())
        }
    }
}
public enum MmPairingProtocol {
    case RpPairing
    case Lockdown
    case Unknown
}
extension MmPairingProtocol {
    func intoFfiRepr() -> __swift_bridge__$MmPairingProtocol {
        switch self {
            case MmPairingProtocol.RpPairing:
                return __swift_bridge__$MmPairingProtocol(tag: __swift_bridge__$MmPairingProtocol$RpPairing)
            case MmPairingProtocol.Lockdown:
                return __swift_bridge__$MmPairingProtocol(tag: __swift_bridge__$MmPairingProtocol$Lockdown)
            case MmPairingProtocol.Unknown:
                return __swift_bridge__$MmPairingProtocol(tag: __swift_bridge__$MmPairingProtocol$Unknown)
        }
    }
}
extension __swift_bridge__$MmPairingProtocol {
    func intoSwiftRepr() -> MmPairingProtocol {
        switch self.tag {
            case __swift_bridge__$MmPairingProtocol$RpPairing:
                return MmPairingProtocol.RpPairing
            case __swift_bridge__$MmPairingProtocol$Lockdown:
                return MmPairingProtocol.Lockdown
            case __swift_bridge__$MmPairingProtocol$Unknown:
                return MmPairingProtocol.Unknown
            default:
                fatalError("Unreachable")
        }
    }
}
extension __swift_bridge__$Option$MmPairingProtocol {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmPairingProtocol> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }
    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmPairingProtocol>) -> __swift_bridge__$Option$MmPairingProtocol {
        if let v = val {
            return __swift_bridge__$Option$MmPairingProtocol(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmPairingProtocol(is_some: false, val: __swift_bridge__$MmPairingProtocol())
        }
    }
}
extension MmPairingProtocol: Vectorizable {
    public static func vecOfSelfNew() -> UnsafeMutableRawPointer {
        __swift_bridge__$Vec_MmPairingProtocol$new()
    }

    public static func vecOfSelfFree(vecPtr: UnsafeMutableRawPointer) {
        __swift_bridge__$Vec_MmPairingProtocol$drop(vecPtr)
    }

    public static func vecOfSelfPush(vecPtr: UnsafeMutableRawPointer, value: Self) {
        __swift_bridge__$Vec_MmPairingProtocol$push(vecPtr, value.intoFfiRepr())
    }

    public static func vecOfSelfPop(vecPtr: UnsafeMutableRawPointer) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmPairingProtocol$pop(vecPtr)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfGet(vecPtr: UnsafeMutableRawPointer, index: UInt) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmPairingProtocol$get(vecPtr, index)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfGetMut(vecPtr: UnsafeMutableRawPointer, index: UInt) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmPairingProtocol$get_mut(vecPtr, index)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfAsPtr(vecPtr: UnsafeMutableRawPointer) -> UnsafePointer<Self> {
        UnsafePointer<Self>(OpaquePointer(__swift_bridge__$Vec_MmPairingProtocol$as_ptr(vecPtr)))
    }

    public static func vecOfSelfLen(vecPtr: UnsafeMutableRawPointer) -> UInt {
        __swift_bridge__$Vec_MmPairingProtocol$len(vecPtr)
    }
}
public struct MmPairingOutcome {
    public var success: Bool
    public var protocol: MmPairingProtocol
    public var error: MmErrorInfo

    public init(success: Bool,protocol: MmPairingProtocol,error: MmErrorInfo) {
        self.success = success
        self.protocol = protocol
        self.error = error
    }

    @inline(__always)
    func intoFfiRepr() -> __swift_bridge__$MmPairingOutcome {
        { let val = self; return __swift_bridge__$MmPairingOutcome(success: val.success, protocol: val.protocol.intoFfiRepr(), error: val.error.intoFfiRepr()); }()
    }
}
extension __swift_bridge__$MmPairingOutcome {
    @inline(__always)
    func intoSwiftRepr() -> MmPairingOutcome {
        { let val = self; return MmPairingOutcome(success: val.success, protocol: val.protocol.intoSwiftRepr(), error: val.error.intoSwiftRepr()); }()
    }
}
extension __swift_bridge__$Option$MmPairingOutcome {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmPairingOutcome> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }

    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmPairingOutcome>) -> __swift_bridge__$Option$MmPairingOutcome {
        if let v = val {
            return __swift_bridge__$Option$MmPairingOutcome(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmPairingOutcome(is_some: false, val: __swift_bridge__$MmPairingOutcome())
        }
    }
}
public enum MmConnectionMode {
    case LocalVpn
    case RemoteServer
    case NotConfigured
}
extension MmConnectionMode {
    func intoFfiRepr() -> __swift_bridge__$MmConnectionMode {
        switch self {
            case MmConnectionMode.LocalVpn:
                return __swift_bridge__$MmConnectionMode(tag: __swift_bridge__$MmConnectionMode$LocalVpn)
            case MmConnectionMode.RemoteServer:
                return __swift_bridge__$MmConnectionMode(tag: __swift_bridge__$MmConnectionMode$RemoteServer)
            case MmConnectionMode.NotConfigured:
                return __swift_bridge__$MmConnectionMode(tag: __swift_bridge__$MmConnectionMode$NotConfigured)
        }
    }
}
extension __swift_bridge__$MmConnectionMode {
    func intoSwiftRepr() -> MmConnectionMode {
        switch self.tag {
            case __swift_bridge__$MmConnectionMode$LocalVpn:
                return MmConnectionMode.LocalVpn
            case __swift_bridge__$MmConnectionMode$RemoteServer:
                return MmConnectionMode.RemoteServer
            case __swift_bridge__$MmConnectionMode$NotConfigured:
                return MmConnectionMode.NotConfigured
            default:
                fatalError("Unreachable")
        }
    }
}
extension __swift_bridge__$Option$MmConnectionMode {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmConnectionMode> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }
    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmConnectionMode>) -> __swift_bridge__$Option$MmConnectionMode {
        if let v = val {
            return __swift_bridge__$Option$MmConnectionMode(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmConnectionMode(is_some: false, val: __swift_bridge__$MmConnectionMode())
        }
    }
}
extension MmConnectionMode: Vectorizable {
    public static func vecOfSelfNew() -> UnsafeMutableRawPointer {
        __swift_bridge__$Vec_MmConnectionMode$new()
    }

    public static func vecOfSelfFree(vecPtr: UnsafeMutableRawPointer) {
        __swift_bridge__$Vec_MmConnectionMode$drop(vecPtr)
    }

    public static func vecOfSelfPush(vecPtr: UnsafeMutableRawPointer, value: Self) {
        __swift_bridge__$Vec_MmConnectionMode$push(vecPtr, value.intoFfiRepr())
    }

    public static func vecOfSelfPop(vecPtr: UnsafeMutableRawPointer) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmConnectionMode$pop(vecPtr)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfGet(vecPtr: UnsafeMutableRawPointer, index: UInt) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmConnectionMode$get(vecPtr, index)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfGetMut(vecPtr: UnsafeMutableRawPointer, index: UInt) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmConnectionMode$get_mut(vecPtr, index)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfAsPtr(vecPtr: UnsafeMutableRawPointer) -> UnsafePointer<Self> {
        UnsafePointer<Self>(OpaquePointer(__swift_bridge__$Vec_MmConnectionMode$as_ptr(vecPtr)))
    }

    public static func vecOfSelfLen(vecPtr: UnsafeMutableRawPointer) -> UInt {
        __swift_bridge__$Vec_MmConnectionMode$len(vecPtr)
    }
}
public struct MmInterfaceInfo {
    public var name: RustString
    public var ip: RustString
    public var ipv6: RustString
    public var subnet: RustString
    public var interface_type: RustString
    public var is_vpn: Bool

    public init(name: RustString,ip: RustString,ipv6: RustString,subnet: RustString,interface_type: RustString,is_vpn: Bool) {
        self.name = name
        self.ip = ip
        self.ipv6 = ipv6
        self.subnet = subnet
        self.interface_type = interface_type
        self.is_vpn = is_vpn
    }

    @inline(__always)
    func intoFfiRepr() -> __swift_bridge__$MmInterfaceInfo {
        { let val = self; return __swift_bridge__$MmInterfaceInfo(name: { let rustString = val.name.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), ip: { let rustString = val.ip.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), ipv6: { let rustString = val.ipv6.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), subnet: { let rustString = val.subnet.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), interface_type: { let rustString = val.interface_type.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), is_vpn: val.is_vpn); }()
    }
}
extension __swift_bridge__$MmInterfaceInfo {
    @inline(__always)
    func intoSwiftRepr() -> MmInterfaceInfo {
        { let val = self; return MmInterfaceInfo(name: RustString(ptr: val.name), ip: RustString(ptr: val.ip), ipv6: RustString(ptr: val.ipv6), subnet: RustString(ptr: val.subnet), interface_type: RustString(ptr: val.interface_type), is_vpn: val.is_vpn); }()
    }
}
extension __swift_bridge__$Option$MmInterfaceInfo {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmInterfaceInfo> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }

    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmInterfaceInfo>) -> __swift_bridge__$Option$MmInterfaceInfo {
        if let v = val {
            return __swift_bridge__$Option$MmInterfaceInfo(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmInterfaceInfo(is_some: false, val: __swift_bridge__$MmInterfaceInfo())
        }
    }
}
public struct MmNetSnapshot {
    public var interfaces: RustVec<MmInterfaceInfo>
    public var wifi_satisfied: Bool
    public var wired_satisfied: Bool
    public var usb_satisfied: Bool
    public var bridge_satisfied: Bool
    public var utun_available: Bool
    public var ipsec_available: Bool

    public init(interfaces: RustVec<MmInterfaceInfo>,wifi_satisfied: Bool,wired_satisfied: Bool,usb_satisfied: Bool,bridge_satisfied: Bool,utun_available: Bool,ipsec_available: Bool) {
        self.interfaces = interfaces
        self.wifi_satisfied = wifi_satisfied
        self.wired_satisfied = wired_satisfied
        self.usb_satisfied = usb_satisfied
        self.bridge_satisfied = bridge_satisfied
        self.utun_available = utun_available
        self.ipsec_available = ipsec_available
    }

    @inline(__always)
    func intoFfiRepr() -> __swift_bridge__$MmNetSnapshot {
        { let val = self; return __swift_bridge__$MmNetSnapshot(interfaces: { let val = val.interfaces; val.isOwned = false; return val.ptr }(), wifi_satisfied: val.wifi_satisfied, wired_satisfied: val.wired_satisfied, usb_satisfied: val.usb_satisfied, bridge_satisfied: val.bridge_satisfied, utun_available: val.utun_available, ipsec_available: val.ipsec_available); }()
    }
}
extension __swift_bridge__$MmNetSnapshot {
    @inline(__always)
    func intoSwiftRepr() -> MmNetSnapshot {
        { let val = self; return MmNetSnapshot(interfaces: RustVec(ptr: val.interfaces), wifi_satisfied: val.wifi_satisfied, wired_satisfied: val.wired_satisfied, usb_satisfied: val.usb_satisfied, bridge_satisfied: val.bridge_satisfied, utun_available: val.utun_available, ipsec_available: val.ipsec_available); }()
    }
}
extension __swift_bridge__$Option$MmNetSnapshot {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmNetSnapshot> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }

    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmNetSnapshot>) -> __swift_bridge__$Option$MmNetSnapshot {
        if let v = val {
            return __swift_bridge__$Option$MmNetSnapshot(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmNetSnapshot(is_some: false, val: __swift_bridge__$MmNetSnapshot())
        }
    }
}
public struct MmEndpointOutcome {
    public var success: Bool
    public var ip: RustString
    public var error: MmErrorInfo

    public init(success: Bool,ip: RustString,error: MmErrorInfo) {
        self.success = success
        self.ip = ip
        self.error = error
    }

    @inline(__always)
    func intoFfiRepr() -> __swift_bridge__$MmEndpointOutcome {
        { let val = self; return __swift_bridge__$MmEndpointOutcome(success: val.success, ip: { let rustString = val.ip.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), error: val.error.intoFfiRepr()); }()
    }
}
extension __swift_bridge__$MmEndpointOutcome {
    @inline(__always)
    func intoSwiftRepr() -> MmEndpointOutcome {
        { let val = self; return MmEndpointOutcome(success: val.success, ip: RustString(ptr: val.ip), error: val.error.intoSwiftRepr()); }()
    }
}
extension __swift_bridge__$Option$MmEndpointOutcome {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmEndpointOutcome> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }

    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmEndpointOutcome>) -> __swift_bridge__$Option$MmEndpointOutcome {
        if let v = val {
            return __swift_bridge__$Option$MmEndpointOutcome(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmEndpointOutcome(is_some: false, val: __swift_bridge__$MmEndpointOutcome())
        }
    }
}
public enum MmGatewayBackend {
    case Idevice
    case Libimobiledevice
}
extension MmGatewayBackend {
    func intoFfiRepr() -> __swift_bridge__$MmGatewayBackend {
        switch self {
            case MmGatewayBackend.Idevice:
                return __swift_bridge__$MmGatewayBackend(tag: __swift_bridge__$MmGatewayBackend$Idevice)
            case MmGatewayBackend.Libimobiledevice:
                return __swift_bridge__$MmGatewayBackend(tag: __swift_bridge__$MmGatewayBackend$Libimobiledevice)
        }
    }
}
extension __swift_bridge__$MmGatewayBackend {
    func intoSwiftRepr() -> MmGatewayBackend {
        switch self.tag {
            case __swift_bridge__$MmGatewayBackend$Idevice:
                return MmGatewayBackend.Idevice
            case __swift_bridge__$MmGatewayBackend$Libimobiledevice:
                return MmGatewayBackend.Libimobiledevice
            default:
                fatalError("Unreachable")
        }
    }
}
extension __swift_bridge__$Option$MmGatewayBackend {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmGatewayBackend> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }
    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmGatewayBackend>) -> __swift_bridge__$Option$MmGatewayBackend {
        if let v = val {
            return __swift_bridge__$Option$MmGatewayBackend(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmGatewayBackend(is_some: false, val: __swift_bridge__$MmGatewayBackend())
        }
    }
}
extension MmGatewayBackend: Vectorizable {
    public static func vecOfSelfNew() -> UnsafeMutableRawPointer {
        __swift_bridge__$Vec_MmGatewayBackend$new()
    }

    public static func vecOfSelfFree(vecPtr: UnsafeMutableRawPointer) {
        __swift_bridge__$Vec_MmGatewayBackend$drop(vecPtr)
    }

    public static func vecOfSelfPush(vecPtr: UnsafeMutableRawPointer, value: Self) {
        __swift_bridge__$Vec_MmGatewayBackend$push(vecPtr, value.intoFfiRepr())
    }

    public static func vecOfSelfPop(vecPtr: UnsafeMutableRawPointer) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmGatewayBackend$pop(vecPtr)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfGet(vecPtr: UnsafeMutableRawPointer, index: UInt) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmGatewayBackend$get(vecPtr, index)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfGetMut(vecPtr: UnsafeMutableRawPointer, index: UInt) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmGatewayBackend$get_mut(vecPtr, index)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfAsPtr(vecPtr: UnsafeMutableRawPointer) -> UnsafePointer<Self> {
        UnsafePointer<Self>(OpaquePointer(__swift_bridge__$Vec_MmGatewayBackend$as_ptr(vecPtr)))
    }

    public static func vecOfSelfLen(vecPtr: UnsafeMutableRawPointer) -> UInt {
        __swift_bridge__$Vec_MmGatewayBackend$len(vecPtr)
    }
}
public struct MmOutcome {
    public var success: Bool
    public var error: MmErrorInfo

    public init(success: Bool,error: MmErrorInfo) {
        self.success = success
        self.error = error
    }

    @inline(__always)
    func intoFfiRepr() -> __swift_bridge__$MmOutcome {
        { let val = self; return __swift_bridge__$MmOutcome(success: val.success, error: val.error.intoFfiRepr()); }()
    }
}
extension __swift_bridge__$MmOutcome {
    @inline(__always)
    func intoSwiftRepr() -> MmOutcome {
        { let val = self; return MmOutcome(success: val.success, error: val.error.intoSwiftRepr()); }()
    }
}
extension __swift_bridge__$Option$MmOutcome {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmOutcome> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }

    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmOutcome>) -> __swift_bridge__$Option$MmOutcome {
        if let v = val {
            return __swift_bridge__$Option$MmOutcome(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmOutcome(is_some: false, val: __swift_bridge__$MmOutcome())
        }
    }
}
public struct MmStringOutcome {
    public var success: Bool
    public var value: RustString
    public var error: MmErrorInfo

    public init(success: Bool,value: RustString,error: MmErrorInfo) {
        self.success = success
        self.value = value
        self.error = error
    }

    @inline(__always)
    func intoFfiRepr() -> __swift_bridge__$MmStringOutcome {
        { let val = self; return __swift_bridge__$MmStringOutcome(success: val.success, value: { let rustString = val.value.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), error: val.error.intoFfiRepr()); }()
    }
}
extension __swift_bridge__$MmStringOutcome {
    @inline(__always)
    func intoSwiftRepr() -> MmStringOutcome {
        { let val = self; return MmStringOutcome(success: val.success, value: RustString(ptr: val.value), error: val.error.intoSwiftRepr()); }()
    }
}
extension __swift_bridge__$Option$MmStringOutcome {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmStringOutcome> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }

    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmStringOutcome>) -> __swift_bridge__$Option$MmStringOutcome {
        if let v = val {
            return __swift_bridge__$Option$MmStringOutcome(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmStringOutcome(is_some: false, val: __swift_bridge__$MmStringOutcome())
        }
    }
}
public struct MmServiceOutcome {
    public var success: Bool
    public var service_name: RustString
    public var port: UInt16
    public var enable_ssl: Bool
    public var error: MmErrorInfo

    public init(success: Bool,service_name: RustString,port: UInt16,enable_ssl: Bool,error: MmErrorInfo) {
        self.success = success
        self.service_name = service_name
        self.port = port
        self.enable_ssl = enable_ssl
        self.error = error
    }

    @inline(__always)
    func intoFfiRepr() -> __swift_bridge__$MmServiceOutcome {
        { let val = self; return __swift_bridge__$MmServiceOutcome(success: val.success, service_name: { let rustString = val.service_name.intoRustString(); rustString.isOwned = false; return rustString.ptr }(), port: val.port, enable_ssl: val.enable_ssl, error: val.error.intoFfiRepr()); }()
    }
}
extension __swift_bridge__$MmServiceOutcome {
    @inline(__always)
    func intoSwiftRepr() -> MmServiceOutcome {
        { let val = self; return MmServiceOutcome(success: val.success, service_name: RustString(ptr: val.service_name), port: val.port, enable_ssl: val.enable_ssl, error: val.error.intoSwiftRepr()); }()
    }
}
extension __swift_bridge__$Option$MmServiceOutcome {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmServiceOutcome> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }

    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmServiceOutcome>) -> __swift_bridge__$Option$MmServiceOutcome {
        if let v = val {
            return __swift_bridge__$Option$MmServiceOutcome(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmServiceOutcome(is_some: false, val: __swift_bridge__$MmServiceOutcome())
        }
    }
}
public struct MmReadyOutcome {
    public var success: Bool
    public var ready: Bool
    public var error: MmErrorInfo

    public init(success: Bool,ready: Bool,error: MmErrorInfo) {
        self.success = success
        self.ready = ready
        self.error = error
    }

    @inline(__always)
    func intoFfiRepr() -> __swift_bridge__$MmReadyOutcome {
        { let val = self; return __swift_bridge__$MmReadyOutcome(success: val.success, ready: val.ready, error: val.error.intoFfiRepr()); }()
    }
}
extension __swift_bridge__$MmReadyOutcome {
    @inline(__always)
    func intoSwiftRepr() -> MmReadyOutcome {
        { let val = self; return MmReadyOutcome(success: val.success, ready: val.ready, error: val.error.intoSwiftRepr()); }()
    }
}
extension __swift_bridge__$Option$MmReadyOutcome {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmReadyOutcome> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }

    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmReadyOutcome>) -> __swift_bridge__$Option$MmReadyOutcome {
        if let v = val {
            return __swift_bridge__$Option$MmReadyOutcome(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmReadyOutcome(is_some: false, val: __swift_bridge__$MmReadyOutcome())
        }
    }
}
public struct MmListOutcome {
    public var success: Bool
    public var entries: RustVec<RustString>
    public var error: MmErrorInfo

    public init(success: Bool,entries: RustVec<RustString>,error: MmErrorInfo) {
        self.success = success
        self.entries = entries
        self.error = error
    }

    @inline(__always)
    func intoFfiRepr() -> __swift_bridge__$MmListOutcome {
        { let val = self; return __swift_bridge__$MmListOutcome(success: val.success, entries: { let val = val.entries; val.isOwned = false; return val.ptr }(), error: val.error.intoFfiRepr()); }()
    }
}
extension __swift_bridge__$MmListOutcome {
    @inline(__always)
    func intoSwiftRepr() -> MmListOutcome {
        { let val = self; return MmListOutcome(success: val.success, entries: RustVec(ptr: val.entries), error: val.error.intoSwiftRepr()); }()
    }
}
extension __swift_bridge__$Option$MmListOutcome {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmListOutcome> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }

    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmListOutcome>) -> __swift_bridge__$Option$MmListOutcome {
        if let v = val {
            return __swift_bridge__$Option$MmListOutcome(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmListOutcome(is_some: false, val: __swift_bridge__$MmListOutcome())
        }
    }
}
public struct MmDataOutcome {
    public var success: Bool
    public var data: RustVec<UInt8>
    public var error: MmErrorInfo

    public init(success: Bool,data: RustVec<UInt8>,error: MmErrorInfo) {
        self.success = success
        self.data = data
        self.error = error
    }

    @inline(__always)
    func intoFfiRepr() -> __swift_bridge__$MmDataOutcome {
        { let val = self; return __swift_bridge__$MmDataOutcome(success: val.success, data: { let val = val.data; val.isOwned = false; return val.ptr }(), error: val.error.intoFfiRepr()); }()
    }
}
extension __swift_bridge__$MmDataOutcome {
    @inline(__always)
    func intoSwiftRepr() -> MmDataOutcome {
        { let val = self; return MmDataOutcome(success: val.success, data: RustVec(ptr: val.data), error: val.error.intoSwiftRepr()); }()
    }
}
extension __swift_bridge__$Option$MmDataOutcome {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmDataOutcome> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }

    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmDataOutcome>) -> __swift_bridge__$Option$MmDataOutcome {
        if let v = val {
            return __swift_bridge__$Option$MmDataOutcome(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmDataOutcome(is_some: false, val: __swift_bridge__$MmDataOutcome())
        }
    }
}
public struct MmAfcFileInfoOutcome {
    public var success: Bool
    public var is_directory: Bool
    public var file_size: Int64
    public var error: MmErrorInfo

    public init(success: Bool,is_directory: Bool,file_size: Int64,error: MmErrorInfo) {
        self.success = success
        self.is_directory = is_directory
        self.file_size = file_size
        self.error = error
    }

    @inline(__always)
    func intoFfiRepr() -> __swift_bridge__$MmAfcFileInfoOutcome {
        { let val = self; return __swift_bridge__$MmAfcFileInfoOutcome(success: val.success, is_directory: val.is_directory, file_size: val.file_size, error: val.error.intoFfiRepr()); }()
    }
}
extension __swift_bridge__$MmAfcFileInfoOutcome {
    @inline(__always)
    func intoSwiftRepr() -> MmAfcFileInfoOutcome {
        { let val = self; return MmAfcFileInfoOutcome(success: val.success, is_directory: val.is_directory, file_size: val.file_size, error: val.error.intoSwiftRepr()); }()
    }
}
extension __swift_bridge__$Option$MmAfcFileInfoOutcome {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmAfcFileInfoOutcome> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }

    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmAfcFileInfoOutcome>) -> __swift_bridge__$Option$MmAfcFileInfoOutcome {
        if let v = val {
            return __swift_bridge__$Option$MmAfcFileInfoOutcome(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmAfcFileInfoOutcome(is_some: false, val: __swift_bridge__$MmAfcFileInfoOutcome())
        }
    }
}
public enum MmProfileDumpMode {
    case Raw
    case Zip
}
extension MmProfileDumpMode {
    func intoFfiRepr() -> __swift_bridge__$MmProfileDumpMode {
        switch self {
            case MmProfileDumpMode.Raw:
                return __swift_bridge__$MmProfileDumpMode(tag: __swift_bridge__$MmProfileDumpMode$Raw)
            case MmProfileDumpMode.Zip:
                return __swift_bridge__$MmProfileDumpMode(tag: __swift_bridge__$MmProfileDumpMode$Zip)
        }
    }
}
extension __swift_bridge__$MmProfileDumpMode {
    func intoSwiftRepr() -> MmProfileDumpMode {
        switch self.tag {
            case __swift_bridge__$MmProfileDumpMode$Raw:
                return MmProfileDumpMode.Raw
            case __swift_bridge__$MmProfileDumpMode$Zip:
                return MmProfileDumpMode.Zip
            default:
                fatalError("Unreachable")
        }
    }
}
extension __swift_bridge__$Option$MmProfileDumpMode {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmProfileDumpMode> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }
    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmProfileDumpMode>) -> __swift_bridge__$Option$MmProfileDumpMode {
        if let v = val {
            return __swift_bridge__$Option$MmProfileDumpMode(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmProfileDumpMode(is_some: false, val: __swift_bridge__$MmProfileDumpMode())
        }
    }
}
extension MmProfileDumpMode: Vectorizable {
    public static func vecOfSelfNew() -> UnsafeMutableRawPointer {
        __swift_bridge__$Vec_MmProfileDumpMode$new()
    }

    public static func vecOfSelfFree(vecPtr: UnsafeMutableRawPointer) {
        __swift_bridge__$Vec_MmProfileDumpMode$drop(vecPtr)
    }

    public static func vecOfSelfPush(vecPtr: UnsafeMutableRawPointer, value: Self) {
        __swift_bridge__$Vec_MmProfileDumpMode$push(vecPtr, value.intoFfiRepr())
    }

    public static func vecOfSelfPop(vecPtr: UnsafeMutableRawPointer) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmProfileDumpMode$pop(vecPtr)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfGet(vecPtr: UnsafeMutableRawPointer, index: UInt) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmProfileDumpMode$get(vecPtr, index)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfGetMut(vecPtr: UnsafeMutableRawPointer, index: UInt) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmProfileDumpMode$get_mut(vecPtr, index)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfAsPtr(vecPtr: UnsafeMutableRawPointer) -> UnsafePointer<Self> {
        UnsafePointer<Self>(OpaquePointer(__swift_bridge__$Vec_MmProfileDumpMode$as_ptr(vecPtr)))
    }

    public static func vecOfSelfLen(vecPtr: UnsafeMutableRawPointer) -> UInt {
        __swift_bridge__$Vec_MmProfileDumpMode$len(vecPtr)
    }
}
public enum MmCoreStatus {
    case Started
    case InProgress
    case Stopped
}
extension MmCoreStatus {
    func intoFfiRepr() -> __swift_bridge__$MmCoreStatus {
        switch self {
            case MmCoreStatus.Started:
                return __swift_bridge__$MmCoreStatus(tag: __swift_bridge__$MmCoreStatus$Started)
            case MmCoreStatus.InProgress:
                return __swift_bridge__$MmCoreStatus(tag: __swift_bridge__$MmCoreStatus$InProgress)
            case MmCoreStatus.Stopped:
                return __swift_bridge__$MmCoreStatus(tag: __swift_bridge__$MmCoreStatus$Stopped)
        }
    }
}
extension __swift_bridge__$MmCoreStatus {
    func intoSwiftRepr() -> MmCoreStatus {
        switch self.tag {
            case __swift_bridge__$MmCoreStatus$Started:
                return MmCoreStatus.Started
            case __swift_bridge__$MmCoreStatus$InProgress:
                return MmCoreStatus.InProgress
            case __swift_bridge__$MmCoreStatus$Stopped:
                return MmCoreStatus.Stopped
            default:
                fatalError("Unreachable")
        }
    }
}
extension __swift_bridge__$Option$MmCoreStatus {
    @inline(__always)
    func intoSwiftRepr() -> Optional<MmCoreStatus> {
        if self.is_some {
            return self.val.intoSwiftRepr()
        } else {
            return nil
        }
    }
    @inline(__always)
    static func fromSwiftRepr(_ val: Optional<MmCoreStatus>) -> __swift_bridge__$Option$MmCoreStatus {
        if let v = val {
            return __swift_bridge__$Option$MmCoreStatus(is_some: true, val: v.intoFfiRepr())
        } else {
            return __swift_bridge__$Option$MmCoreStatus(is_some: false, val: __swift_bridge__$MmCoreStatus())
        }
    }
}
extension MmCoreStatus: Vectorizable {
    public static func vecOfSelfNew() -> UnsafeMutableRawPointer {
        __swift_bridge__$Vec_MmCoreStatus$new()
    }

    public static func vecOfSelfFree(vecPtr: UnsafeMutableRawPointer) {
        __swift_bridge__$Vec_MmCoreStatus$drop(vecPtr)
    }

    public static func vecOfSelfPush(vecPtr: UnsafeMutableRawPointer, value: Self) {
        __swift_bridge__$Vec_MmCoreStatus$push(vecPtr, value.intoFfiRepr())
    }

    public static func vecOfSelfPop(vecPtr: UnsafeMutableRawPointer) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmCoreStatus$pop(vecPtr)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfGet(vecPtr: UnsafeMutableRawPointer, index: UInt) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmCoreStatus$get(vecPtr, index)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfGetMut(vecPtr: UnsafeMutableRawPointer, index: UInt) -> Optional<Self> {
        let maybeEnum = __swift_bridge__$Vec_MmCoreStatus$get_mut(vecPtr, index)
        return maybeEnum.intoSwiftRepr()
    }

    public static func vecOfSelfAsPtr(vecPtr: UnsafeMutableRawPointer) -> UnsafePointer<Self> {
        UnsafePointer<Self>(OpaquePointer(__swift_bridge__$Vec_MmCoreStatus$as_ptr(vecPtr)))
    }

    public static func vecOfSelfLen(vecPtr: UnsafeMutableRawPointer) -> UInt {
        __swift_bridge__$Vec_MmCoreStatus$len(vecPtr)
    }
}

@_cdecl("__swift_bridge__$MmConnectionConfig$_free")
func __swift_bridge__MmConnectionConfig__free (ptr: UnsafeMutableRawPointer) {
    let _ = Unmanaged<MmConnectionConfig>.fromOpaque(ptr).takeRetainedValue()
}


@_cdecl("__swift_bridge__$MmEventSink$_free")
func __swift_bridge__MmEventSink__free (ptr: UnsafeMutableRawPointer) {
    let _ = Unmanaged<MmEventSink>.fromOpaque(ptr).takeRetainedValue()
}



