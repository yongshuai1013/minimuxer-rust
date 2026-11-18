//! `UsbmuxdProxyServer` — Rust equivalent of the Swift usbmuxd
//! proxy (INVENTORY 2.7), following the line-by-line behaviors in
//! `docs/m3-gateway-diff.md` §4:
//!
//! - listens on 127.0.0.1:27015 (the in-App replacement usbmuxd
//!   the libimobiledevice backend is retargeted to),
//! - multiple simultaneous connections,
//! - framing: a 4-byte little-endian size prefix covers the
//!   whole packet; valid sizes are 16..=4095, anything else
//!   closes the connection,
//! - packets are a 16-byte little-endian `RawPacket` header plus
//!   an XML plist body; responses use version 1, message 8, and
//!   echo the request's tag,
//! - handler errors are *swallowed*: no response, the
//!   connection stays open (Swift `do {} catch {}` — the code,
//!   not the inventory summary, is authoritative),
//! - `ListDevices` answers from the attach state fed by the
//!   network layer (mm-net's `AttachListener`), with Swift's
//!   exact entry shape: DeviceID 1, InterfaceIndex 0, and the
//!   152-byte sockaddr `NetworkAddress` (see
//!   [`encode_network_address`]).

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mm_common::error::MinimuxerError;
use mm_common::MmErrorInfo;
use mm_gateway::{DeviceGateway, GatewayProvider};
use plist::{Dictionary, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

/// The address MinimuxerImpl retargets `USBMUXD_SOCKET_ADDRESS`
/// to and the proxy binds.
pub const USBMUXD_PROXY_ADDR: &str = "127.0.0.1:27015";
/// Swift's fallback when the pairing dict has no SystemBUID.
pub const ZERO_UUID: &str = "00000000-0000-0000-0000-000000000000";
/// Swift hardcodes the proxied device's id to 1.
const DEVICE_ID: i64 = 1;

// MARK: - RawPacket

/// A usbmuxd packet: 16-byte little-endian header + payload.
/// Rust equivalent of Swift's `RawPacket`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPacket {
    pub size: u32,
    pub version: u32,
    pub message: u32,
    pub tag: u32,
    pub payload: Vec<u8>,
}

impl RawPacket {
    /// Response constructor, mirroring Swift's
    /// `RawPacket(payload:tag:)`: version 1, message 8 ("result"
    /// in usbmuxd terms), size derived from the payload.
    pub fn new(payload: Vec<u8>, tag: u32) -> Self {
        Self {
            size: 16 + payload.len() as u32,
            version: 1,
            message: 8,
            tag,
            payload,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.size as usize);
        out.extend_from_slice(&self.size.to_le_bytes());
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&self.message.to_le_bytes());
        out.extend_from_slice(&self.tag.to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }

    /// Parse a complete packet (exactly `size` bytes, header
    /// included). Returns `None` for truncated or out-of-range
    /// buffers.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 16 {
            return None;
        }
        let size = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
        if !(16..=4095).contains(&size) || bytes.len() < size as usize {
            return None;
        }
        Some(Self {
            size,
            version: u32::from_le_bytes(bytes[4..8].try_into().ok()?),
            message: u32::from_le_bytes(bytes[8..12].try_into().ok()?),
            tag: u32::from_le_bytes(bytes[12..16].try_into().ok()?),
            payload: bytes[16..size as usize].to_vec(),
        })
    }
}

// MARK: - NetworkAddress (Swift convertIp)

/// Swift's `convertIp`: a 152-byte `sockaddr_storage` buffer.
///
/// - IPv4: `sin_len` 16, family 2 (AF_INET), port 0, address at
///   bytes 4..8.
/// - IPv6: `sin6_len` 28, family 30 (AF_INET6 on Darwin),
///   address at bytes 8..24, scope id (little-endian) at
///   bytes 24..28.
///
/// The scope comes from the `%iface` suffix when present;
/// otherwise link-local (`fe80:`) addresses try `en0`, `awdl0`,
/// `lo0` in order — exactly Swift's fallbacks. Unparseable input
/// leaves the all-zero buffer, like Swift's early return.
pub fn encode_network_address(ip: &str) -> Vec<u8> {
    let mut data = vec![0u8; 152];
    let host = ip.split('%').next().unwrap_or(ip);
    if let Ok(v6) = host.parse::<std::net::Ipv6Addr>() {
        data[0] = 28;
        data[1] = 30;
        data[8..24].copy_from_slice(&v6.octets());
        let scope = scope_id_for(ip);
        data[24..28].copy_from_slice(&scope.to_le_bytes());
    } else if let Ok(v4) = host.parse::<std::net::Ipv4Addr>() {
        data[0] = 16;
        data[1] = 2;
        data[4..8].copy_from_slice(&v4.octets());
    }
    data
}

