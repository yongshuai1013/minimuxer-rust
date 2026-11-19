//! `IdeviceGateway` — port of Swift `IdeviceGateway`
//! (yongshuai1013/minimuxer @714aafa3) over the `idevice` crate.
//!
//! M3 covers the lockdown basic chain: `start`/`stop`,
//! `fetch_udid`, `get_lockdown_value` (fresh TCP connection +
//! StartSession per call, exactly like Swift's
//! `performWithEitherService` → `performWithTcpService` route), an
//! explicit session slot, and `open_service` (StartService +
//! connect). M7 adds the RP route of
//! `performWithEitherService`: a cached mm-rp session
//! (`ensureRPConnection`), the invalidate-rebuild-retry-once
//! of `performWithService`, and the RP operations in
//! `gw_idevice_rp` (diff list `docs/m7-rp-diff.md`). The
//! usbmuxd route (`performWithUsbmuxdService`, which has no
//! call sites in Swift) is implemented as a helper.

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::io;
use std::sync::Mutex;
use std::task::{Context, Poll};

use idevice::lockdown::LockdownClient;
use idevice::pairing_file::PairingFile;
use idevice::provider::{IdeviceProvider, TcpProvider, UsbmuxdProvider};
use idevice::usbmuxd::UsbmuxdAddr;
use idevice::{Idevice, IdeviceError, IdeviceService};
use mm_common::constants::{APP_NAME, USBMUXD_SOCKET};
use mm_common::logging::debug_log;
use mm_common::{DeviceGatewayCode, MmErrorInfo, PairingProtocol};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

use crate::base::GatewayCore;
use crate::gateway::{gw_error, DeviceGateway, GatewayBackend, GatewayFuture, OpenedService};

/// Swift `IdeviceGateway` (`requiresUsbmuxd = false`).
pub struct IdeviceGateway {
    core: GatewayCore,
    /// The parsed IDevice pairing file (Swift's `pairingFile`
    /// FFI handle), built at `start` for lockdown files.
    pairing_file: Mutex<Option<PairingFile>>,
    /// Explicit session slot (see `start_session`).
    session: Mutex<Option<SessionSlot>>,
    /// Open service connections by service identifier.
    services: Mutex<HashMap<String, ServiceSlot>>,
    /// The loaded RemotePairing identity (M7; Swift's
    /// `rpPairingFile` FFI object).
    rp_identity: Mutex<Option<mm_rp::RpIdentity>>,
    /// Swift `ensureRPConnection`'s cache: the established
    /// (adapter, handshake) pair, as one `RpSession`.
    rp_session: Mutex<Option<RpSessionSlot>>,
    /// Open RSD service streams by service identifier.
    rp_services: Mutex<HashMap<String, RpServiceSlot>>,
}

struct RpSessionSlot {
    session: crate::gw_idevice_rp::RpSessionHandle,
    generation: u64,
}

struct RpServiceSlot {
    #[allow(dead_code)]
    stream: mm_rp::RpStream,
    #[allow(dead_code)]
    generation: u64,
}

struct SessionSlot {
    #[allow(dead_code)]
    client: LockdownClient,
    generation: u64,
}

struct ServiceSlot {
    #[allow(dead_code)]
    connection: Idevice,
    generation: u64,
}

/// NODELAY probe provider (2026-11-19): a `TcpProvider` clone
/// that sets TCP_NODELAY on the stream right after connect.
/// The idevice crate never setsockopts, so Nagle stays on and
/// the first lockdown exchange leaves as a lone 4-byte length
/// header with the body held back for the ACK; libimobiledevice
/// sets TCP_NODELAY and its header+body leave back-to-back —
/// the one measured wire-level difference between the stack
/// that dies deterministically at its first exchange over the
/// LocalDevVPN tunnel and the one that completes handshakes.
/// Only `connect_lockdown` uses this provider; every other
/// connection in this file keeps the stock `TcpProvider`.
/// Since rust-v0.1.5 the returned stream is additionally
/// wrapped in `CoalescingStream` (single-write probe; see its
/// doc comment) — NODELAY stays on underneath it.
#[derive(Debug)]
struct NodelayTcpProvider {
    addr: IpAddr,
    pairing_file: PairingFile,
    label: String,
}

