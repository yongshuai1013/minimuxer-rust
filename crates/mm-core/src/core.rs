//! `MinimuxerCore` — port of Swift `MinimuxerImpl`
//! (yongshuai1013/minimuxer @714aafa3), the facade behind
//! `MinimuxerAPI`: the start/stop/restart state machine, the
//! nine-gate `isReady`, the status bus fed by network changes,
//! and the `runWithChecks` error mapping every gateway call
//! flows through. Line-by-line behaviour is catalogued in
//! `docs/m4-core-diff.md`.
//!
//! Scope of this milestone (M4): the lifecycle + readiness +
//! heartbeat composition. The operation forwards whose gateway
//! counterparts land in M5 (install/AFC/profiles/debug) are not
//! here yet; `mount_ddi` keeps Swift's mount-task management
//! with the Mounter body still reporting unsupported (diff list
//! D-M4-4).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};

use mm_common::constants::REMOTE_PAIRING_PORT;
use mm_common::error::{DeviceGatewayError, MinimuxerError};
use mm_common::logging::{debug_log, verbose_log};
use mm_common::{MmErrorInfo, PairingProtocol};
use mm_gateway::{DeviceGateway, GatewayBackend, GatewayProvider};
use mm_net::{
    ConnectionConfigBinding, DeviceConnectionMode, NetStack, PollingObserver,
};
use tokio::task::JoinHandle;

use crate::heartbeat::{HeartbeatDeps, HeartbeatFuture, HeartbeatService, TokioClock};
use crate::usbmuxd_proxy::{
    retarget_usbmuxd_address, start_proxy_for_gateway, UsbmuxdProxyServer,
};

pub type CoreFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

// ---------------------------------------------------------------------------
// Provider + OS-version seams
// ---------------------------------------------------------------------------

/// Swift `DeviceProvider` plus the backend-switching half of
/// `Minimuxer.set`: the core always reads the *current* gateway
/// through the provider, so a switch takes effect immediately
/// for every subsystem.
pub trait CoreGatewayProvider: GatewayProvider {
    fn backend(&self) -> GatewayBackend;
    /// Builds a fresh gateway for `backend` and swaps it in as
    /// current (Swift `Minimuxer.set` + `DeviceProvider.set`).
    fn set_backend(&self, backend: GatewayBackend) -> Arc<dyn DeviceGateway>;
}

/// The `#available(iOS 26.4, *)` seam of isReady gate 4: Swift
/// evaluates the OS version at runtime; the port reads
/// `kern.osproductversion` via sysctl on Apple targets (DESIGN
/// risk 10). Off Apple targets — and whenever the version
/// cannot be read — every `at_least` is false, which is also
/// the behaviour on the iOS 15 target device.
pub trait OsVersionProvider: Send + Sync {
    fn at_least(&self, major: u32, minor: u32) -> bool;
}

/// The production [`OsVersionProvider`].
pub struct SystemOsVersion {
    cached: Mutex<Option<Option<(u32, u32, u32)>>>,
}

impl Default for SystemOsVersion {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemOsVersion {
    pub fn new() -> Self {
        Self {
            cached: Mutex::new(None),
        }
    }

    fn version(&self) -> Option<(u32, u32, u32)> {
        let mut slot = self.cached.lock().unwrap();
        if slot.is_none() {
            *slot = Some(read_os_product_version());
        }
        slot.unwrap()
    }
}

impl OsVersionProvider for SystemOsVersion {
    fn at_least(&self, major: u32, minor: u32) -> bool {
        match self.version() {
            Some((maj, min, _)) => maj > major || (maj == major && min >= minor),
            None => false,
        }
    }
}

/// Parses `kern.osproductversion` ("26.4.1") — used by the
/// sysctl reader on Apple targets; unit-tested everywhere.
#[cfg_attr(not(target_vendor = "apple"), allow(dead_code))]
fn parse_os_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.trim().split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

#[cfg(target_vendor = "apple")]
fn read_os_product_version() -> Option<(u32, u32, u32)> {
    let name = c"kern.osproductversion";
    let mut size: libc::size_t = 0;
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size];
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    let text = String::from_utf8_lossy(&buf);
    parse_os_version(text.trim_end_matches('\0'))
}

