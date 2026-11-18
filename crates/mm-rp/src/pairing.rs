//! Wireless pair-setup wiring (M8; behavior list
//! `docs/m8-wireless-behavior.md` §B/§C, diff list
//! D-M8-1).
//!
//! The verdict of the M8 survey is the M7 one: the
//! pair-setup engine is idevice 0.1.68's own — the very
//! functions the Swift idevice gateway reaches through
//! the IDevice FFI (`pairable_host_prepare` /
//! `pairable_host_accept_fd` / `rppairing_pair_network`)
//! are thin C wrappers over the Rust API used here
//! (ffi/src/pairing_host.rs and ffi/src/tunnel_provider
//! .rs at tag v0.1.68 are the evidence). This module is
//! the FFI shapes re-expressed as Rust values:
//!
//! - [`PairHostPreparation`] — `pairable_host_prepare`:
//!   a fresh [`RpPairingFile`] (identifier = UUIDv3(DNS,
//!   host name)) plus a [`PairableHostInfo`] with a
//!   random alt-IRK; the service id and TXT records the
//!   caller advertises; the host alt-IRK the caller
//!   keeps for the finalize step.
//! - [`PairHostPreparation::accept`] — `pairable_host_
//!   accept_fd`: the responder handshake + SRP pair-
//!   setup over one accepted stream. The PIN is
//!   generated host-side and surfaced once through the
//!   callback before M2 is sent (behavior G2/W4).
//! - [`pair_network`] — `rppairing_pair_network`:
//!   connect + pair-verify with pair-setup fallback
//!   (`RemotePairingClient::connect`); yields the peer
//!   device only when a pair-setup actually ran.
//! - [`TcpAcceptor`] — Swift MinimuxerCommon's
//!   `TCPAcceptor`: an INADDR_ANY ephemeral listener
//!   (behavior G3).

use std::future::Future;
use std::net::SocketAddr;

use idevice68::remote_pairing::{
    PairableHost, PairableHostInfo, PeerDevice, RemotePairingClient,
    RpPairingFile, RpPairingSocket,
};
use tokio::net::{TcpListener, TcpStream};

use crate::session::{RpError, RpResult};

/// Swift `TCPAcceptor` (behavior G3): binds INADDR_ANY
/// on an ephemeral port with backlog 5 and reports the
/// bound port. The Swift class sets SO_REUSEADDR and
/// (on Darwin) SO_REUSEPORT; tokio's listener sets
/// SO_REUSEADDR on Unix — for an ephemeral listener the
/// REUSEPORT half has no observable effect.
pub struct TcpAcceptor {
    listener: TcpListener,
    port: u16,
}

impl TcpAcceptor {
    pub async fn bind() -> std::io::Result<Self> {
        let listener = TcpListener::bind(("0.0.0.0", 0)).await?;
        let port = listener.local_addr()?.port();
        Ok(Self { listener, port })
    }

    /// The bound port (Swift `TCPAcceptor.port`).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Blocks until one connection arrives (Swift
    /// `accept()`); the accepted stream is handed to
    /// the pairing conversation and closed at the end
    /// of it, like the Swift `defer { close(clientFd) }`.
    pub async fn accept(&self) -> std::io::Result<TcpStream> {
        let (stream, _) = self.listener.accept().await?;
        Ok(stream)
    }
}

/// The prepared pairable host (FFI `PairableHostHandle`
/// plus its out-parameters, held as values).
pub struct PairHostPreparation {
    pairing_file: RpPairingFile,
    host_info: PairableHostInfo,
}

impl PairHostPreparation {
    /// `pairable_host_prepare(name, model, false)`:
    /// a fresh identity is generated on every call.
    /// The Swift gateway always passes `false` for
    /// pinless pairing, so the prepared host always
    /// runs the random-PIN flow.
    pub fn prepare(host_name: &str, host_model: &str) -> Self {
        let pairing_file = RpPairingFile::generate(host_name);
        let mut host_info = PairableHostInfo::generate(host_name, host_model);
        host_info.allows_pinless_pairing = false;
        Self {
            pairing_file,
            host_info,
        }
    }

    /// The Bonjour service instance name (the pairing
    /// file's identifier, UUIDv3(DNS, host name)).
    pub fn service_id(&self) -> &str {
        &self.pairing_file.identifier
    }

    /// The TXT records to advertise, as key/value pairs
    /// (the FFI serializes these into an XML plist
    /// dictionary; the pairs are the same content —
    /// behavior G2).
    pub fn txt_records(&self) -> Vec<(String, String)> {
        self.host_info.mdns_txt_records(self.service_id())
    }

    /// The host's 16-byte alt-IRK (FFI `out_host_alt_
    /// irk`), carried into the finalize step.
    pub fn host_alt_irk(&self) -> [u8; 16] {
        self.host_info.alt_irk
    }

    /// `pairable_host_accept_fd`: runs the responder
    /// handshake and the full SRP pair-setup over the
    /// accepted stream. `on_pin` is invoked once with
    /// the host-generated 6-digit PIN (to be shown to
    /// the user, who types it into the device). On
    /// success returns the updated pairing file (now
    /// carrying the device's alt-IRK) and the peer
    /// device.
    pub async fn accept<Fut>(
        self,
        stream: TcpStream,
        on_pin: impl FnOnce(String) -> Fut,
    ) -> RpResult<(RpPairingFile, PeerDevice)>
    where
        Fut: Future<Output = ()>,
    {
        let socket = RpPairingSocket::new_device(stream);
        let mut host = PairableHost::new(socket, self.host_info);
        let mut file = self.pairing_file;
        let peer = host
            .accept(&mut file, on_pin)
            .await
            .map_err(|e| RpError::from68("pair_setup", e))?;
        Ok((file, peer))
    }
}

/// `rppairing_pair_network`: connects to the target's
/// pairing service and runs `RemotePairingClient::
/// connect` — pair-verify first, pair-setup on
/// verification failure — against `pairing_file`
/// (updated in place, FFI semantics). The PIN callback
/// fires only when the device actually asks for a PIN;
/// on a consent-first reply the crate presets "000000"
/// itself (behavior G7). Returns the peer device when
/// a pair-setup ran, `None` after a pure pair-verify.
pub async fn pair_network<Fut>(
    target: SocketAddr,
    host_name: &str,
    pairing_file: &mut RpPairingFile,
    pin_callback: impl Fn() -> Fut,
) -> RpResult<Option<PeerDevice>>
where
    Fut: Future<Output = String>,
{
    let stream = TcpStream::connect(target)
        .await
        .map_err(|e| RpError::plain("connect", e.to_string()))?;
    let socket = RpPairingSocket::new(stream);
    let mut client = RemotePairingClient::new(socket, host_name);
    client
        .connect(pairing_file, pin_callback)
        .await
        .map_err(|e| RpError::from68("pair_setup", e))?;
    Ok(client.paired_peer_device().ok().cloned())
}