impl IdeviceProvider for NodelayTcpProvider {
    fn connect(
        &self,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = Result<Idevice, IdeviceError>> + Send>> {
        let addr = self.addr;
        let label = self.label.clone();
        Box::pin(async move {
            let stream = TcpStream::connect(SocketAddr::new(addr, port)).await?;
            // NODELAY 判別槍：連上後立即開 TCP_NODELAY，其餘與 TcpProvider 逐字相同。
            stream.set_nodelay(true)?;
            // 單次寫入判別槍：再包一層 CoalescingStream，把每訊息的
            // 頭+身兩次 write 併成一次 underlying write（見其註解）。
            Ok(Idevice::new(Box::new(CoalescingStream::new(stream)), label))
        })
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn get_pairing_file(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<PairingFile, IdeviceError>> + Send>> {
        let pairing_file = self.pairing_file.clone();
        Box::pin(async move { Ok(pairing_file) })
    }
}

/// 單次寫入判別槍 (single-write probe, 2026-11-19): a stream
/// wrapper that coalesces the idevice crate's per-message write
/// pairs into single underlying writes, stacked on top of the
/// NODELAY provider's stream. Verified in the idevice 0.1.29
/// crate source: `Idevice::send_plist` emits every lockdown
/// message as exactly two `write_all` calls — a 4-byte
/// big-endian length header, then the XML body — and every
/// plaintext lockdown operation (QueryType, GetValue,
/// StartSession) goes through it. The NODELAY probe
/// (rust-v0.1.4) removed Nagle's wait between the two writes and
/// the first exchange still died; what then remained different
/// from libimobiledevice — whose glue assembles header+body and
/// sends them in ONE write — is the boundary itself: two writes
/// reach the LocalDevVPN loopback as two separate arrivals.
/// This wrapper holds the first write of each pair and, when
/// the second arrives, sends header+body in a single underlying
/// write, mimicking the glue shape on this connection only.
///
/// Coverage is the connection's first four messages (eight
/// writes): the warmup QueryType, GetValue ProductVersion,
/// GetValue DeviceClass, and StartSession itself — all
/// plaintext. From the ninth write on the wrapper is a
/// permanent passthrough, so the TLS upgrade and everything
/// after it are untouched. `poll_flush`/`poll_shutdown` with a
/// held half-write send it first (safety valve; byte order is
/// preserved regardless, since coalescing only concatenates).
/// The read side is a pure passthrough.
#[derive(Debug)]
struct CoalescingStream<S> {
    inner: S,
    /// Pairwise-coalesced writes remaining (starts at
    /// `COALESCE_WRITES`); zero means permanent passthrough.
    coalesce_writes_left: u8,
    /// First write of a pair, held until the second arrives.
    pending: Option<Vec<u8>>,
    /// Bytes on their way to `inner`: a combined pair, or a
    /// flush-released half.
    out: Vec<u8>,
    /// How much of `out` `inner` has accepted so far.
    out_pos: usize,
    /// What the interrupted caller of a pair's second write
    /// must be told once `out` drains. `None` while `out` is
    /// flush-owned (no caller waits on a count). Invariant:
    /// `out` non-empty implies `out_report` names its owner.
    out_report: Option<usize>,
}

/// Writes covered by pairwise coalescing: four lockdown
/// messages (warmup ×3 + StartSession) × two writes each.
const COALESCE_WRITES: u8 = 8;

impl<S> CoalescingStream<S> {
    fn new(inner: S) -> Self {
        Self {
            inner,
            coalesce_writes_left: COALESCE_WRITES,
            pending: None,
            out: Vec::new(),
            out_pos: 0,
            out_report: None,
        }
    }
}

impl<S: AsyncWrite + Unpin> CoalescingStream<S> {
    /// Drives `out` into `inner` until it drains. On success
    /// `out` is cleared; `out_report` is left in place for its
    /// owner to consume.
    fn poll_drain_out(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.out_pos < self.out.len() {
            let res = Pin::new(&mut self.inner).poll_write(cx, &self.out[self.out_pos..]);
            match res {
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "CoalescingStream: underlying write returned 0",
                    )));
                }
                Poll::Ready(Ok(n)) => self.out_pos += n,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        self.out.clear();
        self.out_pos = 0;
        Poll::Ready(Ok(()))
    }

    /// Flush preparation shared by `poll_flush` and
    /// `poll_shutdown`: release a held half-write into `out`
    /// (when no pair is in flight) and drain whatever is in
    /// flight, so no held bytes are ever stranded.
    fn poll_drain_for_flush(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.out.is_empty() {
            if let Some(half) = self.pending.take() {
                self.out = half;
                self.out_report = None;
            }
        }
        if self.out.is_empty() {
            return Poll::Ready(Ok(()));
        }
        self.poll_drain_out(cx)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for CoalescingStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        // A combined pair is still draining (an earlier
        // poll_write returned Pending): finish it before
        // looking at `buf` — on this retry `buf` is the same
        // second half that is already inside `out`.
        if !this.out.is_empty() {
            match this.poll_drain_out(cx) {
                Poll::Ready(Ok(())) => {
                    if let Some(n) = this.out_report.take() {
                        return Poll::Ready(Ok(n));
                    }
                    // Flush-owned bytes finished draining;
                    // fall through and treat `buf` as fresh.
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        if this.coalesce_writes_left == 0 {
            return Pin::new(&mut this.inner).poll_write(cx, buf);
        }
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        match this.pending.take() {
            None => {
                // First write of a pair: hold it, report it
                // consumed, send nothing yet.
                this.pending = Some(buf.to_vec());
                this.coalesce_writes_left -= 1;
                Poll::Ready(Ok(buf.len()))
            }
            Some(mut first) => {
                // Second write of a pair: a single underlying
                // write carrying header+body together.
                first.extend_from_slice(buf);
                this.out = first;
                this.out_pos = 0;
                this.out_report = Some(buf.len());
                this.coalesce_writes_left -= 1;
                match this.poll_drain_out(cx) {
                    Poll::Ready(Ok(())) => {
                        let n = this.out_report.take().unwrap_or(buf.len());
                        Poll::Ready(Ok(n))
                    }
                    Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match this.poll_drain_for_flush(cx) {
            Poll::Ready(Ok(())) => Pin::new(&mut this.inner).poll_flush(cx),
            other => other,
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match this.poll_drain_for_flush(cx) {
            Poll::Ready(Ok(())) => Pin::new(&mut this.inner).poll_shutdown(cx),
            other => other,
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for CoalescingStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl Default for IdeviceGateway {
    fn default() -> Self {
        Self::new()
    }
}

impl IdeviceGateway {
    pub fn new() -> Self {
        Self {
            core: GatewayCore::new(),
            pairing_file: Mutex::new(None),
            session: Mutex::new(None),
            services: Mutex::new(HashMap::new()),
            rp_identity: Mutex::new(None),
            rp_session: Mutex::new(None),
            rp_services: Mutex::new(HashMap::new()),
        }
    }

    /// Swift `cleanup()` (subclass half): drop the pairing file
    /// handle and every cached connection, then the base cleanup.
    fn cleanup(&self) {
        debug_log("[IdeviceGateway] cleanup() called");
        *self.pairing_file.lock().unwrap() = None;
        *self.session.lock().unwrap() = None;
        self.services.lock().unwrap().clear();
        *self.rp_identity.lock().unwrap() = None;
        *self.rp_session.lock().unwrap() = None;
        self.rp_services.lock().unwrap().clear();
        self.core.cleanup();
    }

    /// Swift `verifyInitialized()` — flag only (the limd backend is
    /// stricter; see the diff list §3).
    fn verify_initialized(&self) -> Result<(), MmErrorInfo> {
        if self.core.is_initialized() {
            Ok(())
        } else {
            Err(gw_error(DeviceGatewayCode::NotInitialized, ""))
        }
    }

    fn require_endpoint_ip(&self) -> Result<String, MmErrorInfo> {
        self.core
            .device_endpoint_ip()
            .filter(|ip| !ip.is_empty())
            .ok_or_else(|| gw_error(DeviceGatewayCode::DeviceEndpointIpNotAvailable, ""))
    }

    fn require_pairing_file(&self) -> Result<PairingFile, MmErrorInfo> {
        self.pairing_file
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| {
                gw_error(
                    DeviceGatewayCode::ConnectionFailed,
                    "pairingFileData is nil",
                )
            })
    }

    /// Swift `performWithTcpService` provider construction: a
    /// `TcpProvider` to the endpoint IP at the *constant* lockdownd
    /// port (the provider connects to 62078 itself; the port table
    /// is not consulted on this path — diff list D-4).
    fn tcp_provider(&self) -> Result<TcpProvider, MmErrorInfo> {
        let ip_string = self.require_endpoint_ip()?;
        let pairing_file = self.require_pairing_file()?;
        let addr = parse_endpoint_ip(&ip_string)?;
        Ok(TcpProvider {
            addr,
            pairing_file,
            label: APP_NAME.to_string(),
        })
    }

    /// The NODELAY probe's provider for `connect_lockdown`:
    /// the same fields as `tcp_provider`, but connections are
    /// established through `NodelayTcpProvider` (TCP_NODELAY
    /// set right after connect — see its doc comment).
    fn nodelay_tcp_provider(&self) -> Result<NodelayTcpProvider, MmErrorInfo> {
        let ip_string = self.require_endpoint_ip()?;
        let pairing_file = self.require_pairing_file()?;
        let addr = parse_endpoint_ip(&ip_string)?;
        Ok(NodelayTcpProvider {
            addr,
            pairing_file,
            label: APP_NAME.to_string(),
        })
    }

    /// The M5 operations context (endpoint + pairing file).
    fn m5_ctx(&self) -> Result<crate::gw_idevice_ops::IdeviceCtx, MmErrorInfo> {
        let ip_string = self.require_endpoint_ip()?;
        let addr = parse_endpoint_ip(&ip_string)?;
        let pairing_file = self.require_pairing_file()?;
        Ok(crate::gw_idevice_ops::IdeviceCtx {
            endpoint_ip: addr,
            pairing_file,
        })
    }

    /// Connects a fresh lockdownd client over TCP (the lockdown
    /// half of Swift `performWithEitherService`). Goes through
    /// the NODELAY probe provider (see `NodelayTcpProvider`).
    async fn connect_lockdown(&self) -> Result<LockdownClient, MmErrorInfo> {
        let provider = self.nodelay_tcp_provider()?;
        LockdownClient::connect(&provider).await.map_err(|e| {
            gw_error(
                DeviceGatewayCode::ServiceError,
                format!("Failed to connect to lockdownd, error: ({e})"),
            )
        })
    }

    /// Starts a session on `client` with the loaded pairing file,
    /// mapping failures with Swift's `syncGetLockdownValue`
    /// classification (pairing-class → invalidPairingFile, else
    /// connectionFailed).
    ///
    /// PROBE (warmup, 2026-11-19): `warmup_lockdown` runs first, on
    /// this same connection. The idevice crate's `start_session`
    /// otherwise makes StartSession the first request on a fresh
    /// connection, which the LocalDevVPN tunnel path cuts before
    /// the response (field verdict: four independent idevice-crate
    /// stacks dead at this exchange; limd's handshake, which warms
    /// the connection up first, completes). If the warmup itself
    /// dies, its step-identifying reason says where; if the warmup
    /// passes and StartSession still dies, the reason below is
    /// unchanged — the two outcomes are distinguishable from the
    /// App's error string alone.
    async fn start_session_on(
        &self,
        client: &mut LockdownClient,
    ) -> Result<(), MmErrorInfo> {
        self.warmup_lockdown(client).await?;
        let pairing_file = self.require_pairing_file()?;
        client.start_session(&pairing_file).await.map_err(|e| {
            if is_pairing_error(&e) {
                gw_error(
                    DeviceGatewayCode::InvalidPairingFile,
                    format!("Lockdown session failed (pairing invalid): {e}"),
                )
            } else {
                gw_error(
                    DeviceGatewayCode::ConnectionFailed,
                    format!("Lockdown session failed: {e}"),
                )
            }
        })
    }

    /// The limd-style pre-session warmup (see `start_session_on`):
    /// on this connection, before StartSession, replay the three
    /// plaintext exchanges libimobiledevice's
    /// `lockdownd_client_new_with_handshake` performs —
    /// QueryType, GetValue "ProductVersion", GetValue "DeviceClass".
    ///
    /// Crate API notes (idevice 0.1.29): `LockdownClient` has no
    /// QueryType method of its own, but its `idevice` field is
    /// public and `Idevice::get_type` is exactly the QueryType
    /// exchange (and returns the Type string, verified here the
    /// way limd verifies it). The GetValue reads use
    /// `LockdownClient::get_value`, which the crate does not gate
    /// on an established session — the same pre-session reads limd
    /// performs. Any step failing aborts the session attempt with
    /// a step-identifying reason (connectionFailed class).
    async fn warmup_lockdown(
        &self,
        client: &mut LockdownClient,
    ) -> Result<(), MmErrorInfo> {
        let device_type = client.idevice.get_type().await.map_err(|e| {
            gw_error(
                DeviceGatewayCode::ConnectionFailed,
                format!("warmup QueryType failed: {e}"),
            )
        })?;
        if device_type != "com.apple.mobile.lockdown" {
            return Err(gw_error(
                DeviceGatewayCode::ConnectionFailed,
                format!(
                    "warmup QueryType failed: {}",
                    IdeviceError::UnexpectedResponse
                ),
            ));
        }
        debug_log("[IdeviceGateway] warmup QueryType ok");
        client.get_value("ProductVersion").await.map_err(|e| {
            gw_error(
                DeviceGatewayCode::ConnectionFailed,
                format!("warmup GetValue ProductVersion failed: {e}"),
            )
        })?;
        debug_log("[IdeviceGateway] warmup GetValue ProductVersion ok");
        client.get_value("DeviceClass").await.map_err(|e| {
            gw_error(
                DeviceGatewayCode::ConnectionFailed,
                format!("warmup GetValue DeviceClass failed: {e}"),
            )
        })?;
        debug_log("[IdeviceGateway] warmup GetValue DeviceClass ok");
        Ok(())
    }

    fn drop_stale_services(&self) {
        let generation = self.core.generation();
        self.services
            .lock()
            .unwrap()
            .retain(|_, slot| slot.generation == generation);
    }

    // -- M7: RemotePairing session management ----------------

    fn require_rp_identity(&self) -> Result<mm_rp::RpIdentity, MmErrorInfo> {
        self.rp_identity
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| {
                gw_error(DeviceGatewayCode::NotInitialized, "pairingFile is nil")
            })
    }

    /// Swift `ensureRPConnection`: return the cached session
    /// while the core generation is unchanged, otherwise
    /// establish a fresh one (pair-verify + tunnel + RSD
    /// handshake, via mm-rp).
    async fn rp_session(
        &self,
    ) -> Result<crate::gw_idevice_rp::RpSessionHandle, MmErrorInfo> {
        let generation = self.core.generation();
        {
            let slot = self.rp_session.lock().unwrap();
            if let Some(slot) = slot.as_ref() {
                if slot.generation == generation {
                    return Ok(slot.session.clone());
                }
            }
        }
        let identity = self.require_rp_identity()?;
        let endpoint = self.require_endpoint_ip()?;
        let port = self.core.get_port(PairingProtocol::RpPairing);
        let session = mm_rp::RpSession::establish(&endpoint, port, APP_NAME, &identity)
            .await
            .map_err(|e| {
                if e.pairing {
                    gw_error(
                        DeviceGatewayCode::InvalidPairingFile,
                        format!("Handshake failed: {}", e.message),
                    )
                } else {
                    gw_error(
                        DeviceGatewayCode::ConnectionFailed,
                        format!(
                            "Failed to establish RemotePairing tunnel: {}",
                            e.message
                        ),
                    )
                }
            })?;
        let handle: crate::gw_idevice_rp::RpSessionHandle =
            std::sync::Arc::new(tokio::sync::Mutex::new(session));
        *self.rp_session.lock().unwrap() = Some(RpSessionSlot {
            session: handle.clone(),
            generation,
        });
        Ok(handle)
    }

    /// Swift `invalidateConnection` (RP half).
    fn invalidate_rp_session(&self) {
        *self.rp_session.lock().unwrap() = None;
        self.rp_services.lock().unwrap().clear();
    }

    /// Swift `performWithService` (RP route): run `op` on the
    /// cached session; on failure invalidate, rebuild the
    /// session and retry exactly once (diff list D-M7-8).
    async fn perform_rp<T, F, Fut>(&self, op: F) -> Result<T, MmErrorInfo>
    where
        F: Fn(crate::gw_idevice_rp::RpSessionHandle) -> Fut,
        Fut: std::future::Future<Output = Result<T, MmErrorInfo>>,
    {
        let session = self.rp_session().await?;
        match op(session).await {
            Ok(value) => Ok(value),
            Err(_) => {
                self.invalidate_rp_session();
                let session = self.rp_session().await?;
                op(session).await
            }
        }
    }
}

impl DeviceGateway for IdeviceGateway {
    fn backend(&self) -> GatewayBackend {
        GatewayBackend::Idevice
    }

    fn core(&self) -> &GatewayCore {
        &self.core
    }

    fn set_logging(&self, enabled: bool) {
        // Swift sets the IDevice FFI logger level; the Rust crate
        // logs through the `log` facade, whose level is a host
        // concern (diff list D-9). Only the core flag is kept here.
        self.core.set_logging(enabled);
    }

    fn start<'a>(
        &'a self,
        pairing_file_content: &'a str,
        preferred: Option<PairingProtocol>,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            debug_log(&format!(
                "[IdeviceGateway] start() called, pairingFileContent length: {}",
                pairing_file_content.len()
            ));
            self.cleanup();
            // Swift rethrows the parser's PairingError unwrapped.
            let parsed = mm_common::parse_pairing_content(pairing_file_content, preferred)
                .map_err(|e| e.to_info())?;
            self.core
                .set_pairing_file_data(Some(parsed.raw_data().to_vec()));
            self.core.set_pairing_file_type(parsed.mode());
            match parsed.mode() {
                PairingProtocol::Lockdown => {
                    let pf = PairingFile::from_bytes(parsed.raw_data()).map_err(|e| {
                        debug_log(&format!(
                            "[IdeviceGateway] start() idevice_pairing_file_from_bytes failed: {e}"
                        ));
                        gw_error(
                            DeviceGatewayCode::InvalidPairingFile,
                            "idevice_pairing_file_from_bytes failed",
                        )
                    })?;
                    *self.pairing_file.lock().unwrap() = Some(pf);
                }
                PairingProtocol::RpPairing => {
                    // M7: build the RP identity object (Swift's
                    // `rp_pairing_file_from_bytes` at start).
                    let identity = mm_rp::RpIdentity::load(parsed.raw_data())
                        .map_err(|e| {
                            debug_log(&format!(
                                "[IdeviceGateway] start() rp_pairing_file_from_bytes failed: {}",
                                e.message
                            ));
                            gw_error(
                                DeviceGatewayCode::InvalidPairingFile,
                                "rp_pairing_file_from_bytes failed",
                            )
                        })?;
                    *self.rp_identity.lock().unwrap() = Some(identity);
                }
                PairingProtocol::Unknown => {
                    // The parser never yields Unknown on success.
                    return Err(gw_error(
                        DeviceGatewayCode::InvalidPairingFile,
                        "Unknown pairing file format",
                    ));
                }
            }
            self.core.set_initialized(true);
            Ok(())
        })
    }

    fn stop<'a>(&'a self) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.cleanup();
            Ok(())
        })
    }

    fn fetch_udid<'a>(&'a self) -> GatewayFuture<'a, String> {
        Box::pin(async move {
            let udid = self.get_lockdown_value("UniqueDeviceID").await?;
            if udid.is_empty() {
                return Err(gw_error(
                    DeviceGatewayCode::ServiceError,
                    "UniqueDeviceID not found on device",
                ));
            }
            Ok(udid)
        })
    }