#[cfg(not(target_vendor = "apple"))]
fn read_os_product_version() -> Option<(u32, u32, u32)> {
    None
}

// ---------------------------------------------------------------------------
// runWithChecks (Swift MinimuxerImpl.runWithChecks)
// ---------------------------------------------------------------------------

/// The error half of Swift's `runWithChecks`: a
/// `DeviceGatewayError` maps through `asMinimuxerError`
/// (invalidPairingFile → invalidPairing, VPN-drop →
/// invalidVPN, else the caller's catch-all); a `MinimuxerError`
/// passes through untouched; anything else lands in the
/// catch-all with the error's ported description (diff list
/// D-M4-5). Swift additionally detaches the action at medium
/// priority (`matchingPriority`); that has no observable
/// semantics at this boundary and is not reproduced.
pub fn map_gateway_error(
    info: MmErrorInfo,
    active_protocol: PairingProtocol,
    catch_all: impl Fn(String) -> MinimuxerError,
) -> MinimuxerError {
    if let Some(gw) = DeviceGatewayError::from_info(&info) {
        return gw.as_minimuxer_error(active_protocol, catch_all);
    }
    if let Some(err) = MinimuxerError::from_info(&info) {
        return err;
    }
    catch_all(info.description())
}

/// `runWithChecks` for callers outside the core: runs `action`
/// and maps its error with the protocol in effect *after* the
/// action ran (Swift's catch-all closures read
/// `self.activeProtocol` at throw time).
pub async fn run_with_checks<T, F>(
    protocol_now: impl Fn() -> PairingProtocol,
    catch_all: impl Fn(String, PairingProtocol) -> MinimuxerError,
    action: F,
) -> Result<T, MinimuxerError>
where
    F: Future<Output = Result<T, MmErrorInfo>>,
{
    match action.await {
        Ok(value) => Ok(value),
        Err(info) => {
            let protocol = protocol_now();
            Err(map_gateway_error(info, protocol, |reason| {
                catch_all(reason, protocol)
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// Facade parameters (Swift MinimuxerParams)
// ---------------------------------------------------------------------------

/// Swift `MinimuxerParams`: every field optional; `set` applies
/// only the fields that are present *and* changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MinimuxerParams {
    pub backend: Option<GatewayBackend>,
    pub remote_pairing_port: Option<u16>,
    pub device_probe_timeout: Option<i32>,
}

/// Swift `MinimuxerStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreStatus {
    Started,
    InProgress,
    Stopped,
}

/// A status-bus subscriber (Swift `statusPublisher`): receives
/// the result of every network-change re-probe.
pub type StatusHandler = Arc<dyn Fn(Result<bool, MinimuxerError>) + Send + Sync>;

/// Swift `State` actor.
struct CoreState {
    status: CoreStatus,
    mount_task: Option<JoinHandle<Result<bool, MmErrorInfo>>>,
    last_docs_path: Option<String>,
    preferred_protocol: Option<PairingProtocol>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            status: CoreStatus::Stopped,
            mount_task: None,
            last_docs_path: None,
            preferred_protocol: None,
        }
    }
}

/// Swift `MinimuxerImpl`.
pub struct MinimuxerCore {
    provider: Arc<dyn CoreGatewayProvider>,
    stack: Arc<NetStack>,
    observer: Arc<PollingObserver>,
    proxy: Arc<UsbmuxdProxyServer>,
    heartbeat: Arc<HeartbeatService>,
    state: Mutex<CoreState>,
    os_version: Arc<dyn OsVersionProvider>,
    /// The facade's stored remote-pairing port (Swift
    /// `Minimuxer.currentRemotePairingPort`, initially the
    /// constant default).
    current_rp_port: Mutex<u16>,
    status_handlers: Mutex<Vec<StatusHandler>>,
}

impl MinimuxerCore {
    /// Builds the core and wires the status bus: every network
    /// change re-probes `is_ready` with the default arguments
    /// and publishes the result (Swift's `onNetworkChanged`).
    pub fn new(
        provider: Arc<dyn CoreGatewayProvider>,
        stack: Arc<NetStack>,
        observer: Arc<PollingObserver>,
        proxy: Arc<UsbmuxdProxyServer>,
        os_version: Arc<dyn OsVersionProvider>,
    ) -> Arc<Self> {
        let heartbeat = Arc::new(HeartbeatService::new(
            Arc::new(CoreHeartbeatDeps {
                provider: provider.clone(),
                stack: stack.clone(),
                proxy: proxy.clone(),
            }),
            Arc::new(TokioClock),
        ));
        let core = Arc::new(Self {
            provider,
            stack: stack.clone(),
            observer,
            proxy,
            heartbeat,
            state: Mutex::new(CoreState::default()),
            os_version,
            current_rp_port: Mutex::new(REMOTE_PAIRING_PORT),
            status_handlers: Mutex::new(Vec::new()),
        });
        let weak: Weak<MinimuxerCore> = Arc::downgrade(&core);
        stack.add_change_handler(Arc::new(move || {
            let weak = weak.clone();
            Box::pin(async move {
                if let Some(core) = weak.upgrade() {
                    let result = core.is_ready(true, false).await;
                    debug_log(
                        "[minimuxer] [net] publishing status update to subscribers",
                    );
                    core.publish_status(result);
                }
            })
        }));
        core
    }

    // -- accessors -----------------------------------------------------------

    /// The current gateway (Swift `deviceProvider.gateway` —
    /// always materialized; the provider seam guarantees an
    /// instance).
    pub fn gateway(&self) -> Arc<dyn DeviceGateway> {
        self.provider
            .current_gateway()
            .expect("core gateway provider must always yield a gateway")
    }

    /// Swift `activeProtocol`.
    pub fn active_protocol(&self) -> PairingProtocol {
        self.gateway().pairing_file_type()
    }

    /// Swift `pairingFileType`.
    pub fn pairing_file_type(&self) -> PairingProtocol {
        self.gateway().pairing_file_type()
    }

    /// Swift `isPairingFileLoaded`.
    pub fn is_pairing_file_loaded(&self) -> bool {
        self.pairing_file_type() != PairingProtocol::Unknown
    }

    /// Swift `isLoggingEnabled`.
    pub fn is_logging_enabled(&self) -> bool {
        mm_common::logging::is_logging_enabled()
    }

    /// The lifecycle status (Swift's internal `State.status`).
    pub fn status(&self) -> CoreStatus {
        self.state.lock().unwrap().status
    }

    /// The heartbeat service (Swift's `heartbeat`).
    pub fn heartbeat(&self) -> &Arc<HeartbeatService> {
        &self.heartbeat
    }

    /// Swift `describeError`.
    pub fn describe_error(&self, error: &MinimuxerError) -> String {
        error.description()
    }

    /// Swift `getConnectionMode`.
    pub fn get_connection_mode(&self) -> DeviceConnectionMode {
        self.stack.manager.get_preferred_connection_mode()
    }

    /// Swift `deviceProbeTimeout`.
    pub fn device_probe_timeout(&self) -> i32 {
        self.stack.manager.device_probe_timeout()
    }

    /// Swift `setDeviceProbeTimeout`.
    pub fn set_device_probe_timeout(&self, timeout_ms: i32) {
        self.stack.manager.set_device_probe_timeout(timeout_ms);
    }

    /// Swift `setLogging`.
    pub fn set_logging(&self, enabled: bool) {
        mm_common::logging::set_logging_enabled(enabled);
        self.gateway().set_logging(enabled);
    }

    /// Swift `bindConnectionConfig`: bind, then refresh the
    /// endpoint against the new binding.
    pub async fn bind_connection_config(&self, binding: ConnectionConfigBinding) {
        self.stack.manager.bind_connection_config(binding);
        self.stack.refresh_endpoint().await;
    }

    /// Swift `testDeviceConnection(ifaddr:timeout:)`: TCP probe
    /// of the device IP at the gateway's service port. `None`
    /// is Swift's no-timeout overload, which uses
    /// `deviceProbeTimeout`.
    pub fn test_device_connection(&self, ifaddr: &str, timeout_ms: Option<i32>) -> bool {
        let timeout = timeout_ms.unwrap_or_else(|| self.device_probe_timeout());
        mm_common::test_tcp(ifaddr, self.gateway().service_port(), timeout)
    }

    // -- status bus ----------------------------------------------------------

    /// Subscribes to status results (Swift `statusPublisher`;
    /// a PassthroughSubject: no replay of past values).
    pub fn subscribe_status(&self, handler: StatusHandler) {
        self.status_handlers.lock().unwrap().push(handler);
    }

    fn publish_status(&self, result: Result<bool, MinimuxerError>) {
        let handlers = self.status_handlers.lock().unwrap().clone();
        for handler in handlers {
            handler(result.clone());
        }
    }

    // -- facade set (Swift Minimuxer.set) -------------------------------------

    /// Swift `Minimuxer.set(MinimuxerParams)`: applies each
    /// present-and-changed field. A backend switch builds a
    /// fresh gateway and sets the facade's stored RP port into
    /// it (see diff list D-M4-2 for the hub interaction).
    pub fn set_params(&self, params: MinimuxerParams) {
        if let Some(backend) = params.backend {
            if backend != self.provider.backend() {
                let new_gateway = self.provider.set_backend(backend);
                let rp_port = *self.current_rp_port.lock().unwrap();
                new_gateway.set_port(rp_port, PairingProtocol::RpPairing);
            }
        }
        if let Some(port) = params.remote_pairing_port {
            let changed = {
                let mut stored = self.current_rp_port.lock().unwrap();
                if port != *stored {
                    *stored = port;
                    true
                } else {
                    false
                }
            };
            if changed {
                self.gateway().set_port(port, PairingProtocol::RpPairing);
            }
        }
        if let Some(timeout) = params.device_probe_timeout {
            if timeout != self.device_probe_timeout() {
                self.set_device_probe_timeout(timeout);
            }
        }
    }

    // -- lifecycle -------------------------------------------------------------

    /// Swift `start(pairingFile:mountPath:preferred:)`.
    pub async fn start(
        &self,
        pairing_file: &str,
        mount_path: &str,
        preferred: Option<PairingProtocol>,
    ) -> Result<(), MinimuxerError> {
        if self.get_connection_mode() == DeviceConnectionMode::NotConfigured {
            return Err(self.connection_not_configured_error());
        }
        // Swift `network.start()` (its Bool result is ignored).
        self.observer
            .start(&tokio::runtime::Handle::current());

        {
            let mut st = self.state.lock().unwrap();
            st.status = CoreStatus::InProgress;
            st.last_docs_path = Some(mount_path.to_string());
            st.preferred_protocol = preferred;
        }

        let gw = self.gateway();
        self.run_checks(gw.start(pairing_file, preferred), |reason, protocol| {
            MinimuxerError::InvalidPairing {
                protocol,
                reason,
            }
        })
        .await?;

        if gw.requires_usbmuxd() {
            // Not wrapped in runWithChecks in Swift:
            // restartMuxerServer throws MinimuxerError directly.
            retarget_usbmuxd_address();
            start_proxy_for_gateway(&self.proxy, &gw)
                .await
                .map_err(|info| {
                    MinimuxerError::from_info(&info).unwrap_or_else(|| {
                        MinimuxerError::Connect(info.description())
                    })
                })?;
        }

        self.state.lock().unwrap().status = CoreStatus::Started;

        // The heartbeat keep-alive lives and dies with
        // start()/stop() (714aafa3 keep-alive fix).
        self.heartbeat.start();
        Ok(())
    }

    /// Swift `stop()`.
    pub async fn stop(&self) -> Result<(), MinimuxerError> {
        // Stop the heartbeat before tearing down the transports
        // it beats over.
        self.heartbeat.stop().await;

        let old_task = {
            let mut st = self.state.lock().unwrap();
            st.status = CoreStatus::InProgress;
            st.mount_task.take()
        };
        if let Some(task) = old_task {
            task.abort();
            let _ = task.await;
        }

        if self.gateway().requires_usbmuxd() {
            self.proxy.stop().await;
        }

        let gw = self.gateway();
        self.run_checks(gw.stop(), |reason, _| MinimuxerError::Close(reason))
            .await?;

        self.state.lock().unwrap().status = CoreStatus::Stopped;
        Ok(())
    }

    /// Swift `restart()`.
    pub async fn restart(&self) -> Result<(), MinimuxerError> {
        verbose_log("[minimuxer] Restarting services...");
        let active_protocol = self.gateway().pairing_file_type();
        let pairing_file = self
            .gateway()
            .pairing_file_data()
            .and_then(|data| String::from_utf8(data).ok());
        let Some(pairing_file) = pairing_file else {
            debug_log("[minimuxer] restart: no existing pairing file — cannot restart");
            return Err(MinimuxerError::InvalidPairing {
                protocol: active_protocol,
                reason: "No existing pairing file found in gateway during restart"
                    .to_string(),
            });
        };
        self.restart_with(&pairing_file, "restart").await?;
        self.stack.refresh_endpoint().await;
        Ok(())
    }

    /// Swift `reinitializePairingData(pairingFile:)`.
    pub async fn reinitialize_pairing_data(
        &self,
        pairing_file: &str,
    ) -> Result<(), MinimuxerError> {
        verbose_log("[minimuxer] Reinitializing with new pairing file...");
        self.restart_with(pairing_file, "reinitializePairingData")
            .await
    }

    /// Swift `restartWith(pairingFile:op:)`.
    async fn restart_with(
        &self,
        pairing_file: &str,
        op: &str,
    ) -> Result<(), MinimuxerError> {
        let (mount_path, preferred) = {
            let st = self.state.lock().unwrap();
            (st.last_docs_path.clone(), st.preferred_protocol)
        };
        let Some(mount_path) = mount_path else {
            return Err(MinimuxerError::Mount {
                protocol: self.gateway().pairing_file_type(),
                reason: format!(
                    "start() should be invoked before requesting {op}. cause: lastDocsPath is nil"
                ),
            });
        };
        self.stop().await?;
        self.start(pairing_file, &mount_path, preferred).await
    }

    // -- readiness -------------------------------------------------------------

    /// Swift `isReady(withNetworkCheck:withDDIMountCheck:)` —
    /// the nine gates, in Swift's order, with Swift's reasons
    /// (diff list §2).
    pub async fn is_ready(
        &self,
        with_network_check: bool,
        with_ddi_mount_check: bool,
    ) -> Result<bool, MinimuxerError> {
        // Gate 1: pairing file loaded.
        if !self.is_pairing_file_loaded() {
            debug_log("[minimuxer] minimuxer not ready: pairing file not loaded");
            return Err(MinimuxerError::PairingNotLoaded(
                "No valid pairing file has been loaded".to_string(),
            ));
        }

        // Gate 2: started.
        if self.status() != CoreStatus::Started {
            debug_log("[minimuxer] minimuxer not ready: minimuxer has not been started");
            return Err(MinimuxerError::NotStarted(
                "Minimuxer has not been started".to_string(),
            ));
        }

        let snap = self.stack.snapshot();

        // Gate 3: network check — wifi only; the wired/usb/bridge
        // clauses are commented out in the Swift source and are
        // reproduced as absent here.
        if with_network_check && !mm_net::is_wifi_satisfied(&snap) {
            debug_log("[minimuxer] minimuxer not ready: no network connection");
            return Err(MinimuxerError::NoConnection(
                "No wifi interface satisfied".to_string(),
            ));
        }

        // Gate 4: connection mode.
        let connection_mode = self.get_connection_mode();
        match connection_mode {
            DeviceConnectionMode::NotConfigured => {
                return Err(self.connection_not_configured_error());
            }
            DeviceConnectionMode::LocalVpn => {
                if !mm_net::is_utun_available(&snap.interfaces) {
                    debug_log("[minimuxer] minimuxer not ready: no utun interface found");
                    return Err(MinimuxerError::NoVPN(
                        "No utun interface detected — LocalDevVPN is not connected"
                            .to_string(),
                    ));
                }
                // Lockdown on iOS 26.4+ additionally requires an
                // ipsec interface (Swift checks the interface
                // first, then the #available gate).
                if self.gateway().pairing_file_type() != PairingProtocol::RpPairing
                    && !mm_net::is_ikev2_ipsec_available(&snap.interfaces)
                    && self.os_version.at_least(26, 4)
                {
                    debug_log("[minimuxer] minimuxer not ready: no ipsec interface (required for lockdown on iOS 26.4+)");
                    return Err(MinimuxerError::InvalidVPN(
                        "utun is present but no ipsec/IKEv2 interface found — LocalDevVPN may not support the lockdown protocol on iOS 26.4+".to_string(),
                    ));
                }
            }
            DeviceConnectionMode::RemoteServer => {}
        }

        // Gate 5: pairing file still loaded (verbatim second
        // check, with its own reason string).
        if self.pairing_file_type() == PairingProtocol::Unknown {
            debug_log("[minimuxer] minimuxer not ready: no valid pairing file loaded");
            return Err(MinimuxerError::PairingNotLoaded(
                "No valid pairing file has been loaded in Minimuxer".to_string(),
            ));
        }

        // Gate 6: endpoint initialized.
        let device_ip = match self.stack.endpoint.ip() {
            Ok(ip) => ip,
            Err(err) => {
                return Err(match connection_mode {
                    DeviceConnectionMode::LocalVpn => {
                        debug_log("[minimuxer] minimuxer not ready: tunnel peer IP not available despite tunnel iface being present");
                        MinimuxerError::NoDevice(format!(
                            "VPN tunnel iface is up but tunnel peer IP is not yet reachable — VPN may not be routing device traffic correctly. Cause: {}",
                            err.description()
                        ))
                    }
                    DeviceConnectionMode::RemoteServer => {
                        debug_log("[minimuxer] minimuxer not ready: remote endpoint IP is not configured or reachable");
                        MinimuxerError::NoDevice(format!(
                            "Remote endpoint IP is not configured or reachable. Cause: {}",
                            err.description()
                        ))
                    }
                    DeviceConnectionMode::NotConfigured => {
                        self.connection_not_configured_error()
                    }
                });
            }
        };

        // Gate 7: TCP poll of the peer.
        if !self.test_device_connection(&device_ip, None) {
            return Err(match connection_mode {
                DeviceConnectionMode::LocalVpn => {
                    debug_log("[minimuxer] minimuxer not ready: failed to connect to tunnel peer IP");
                    MinimuxerError::InvalidVPN(format!(
                        "VPN tunnel iface is up and tunnel peer IP {device_ip} is known, but TCP port poll failed — device may be unreachable on this interface"
                    ))
                }
                DeviceConnectionMode::RemoteServer => {
                    debug_log(&format!("[minimuxer] minimuxer not ready: failed to connect to remote endpoint IP {device_ip}"));
                    MinimuxerError::NotReachable(format!(
                        "Remote endpoint {device_ip} is configured, but TCP port poll failed — target device is unreachable"
                    ))
                }
                DeviceConnectionMode::NotConfigured => {
                    self.connection_not_configured_error()
                }
            });
        }

        // Gate 8: a real UDID read through the gateway; the
        // (already mapped) error propagates as-is.
        let _device_udid = self.fetch_udid().await?;

        // Gate 9: the usbmuxd proxy must be listening for the
        // backends that require it.
        if self.gateway().requires_usbmuxd() && !self.proxy.is_listening() {
            return Err(MinimuxerError::MuxerNotListening(
                "Usbmuxd fake server is not listening".to_string(),
            ));
        }

        if with_ddi_mount_check {
            return self.is_ddi_mounted().await;
        }

        Ok(true)
    }

    // -- gateway forwards used by the readiness chain ---------------------------

    /// Swift `fetchUDID()`.
    pub async fn fetch_udid(&self) -> Result<String, MinimuxerError> {
        let gw = self.gateway();
        self.run_checks(gw.fetch_udid(), |reason, _| {
            MinimuxerError::FetchUDID(reason)
        })
        .await
    }

    /// Swift `isDDIMounted()`.
    pub async fn is_ddi_mounted(&self) -> Result<bool, MinimuxerError> {
        let gw = self.gateway();
        self.run_checks(gw.is_ddi_mounted(), |reason, protocol| {
            MinimuxerError::Mount { protocol, reason }
        })
        .await
    }

    /// Swift `mountDDI(docsPath:)`. The mount-task management
    /// (record the docs path, cancel + await the previous task,
    /// store the new one) is Swift's; the task body is the
    /// Mounter, which lands in M5 — until then it reports the
    /// gateway's unsupportedOperation through the usual mapping
    /// (diff list D-M4-4).
    pub async fn mount_ddi(&self, docs_path: &str) -> Result<bool, MinimuxerError> {
        let docs_path = docs_path.to_string();
        self.run_checks(
            async {
                let old_task = {
                    let mut st = self.state.lock().unwrap();
                    st.last_docs_path = Some(docs_path.clone());
                    st.mount_task.take()
                };
                if let Some(task) = old_task {
                    task.abort();
                    let _ = task.await;
                }
                let task = tokio::spawn(async move {
                    Err::<bool, MmErrorInfo>(mm_gateway::unsupported("mountDDI"))
                });
                let result = task.await.unwrap_or_else(|e| {
                    Err(MmErrorInfo::new(
                        "MinimuxerInternalError",
                        "unknown",
                        &format!("mount task failed: {e}"),
                    ))
                });
                // The task already completed; Swift stores it so
                // stop()/mountDDI can cancel the *next* one — a
                // finished handle is kept for shape fidelity.
                let stored = result.clone();
                self.state.lock().unwrap().mount_task =
                    Some(tokio::spawn(async move { stored }));
                result
            },
            |reason, protocol| MinimuxerError::Mount { protocol, reason },
        )
        .await
    }

    // -- helpers ---------------------------------------------------------------

    /// Swift `connectionNotConfiguredError()`.
    fn connection_not_configured_error(&self) -> MinimuxerError {
        debug_log("[minimuxer] minimuxer not ready: connection mode not configured. Supported modes: [localVPN, remoteServer]");
        MinimuxerError::ConnectionModeNotConfigured(
            "Connection mode not configured. Supported modes: [localVPN, remoteServer]"
                .to_string(),
        )
    }

    /// `runWithChecks` bound to this core: the active protocol
    /// is read when the error is mapped, like Swift.
    async fn run_checks<T>(
        &self,
        action: impl Future<Output = Result<T, MmErrorInfo>>,
        catch_all: impl Fn(String, PairingProtocol) -> MinimuxerError,
    ) -> Result<T, MinimuxerError> {
        run_with_checks(|| self.active_protocol(), catch_all, action).await
    }
}

// ---------------------------------------------------------------------------
// Heartbeat deps over the real subsystems
// ---------------------------------------------------------------------------

struct CoreHeartbeatDeps {
    provider: Arc<dyn CoreGatewayProvider>,
    stack: Arc<NetStack>,
    proxy: Arc<UsbmuxdProxyServer>,
}

impl HeartbeatDeps for CoreHeartbeatDeps {
    fn requires_usbmuxd(&self) -> bool {
        self.provider
            .current_gateway()
            .map(|gw| gw.requires_usbmuxd())
            .unwrap_or(false)
    }

    fn proxy_listening(&self) -> bool {
        self.proxy.is_listening()
    }

    fn endpoint_ip(&self) -> Option<String> {
        self.stack.endpoint.ip().ok()
    }

    fn probe(&self, ip: &str) -> bool {
        let port = self
            .provider
            .current_gateway()
            .map(|gw| gw.service_port())
            .unwrap_or(62078);
        mm_common::test_tcp_default(ip, port)
    }

    fn perform_heartbeat<'a>(
        &'a self,
        interval: u64,
    ) -> HeartbeatFuture<'a, Result<u64, MmErrorInfo>> {
        let gw = self.provider.current_gateway();
        Box::pin(async move {
            match gw {
                Some(gw) => gw.perform_heartbeat(interval).await,
                None => Err(mm_gateway::unsupported("performHeartbeat")),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::parse_os_version;

    #[test]
    fn os_version_parsing() {
        assert_eq!(parse_os_version("26.4.1"), Some((26, 4, 1)));
        assert_eq!(parse_os_version("26.4"), Some((26, 4, 0)));
        assert_eq!(parse_os_version("15.8.8"), Some((15, 8, 8)));
        assert_eq!(parse_os_version(" 17.0 "), Some((17, 0, 0)));
        assert_eq!(parse_os_version(""), None);
        assert_eq!(parse_os_version("x.y"), None);
    }
}