fn scope_id_for(ip: &str) -> u32 {
    if let Some(iface) = ip.split('%').nth(1) {
        if !iface.is_empty() {
            return if_nametoindex(iface).unwrap_or(0);
        }
    }
    let host = ip.split('%').next().unwrap_or(ip).to_lowercase();
    if host.starts_with("fe80:") {
        for candidate in ["en0", "awdl0", "lo0"] {
            if let Some(idx) = if_nametoindex(candidate) {
                return idx;
            }
        }
    }
    0
}

fn if_nametoindex(name: &str) -> Option<u32> {
    let cstr = std::ffi::CString::new(name).ok()?;
    let idx = unsafe { libc::if_nametoindex(cstr.as_ptr()) };
    if idx == 0 {
        None
    } else {
        Some(idx)
    }
}

// MARK: - UsbmuxdProxyServer

struct ProxyState {
    /// Peer IP of the attached device, fed by the network layer.
    attached_peer_ip: Option<String>,
    /// UDID passed to `start` (Swift stores it on the server).
    device_udid: Option<String>,
}

/// The usbmuxd proxy server. One instance serves the current
/// gateway (looked up per packet through the provider, so a
/// backend switch takes effect without restarting the proxy —
/// Swift reads `DeviceProvider.shared.currentGateway` the same
/// way).
pub struct UsbmuxdProxyServer {
    provider: Arc<dyn GatewayProvider>,
    state: Arc<Mutex<ProxyState>>,
    running: Arc<AtomicBool>,
    shutdown: Arc<Notify>,
    bind_addr: String,
    local_port: Mutex<Option<u16>>,
}

impl UsbmuxdProxyServer {
    pub fn new(provider: Arc<dyn GatewayProvider>) -> Self {
        Self::with_bind_addr(provider, USBMUXD_PROXY_ADDR)
    }

    /// Test/seam constructor: production always uses
    /// [`USBMUXD_PROXY_ADDR`].
    pub fn with_bind_addr(provider: Arc<dyn GatewayProvider>, bind_addr: &str) -> Self {
        Self {
            provider,
            state: Arc::new(Mutex::new(ProxyState {
                attached_peer_ip: None,
                device_udid: None,
            })),
            running: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(Notify::new()),
            bind_addr: bind_addr.to_string(),
            local_port: Mutex::new(None),
        }
    }

