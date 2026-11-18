//! Raw service connections for gw-idevice (diff list
//! D-M5-2/4/5 support machinery).
//!
//! idevice 0.1.29's `Idevice` keeps its plist/raw I/O private
//! to the crate, so the operations that must speak a service
//! protocol directly — instproxy install/uninstall (plist
//! RPC), the house-arrest vend (plist, then AFC on the same
//! connection), and the debug proxy (a raw stream out of
//! usbmuxd) — build their connections here from public parts:
//!
//! - service coordinates come from the crate's own
//!   `LockdownClient` (connect + StartSession + StartService)
//!   over a `TcpProvider`, exactly like the crate's service
//!   clients do;
//! - the data connection is an owned `TcpStream` to the
//!   endpoint, TLS-upgraded when the service descriptor asks
//!   for it, with a rustls config mirroring the crate's
//!   (private) `sni::create_client_config`: the root store
//!   holds the pairing file's root certificate, client auth
//!   uses the host certificate/key, and the server verifier
//!   accepts the presented certificate (the device cert is
//!   not WebPKI-shaped; the crate's verifier asserts the
//!   same way);
//! - the usbmuxd side speaks the mux plist protocol directly
//!   (16-byte little-endian header + XML plist), mirroring
//!   the crate's `UsbmuxdConnection` framing.

use std::io::Cursor;
use std::net::IpAddr;
use std::sync::Arc;

use idevice::lockdown::LockdownClient;
use idevice::pairing_file::PairingFile;
use idevice::provider::{IdeviceProvider, TcpProvider};
use idevice::IdeviceService as _;
use idevice::usbmuxd::UsbmuxdAddr;
use idevice::{IdeviceError, ReadWrite};
use mm_common::{DeviceGatewayCode, MmErrorInfo};
use plist::{Dictionary, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::gateway::gw_error;

/// A raw, owned device-service stream.
pub type RawStream = Box<dyn ReadWrite>;

// ---------------------------------------------------------------------------
// Plist framing (the lockdownd/service wire format: 4-byte
// big-endian length + XML plist)
// ---------------------------------------------------------------------------

pub async fn write_plist_frame<S>(stream: &mut S, dict: Dictionary) -> Result<(), IdeviceError>
where
    S: AsyncWrite + Unpin,
{
    let mut buf = Vec::new();
    Value::Dictionary(dict).to_writer_xml(&mut buf)?;
    let len = (buf.len() as u32).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(&buf).await?;
    stream.flush().await?;
    Ok(())
}

pub async fn read_plist_frame<S>(stream: &mut S) -> Result<Dictionary, IdeviceError>
where
    S: AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > 64 * 1024 * 1024 {
        return Err(IdeviceError::UnexpectedResponse);
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    match Value::from_reader(Cursor::new(body.as_slice()))? {
        Value::Dictionary(d) => Ok(d),
        _ => Err(IdeviceError::UnexpectedResponse),
    }
}

// ---------------------------------------------------------------------------
// TLS (mirrors the idevice crate's sni client config)
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct AcceptAnyServerCert {
    schemes: Vec<rustls::SignatureScheme>,
}

impl rustls::client::danger::ServerCertVerifier for AcceptAnyServerCert {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.schemes.clone()
    }
}

fn tls_client_config(pairing_file: &PairingFile) -> Result<rustls::ClientConfig, IdeviceError> {
    // The crate installs the aws-lc-rs provider as the
    // process default on first use; do the same (idempotent).
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    }
    use rustls::pki_types::pem::PemObject as _;
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add(pairing_file.root_certificate.clone())?;
    let private_key =
        rustls::pki_types::PrivateKeyDer::from_pem_slice(&pairing_file.host_private_key)?;
    let mut config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store.clone())
        .with_client_auth_cert(
            vec![pairing_file.host_certificate.clone()],
            private_key,
        )
        .map_err(|e| IdeviceError::InternalError(e.to_string()))?;
    let inner = rustls::client::WebPkiServerVerifier::builder(Arc::new(root_store))
        .build()
        .map_err(|e| IdeviceError::InternalError(e.to_string()))?;
    let schemes =
        rustls::client::danger::ServerCertVerifier::supported_verify_schemes(&*inner);
    config
        .dangerous()
        .set_certificate_verifier(Arc::new(AcceptAnyServerCert { schemes }));
    Ok(config)
}

async fn tls_upgrade(
    stream: TcpStream,
    pairing_file: &PairingFile,
) -> Result<RawStream, IdeviceError> {
    let config = tls_client_config(pairing_file)?;
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let server_name = rustls::pki_types::ServerName::try_from("iOS")
        .map_err(|e| IdeviceError::InternalError(e.to_string()))?;
    let tls = connector.connect(server_name, stream).await?;
    Ok(Box::new(tls))
}

// ---------------------------------------------------------------------------
// Service connections over the lockdown TCP route
// ---------------------------------------------------------------------------

/// Builds service connections against one device endpoint
/// (Swift `performWithTcpService`'s provider shape).
pub struct ServiceConnector {
    endpoint_ip: IpAddr,
    pairing_file: PairingFile,
    label: String,
}

impl ServiceConnector {
    pub fn new(endpoint_ip: IpAddr, pairing_file: PairingFile, label: String) -> Self {
        Self {
            endpoint_ip,
            pairing_file,
            label,
        }
    }

    fn tcp_provider(&self) -> TcpProvider {
        TcpProvider {
            addr: self.endpoint_ip,
            pairing_file: self.pairing_file.clone(),
            label: self.label.clone(),
        }
    }