    fn get_lockdown_value<'a>(&'a self, key: &'a str) -> GatewayFuture<'a, String> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::get_lockdown_value(&session, key).await
                    })
                    .await;
            }
            let mut client = self.connect_lockdown().await?;
            self.start_session_on(&mut client).await?;
            let value = client.get_value(key).await.map_err(|e| {
                if is_pairing_error(&e) {
                    gw_error(
                        DeviceGatewayCode::InvalidPairingFile,
                        format!("Failed to get lockdown value for key {key}, error: ({e})"),
                    )
                } else {
                    gw_error(
                        DeviceGatewayCode::ServiceError,
                        format!("Failed to get lockdown value for key {key}, error: ({e})"),
                    )
                }
            })?;
            match value {
                plist::Value::String(s) => Ok(s),
                _ => Err(gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("Lockdown value for key '{key}' could not be decoded as string"),
                )),
            }
        })
    }

    fn start_session<'a>(&'a self) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                // RP analog: establishing the RP session is
                // the authenticated-session proof on this route.
                return self.rp_session().await.map(|_| ());
            }
            let generation = self.core.generation();
            {
                let slot = self.session.lock().unwrap();
                if let Some(slot) = slot.as_ref() {
                    if slot.generation == generation {
                        return Ok(());
                    }
                }
            }
            let mut client = self.connect_lockdown().await?;
            self.start_session_on(&mut client).await?;
            *self.session.lock().unwrap() = Some(SessionSlot { client, generation });
            Ok(())
        })
    }

    fn stop_session<'a>(&'a self) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            *self.session.lock().unwrap() = None;
            self.invalidate_rp_session();
            Ok(())
        })
    }

    fn perform_heartbeat<'a>(&'a self, interval: u64) -> GatewayFuture<'a, u64> {
        Box::pin(async move {
            debug_log(&format!(
                "[IdeviceGateway] performHeartbeat() called, interval: {interval}"
            ));
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                let new_interval = self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::perform_heartbeat(&session, interval).await
                    })
                    .await?;
                debug_log(&format!(
                    "[IdeviceGateway] performHeartbeat() succeeded, newInterval: {new_interval}"
                ));
                return Ok(new_interval);
            }
            // Swift `performWithEitherService` (lockdown route):
            // a fresh heartbeat connection per beat.
            let provider = self.tcp_provider()?;
            let mut client =
                idevice::heartbeat::HeartbeatClient::connect(&provider)
                    .await
                    .map_err(|e| {
                        gw_error(
                            DeviceGatewayCode::ServiceError,
                            format!("Failed to connect to heartbeat, error: ({e})"),
                        )
                    })?;
            let new_interval = client.get_marco(interval).await.map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("Heartbeat receive failed, error: ({e})"),
                )
            })?;
            client.send_polo().await.map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("Heartbeat send failed, error: ({e})"),
                )
            })?;
            debug_log(&format!(
                "[IdeviceGateway] performHeartbeat() succeeded, newInterval: {new_interval}"
            ));
            Ok(if new_interval > 0 {
                new_interval
            } else {
                interval
            })
        })
    }

    fn open_service<'a>(&'a self, service_name: &'a str) -> GatewayFuture<'a, OpenedService> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                let (opened, stream) = self
                    .perform_rp(|session| async move {
                        let mut guard = session.lock().await;
                        let port = guard.service_port(service_name).ok_or_else(|| {
                            gw_error(
                                DeviceGatewayCode::ServiceError,
                                format!(
                                    "Failed to connect to {service_name}, error: (RSD service not found)"
                                ),
                            )
                        })?;
                        let stream = guard
                            .open_stream(service_name)
                            .await
                            .map_err(|e| {
                                gw_error(
                                    DeviceGatewayCode::ServiceError,
                                    format!(
                                        "Failed to connect to {service_name}, error: ({})",
                                        e.message
                                    ),
                                )
                            })?;
                        Ok((
                            OpenedService {
                                service_name: service_name.to_string(),
                                port,
                                enable_ssl: false,
                            },
                            stream,
                        ))
                    })
                    .await?;
                let generation = self.core.generation();
                self.rp_services.lock().unwrap().insert(
                    service_name.to_string(),
                    RpServiceSlot { stream, generation },
                );
                return Ok(opened);
            }
            self.drop_stale_services();
            let provider = self.tcp_provider()?;
            let mut client = LockdownClient::connect(&provider).await.map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("Failed to connect to lockdownd, error: ({e})"),
                )
            })?;
            let (port, enable_ssl) =
                client.start_service(service_name).await.map_err(|e| {
                    gw_error(
                        DeviceGatewayCode::ServiceError,
                        format!("Failed to connect to {service_name}, error: ({e})"),
                    )
                })?;
            let connection = provider.connect(port).await.map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("Failed to connect to {service_name}, error: ({e})"),
                )
            })?;
            let generation = self.core.generation();
            self.services.lock().unwrap().insert(
                service_name.to_string(),
                ServiceSlot {
                    connection,
                    generation,
                },
            );
            Ok(OpenedService {
                service_name: service_name.to_string(),
                port,
                enable_ssl,
            })
        })
    }
    // ------------------------------------------------------------------
    // M5: DDI mount, install pipeline, profiles, debug
    // (docs/m5-install-ddi-debug-diff.md). M7: the RP routes
    // dispatch to gw_idevice_rp through perform_rp.
    // ------------------------------------------------------------------

    fn is_ddi_mounted<'a>(&'a self) -> GatewayFuture<'a, bool> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        Ok(crate::gw_idevice_rp::is_ddi_mounted(&session).await)
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            Ok(crate::gw_idevice_ops::is_ddi_mounted(&ctx).await)
        })
    }

    fn mount_developer_image<'a>(
        &'a self,
        image: &'a [u8],
        signature: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::mount_developer_image(
                            &session, image, signature,
                        )
                        .await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::mount_developer_image(&ctx, image, signature).await
        })
    }

    fn mount_personalized_ddi<'a>(
        &'a self,
        image: &'a [u8],
        trustcache: &'a [u8],
        build_manifest: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::mount_personalized_ddi(
                            &session,
                            image,
                            trustcache,
                            build_manifest,
                        )
                        .await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::mount_personalized_ddi(
                &ctx,
                image,
                trustcache,
                build_manifest,
            )
            .await
        })
    }

    fn install_provisioning_profile<'a>(
        &'a self,
        profile: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::misagent_install(&session, profile).await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::misagent_install(&ctx, profile).await
        })
    }

    fn remove_provisioning_profile<'a>(
        &'a self,
        profile_id: &'a str,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::misagent_remove(&session, profile_id).await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::misagent_remove(&ctx, profile_id).await
        })
    }

    fn dump_profiles<'a>(&'a self, docs_path: &'a str) -> GatewayFuture<'a, String> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                let payloads = self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::misagent_copy_all(&session).await
                    })
                    .await?;
                return Ok(dump_profiles_to_disk(docs_path, &payloads));
            }
            let ctx = self.m5_ctx()?;
            let payloads = crate::gw_idevice_ops::misagent_copy_all(&ctx).await?;
            Ok(dump_profiles_to_disk(docs_path, &payloads))
        })
    }

    fn remove_app<'a>(&'a self, bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::instproxy_uninstall(&session, bundle_id)
                            .await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::instproxy_uninstall(&ctx, bundle_id).await
        })
    }

    fn send_ipa_afc<'a>(
        &'a self,
        bundle_id: &'a str,
        ipa_bytes: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::send_ipa_afc(&session, bundle_id, ipa_bytes)
                            .await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::send_ipa_afc(&ctx, bundle_id, ipa_bytes).await
        })
    }

    fn send_app_bundle_afc<'a>(
        &'a self,
        bundle_id: &'a str,
        app_path: &'a str,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::send_app_bundle_afc(
                            &session,
                            bundle_id,
                            std::path::Path::new(app_path),
                        )
                        .await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::send_app_bundle_afc(
                &ctx,
                bundle_id,
                std::path::Path::new(app_path),
            )
            .await
        })
    }

    fn install_ipa<'a>(&'a self, bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        let package_path =
                            format!("PublicStaging/{bundle_id}/app.ipa");
                        crate::gw_idevice_rp::instproxy_install(
                            &session,
                            &package_path,
                            None,
                            "Failed to install IPA",
                        )
                        .await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            let package_path = format!("PublicStaging/{bundle_id}/app.ipa");
            crate::gw_idevice_ops::instproxy_install(
                &ctx,
                &package_path,
                None,
                "Failed to install IPA",
            )
            .await
        })
    }

    fn install_app_bundle<'a>(
        &'a self,
        bundle_id: &'a str,
        app_name: &'a str,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        let package_path =
                            format!("PublicStaging/{bundle_id}/{app_name}");
                        let mut options = plist::Dictionary::new();
                        options.insert(
                            "PackageType".into(),
                            plist::Value::String("Developer".into()),
                        );
                        crate::gw_idevice_rp::instproxy_install(
                            &session,
                            &package_path,
                            Some(options),
                            "Failed to install App directory",
                        )
                        .await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            let package_path = format!("PublicStaging/{bundle_id}/{app_name}");
            let mut options = plist::Dictionary::new();
            options.insert(
                "PackageType".into(),
                plist::Value::String("Developer".into()),
            );
            crate::gw_idevice_ops::instproxy_install(
                &ctx,
                &package_path,
                Some(options),
                "Failed to install App directory",
            )
            .await
        })
    }

    fn wipe_container<'a>(&'a self, bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::wipe_container(&session, bundle_id).await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::wipe_container(&ctx, bundle_id).await
        })
    }

    fn debug_app<'a>(&'a self, bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::debug_app(&session, bundle_id).await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            let mux_addr = usbmuxd_addr_from_env(
                std::env::var(mm_common::constants::USBMUXD_ENV_KEY)
                    .ok()
                    .as_deref(),
            );
            crate::gw_idevice_ops::debug_app(&ctx, bundle_id, &mux_addr).await
        })
    }

    fn debug_process<'a>(&'a self, pid: u32) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                // The RP route is where this operation exists
                // (M7); the lockdown route stays unsupported.
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::debug_process(&session, pid).await
                    })
                    .await;
            }
            // Swift parity (D-M5-1): the idevice backend's
            // lockdown debugProcess throws unsupportedOperation
            // with this reason, verbatim.
            Err(gw_error(
                DeviceGatewayCode::UnsupportedOperation,
                "Attaching to process by PID is only supported in.rppairing mode",
            ))
        })
    }

    fn start_wireless_pair<'a>(
        &'a self,
        args: crate::gateway::WirelessPairStart<'a>,
    ) -> GatewayFuture<'a, crate::gateway::PairedDeviceRecord> {
        Box::pin(crate::gw_idevice_wireless::start_wireless_pair(args))
    }

    fn trigger_wireless_pair<'a>(
        &'a self,
        args: crate::gateway::WirelessPairTrigger<'a>,
    ) -> GatewayFuture<'a, crate::gateway::PairedDeviceRecord> {
        Box::pin(crate::gw_idevice_wireless::trigger_wireless_pair(args))
    }

    fn afc_list_directory<'a>(
        &'a self,
        bundle_id: &'a str,
        path: &'a str,
    ) -> GatewayFuture<'a, Vec<String>> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::afc_list_directory(
                            &session, bundle_id, path,
                        )
                        .await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::afc_list_directory(&ctx, bundle_id, path).await
        })
    }

    fn afc_read_file<'a>(
        &'a self,
        bundle_id: &'a str,
        path: &'a str,
    ) -> GatewayFuture<'a, Vec<u8>> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::afc_read_file(&session, bundle_id, path)
                            .await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::afc_read_file(&ctx, bundle_id, path).await
        })
    }

    fn afc_get_file_info<'a>(
        &'a self,
        bundle_id: &'a str,
        path: &'a str,
    ) -> GatewayFuture<'a, crate::gateway::AfcFileInfo> {
        Box::pin(async move {
            self.verify_initialized()?;
            if self.core.pairing_file_type() == PairingProtocol::RpPairing {
                return self
                    .perform_rp(|session| async move {
                        crate::gw_idevice_rp::afc_get_file_info(
                            &session, bundle_id, path,
                        )
                        .await
                    })
                    .await;
            }
            let ctx = self.m5_ctx()?;
            crate::gw_idevice_ops::afc_get_file_info(&ctx, bundle_id, path).await
        })
    }
}

