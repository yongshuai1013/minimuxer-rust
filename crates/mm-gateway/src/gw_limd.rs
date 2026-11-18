//! `LibimobiledeviceGateway` — port of Swift
//! `LibimobiledeviceGateway` (yongshuai1013/minimuxer @714aafa3)
//! over rusty_libimobiledevice (feature `limd`, diff list D-7).
//!
//! M3 covers the lockdown basic chain: `start`/`stop`,
//! `fetch_udid` (live query with cached-UDID fallback — this
//! backend's own semantics, distinct from gw-idevice),
//! `get_lockdown_value`, session proof via the handshake client,
//! and `open_service` (lockdownd_start_service). Every operation
//! runs the Swift `withDevice` → `withLockdown` → `withService`
//! chain on a blocking thread: `idevice_new_with_options`
//! (USBMUX|NETWORK lookup by UDID; in-App the usbmuxd lookups are
//! retargeted to the mm-core proxy via `USBMUXD_SOCKET_ADDRESS`),
//! then `lockdownd_client_new_with_handshake`.
//!
//! Error reasons embed the C error code, exactly like the Swift
//! code's `code \(err.rawValue)` formats; the numeric codes are
//! recovered from the wrapper's error enums via the reverse tables
//! below (the wrapper itself only keeps the enum).

use std::sync::Mutex;

use mm_common::constants::APP_NAME;
use mm_common::logging::debug_log;
use mm_common::{DeviceGatewayCode, MmErrorInfo, PairingProtocol};
use idevice::provider::IdeviceProvider as _;
use rusty_libimobiledevice::error::{IdeviceError as LimdIdeviceError, LockdowndError};

use crate::base::GatewayCore;
use crate::gateway::{gw_error, DeviceGateway, GatewayBackend, GatewayFuture, OpenedService};

/// Swift `LibimobiledeviceGateway` (`requiresUsbmuxd = true`).
pub struct LibimobiledeviceGateway {
    core: GatewayCore,
    /// Swift's `cachedUDID`: set from the pairing file at `start`,
    /// refreshed by successful live UDID queries, and part of this
    /// backend's `verifyInitialized` condition.
    cached_udid: Mutex<Option<String>>,
    /// The loaded RemotePairing identity (M7; Swift's
    /// `rpIdentity`).
    rp_identity: Mutex<Option<mm_rp::RpIdentity>>,
    /// Swift's `activeTunnel`/`activeRsd` pair, as one cached
    /// mm-rp session (Swift `cleanupRPTunnel` = dropping it).
    rp_session: Mutex<Option<RpSessionSlot>>,
    /// Open RSD service streams by service identifier.
    rp_services: Mutex<std::collections::HashMap<String, RpServiceSlot>>,
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

impl Default for LibimobiledeviceGateway {
    fn default() -> Self {
        Self::new()
    }
}

impl LibimobiledeviceGateway {
    pub fn new() -> Self {
        Self {
            core: GatewayCore::new(),
            cached_udid: Mutex::new(None),
            rp_identity: Mutex::new(None),
            rp_session: Mutex::new(None),
            rp_services: Mutex::new(std::collections::HashMap::new()),
        }
    }

    fn cleanup(&self) {
        debug_log("[LibimobiledeviceGateway] cleanup() called");
        *self.cached_udid.lock().unwrap() = None;
        *self.rp_identity.lock().unwrap() = None;
        *self.rp_session.lock().unwrap() = None;
        self.rp_services.lock().unwrap().clear();
        self.core.cleanup();
    }

    /// Swift `verifyInitialized()` — stricter than gw-idevice:
    /// requires a cached UDID as well as the flag.
    fn verify_initialized(&self) -> Result<String, MmErrorInfo> {
        let cached = self.cached_udid.lock().unwrap().clone();
        match (self.core.is_initialized(), cached) {
            (true, Some(udid)) => Ok(udid),
            _ => Err(gw_error(DeviceGatewayCode::NotInitialized, "")),
        }
    }

    // -- M7: RemotePairing session management ----------------

    /// The RP-mode verification (diff list D-M7-1): Swift's
    /// `verifyInitialized` can never pass in RP mode (its
    /// cachedUDID is unset there and the only writer is
    /// itself gated) — the port verifies the RP preconditions
    /// directly: initialized, with an identity loaded.
    fn verify_initialized_rp(&self) -> Result<mm_rp::RpIdentity, MmErrorInfo> {
        let identity = self.rp_identity.lock().unwrap().clone();
        match (self.core.is_initialized(), identity) {
            (true, Some(identity)) => Ok(identity),
            _ => Err(gw_error(DeviceGatewayCode::NotInitialized, "")),
        }
    }

