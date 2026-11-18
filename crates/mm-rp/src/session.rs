//! RP identity and session establishment (inventory §2–§5).
//!
//! `RpSession::establish` is the one establishment sequence
//! both Swift gateways perform, whatever their caching shape:
//! TCP connect to the rppairing port → `RemotePairingClient`
//! pair-verify (verify only, never pair-setup) →
//! `create_tcp_listener` → a second TCP connection wrapped in
//! the TLS-PSK CDTunnel handshake → jktcp `Adapter` → RSD
//! handshake on the tunnel's RSD port. The pairing client is
//! dropped once the tunnel exists, exactly like Swift limd's
//! `withRPClient` scope; the tunnel is independent of it.

use std::net::IpAddr;

use idevice68::remote_pairing::{
    connect_tls_psk_tunnel_native, RemotePairingClient, RpPairingFile, RpPairingSocket,
};
use idevice68::services::rsd::RsdHandshake;
use idevice68::tcp::adapter::Adapter;
use idevice68::tcp::handle::AdapterHandle;
use idevice68::{IdeviceError, RsdService};
use tokio::net::TcpStream;

use crate::stream::RpStream;

/// One RP failure. `stage` names the establishment step (or
/// `"service"` for post-establishment service work) so the
/// gateways can pick their Swift reason templates; `pairing`
/// carries Swift `isPairingError`'s classification of the
/// underlying crate error (FFI codes 18 InvalidHostID / 103
/// RemotePairing, or the message rules).
#[derive(Debug)]
pub struct RpError {
    pub stage: &'static str,
    pub message: String,
    pub pairing: bool,
}

impl RpError {
    pub fn from68(stage: &'static str, err: IdeviceError) -> Self {
        Self {
            stage,
            message: err.to_string(),
            pairing: is_pairing_error68(&err),
        }
    }

    pub fn plain(stage: &'static str, message: impl Into<String>) -> Self {
        Self {
            stage,
            message: message.into(),
            pairing: false,
        }
    }
}

impl std::fmt::Display for RpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.stage, self.message)
    }
}

impl std::error::Error for RpError {}

pub type RpResult<T> = Result<T, RpError>;

