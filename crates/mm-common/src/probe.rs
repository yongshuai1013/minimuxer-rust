//! TCP reachability probe — port of Swift `NetworkUtils.testTCP`
//! (MinimuxerCommon): non-blocking connect + poll(POLLOUT) + SO_ERROR
//! confirmation. Source: yongshuai1013/minimuxer @714aafa3,
//! `Common/NetworkUtils.swift`.
//!
//! Semantics replicated: empty address rejected; any address containing
//! "/" (CIDR notation) rejected; IPv6 `%scope` honored via
//! if_nametoindex; link-local fe80::/10 without a scope defaults to
//! en0, then awdl0, then lo0.

use crate::constants::DEFAULT_TCP_PROBE_TIMEOUT_MS;

/// Probes with the Swift default timeout (250 ms).
pub fn test_tcp_default(ip: &str, port: u16) -> bool {
    test_tcp(ip, port, DEFAULT_TCP_PROBE_TIMEOUT_MS)
}

/// Returns true iff a TCP connection to `ip:port` completes within
/// `timeout_ms`.
pub fn test_tcp(ip: &str, port: u16, timeout_ms: i32) -> bool {
    if ip.is_empty() {
        return false;
    }
    if ip.contains('/') {
        return false;
    }
    let is_ipv6 = ip.contains(':');
    unsafe {
        if is_ipv6 {
            test_tcp_v6(ip, port, timeout_ms)
        } else {
            test_tcp_v4(ip, port, timeout_ms)
        }
    }
}

struct FdGuard(libc::c_int);
impl Drop for FdGuard {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

fn if_index(name: &str) -> u32 {
    let cname = match std::ffi::CString::new(name) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    unsafe { libc::if_nametoindex(cname.as_ptr()) }
}

unsafe fn poll_connect(fd: libc::c_int, timeout_ms: i32) -> bool {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLOUT as libc::c_short,
        revents: 0,
    };
    let result = libc::poll(&mut pfd, 1, timeout_ms);
    let has_output = (pfd.revents & (libc::POLLOUT as libc::c_short)) != 0;
    let err_mask = (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) as libc::c_short;
    let has_error = (pfd.revents & err_mask) != 0;
    if result > 0 && has_output && !has_error {
        let mut sock_err: libc::c_int = 0;
        let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            &mut sock_err as *mut libc::c_int as *mut libc::c_void,
            &mut len,
        );
        return sock_err == 0;
    }
    false
}

unsafe fn set_nonblocking(fd: libc::c_int) {
    let flags = libc::fcntl(fd, libc::F_GETFL, 0);
    libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
}

fn sockaddr_v4(ip: &str, port: u16) -> Option<libc::sockaddr_in> {
    let mut addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    // The libc crate does not export inet_pton; std's parser is used
    // instead (same strict dotted-quad acceptance as inet_pton).
    let parsed: std::net::Ipv4Addr = ip.parse().ok()?;
    addr.sin_addr = libc::in_addr {
        s_addr: u32::from(parsed).to_be(),
    };
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos"
    ))]
    {
        addr.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
    }
    addr.sin_family = libc::AF_INET as libc::sa_family_t;
    addr.sin_port = port.to_be();
    Some(addr)
}

unsafe fn test_tcp_v4(ip: &str, port: u16, timeout_ms: i32) -> bool {
    let addr = match sockaddr_v4(ip, port) {
        Some(a) => a,
        None => return false,
    };

    let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
    if fd < 0 {
        return false;
    }
    let _guard = FdGuard(fd);
    set_nonblocking(fd);
    libc::connect(
        fd,
        &addr as *const libc::sockaddr_in as *const libc::sockaddr,
        std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
    );
    poll_connect(fd, timeout_ms)
}

fn sockaddr_v6(ip: &str, port: u16) -> Option<libc::sockaddr_in6> {
    let mut addr: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
    let (clean_ip, scope_id) = match ip.split_once('%') {
        Some((clean, iface)) => (clean.to_string(), if_index(iface)),
        None => {
            let scope = if ip.to_lowercase().starts_with("fe80:") {
                let en0 = if_index("en0");
                if en0 != 0 {
                    en0
                } else {
                    let awdl0 = if_index("awdl0");
                    if awdl0 != 0 {
                        awdl0
                    } else {
                        if_index("lo0")
                    }
                }
            } else {
                0
            };
            (ip.to_string(), scope)
        }
    };
    addr.sin6_scope_id = scope_id;
    let parsed: std::net::Ipv6Addr = match clean_ip.parse() {
        Ok(a) => a,
        Err(_) => return None,
    };
    addr.sin6_addr = libc::in6_addr {
        s6_addr: parsed.octets(),
    };
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos"
    ))]
    {
        addr.sin6_len = std::mem::size_of::<libc::sockaddr_in6>() as u8;
    }
    addr.sin6_family = libc::AF_INET6 as libc::sa_family_t;
    addr.sin6_port = port.to_be();
    Some(addr)
}