/// Swift `withSockaddr` address parsing: `%scope` suffixes are
/// resolved through `if_nametoindex` for the sockaddr; the
/// `TcpProvider` carries only the address, so the scope is stripped
/// here (the tunnel interfaces this port targets are point-to-point
/// links where the route is unambiguous). Invalid input maps to
/// Swift's `invalidTargetEndpoint` reasons, verbatim.
fn parse_endpoint_ip(ip: &str) -> Result<IpAddr, MmErrorInfo> {
    let bare = ip.split('%').next().unwrap_or(ip);
    bare.parse::<IpAddr>().map_err(|_| {
        if ip.contains(':') {
            gw_error(
                DeviceGatewayCode::InvalidTargetEndpoint,
                format!("Invalid IPv6 address: {ip}"),
            )
        } else {
            gw_error(
                DeviceGatewayCode::InvalidTargetEndpoint,
                format!("Invalid IPv4 address: {ip}"),
            )
        }
    })
}

/// Swift `isPairingError`: FFI codes 18 (InvalidHostID) and 103
/// (RemotePairing), or a message containing (case-insensitively)
/// `invalidconf`, `pairing`, `handshake`, `bad_certificate` or
/// `certificate_unknown`. Applied to the idevice crate's error
/// surface: `InvalidHostID` hits directly, the rest go through the
/// message rule on the error's Display text.
pub fn is_pairing_error(err: &IdeviceError) -> bool {
    if matches!(err, IdeviceError::InvalidHostID) {
        return true;
    }
    let msg = err.to_string().to_lowercase();
    [
        "invalidconf",
        "pairing",
        "handshake",
        "bad_certificate",
        "certificate_unknown",
    ]
    .iter()
    .any(|needle| msg.contains(needle))
}

