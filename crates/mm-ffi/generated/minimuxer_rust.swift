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
@_cdecl("__swift_bridge__$MmEventSink$on_log")
func __swift_bridge__MmEventSink_on_log (_ this: UnsafeMutableRawPointer, _ message: UnsafeMutableRawPointer) {
    Unmanaged<MmEventSink>.fromOpaque(this).takeUnretainedValue().on_log(message: RustString(ptr: message))
}

@_cdecl("__swift_bridge__$MmEventSink$on_status")
func __swift_bridge__MmEventSink_on_status (_ this: UnsafeMutableRawPointer, _ seq: UInt64, _ success: Bool, _ domain: UnsafeMutableRawPointer, _ code: UnsafeMutableRawPointer, _ reason: UnsafeMutableRawPointer) {
    Unmanaged<MmEventSink>.fromOpaque(this).takeUnretainedValue().on_status(seq: seq, success: success, domain: RustString(ptr: domain), code: RustString(ptr: code), reason: RustString(ptr: reason))
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

@_cdecl("__swift_bridge__$MmEventSink$_free")
func __swift_bridge__MmEventSink__free (ptr: UnsafeMutableRawPointer) {
    let _ = Unmanaged<MmEventSink>.fromOpaque(ptr).takeRetainedValue()
}



