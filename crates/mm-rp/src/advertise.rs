//! Bonjour advertising for the pairable host (DESIGN
//! §5.2, boundary exception E2's main route): Rust
//! calls the dns-sd C API directly — `DNSService
//! Register` plus a socket pump thread running
//! `DNSServiceProcessResult`. The Swift NetService
//! shell (E2) is only the fallback if this route
//! proves unusable on device; the spike verdict is
//! recorded in `docs/m8-wireless-diff.md` D-M8-9.
//!
//! The service layer's contract mirrors Swift's
//! `NetService.publish()` as WirelessPairService uses
//! it: fire-and-forget (no delegate is set in Swift,
//! so registration errors never surface there either;
//! here they are logged), and `stop()` deregisters.
//!
//! On non-Apple targets there is no dns-sd in
//! libSystem; `register` reports
//! [`AdvertiseError::PlatformUnsupported`] and the
//! host tests gate on that (the authoritative spike
//! runs on macOS CI).

use std::fmt;

/// One advertising failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvertiseError {
    /// dns-sd is not available on this platform
    /// (non-Apple host builds).
    PlatformUnsupported,
    /// A TXT entry exceeded the 255-byte wire limit.
    TxtEntryTooLong(String),
    /// `DNSServiceRegister` returned an error code.
    ServiceError(i32),
    /// A name/type/domain string contained a NUL.
    InvalidString,
}

impl fmt::Display for AdvertiseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlatformUnsupported => {
                write!(f, "dns-sd advertising is not available on this platform")
            }
            Self::TxtEntryTooLong(key) => {
                write!(f, "TXT entry for key '{key}' exceeds 255 bytes")
            }
            Self::ServiceError(code) => {
                write!(f, "DNSServiceRegister failed with error {code}")
            }
            Self::InvalidString => {
                write!(f, "service name/type/domain contains a NUL byte")
            }
        }
    }
}

impl std::error::Error for AdvertiseError {}

/// Builds the dns-sd TXT wire format (the bytes
/// `NetService.data(fromTXTRecord:)` produces): a
/// sequence of entries, each one length byte followed
/// by `key=value` (or bare `key` for an empty value).
/// Pure and platform-independent; unit-tested on every
/// host.
pub fn build_txt_record(
    pairs: &[(String, String)],
) -> Result<Vec<u8>, AdvertiseError> {
    let mut out = Vec::new();
    for (key, value) in pairs {
        let entry = if value.is_empty() {
            key.clone()
        } else {
            format!("{key}={value}")
        };
        let bytes = entry.into_bytes();
        if bytes.len() > 255 {
            return Err(AdvertiseError::TxtEntryTooLong(key.clone()));
        }
        out.push(bytes.len() as u8);
        out.extend_from_slice(&bytes);
    }
    Ok(out)
}

/// A live advertisement. Dropping or calling
/// [`Advertisement::stop`] deregisters the service.
#[cfg(target_vendor = "apple")]
pub struct Advertisement {
    inner: apple::Inner,
}

#[cfg(not(target_vendor = "apple"))]
pub struct Advertisement {
    _priv: (),
}

impl fmt::Debug for Advertisement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Advertisement")
    }
}

impl Advertisement {
    /// Registers `name` under `regtype`/`domain` at
    /// `port` with the given TXT pairs and starts the
    /// result pump. `domain` "" is the default (local)
    /// domain, as in Swift's `defaultAdDomain`.
    pub fn register(
        name: &str,
        regtype: &str,
        domain: &str,
        port: u16,
        txt_pairs: &[(String, String)],
    ) -> Result<Self, AdvertiseError> {
        let txt = build_txt_record(txt_pairs)?;
        #[cfg(target_vendor = "apple")]
        {
            let inner = apple::Inner::register(name, regtype, domain, port, &txt)?;
            Ok(Self { inner })
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = (name, regtype, domain, port, txt);
            Err(AdvertiseError::PlatformUnsupported)
        }
    }

    /// Deregisters the service (Swift `NetService.
    /// stop()`).
    pub fn stop(self) {
        #[cfg(target_vendor = "apple")]
        self.inner.stop();
    }

    /// Spike/diagnostic accessor (Apple route): the
    /// registration callback's error code once the
    /// daemon has answered (0 = success).
    #[cfg(target_vendor = "apple")]
    pub fn callback_result(&self) -> Option<i32> {
        self.inner.callback_result()
    }
}

#[cfg(target_vendor = "apple")]
mod apple {
    use std::ffi::{c_char, c_void, CString};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use super::AdvertiseError;

    type DNSServiceRef = *mut c_void;
    type DNSServiceErrorType = i32;

    type RegisterReply = Option<
        extern "C" fn(
            sd_ref: DNSServiceRef,
            flags: u32,
            error_code: DNSServiceErrorType,
            name: *const c_char,
            regtype: *const c_char,
            domain: *const c_char,
            context: *mut c_void,
        ),
    >;