/// Swift `performWithUsbmuxdService`'s address resolution, kept as
/// a pure function: the `USBMUXD_SOCKET_ADDRESS` env value when set
/// (host:port → TCP, otherwise a unix socket path), else the
/// default — which for this port is the mm-core proxy address
/// (`idevice_usbmuxd_default_addr_new`'s equivalent in-App is the
/// retargeted env value; the constant default mirrors it).
pub fn usbmuxd_addr_from_env(value: Option<&str>) -> UsbmuxdAddr {
    match value {
        Some(v) if v.contains(':') => {
            if let Ok(addr) = v.parse() {
                UsbmuxdAddr::TcpSocket(addr)
            } else {
                UsbmuxdAddr::TcpSocket(USBMUXD_SOCKET.parse().unwrap())
            }
        }
        #[cfg(unix)]
        Some(v) => UsbmuxdAddr::UnixSocket(v.to_string()),
        #[cfg(not(unix))]
        Some(_) => UsbmuxdAddr::TcpSocket(USBMUXD_SOCKET.parse().unwrap()),
        None => UsbmuxdAddr::TcpSocket(USBMUXD_SOCKET.parse().unwrap()),
    }
}

/// Builds the `UsbmuxdProvider` half of Swift
/// `performWithUsbmuxdService` (diff list D-1: defined in Swift but
/// uncalled there; provided here for parity and used by tests
/// against a full usbmuxd). Lists devices on the mux and takes the
/// first, exactly like the Swift helper.
pub async fn usbmuxd_provider(
    addr: UsbmuxdAddr,
    label: &str,
) -> Result<UsbmuxdProvider, MmErrorInfo> {
    let socket = addr.to_socket().await.map_err(|e| {
        gw_error(
            DeviceGatewayCode::ConnectionFailed,
            format!("Failed to connect to usbmuxd: {e}"),
        )
    })?;
    let mut conn = idevice::usbmuxd::UsbmuxdConnection::new(socket, 0);
    let devices = conn.get_devices().await.map_err(|e| {
        gw_error(
            DeviceGatewayCode::ConnectionFailed,
            format!("Failed to list usbmuxd devices: {e}"),
        )
    })?;
    let device = devices.into_iter().next().ok_or_else(|| {
        gw_error(
            DeviceGatewayCode::ConnectionFailed,
            "No devices found on usbmuxd",
        )
    })?;
    Ok(UsbmuxdProvider {
        addr,
        tag: 0,
        udid: device.udid,
        device_id: device.device_id,
        label: label.to_string(),
    })
}