    /// Swift `isListening`.
    pub fn is_listening(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Swift `started` (mirrors the listener's existence).
    pub fn started(&self) -> bool {
        self.is_listening()
    }

    /// The port the listener actually bound (differs from the
    /// configured address only for ephemeral test binds).
    pub fn local_port(&self) -> Option<u16> {
        *self.local_port.lock().unwrap()
    }

    /// Swift `start(deviceUDID:)`: `Ok(false)` when already
    /// running; a bind failure is `MinimuxerError.connect` with
    /// Swift's message prefix.
    pub async fn start(&self, udid: &str) -> Result<bool, MmErrorInfo> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(false);
        }
        let listener = TcpListener::bind(&self.bind_addr).await.map_err(|e| {
            MinimuxerError::Connect(format!(
                "UsbmuxdProxyServer failed to bind: {e}"
            ))
            .to_info()
        })?;
        *self.local_port.lock().unwrap() = listener.local_addr().ok().map(|a| a.port());
        {
            let mut state = self.state.lock().unwrap();
            state.device_udid = Some(udid.to_string());
        }
        self.running.store(true, Ordering::SeqCst);

        let provider = self.provider.clone();
        let state = self.state.clone();
        let running = self.running.clone();
        let shutdown = self.shutdown.clone();
        tokio::spawn(async move {
            loop {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                tokio::select! {
                    _ = shutdown.notified() => break,
                    // Tick so a stop() that raced the select is
                    // still observed via the running flag.
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((socket, _)) => {
                                let p = provider.clone();
                                let s = state.clone();
                                tokio::spawn(handle_connection(socket, p, s));
                            }
                            Err(_) => {
                                tokio::time::sleep(Duration::from_millis(10)).await;
                            }
                        }
                    }
                }
            }
        });
        Ok(true)
    }

    /// Swift `stop()`: cancels the listener. In-flight
    /// connections are dropped with their tasks.
    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.shutdown.notify_waiters();
        *self.local_port.lock().unwrap() = None;
    }

    /// Swift `notifyDeviceAttached(peerIp:)`.
    pub fn notify_device_attached(&self, peer_ip: &str) {
        self.state.lock().unwrap().attached_peer_ip = Some(peer_ip.to_string());
    }

    /// Swift `notifyDeviceDetached()`.
    pub fn notify_device_detached(&self) {
        self.state.lock().unwrap().attached_peer_ip = None;
    }
}

/// mm-net's attach feed plugs straight into the proxy
/// (MinimuxerImpl's NetworkObserverService does the same in
/// Swift via `MuxerServerHolder`).
impl mm_net::AttachListener for UsbmuxdProxyServer {
    fn device_attached(&self, peer_ip: &str) {
        self.notify_device_attached(peer_ip);
    }

    fn device_detached(&self) {
        self.notify_device_detached();
    }
}

async fn handle_connection(
    mut socket: TcpStream,
    provider: Arc<dyn GatewayProvider>,
    state: Arc<Mutex<ProxyState>>,
) {
    loop {
        // Swift reads the 4-byte little-endian size first and
        // validates the range before reading the rest.
        let mut size_buf = [0u8; 4];
        if socket.read_exact(&mut size_buf).await.is_err() {
            return;
        }
        let size = u32::from_le_bytes(size_buf) as usize;
        if !(16..=4095).contains(&size) {
            return;
        }
        let mut full = size_buf.to_vec();
        let mut rest = vec![0u8; size - 4];
        if socket.read_exact(&mut rest).await.is_err() {
            return;
        }
        full.extend_from_slice(&rest);
        let Some(packet) = RawPacket::parse(&full) else {
            return;
        };
        // Swift wraps handlePacket in do/catch and swallows
        // every error: no response, connection stays open.
        if let Ok(Some(response)) = handle_packet(&packet, provider.as_ref(), &state) {
            if socket.write_all(&response.encode()).await.is_err() {
                return;
            }
        }
    }
}

fn proxy_error(message: impl Into<String>) -> MmErrorInfo {
    // Handler errors are swallowed by the connection loop, so
    // this marker is never observable on the wire; it exists
    // only to keep handle_packet's error type uniform.
    MmErrorInfo::new("MinimuxerError", "internalError", &message.into())
}