    extern "C" {
        fn DNSServiceRegister(
            sd_ref: *mut DNSServiceRef,
            flags: u32,
            interface_index: u32,
            name: *const c_char,
            regtype: *const c_char,
            domain: *const c_char,
            host: *const c_char,
            port: u16,
            txt_len: u16,
            txt_record: *const u8,
            call_back: RegisterReply,
            context: *mut c_void,
        ) -> DNSServiceErrorType;
        fn DNSServiceRefSockFD(sd_ref: DNSServiceRef) -> i32;
        fn DNSServiceProcessResult(sd_ref: DNSServiceRef) -> DNSServiceErrorType;
        fn DNSServiceRefDeallocate(sd_ref: DNSServiceRef);
    }

    /// Registration callback state, shared between the
    /// C callback (pump thread) and the owner.
    struct CallbackState {
        result: Mutex<Option<i32>>,
    }

    extern "C" fn register_reply(
        _sd_ref: DNSServiceRef,
        _flags: u32,
        error_code: DNSServiceErrorType,
        _name: *const c_char,
        _regtype: *const c_char,
        _domain: *const c_char,
        context: *mut c_void,
    ) {
        if context.is_null() {
            return;
        }
        let state = unsafe { &*(context as *const CallbackState) };
        if let Ok(mut slot) = state.result.lock() {
            *slot = Some(error_code);
        }
        if error_code != 0 {
            mm_common::logging::debug_log(&format!(
                "[mm-rp] DNSServiceRegister callback error: {error_code}"
            ));
        }
    }

    pub struct Inner {
        service_ref: usize, // DNSServiceRef as usize (Send across the stop call)
        state: Arc<CallbackState>,
        stop_flag: Arc<AtomicBool>,
        pump: Option<std::thread::JoinHandle<()>>,
    }

    // The service ref is only ever touched by the pump
    // thread (ProcessResult) and, after the pump has
    // joined, by stop() (Deallocate) — never
    // concurrently.
    unsafe impl Send for Inner {}

    impl Inner {
        pub fn register(
            name: &str,
            regtype: &str,
            domain: &str,
            port: u16,
            txt: &[u8],
        ) -> Result<Self, AdvertiseError> {
            let name_c = CString::new(name).map_err(|_| AdvertiseError::InvalidString)?;
            let type_c =
                CString::new(regtype).map_err(|_| AdvertiseError::InvalidString)?;
            let domain_c =
                CString::new(domain).map_err(|_| AdvertiseError::InvalidString)?;
            let state = Arc::new(CallbackState {
                result: Mutex::new(None),
            });
            let context = Arc::as_ptr(&state) as *mut c_void;
            let mut service_ref: DNSServiceRef = std::ptr::null_mut();
            let err = unsafe {
                DNSServiceRegister(
                    &mut service_ref,
                    0,
                    0,
                    name_c.as_ptr(),
                    type_c.as_ptr(),
                    domain_c.as_ptr(),
                    std::ptr::null(),
                    port.to_be(),
                    txt.len() as u16,
                    txt.as_ptr(),
                    register_reply,
                    context,
                )
            };
            if err != 0 || service_ref.is_null() {
                return Err(AdvertiseError::ServiceError(err));
            }
            let fd = unsafe { DNSServiceRefSockFD(service_ref) };
            let stop_flag = Arc::new(AtomicBool::new(false));
            let pump = {
                let stop_flag = stop_flag.clone();
                let service_ref_usize = service_ref as usize;
                std::thread::Builder::new()
                    .name("mm-rp-dnssd".to_string())
                    .spawn(move || {
                        let service_ref = service_ref_usize as DNSServiceRef;
                        while !stop_flag.load(Ordering::Relaxed) {
                            let mut pfd = libc::pollfd {
                                fd,
                                events: libc::POLLIN,
                                revents: 0,
                            };
                            let ready =
                                unsafe { libc::poll(&mut pfd, 1, 100) };
                            if ready > 0 && pfd.revents & libc::POLLIN != 0 {
                                let result =
                                    unsafe { DNSServiceProcessResult(service_ref) };
                                if result != 0 {
                                    break;
                                }
                            }
                        }
                    })
                    .expect("failed to spawn dns-sd pump thread")
            };
            Ok(Self {
                service_ref: service_ref as usize,
                state,
                stop_flag,
                pump: Some(pump),
            })
        }

        /// The registration callback's error code once
        /// the daemon has answered (0 = success); the
        /// spike test polls this.
        pub fn callback_result(&self) -> Option<i32> {
            self.state.result.lock().ok().and_then(|slot| *slot)
        }

        pub fn stop(mut self) {
            self.stop_flag.store(true, Ordering::Relaxed);
            if let Some(pump) = self.pump.take() {
                let _ = pump.join();
            }
            unsafe {
                DNSServiceRefDeallocate(self.service_ref as DNSServiceRef);
            }
        }
    }
}