/// Swift `isPairingError` over the 0.1.68 error surface: the
/// FFI codes kept their numbering across the line (18 =
/// `InvalidHostID`, 103 = `RemotePairing`), the rest go
/// through Swift's message rules on the Display text.
pub fn is_pairing_error68(err: &IdeviceError) -> bool {
    if matches!(
        err,
        IdeviceError::InvalidHostID | IdeviceError::RemotePairing(_)
    ) {
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

/// A loaded RemotePairing identity (the `_rp.plist` file).
/// The bytes are kept and re-parsed per establishment:
/// pair-verify takes the pairing file by `&mut` and the
/// gateways' sessions are rebuilt on invalidation, so no
/// crate handle is cached here.
#[derive(Clone)]
pub struct RpIdentity {
    bytes: Vec<u8>,
    identifier: String,
}

impl RpIdentity {
    /// RPK `rppairing_identity_from_plist`: parse and validate.
    pub fn load(bytes: &[u8]) -> RpResult<Self> {
        let file = RpPairingFile::from_bytes(bytes)
            .map_err(|e| RpError::from68("identity", e))?;
        Ok(Self {
            bytes: bytes.to_vec(),
            identifier: file.identifier.clone(),
        })
    }

    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    fn fresh_file(&self) -> RpResult<RpPairingFile> {
        RpPairingFile::from_bytes(&self.bytes)
            .map_err(|e| RpError::from68("identity", e))
    }
}

/// The tunnel parameters the device assigned (RPK
/// `rppairing_tunnel_info_t`, idevice `TunnelInfo` — the five
/// fields share names across all three).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpTunnelInfo {
    pub client_address: String,
    pub client_netmask: String,
    pub server_address: String,
    pub server_rsd_port: u16,
    pub mtu: u16,
}

/// One established RemotePairing session: the TCP adapter
/// (user-space stack over the TLS-PSK tunnel) plus the RSD
/// handshake taken on it. Dropping the session tears the
/// whole chain down (RPK's `rsd_free`/`tunnel_close`).
pub struct RpSession {
    handle: AdapterHandle,
    handshake: RsdHandshake,
    info: RpTunnelInfo,
}

impl RpSession {
    /// The full establishment sequence. `host` is the device
    /// endpoint (the tunnel address the App resolved), `port`
    /// the rppairing port (49152 by default), `host_name` the
    /// client label (Swift passes the app name on both
    /// backends).
    pub async fn establish(
        host: &str,
        port: u16,
        host_name: &str,
        identity: &RpIdentity,
    ) -> RpResult<Self> {
        let ip: IpAddr = host
            .split('%')
            .next()
            .unwrap_or(host)
            .parse()
            .map_err(|_| {
                RpError::plain("connect", format!("Invalid endpoint address: {host}"))
            })?;
        let stream = TcpStream::connect((ip, port))
            .await
            .map_err(|e| RpError::plain("connect", e.to_string()))?;
        let socket = RpPairingSocket::new(stream);
        let mut client = RemotePairingClient::new(socket, host_name);
        // Pair-verify, steps 1–8 of inventory §2.2. Verify
        // only: a mismatch is an error here, never a silent
        // fall into pair-setup (that is M8's explicit flow).
        client
            .attempt_pair_verify()
            .await
            .map_err(|e| RpError::from68("pair_verify", e))?;
        let mut file = identity.fresh_file()?;
        client
            .validate_pairing(&mut file)
            .await
            .map_err(|e| RpError::from68("pair_verify", e))?;
        let key = client.encryption_key().to_vec();
        let tunnel_port = client
            .create_tcp_listener()
            .await
            .map_err(|e| RpError::from68("create_tunnel_listener", e))?;
        drop(client);
        let tunnel_stream = TcpStream::connect((ip, tunnel_port))
            .await
            .map_err(|e| RpError::plain("tunnel_connect", e.to_string()))?;
        let tunnel = connect_tls_psk_tunnel_native(tunnel_stream, &key)
            .await
            .map_err(|e| RpError::from68("tunnel_connect", e))?;
        let info = RpTunnelInfo {
            client_address: tunnel.info.client_address.clone(),
            client_netmask: tunnel.info.netmask.clone(),
            server_address: tunnel.info.server_address.clone(),
            server_rsd_port: tunnel.info.server_rsd_port,
            mtu: tunnel.info.mtu,
        };
        let our_ip: IpAddr = info.client_address.parse().map_err(|_| {
            RpError::plain(
                "tunnel_connect",
                format!("Invalid tunnel client address: {}", info.client_address),
            )
        })?;
        let their_ip: IpAddr = info.server_address.parse().map_err(|_| {
            RpError::plain(
                "tunnel_connect",
                format!("Invalid tunnel server address: {}", info.server_address),
            )
        })?;
        let mut adapter = Adapter::new(Box::new(tunnel.into_inner()), our_ip, their_ip);
        adapter.set_mss(info.mtu.saturating_sub(60) as usize);
        let mut handle = adapter.to_async_handle();
        let rsd_stream = handle
            .connect(info.server_rsd_port)
            .await
            .map_err(|e| RpError::plain("rsd_connect", e.to_string()))?;
        let handshake = RsdHandshake::new(rsd_stream)
            .await
            .map_err(|e| RpError::from68("rsd_connect", e))?;
        Ok(Self {
            handle,
            handshake,
            info,
        })
    }

    pub fn tunnel_info(&self) -> &RpTunnelInfo {
        &self.info
    }

    /// RPK `RsdClient::get_service_port`: the six-level fuzzy
    /// resolution of inventory §2.7 over the handshake's
    /// service directory.
    pub fn service_port(&self, service_name: &str) -> Option<u16> {
        resolve_service_port(
            self.handshake
                .services
                .iter()
                .map(|(name, svc)| (name.as_str(), svc.port)),
            service_name,
        )
    }

    /// Opens a raw stream to a named RSD service (RPK
    /// `rppairing_connect_service_stream`).
    pub async fn open_stream(&mut self, service_name: &str) -> RpResult<RpStream> {
        let port = self.service_port(service_name).ok_or_else(|| {
            RpError::plain(
                "service",
                format!("RSD service not found: {service_name}"),
            )
        })?;
        let stream = self
            .handle
            .connect(port)
            .await
            .map_err(|e| RpError::plain("service", e.to_string()))?;
        Ok(RpStream::new(Box::new(stream)))
    }

    /// Connects one typed service client over the RSD
    /// handshake (the idevice FFI's `*_connect_rsd` family;
    /// the crate's `from_stream` performs the RSD checkin).
    pub async fn connect_service<T: RsdService>(&mut self) -> RpResult<T> {
        self.handshake
            .connect::<T>(&mut self.handle)
            .await
            .map_err(|e| RpError::from68("service", e))
    }

    /// Opens the raw RSD stream for the debugserver service
    /// (its directory name comes from the crate's own
    /// `rsd_service_name`, not a transcription). The gateway
    /// runs the port's `RspClient` over this stream after
    /// [`RpStream::rsd_checkin`]: the crate's
    /// `DebugProxyClient` is its one generic service client,
    /// and its `RsdService` RPIT future does not survive
    /// being held inside the gateways' retry closures (the
    /// "implementation of RsdService is not general enough"
    /// wall, diff list D-M7-9). The wire behavior is the
    /// same checkin + RSP the typed client performs.
    pub async fn open_debug_stream(&mut self) -> RpResult<RpStream> {
        use idevice68::RsdService as _;
        let name = idevice68::services::debug_proxy::DebugProxyClient::<
            Box<dyn idevice68::ReadWrite + 'static>,
        >::rsd_service_name();
        self.open_stream(&name).await
    }

    /// The RSD form of the personalized DDI mount (Swift
    /// idevice's dedicated path, inventory §4): UniqueChipID
    /// from the RSD lockdownd first, then the crate's
    /// `mount_personalized_rsd` over this session's adapter
    /// and handshake.
    pub async fn mount_personalized_ddi(
        &mut self,
        image: &[u8],
        trustcache: &[u8],
        build_manifest: &[u8],
    ) -> RpResult<()> {
        use idevice68::services::lockdown::LockdownClient;
        use idevice68::services::mobile_image_mounter::ImageMounter;
        let mut lockdown = self.connect_service::<LockdownClient>().await?;
        let value = lockdown
            .get_value(Some("UniqueChipID"), None)
            .await
            .map_err(|e| RpError::from68("service", e))?;
        let chip_id = value.as_unsigned_integer().ok_or_else(|| {
            RpError::plain("service", "Failed to get UniqueChipID")
        })?;
        let mut mounter = self.connect_service::<ImageMounter>().await?;
        mounter
            .mount_personalized_rsd(
                &mut self.handle,
                &mut self.handshake,
                image.to_vec(),
                trustcache.to_vec(),
                build_manifest,
                None,
                chip_id,
            )
            .await
            .map_err(|e| RpError::from68("service", e))
    }
}

/// RPK `RsdClient::get_service_port`'s resolution order
/// (inventory §2.7, a behavior spec): (1) exact name,
/// (2) `<name>.remote.trusted`, (3) `<name>.shim.remote`,
/// (4) `<name>.remote`, (5) first prefix match, (6) first
/// substring match; none → `None` (RPK returns 0).
pub fn resolve_service_port<'a>(
    services: impl IntoIterator<Item = (&'a str, u16)>,
    name: &str,
) -> Option<u16> {
    let entries: Vec<(&str, u16)> = services.into_iter().collect();
    let exact = |n: &str| {
        entries
            .iter()
            .find(|(key, _)| *key == n)
            .map(|(_, port)| *port)
    };
    if let Some(port) = exact(name) {
        return Some(port);
    }
    for suffix in [".remote.trusted", ".shim.remote", ".remote"] {
        if let Some(port) = exact(&format!("{name}{suffix}")) {
            return Some(port);
        }
    }
    if let Some((_, port)) = entries.iter().find(|(key, _)| key.starts_with(name)) {
        return Some(*port);
    }
    if let Some((_, port)) = entries.iter().find(|(key, _)| key.contains(name)) {
        return Some(*port);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> Vec<(&'static str, u16)> {
        vec![
            ("com.apple.lockdownd.remote.trusted", 1001),
            ("com.apple.mobile.heartbeat.remote", 1002),
            ("com.apple.mobile.MCInstall.shim.remote", 1003),
            ("com.apple.debugserver.remote", 1004),
        ]
    }

    #[test]
    fn resolve_exact_beats_suffixes() {
        let mut d = dir();
        d.push(("com.apple.lockdownd", 999));
        assert_eq!(
            resolve_service_port(d, "com.apple.lockdownd"),
            Some(999)
        );
    }

    #[test]
    fn resolve_suffix_order_trusted_then_shim_then_remote() {
        assert_eq!(
            resolve_service_port(dir(), "com.apple.lockdownd"),
            Some(1001)
        );
        assert_eq!(
            resolve_service_port(dir(), "com.apple.mobile.MCInstall"),
            Some(1003)
        );
        assert_eq!(
            resolve_service_port(dir(), "com.apple.mobile.heartbeat"),
            Some(1002)
        );
    }

    #[test]
    fn resolve_prefix_then_substring_then_none() {
        assert_eq!(
            resolve_service_port(dir(), "com.apple.debugserver"),
            Some(1004)
        );
        assert_eq!(resolve_service_port(dir(), "debugserver"), Some(1004));
        assert_eq!(resolve_service_port(dir(), "com.apple.nothing"), None);
    }
}