    /// Asks lockdownd for `service_name`'s coordinates (the
    /// crate's LockdownClient, session included).
    pub async fn start_service(
        &self,
        service_name: &str,
    ) -> Result<(u16, bool), IdeviceError> {
        let provider = self.tcp_provider();
        let mut lockdown = LockdownClient::connect(&provider).await?;
        lockdown
            .start_session(&provider.get_pairing_file().await?)
            .await?;
        lockdown.start_service(service_name).await
    }

    /// A raw data connection to `service_name`, TLS-upgraded
    /// when its descriptor requires it.
    pub async fn connect_service(
        &self,
        service_name: &str,
    ) -> Result<RawStream, IdeviceError> {
        let (port, ssl) = self.start_service(service_name).await?;
        let stream = TcpStream::connect((self.endpoint_ip, port)).await?;
        if ssl {
            tls_upgrade(stream, &self.pairing_file).await
        } else {
            Ok(Box::new(stream))
        }
    }
}

// ---------------------------------------------------------------------------
// usbmuxd (the debug proxy's route to the debugserver port)
// ---------------------------------------------------------------------------

async fn write_mux_plist<S>(stream: &mut S, dict: Dictionary) -> Result<(), IdeviceError>
where
    S: AsyncWrite + Unpin,
{
    let mut body = Vec::new();
    Value::Dictionary(dict).to_writer_xml(&mut body)?;
    let total = (16 + body.len()) as u32;
    let mut frame = Vec::with_capacity(total as usize);
    frame.extend_from_slice(&total.to_le_bytes());
    frame.extend_from_slice(&1u32.to_le_bytes()); // plist protocol version
    frame.extend_from_slice(&8u32.to_le_bytes()); // plist message type
    frame.extend_from_slice(&0u32.to_le_bytes()); // tag
    frame.extend_from_slice(&body);
    stream.write_all(&frame).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_mux_plist<S>(stream: &mut S) -> Result<Dictionary, IdeviceError>
where
    S: AsyncRead + Unpin,
{
    let mut header = [0u8; 16];
    stream.read_exact(&mut header).await?;
    let total = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
    if total < 16 || total > 64 * 1024 * 1024 {
        return Err(IdeviceError::UnexpectedResponse);
    }
    let mut body = vec![0u8; total - 16];
    stream.read_exact(&mut body).await?;
    match Value::from_reader(Cursor::new(body.as_slice()))? {
        Value::Dictionary(d) => Ok(d),
        _ => Err(IdeviceError::UnexpectedResponse),
    }
}

fn dict_get_i64(dict: &Dictionary, key: &str) -> Option<i64> {
    match dict.get(key) {
        Some(Value::Integer(i)) => i.as_signed(),
        Some(Value::Real(r)) => Some(*r as i64),
        _ => None,
    }
}

/// Opens a raw stream to `device_port` on the first usbmuxd
/// device — Swift `connectDebugProxy` step by step, with its
/// `connectionFailed` reasons verbatim (D-M5-5).
pub async fn mux_device_stream(
    addr: &UsbmuxdAddr,
    device_port: u16,
    label: &str,
) -> Result<RawStream, MmErrorInfo> {
    let conn_failed = |reason: &str| gw_error(DeviceGatewayCode::ConnectionFailed, reason);
    let mut stream: RawStream = addr
        .to_socket()
        .await
        .map_err(|_| conn_failed("Failed to create usbmuxd connection"))?;

    // ListDevices — Swift takes the first device.
    let mut list_req = Dictionary::new();
    list_req.insert("MessageType".into(), Value::String("ListDevices".into()));
    list_req.insert(
        "ClientVersionString".into(),
        Value::String(label.to_string()),
    );
    list_req.insert("kLibUSBMuxVersion".into(), Value::Integer(3.into()));
    write_mux_plist(&mut stream, list_req)
        .await
        .map_err(|_| conn_failed("Failed to list usbmuxd devices"))?;
    let list_res = read_mux_plist(&mut stream)
        .await
        .map_err(|_| conn_failed("Failed to list usbmuxd devices"))?;
    let device_id = match list_res.get("DeviceList") {
        Some(Value::Array(devices)) => devices.first().and_then(|d| match d {
            Value::Dictionary(d) => dict_get_i64(d, "DeviceID"),
            _ => None,
        }),
        _ => None,
    };
    let device_id = match device_id {
        Some(id) => id,
        None => return Err(conn_failed("No devices found on usbmuxd")),
    };

    // Connect — the port goes on the wire byte-swapped.
    let mut connect_req = Dictionary::new();
    connect_req.insert("MessageType".into(), Value::String("Connect".into()));
    connect_req.insert("DeviceID".into(), Value::Integer(device_id.into()));
    connect_req.insert(
        "PortNumber".into(),
        Value::Integer((device_port.to_be() as i64).into()),
    );
    connect_req.insert(
        "ClientVersionString".into(),
        Value::String(label.to_string()),
    );
    connect_req.insert("kLibUSBMuxVersion".into(), Value::Integer(3.into()));
    let connect_failed_reason = format!("Failed to connect to debugserver port {device_port}");
    write_mux_plist(&mut stream, connect_req)
        .await
        .map_err(|_| conn_failed(&connect_failed_reason))?;
    let connect_res = read_mux_plist(&mut stream)
        .await
        .map_err(|_| conn_failed(&connect_failed_reason))?;
    if dict_get_i64(&connect_res, "Number") != Some(0) {
        return Err(conn_failed(&connect_failed_reason));
    }
    Ok(stream)
}
