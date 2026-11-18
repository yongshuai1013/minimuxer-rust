//! `GatewayCore` — port of Swift `BaseDeviceGateway`
//! (yongshuai1013/minimuxer @714aafa3, `DeviceGateway/BaseDeviceGateway.swift`).
//!
//! Holds the state both backends share: pairing file type/data and
//! its derived plist dictionary, the device endpoint IP, the
//! initialized flag and the per-protocol port table. Port and
//! endpoint-IP changes invalidate cached connections; Swift
//! subclasses free their handles eagerly in `invalidateConnection()`,
//! here invalidation is a monotonically increasing `generation`
//! counter that backends check before reusing a cached connection
//! (lazy invalidation — observably equivalent: a stale connection is
//! never used again).

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Mutex;

use mm_common::logging::debug_log;
use mm_common::PairingProtocol;
use plist::{Dictionary, Value};

struct CoreInner {
    pairing_file_type: PairingProtocol,
    pairing_file_data: Option<Vec<u8>>,
    pairing_data_dict: Option<Dictionary>,
    device_endpoint_ip: Option<String>,
    is_initialized: bool,
    protocol_ports: HashMap<PairingProtocol, u16>,
    generation: u64,
    logging_enabled: bool,
}

/// Shared gateway state (Swift `BaseDeviceGateway`).
pub struct GatewayCore {
    inner: Mutex<CoreInner>,
    /// Supplies the `#available(iOS 17, *)` half of
    /// `PairingProtocol.defaultPort` for the `.unknown` case.
    /// Defaults to `false` (the target device is iOS 15, matching
    /// the choice mm-net's port table already made in M2).
    ios17_available: bool,
}

impl Default for GatewayCore {
    fn default() -> Self {
        Self::new()
    }
}

impl GatewayCore {
    pub fn new() -> Self {
        Self::with_ios17_available(false)
    }

    pub fn with_ios17_available(ios17_available: bool) -> Self {
        Self {
            inner: Mutex::new(CoreInner {
                pairing_file_type: PairingProtocol::Unknown,
                pairing_file_data: None,
                pairing_data_dict: None,
                device_endpoint_ip: None,
                is_initialized: false,
                protocol_ports: HashMap::new(),
                generation: 0,
                logging_enabled: false,
            }),
            ios17_available,
        }
    }

    pub fn pairing_file_type(&self) -> PairingProtocol {
        self.inner.lock().unwrap().pairing_file_type
    }

    pub fn set_pairing_file_type(&self, ty: PairingProtocol) {
        self.inner.lock().unwrap().pairing_file_type = ty;
    }

    pub fn pairing_file_data(&self) -> Option<Vec<u8>> {
        self.inner.lock().unwrap().pairing_file_data.clone()
    }

    /// Swift `pairingFileData.didSet`: setting the data re-derives
    /// `pairingDataDict` (a failed plist parse yields `nil`).
    pub fn set_pairing_file_data(&self, data: Option<Vec<u8>>) {
        let dict = data.as_ref().and_then(|bytes| {
            match Value::from_reader(Cursor::new(bytes.as_slice())) {
                Ok(Value::Dictionary(d)) => Some(d),
                _ => None,
            }
        });
        let mut inner = self.inner.lock().unwrap();
        inner.pairing_file_data = data;
        inner.pairing_data_dict = dict;
    }

    pub fn pairing_data_dict(&self) -> Option<Dictionary> {
        self.inner.lock().unwrap().pairing_data_dict.clone()
    }

    pub fn device_endpoint_ip(&self) -> Option<String> {
        self.inner.lock().unwrap().device_endpoint_ip.clone()
    }

    pub fn is_initialized(&self) -> bool {
        self.inner.lock().unwrap().is_initialized
    }

    pub fn set_initialized(&self, initialized: bool) {
        self.inner.lock().unwrap().is_initialized = initialized;
    }

    /// Swift `getPort(for:)` — the table value, or the protocol's
    /// default port when unset.
    pub fn get_port(&self, protocol: PairingProtocol) -> u16 {
        let inner = self.inner.lock().unwrap();
        inner
            .protocol_ports
            .get(&protocol)
            .copied()
            .unwrap_or_else(|| protocol.default_port(self.ios17_available))
    }

    /// Swift `setPort(_:for:)` — a no-op when the value is already
    /// current; otherwise stores and invalidates the connection.
    /// Returns whether the value changed.
    pub fn set_port(&self, port: u16, protocol: PairingProtocol) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if inner.protocol_ports.get(&protocol) == Some(&port) {
            return false;
        }
        debug_log(&format!(
            "[GatewayCore] setPort({port}, for: .{}) called",
            protocol.raw_value()
        ));
        inner.protocol_ports.insert(protocol, port);
        inner.generation += 1;
        true
    }

    /// Swift `setDeviceEndpointIp(_:)` — a no-op when unchanged;
    /// otherwise stores and invalidates the connection. Returns
    /// whether the value changed.
    pub fn set_device_endpoint_ip(&self, ip: Option<&str>) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if inner.device_endpoint_ip.as_deref() == ip {
            return false;
        }
        debug_log(&format!(
            "[GatewayCore] setDeviceEndpointIp({}) called",
            ip.unwrap_or("nil")
        ));
        inner.device_endpoint_ip = ip.map(str::to_string);
        inner.generation += 1;
        true
    }

    /// The current invalidation generation. Backends tag cached
    /// connections with the generation they were created under and
    /// drop them when it no longer matches (Swift
    /// `invalidateConnection()`).
    pub fn generation(&self) -> u64 {
        self.inner.lock().unwrap().generation
    }

    /// Swift `invalidateConnection()` at the base level: bump the
    /// generation so every cached connection becomes stale.
    pub fn invalidate(&self) {
        self.inner.lock().unwrap().generation += 1;
    }

    pub fn set_logging(&self, enabled: bool) {
        self.inner.lock().unwrap().logging_enabled = enabled;
    }

    pub fn logging_enabled(&self) -> bool {
        self.inner.lock().unwrap().logging_enabled
    }

    /// Swift `cleanup()`: clears initialization, pairing data and
    /// pairing type, and invalidates the connection. The port table
    /// deliberately survives (Swift does not clear it).
    pub fn cleanup(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.is_initialized = false;
        inner.pairing_file_data = None;
        inner.pairing_data_dict = None;
        inner.pairing_file_type = PairingProtocol::Unknown;
        inner.generation += 1;
    }
}