/// Swift `IdeviceGateway.dumpProfiles`' file half: split each
/// misagent payload at its plist markers and write
/// `<UUID>.mobileprovision` (full payload) + `<UUID>.plist`
/// (plist bytes), or `unknown_<i>.mobileprovision` when the
/// plist carries no UUID; payloads without a plist section
/// are skipped; all write errors are swallowed, as in Swift.
fn dump_profiles_to_disk(docs_path: &str, payloads: &[Vec<u8>]) -> String {
    let docs = docs_path.strip_prefix("file://").unwrap_or(docs_path);
    let dump_dir = if docs.ends_with("/Profiles") || docs.ends_with("/Profiles/") {
        docs.to_string()
    } else {
        format!("{docs}/Profiles")
    };
    let _ = std::fs::create_dir_all(&dump_dir);
    for (i, bytes) in payloads.iter().enumerate() {
        let Some(start) = find_subslice(bytes, b"<?xml version=") else {
            continue;
        };
        let Some(end_rel) = find_subslice(&bytes[start..], b"</plist>") else {
            continue;
        };
        let plist_bytes = &bytes[start..start + end_rel + b"</plist>".len()];
        let uuid = plist::Value::from_reader(std::io::Cursor::new(plist_bytes))
            .ok()
            .and_then(|v| match v {
                plist::Value::Dictionary(d) => match d.get("UUID") {
                    Some(plist::Value::String(u)) => Some(u.clone()),
                    _ => None,
                },
                _ => None,
            });
        match uuid {
            Some(uuid) => {
                let _ = std::fs::write(
                    format!("{dump_dir}/{uuid}.mobileprovision"),
                    bytes,
                );
                let _ = std::fs::write(
                    format!("{dump_dir}/{uuid}.plist"),
                    plist_bytes,
                );
            }
            None => {
                let _ = std::fs::write(
                    format!("{dump_dir}/unknown_{i}.mobileprovision"),
                    bytes,
                );
            }
        }
    }
    dump_dir
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

#[cfg(test)]
mod coalescing_tests {
    use super::CoalescingStream;
    use std::io;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};

    /// Records every underlying write call verbatim.
    #[derive(Debug, Default)]
    struct MockStream {
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl AsyncRead for MockStream {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(())) // EOF; the write tests never read
        }
    }

    impl AsyncWrite for MockStream {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.writes.lock().unwrap().push(buf.to_vec());
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn first_eight_writes_coalesce_into_four() {
        let mock = MockStream::default();
        let log = mock.writes.clone();
        let mut stream = CoalescingStream::new(mock);
        let parts: [&[u8]; 8] = [
            b"HDR1",
            b"<plist>one</plist>",
            b"HDR2",
            b"<plist>two</plist>",
            b"HDR3",
            b"<plist>three</plist>",
            b"HDR4",
            b"<plist>four</plist>",
        ];
        for part in parts {
            stream.write_all(part).await.unwrap();
        }
        let writes = log.lock().unwrap();
        assert_eq!(writes.len(), 4, "8 writes must reach the wire as 4");
        for (i, pair) in writes.iter().enumerate() {
            let mut expect = parts[2 * i].to_vec();
            expect.extend_from_slice(parts[2 * i + 1]);
            assert_eq!(*pair, expect, "pair {i} must be header+body");
        }
    }

    #[tokio::test]
    async fn ninth_write_passes_through_immediately() {
        let mock = MockStream::default();
        let log = mock.writes.clone();
        let mut stream = CoalescingStream::new(mock);
        for i in 0..8u8 {
            stream.write_all(&[b'x' + i]).await.unwrap();
        }
        assert_eq!(log.lock().unwrap().len(), 4);
        stream.write_all(b"ninth").await.unwrap();
        let writes = log.lock().unwrap();
        assert_eq!(writes.len(), 5, "9th write must not wait for a pair");
        assert_eq!(writes[4], b"ninth".to_vec());
    }

    #[tokio::test]
    async fn flush_releases_a_held_half_write() {
        let mock = MockStream::default();
        let log = mock.writes.clone();
        let mut stream = CoalescingStream::new(mock);
        stream.write_all(b"HDR!").await.unwrap();
        assert!(log.lock().unwrap().is_empty(), "half must be held");
        stream.flush().await.unwrap();
        let writes = log.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0], b"HDR!".to_vec());
    }
}