fn handle_packet(
    packet: &RawPacket,
    provider: &dyn GatewayProvider,
    state: &Mutex<ProxyState>,
) -> Result<Option<RawPacket>, MmErrorInfo> {
    let value = Value::from_reader(Cursor::new(packet.payload.as_slice()))
        .map_err(|e| proxy_error(format!("Invalid packet payload: {e}")))?;
    let dict = value
        .as_dictionary()
        .ok_or_else(|| proxy_error("Packet payload is not a dictionary"))?;
    let message_type = dict
        .get("MessageType")
        .and_then(|v| v.as_string())
        .ok_or_else(|| proxy_error("Missing MessageType"))?
        .to_string();

    let respond = |body: Dictionary| -> Result<Option<RawPacket>, MmErrorInfo> {
        let mut buf = Vec::new();
        Value::Dictionary(body)
            .to_writer_xml(&mut buf)
            .map_err(|e| proxy_error(format!("Failed to serialize response: {e}")))?;
        Ok(Some(RawPacket::new(buf, packet.tag)))
    };

    match message_type.as_str() {
        "ListDevices" => {
            let state = state.lock().unwrap();
            let mut device_list: Vec<Value> = Vec::new();
            if let (Some(peer), Some(udid)) =
                (&state.attached_peer_ip, &state.device_udid)
            {
                let mut properties = Dictionary::new();
                properties.insert(
                    "ConnectionType".into(),
                    Value::String("Network".into()),
                );
                properties.insert("DeviceID".into(), Value::Integer(DEVICE_ID.into()));
                properties.insert(
                    "EscapedFullServiceName".into(),
                    Value::String(format!("{udid}._apple-mobdev2._tcp.local")),
                );
                properties.insert("InterfaceIndex".into(), Value::Integer(0.into()));
                properties.insert(
                    "NetworkAddress".into(),
                    Value::Data(encode_network_address(peer)),
                );
                properties.insert("SerialNumber".into(), Value::String(udid.clone()));
                let mut entry = Dictionary::new();
                entry.insert("DeviceID".into(), Value::Integer(DEVICE_ID.into()));
                entry.insert("Properties".into(), Value::Dictionary(properties));
                device_list.push(Value::Dictionary(entry));
            }
            let mut body = Dictionary::new();
            body.insert("DeviceList".into(), Value::Array(device_list));
            respond(body)
        }
        "Listen" => {
            let mut body = Dictionary::new();
            body.insert("MessageType".into(), Value::String("Result".into()));
            body.insert("Number".into(), Value::Integer(0.into()));
            respond(body)
        }
        "ReadBUID" => {
            let gateway = provider
                .current_gateway()
                .ok_or_else(|| proxy_error("Device not connected"))?;
            let buid = gateway
                .pairing_data_dict()
                .and_then(|d| {
                    d.get("SystemBUID")
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| ZERO_UUID.to_string());
            let mut body = Dictionary::new();
            body.insert("BUID".into(), Value::String(buid));
            respond(body)
        }
        "ReadPairRecord" => {
            let gateway = provider
                .current_gateway()
                .ok_or_else(|| proxy_error("Device not connected"))?;
            let data = gateway.pairing_file_data().ok_or_else(|| {
                MinimuxerError::PairingNotLoaded("Pairing file not loaded".into()).to_info()
            })?;
            let mut body = Dictionary::new();
            body.insert("PairRecordData".into(), Value::Data(data));
            respond(body)
        }
        other => Err(proxy_error(format!(
            "Unknown packet type {other}, ignoring"
        ))),
    }
}

// MARK: - MinimuxerImpl orchestration equivalents

/// MinimuxerImpl.start's usbmuxd retarget: point the process'
/// usbmuxd address at the in-App proxy before the
/// libimobiledevice backend initializes (its C library reads
/// `USBMUXD_SOCKET_ADDRESS`).
pub fn retarget_usbmuxd_address() {
    // SAFETY: called from the core start path before any
    // libimobiledevice call reads the variable, mirroring
    // Swift's unsetenv/setenv pair.
    unsafe {
        std::env::remove_var("USBMUXD_SOCKET_ADDRESS");
        std::env::set_var("USBMUXD_SOCKET_ADDRESS", USBMUXD_PROXY_ADDR);
    }
}

/// MinimuxerImpl.restartMuxerServer: stop the proxy, validate
/// the gateway's pairing dict and UDID (with Swift's exact
/// errors), then start the proxy for that UDID.
pub async fn start_proxy_for_gateway(
    proxy: &UsbmuxdProxyServer,
    gateway: &Arc<dyn DeviceGateway>,
) -> Result<(), MmErrorInfo> {
    proxy.stop().await;
    let dict = gateway.pairing_data_dict().ok_or_else(|| {
        MinimuxerError::PairingNotLoaded(
            "Pairing dictionary is missing in gateway".into(),
        )
        .to_info()
    })?;
    let udid = dict
        .get("UDID")
        .and_then(|v| v.as_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            MinimuxerError::InvalidPairing {
                protocol: gateway.pairing_file_type(),
                reason: "Pairing file is missing UDID value".into(),
            }
            .to_info()
        })?;
    proxy.start(udid).await?;
    Ok(())
}