    fn is_rp_mode(&self) -> bool {
        self.core.pairing_file_type() == PairingProtocol::RpPairing
    }

    /// Swift `withRPTunnel` + `getActiveRSD`: the cached
    /// session while the core generation is unchanged,
    /// otherwise a fresh establishment (whose pair-verify
    /// client is created inside the establishment and dropped
    /// with it — Swift `withRPClient`'s per-call freshness).
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
        let identity = self.verify_initialized_rp()?;
        let endpoint = self
            .core
            .device_endpoint_ip()
            .filter(|ip| !ip.is_empty())
            .ok_or_else(|| {
                gw_error(DeviceGatewayCode::DeviceEndpointIpNotAvailable, "")
            })?;
        let port = self.core.get_port(PairingProtocol::RpPairing);
        let session = mm_rp::RpSession::establish(&endpoint, port, APP_NAME, &identity)
            .await
            .map_err(|e| match e.stage {
                "pair_verify" => gw_error(
                    DeviceGatewayCode::ConnectionFailed,
                    format!("rppairing_pair_verify failed: {}", e.message),
                ),
                "create_tunnel_listener" => gw_error(
                    DeviceGatewayCode::ConnectionFailed,
                    format!(
                        "rppairing_create_tunnel_listener failed: {}",
                        e.message
                    ),
                ),
                "tunnel_connect" => gw_error(
                    DeviceGatewayCode::ConnectionFailed,
                    format!("rppairing_tunnel_connect failed: {}", e.message),
                ),
                "rsd_connect" => gw_error(
                    DeviceGatewayCode::ConnectionFailed,
                    format!("Failed to connect to RSD: {}", e.message),
                ),
                _ => gw_error(
                    DeviceGatewayCode::ConnectionFailed,
                    format!("Failed to establish RemotePairing tunnel: {}", e.message),
                ),
            })?;
        let handle: crate::gw_idevice_rp::RpSessionHandle =
            std::sync::Arc::new(tokio::sync::Mutex::new(session));
        *self.rp_session.lock().unwrap() = Some(RpSessionSlot {
            session: handle.clone(),
            generation,
        });
        Ok(handle)
    }

    /// Swift `cleanupRPTunnel` (via `invalidateConnection`).
    fn invalidate_rp_session(&self) {
        *self.rp_session.lock().unwrap() = None;
        self.rp_services.lock().unwrap().clear();
    }
}

/// Reverse of the wrapper's `From<i32> for LockdowndError` table,
/// so Swift's `code <n>` reason formats survive the round trip.
pub(crate) fn lockdownd_error_code(err: &LockdowndError) -> i32 {
    use LockdowndError::*;
    match err {
        Success => 0,
        InvalidArg => -1,
        InvalidConf => -2,
        PlistError => -3,
        PairingFailed => -4,
        SslError => -5,
        DictError => -6,
        RecieveTimeout => -7,
        MuxError => -8,
        NoRunningSession => -9,
        InvalidResponse => -10,
        MissingKey => -11,
        MissingValue => -12,
        GetProhibited => -13,
        SetProhibited => -14,
        RemoveProhibited => -15,
        ImmutableValue => -16,
        PasswordProtected => -17,
        UserDeniedPairing => -18,
        PairingDialogueRepsonsePending => -19,
        MissingHostId => -20,
        InvalidHostId => -21,
        SessionActive => -22,
        SessionInactive => -23,
        MissingSessionId => -24,
        InvalidSessionId => -25,
        MissingService => -26,
        InvalidService => -27,
        ServiceLimit => -28,
        MissingPairRecord => -29,
        SavePairRecordFailed => -30,
        InvalidPairRecord => -31,
        InvalidActivationRecord => -32,
        MissingActivationRecord => -33,
        ServiceProhibited => -34,
        EscrowLocked => -35,
        PairingProhibitedOverThisConnection => -36,
        FmipProtected => -37,
        McProtected => -38,
        McChallengeRequired => -39,
        MissingObjectDepenency => -100,
        UnknownError => -999,
    }
}