unsafe fn test_tcp_v6(ip: &str, port: u16, timeout_ms: i32) -> bool {
    let addr = match sockaddr_v6(ip, port) {
        Some(a) => a,
        None => return false,
    };

    let fd = libc::socket(libc::AF_INET6, libc::SOCK_STREAM, 0);
    if fd < 0 {
        return false;
    }
    let _guard = FdGuard(fd);
    set_nonblocking(fd);
    libc::connect(
        fd,
        &addr as *const libc::sockaddr_in6 as *const libc::sockaddr,
        std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
    );
    poll_connect(fd, timeout_ms)
}

// ---------------------------------------------------------------------------
// Outcome probe (M6): the three-way result EMProxy's `probeHost`
// needs. Swift's NWConnection state machine collapses to, in BSD
// terms: SO_ERROR == 0 → Connected (.ready); SO_ERROR ==
// ECONNREFUSED → Refused (.waiting with POSIX 61, which Swift
// counts as success: the tunnel routes, nothing listens yet);
// anything else, including timeout → Failed (.failed/.cancelled
// or the per-attempt cap). See docs/m6-emproxy-behavior.md B4 and
// diff item D-M6-4 for the `.waiting(other)` collapse.
// ---------------------------------------------------------------------------

/// Three-way result of [`probe_tcp_outcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpProbeOutcome {
    /// The connection completed (SO_ERROR == 0).
    Connected,
    /// The peer actively refused (SO_ERROR == ECONNREFUSED).
    Refused,
    /// Any other error, an invalid address, or a timeout.
    Failed,
}

/// Non-blocking connect + poll, reporting the SO_ERROR outcome.
/// Same address acceptance rules as [`test_tcp`] (empty and CIDR
/// forms rejected, IPv6 `%scope` honored).
pub fn probe_tcp_outcome(ip: &str, port: u16, timeout_ms: i32) -> TcpProbeOutcome {
    if ip.is_empty() || ip.contains('/') {
        return TcpProbeOutcome::Failed;
    }
    unsafe {
        if ip.contains(':') {
            let addr = match sockaddr_v6(ip, port) {
                Some(a) => a,
                None => return TcpProbeOutcome::Failed,
            };
            outcome_connect(
                libc::AF_INET6,
                &addr as *const libc::sockaddr_in6 as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                timeout_ms,
            )
        } else {
            let addr = match sockaddr_v4(ip, port) {
                Some(a) => a,
                None => return TcpProbeOutcome::Failed,
            };
            outcome_connect(
                libc::AF_INET,
                &addr as *const libc::sockaddr_in as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                timeout_ms,
            )
        }
    }
}

unsafe fn outcome_connect(
    family: libc::c_int,
    addr: *const libc::sockaddr,
    len: libc::socklen_t,
    timeout_ms: i32,
) -> TcpProbeOutcome {
    let fd = libc::socket(family, libc::SOCK_STREAM, 0);
    if fd < 0 {
        return TcpProbeOutcome::Failed;
    }
    let _guard = FdGuard(fd);
    set_nonblocking(fd);
    let rc = libc::connect(fd, addr, len);
    if rc == 0 {
        return TcpProbeOutcome::Connected;
    }
    let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    if err == libc::ECONNREFUSED {
        return TcpProbeOutcome::Refused;
    }
    if err != libc::EINPROGRESS {
        return TcpProbeOutcome::Failed;
    }
    let mut pfd = libc::pollfd {
        fd,
        events: (libc::POLLOUT | libc::POLLERR | libc::POLLHUP) as libc::c_short,
        revents: 0,
    };
    let result = libc::poll(&mut pfd, 1, timeout_ms);
    if result <= 0 {
        return TcpProbeOutcome::Failed;
    }
    let mut sock_err: libc::c_int = 0;
    let mut opt_len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    libc::getsockopt(
        fd,
        libc::SOL_SOCKET,
        libc::SO_ERROR,
        &mut sock_err as *mut libc::c_int as *mut libc::c_void,
        &mut opt_len,
    );
    if sock_err == 0 {
        TcpProbeOutcome::Connected
    } else if sock_err == libc::ECONNREFUSED {
        TcpProbeOutcome::Refused
    } else {
        TcpProbeOutcome::Failed
    }
}