/// Reverse of the wrapper's `From<i32> for IdeviceError` table
/// (idevice.h). `UnknownError` has no recoverable code; -999 marks
/// the lossy round trip.
pub(crate) fn idevice_error_code(err: &LimdIdeviceError) -> i32 {
    use LimdIdeviceError::*;
    match err {
        Success => 0,
        InvalidArg => -1,
        NoDevice => -3,
        NotEnoughData => -4,
        ConnRefused => -5,
        SslError => -6,
        Timeout => -7,
        UnknownError => -999,
    }
}

/// Swift `withDevice` + `withLockdown`: look the device up by UDID
/// and open a handshake lockdownd client. Runs entirely on the
/// calling (blocking) thread; all C handles are created and dropped
/// inside the closure.
fn with_lockdown<T>(
    udid: &str,
    body: impl FnOnce(
        rusty_libimobiledevice::services::lockdownd::LockdowndClient<'_>,
    ) -> Result<T, MmErrorInfo>,
) -> Result<T, MmErrorInfo> {
    let device = rusty_libimobiledevice::idevice::get_device(udid).map_err(|e| {
        gw_error(
            DeviceGatewayCode::ConnectionFailed,
            format!(
                "idevice_new_with_options failed with code {}",
                idevice_error_code(&e)
            ),
        )
    })?;
    let client = device
        .new_lockdownd_client(APP_NAME)
        .map_err(|e| {
            gw_error(
                DeviceGatewayCode::ConnectionFailed,
                format!(
                    "lockdownd_client_new_with_handshake failed with code {}",
                    lockdownd_error_code(&e)
                ),
            )
        })?;
    body(client)
}

/// Reverse of the wrapper's `From<i32> for HeartbeatError` table,
/// for Swift's `code <n>` reason formats. `UnknownError` has no
/// recoverable code; -999 marks the lossy round trip (same
/// convention as the lockdownd/idevice tables above).
fn heartbeat_error_code(err: &rusty_libimobiledevice::error::HeartbeatError) -> i32 {
    use rusty_libimobiledevice::error::HeartbeatError::*;
    match err {
        Success => 0,
        InvalidArg => -1,
        PlistError => -2,
        MuxError => -3,
        SslError => -4,
        NotEnoughData => -5,
        Timeout => -6,
        UnknownError => -999,
    }
}

/// Reads the `Interval` value out of a heartbeat response plist
/// (Swift reads the uint node; a non-integer or missing node
/// yields `None` and the caller falls back to the input
/// interval).
fn interval_from_heartbeat_response(resp: &plist_plus::Plist) -> Option<u64> {
    let item = resp.dict_get_item("Interval").ok()?;
    if item.plist_type != plist_plus::PlistType::Integer {
        return None;
    }
    item.get_uint_val().ok()
}

/// Swift `syncPerformHeartbeat` (lockdown branch): Pico plist
/// out, response `Interval` back. Runs on a blocking thread.
fn limd_perform_heartbeat(udid: &str, interval: u64) -> Result<u64, MmErrorInfo> {
    let device = rusty_libimobiledevice::idevice::get_device(udid).map_err(|e| {
        gw_error(
            DeviceGatewayCode::ConnectionFailed,
            format!(
                "idevice_new_with_options failed with code {}",
                idevice_error_code(&e)
            ),
        )
    })?;
    let client =
        rusty_libimobiledevice::services::heartbeat::HeartbeatClient::new(&device, APP_NAME)
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!(
                        "Failed to create client for 'com.apple.mobile.heartbeat': code {}",
                        heartbeat_error_code(&e)
                    ),
                )
            })?;
    let mut pico = plist_plus::Plist::new_dict();
    let _ = pico.dict_set_item("Command", plist_plus::Plist::new_string("Pico"));
    client.send(pico).map_err(|e| {
        gw_error(
            DeviceGatewayCode::ServiceError,
            format!(
                "heartbeat_send failed with code {}",
                heartbeat_error_code(&e)
            ),
        )
    })?;
    // Swift `heartbeat_receive_with_timeout(..., 5000)`. The
    // wrapper collapses "error code + non-null plist" into Err
    // (diff list D-M4-7); Swift reads the plist first there.
    let resp = client.receive(5000).map_err(|e| {
        gw_error(
            DeviceGatewayCode::ServiceError,
            format!(
                "heartbeat_receive_with_timeout failed with code {}",
                heartbeat_error_code(&e)
            ),
        )
    })?;
    let new_interval = interval_from_heartbeat_response(&resp).unwrap_or(interval);
    Ok(if new_interval > 0 {
        new_interval
    } else {
        interval
    })
}

/// Swift `syncGetLockdownValue` (lockdown branch).
fn limd_get_value(udid: &str, key: &str) -> Result<String, MmErrorInfo> {
    with_lockdown(udid, |client| {
        let value = client.get_value(key, "").map_err(|e| {
            gw_error(
                DeviceGatewayCode::ServiceError,
                format!(
                    "Failed to get lockdown value for key '{key}': code {}",
                    lockdownd_error_code(&e)
                ),
            )
        })?;
        value.get_string_val().map_err(|_| {
            gw_error(
                DeviceGatewayCode::ServiceError,
                format!("Lockdown value for key '{key}' could not be decoded as string"),
            )
        })
    })
}

impl DeviceGateway for LibimobiledeviceGateway {
    fn backend(&self) -> GatewayBackend {
        GatewayBackend::Libimobiledevice
    }

    fn core(&self) -> &GatewayCore {
        &self.core
    }

    fn set_logging(&self, enabled: bool) {
        self.core.set_logging(enabled);
        // Swift also sets the RP logger level; the RP stack
        // logs through the `log` facade, whose level is a host
        // concern (same disposition as gw-idevice, D-9).
        rusty_libimobiledevice::idevice::set_debug(enabled);
    }

    fn start<'a>(
        &'a self,
        pairing_file_content: &'a str,
        preferred: Option<PairingProtocol>,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            debug_log("[LibimobiledeviceGateway] start() called");
            self.cleanup();
            // Swift rethrows the parser's PairingError unwrapped.
            let parsed = mm_common::parse_pairing_content(pairing_file_content, preferred)
                .map_err(|e| e.to_info())?;
            self.core
                .set_pairing_file_data(Some(parsed.raw_data().to_vec()));
            self.core.set_pairing_file_type(parsed.mode());
            match &parsed {
                mm_common::PairingFile::Lockdown(lockdown) => {
                    if lockdown.udid.is_empty() {
                        return Err(gw_error(
                            DeviceGatewayCode::InvalidPairingFile,
                            "Missing UDID in pairing file",
                        ));
                    }
                    *self.cached_udid.lock().unwrap() = Some(lockdown.udid.clone());
                }
                mm_common::PairingFile::Rp(_) => {
                    // M7: load the RP identity (Swift's
                    // `rppairing_identity_from_plist` at start).
                    let identity = mm_rp::RpIdentity::load(parsed.raw_data())
                        .map_err(|e| {
                            debug_log(&format!(
                                "[LibimobiledeviceGateway] start() rppairing_identity_from_plist failed: {}",
                                e.message
                            ));
                            gw_error(
                                DeviceGatewayCode::InvalidPairingFile,
                                "rppairing_identity_from_plist failed",
                            )
                        })?;
                    *self.rp_identity.lock().unwrap() = Some(identity);
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
            if self.is_rp_mode() {
                // Swift (RP branch): live GetValue first,
                // cached UDID on failure or empty.
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                let live =
                    crate::gw_limd_rp::get_lockdown_value(&session, "UniqueDeviceID")
                        .await;
                return match live {
                    Ok(value) if !value.is_empty() => {
                        *self.cached_udid.lock().unwrap() = Some(value.clone());
                        Ok(value)
                    }
                    _ => {
                        let cached = self.cached_udid.lock().unwrap().clone();
                        match cached {
                            Some(cached) if !cached.is_empty() => Ok(cached),
                            _ => Err(gw_error(
                                DeviceGatewayCode::ServiceError,
                                "UniqueDeviceID not found on device",
                            )),
                        }
                    }
                };
            }
            let udid = self.verify_initialized()?;
            // Swift: try the live query first; on failure *or* an
            // empty value, fall back to the cached UDID.
            let live = tokio::task::spawn_blocking(move || {
                limd_get_value(&udid, "UniqueDeviceID")
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?;
            match live {
                Ok(value) if !value.is_empty() => {
                    *self.cached_udid.lock().unwrap() = Some(value.clone());
                    Ok(value)
                }
                _ => {
                    let cached = self.cached_udid.lock().unwrap().clone();
                    match cached {
                        Some(cached) if !cached.is_empty() => Ok(cached),
                        _ => Err(gw_error(
                            DeviceGatewayCode::ServiceError,
                            "UniqueDeviceID not found on device",
                        )),
                    }
                }
            }
        })
    }

    fn get_lockdown_value<'a>(&'a self, key: &'a str) -> GatewayFuture<'a, String> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                return crate::gw_limd_rp::get_lockdown_value(&session, key).await;
            }
            let udid = self.verify_initialized()?;
            let key = key.to_string();
            tokio::task::spawn_blocking(move || limd_get_value(&udid, &key))
                .await
                .map_err(|e| {
                    gw_error(
                        DeviceGatewayCode::ServiceError,
                        format!("lockdown task failed: {e}"),
                    )
                })?
        })
    }

    fn start_session<'a>(&'a self) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            if self.is_rp_mode() {
                // RP analog: establishing the tunnel + RSD
                // session is the proof on this route.
                self.verify_initialized_rp()?;
                return self.rp_session().await.map(|_| ());
            }
            let udid = self.verify_initialized()?;
            // The handshake client *is* the session on this backend;
            // prove it with the UDID query Swift's client performs
            // implicitly, then drop it (Swift caches no lockdown
            // session either — every operation re-handshakes).
            tokio::task::spawn_blocking(move || {
                with_lockdown(&udid, |client| {
                    client.get_device_udid().map(|_| ()).map_err(|e| {
                        gw_error(
                            DeviceGatewayCode::ConnectionFailed,
                            format!(
                                "lockdownd_client_new_with_handshake failed with code {}",
                                lockdownd_error_code(&e)
                            ),
                        )
                    })
                })
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn stop_session<'a>(&'a self) -> GatewayFuture<'a, ()> {
        // No cached lockdown session exists on this backend
        // (see above); the RP session cache is dropped.
        Box::pin(async move {
            self.invalidate_rp_session();
            Ok(())
        })
    }

    fn perform_heartbeat<'a>(&'a self, interval: u64) -> GatewayFuture<'a, u64> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                return crate::gw_limd_rp::perform_heartbeat(&session, interval)
                    .await;
            }
            let udid = self.verify_initialized()?;
            tokio::task::spawn_blocking(move || limd_perform_heartbeat(&udid, interval))
                .await
                .map_err(|e| {
                    gw_error(
                        DeviceGatewayCode::ServiceError,
                        format!("lockdown task failed: {e}"),
                    )
                })?
        })
    }

    fn open_service<'a>(&'a self, service_name: &'a str) -> GatewayFuture<'a, OpenedService> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
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
                let generation = self.core.generation();
                self.rp_services.lock().unwrap().insert(
                    service_name.to_string(),
                    RpServiceSlot { stream, generation },
                );
                return Ok(OpenedService {
                    service_name: service_name.to_string(),
                    port,
                    enable_ssl: false,
                });
            }
            let udid = self.verify_initialized()?;
            let service_name_owned = service_name.to_string();
            let opened = tokio::task::spawn_blocking(move || {
                with_lockdown(&udid, |mut client| {
                    let service = client
                        .start_service(service_name_owned.clone(), false)
                        .map_err(|e| {
                            gw_error(
                                DeviceGatewayCode::ServiceError,
                                format!(
                                    "Failed to start lockdown service '{}': code {}",
                                    service_name_owned,
                                    lockdownd_error_code(&e)
                                ),
                            )
                        })?;
                    Ok(OpenedService {
                        service_name: service_name_owned.clone(),
                        // The wrapper's start_service leaves the
                        // descriptor port unread (it stores 0);
                        // typed clients consume the descriptor
                        // directly in M5, so the port is not
                        // surfaced on this backend.
                        port: service.port as u16,
                        enable_ssl: false,
                    })
                })
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?;
            opened
        })
    }

    // ------------------------------------------------------------------
    // M5: DDI mount, install pipeline, profiles, debug
    // (docs/m5-install-ddi-debug-diff.md §4). M7: the RP
    // routes dispatch to gw_limd_rp through the cached
    // session; the AFC queries keep the lockdown route only
    // (D-M7-4).
    // ------------------------------------------------------------------

    fn is_ddi_mounted<'a>(&'a self) -> GatewayFuture<'a, bool> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                return Ok(crate::gw_limd_rp::is_ddi_mounted(&session).await);
            }
            let udid = self.verify_initialized()?;
            let mounted = tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::is_ddi_mounted(&udid)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?;
            Ok(mounted)
        })
    }

    fn mount_developer_image<'a>(
        &'a self,
        image: &'a [u8],
        signature: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                return crate::gw_limd_rp::mount_developer_image(
                    &session, image, signature,
                )
                .await;
            }
            let udid = self.verify_initialized()?;
            let image = image.to_vec();
            let signature = signature.to_vec();
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::mount_developer_image(&udid, &image, &signature)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn mount_personalized_ddi<'a>(
        &'a self,
        image: &'a [u8],
        trustcache: &'a [u8],
        build_manifest: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                return crate::gw_limd_rp::mount_personalized_ddi(
                    &session,
                    image,
                    trustcache,
                    build_manifest,
                )
                .await;
            }
            let _udid = self.verify_initialized()?;
            // D-M5-7: the wrapper has no
            // mount_image_with_options, so the personalized
            // mount goes through the idevice crate over a
            // UsbmuxdProvider on the proxy (this backend's
            // device route), reusing gw-idevice's flow.
            let mux_addr = crate::gw_idevice::usbmuxd_addr_from_env(
                std::env::var(mm_common::constants::USBMUXD_ENV_KEY)
                    .ok()
                    .as_deref(),
            );
            let provider = crate::gw_idevice::usbmuxd_provider(mux_addr, APP_NAME)
                .await?;
            let pairing_file = provider
                .get_pairing_file()
                .await
                .map_err(|e| {
                    gw_error(
                        DeviceGatewayCode::ServiceError,
                        format!("Failed to load pairing file, error: ({e})"),
                    )
                })?;
            crate::gw_idevice_ops::mount_personalized_with_provider(
                &provider,
                &pairing_file,
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
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                return crate::gw_limd_rp::misagent_install(&session, profile).await;
            }
            let udid = self.verify_initialized()?;
            let profile = profile.to_vec();
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::install_profile(&udid, &profile)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn remove_provisioning_profile<'a>(
        &'a self,
        profile_id: &'a str,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                return crate::gw_limd_rp::misagent_remove(&session, profile_id)
                    .await;
            }
            let udid = self.verify_initialized()?;
            let profile_id = profile_id.to_string();
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::remove_profile(&udid, &profile_id)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn dump_profiles<'a>(&'a self, docs_path: &'a str) -> GatewayFuture<'a, String> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                return crate::gw_limd_rp::dump_profiles(&session, docs_path).await;
            }
            let udid = self.verify_initialized()?;
            let docs_path = docs_path.to_string();
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::dump_profiles_raw(&udid, &docs_path)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn remove_app<'a>(&'a self, bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                // Swift's RP removeApp is `try?` end to end.
                crate::gw_limd_rp::instproxy_uninstall(&session, bundle_id).await;
                return Ok(());
            }
            let udid = self.verify_initialized()?;
            let bundle_id = bundle_id.to_string();
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::remove_app(&udid, &bundle_id)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn send_ipa_afc<'a>(
        &'a self,
        bundle_id: &'a str,
        ipa_bytes: &'a [u8],
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                return crate::gw_limd_rp::send_ipa_afc(&session, bundle_id, ipa_bytes)
                    .await;
            }
            let udid = self.verify_initialized()?;
            let bundle_id = bundle_id.to_string();
            let ipa_bytes = ipa_bytes.to_vec();
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::send_ipa_afc(&udid, &bundle_id, &ipa_bytes)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn send_app_bundle_afc<'a>(
        &'a self,
        bundle_id: &'a str,
        app_path: &'a str,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                return crate::gw_limd_rp::send_app_bundle_afc(
                    &session,
                    bundle_id,
                    std::path::Path::new(app_path),
                )
                .await;
            }
            let udid = self.verify_initialized()?;
            let bundle_id = bundle_id.to_string();
            let app_path = std::path::PathBuf::from(app_path);
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::send_app_bundle_afc(&udid, &bundle_id, &app_path)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn install_ipa<'a>(&'a self, bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                let package_path = format!("PublicStaging/{bundle_id}.ipa");
                return crate::gw_limd_rp::instproxy_install(&session, &package_path)
                    .await;
            }
            let udid = self.verify_initialized()?;
            let bundle_id = bundle_id.to_string();
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::install_ipa(&udid, &bundle_id)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn install_app_bundle<'a>(
        &'a self,
        bundle_id: &'a str,
        app_name: &'a str,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                let package_path = format!("PublicStaging/{bundle_id}/{app_name}");
                return crate::gw_limd_rp::instproxy_install(&session, &package_path)
                    .await;
            }
            let udid = self.verify_initialized()?;
            let bundle_id = bundle_id.to_string();
            let app_name = app_name.to_string();
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::install_app_bundle(&udid, &bundle_id, &app_name)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn wipe_container<'a>(&'a self, bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                // Swift's RP wipeContainer is `try?` end to end.
                crate::gw_limd_rp::wipe_container(&session, bundle_id).await;
                return Ok(());
            }
            let udid = self.verify_initialized()?;
            let bundle_id = bundle_id.to_string();
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::wipe_container(&udid, &bundle_id)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn debug_app<'a>(&'a self, bundle_id: &'a str) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                return crate::gw_limd_rp::debug_connect_only(&session).await;
            }
            let udid = self.verify_initialized()?;
            let bundle_id = bundle_id.to_string();
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::debug_app(&udid, &bundle_id)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn debug_process<'a>(&'a self, pid: u32) -> GatewayFuture<'a, ()> {
        Box::pin(async move {
            if self.is_rp_mode() {
                self.verify_initialized_rp()?;
                let session = self.rp_session().await?;
                return crate::gw_limd_rp::debug_connect_only(&session).await;
            }
            let udid = self.verify_initialized()?;
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::debug_process(&udid, pid)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    // Behavior G12: Swift's limd backend throws
    // unsupportedOperation for both wireless pair entry
    // points, with these reasons verbatim — the port
    // keeps the same shape rather than pretending a
    // second implementation (diff list §1).
    fn start_wireless_pair<'a>(
        &'a self,
        _args: crate::gateway::WirelessPairStart<'a>,
    ) -> GatewayFuture<'a, crate::gateway::PairedDeviceRecord> {
        Box::pin(async move {
            Err(gw_error(
                DeviceGatewayCode::UnsupportedOperation,
                "startWirelessPair (RemotePairing is not supported on pure Lockdown gateway)",
            ))
        })
    }

    fn trigger_wireless_pair<'a>(
        &'a self,
        _args: crate::gateway::WirelessPairTrigger<'a>,
    ) -> GatewayFuture<'a, crate::gateway::PairedDeviceRecord> {
        Box::pin(async move {
            Err(gw_error(
                DeviceGatewayCode::UnsupportedOperation,
                "triggerWirelessPair (RemotePairing is not supported on pure Lockdown gateway)",
            ))
        })
    }

    fn afc_list_directory<'a>(
        &'a self,
        bundle_id: &'a str,
        path: &'a str,
    ) -> GatewayFuture<'a, Vec<String>> {
        Box::pin(async move {
            let udid = self.verify_initialized()?;
            // No RP branch (D-M7-4): Swift limd's AFC queries
            // only have the lockdown route; in RP mode this
            // backend's verifyInitialized stops the call, as
            // in Swift.
            // Swift limd ignores the bundle id here (D-M5-11).
            let _ = bundle_id;
            let path = path.to_string();
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::afc_list_directory(&udid, &path)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn afc_read_file<'a>(
        &'a self,
        bundle_id: &'a str,
        path: &'a str,
    ) -> GatewayFuture<'a, Vec<u8>> {
        Box::pin(async move {
            let udid = self.verify_initialized()?;
            // No RP branch (D-M7-4, see afcListDirectory).
            let _ = bundle_id;
            let path = path.to_string();
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::afc_read_file(&udid, &path)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

    fn afc_get_file_info<'a>(
        &'a self,
        bundle_id: &'a str,
        path: &'a str,
    ) -> GatewayFuture<'a, crate::gateway::AfcFileInfo> {
        Box::pin(async move {
            let udid = self.verify_initialized()?;
            // No RP branch (D-M7-4, see afcListDirectory).
            let _ = bundle_id;
            let path = path.to_string();
            tokio::task::spawn_blocking(move || {
                crate::gw_limd_ops::afc_get_file_info(&udid, &path)
            })
            .await
            .map_err(|e| {
                gw_error(
                    DeviceGatewayCode::ServiceError,
                    format!("lockdown task failed: {e}"),
                )
            })?
        })
    }

}
