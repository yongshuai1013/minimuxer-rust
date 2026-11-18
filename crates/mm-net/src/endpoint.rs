//! Device endpoint — port of Swift `DeviceEndpoint.swift`
//! (yongshuai1013/minimuxer @714aafa3): the single source of truth
//! for the current device IP.
//!
//! Swift's actor holds the gateway directly and mirrors every
//! update/clear into `gateway.setDeviceEndpointIp` (the gateway
//! invalidates its connection when the IP changes). Here the
//! gateway side is an `EndpointIpSink` trait object so mm-gateway
//! (M3) can attach when it lands; until a sink is attached, values
//! are still tracked and are pushed to the sink on attach.

use std::sync::{Arc, Mutex};

use mm_common::InternalError;

/// Receives endpoint IP changes (Swift: the gateway half of
/// `DeviceEndpoint.update/clear`).
pub trait EndpointIpSink: Send + Sync {
    fn set_device_endpoint_ip(&self, ip: Option<&str>);
}

struct State {
    ip: Option<String>,
    sink: Option<Arc<dyn EndpointIpSink>>,
}

/// Swift `DeviceEndpoint`.
pub struct DeviceEndpoint {
    state: Mutex<State>,
}

impl Default for DeviceEndpoint {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceEndpoint {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State { ip: None, sink: None }),
        }
    }

    /// Attaches the gateway sink; the current value is pushed
    /// immediately so a late-attaching gateway starts in sync.
    pub fn attach_sink(&self, sink: Arc<dyn EndpointIpSink>) {
        let current = {
            let mut st = self.state.lock().unwrap();
            st.sink = Some(sink);
            st.ip.clone()
        };
        if let Some(sink) = self.sink() {
            sink.set_device_endpoint_ip(current.as_deref());
        }
    }

    fn sink(&self) -> Option<Arc<dyn EndpointIpSink>> {
        self.state.lock().unwrap().sink.clone()
    }

    /// Swift `ip()` — throws
    /// `MinimuxerInternalError.deviceEndpointNotInitialized` while
    /// uninitialized.
    pub fn ip(&self) -> Result<String, InternalError> {
        self.state
            .lock()
            .unwrap()
            .ip
            .clone()
            .ok_or(InternalError::DeviceEndpointNotInitialized)
    }

    /// Swift `update(_:)` — stores the IP and mirrors it to the
    /// gateway sink.
    pub fn update(&self, new_ip: &str) {
        {
            let mut st = self.state.lock().unwrap();
            st.ip = Some(new_ip.to_string());
        }
        if let Some(sink) = self.sink() {
            sink.set_device_endpoint_ip(Some(new_ip));
        }
        mm_common::logging::verbose_log(&format!(
            "[minimuxer] device endpoint updated -> {new_ip}"
        ));
    }

    /// Swift `clear()` — drops the IP and mirrors nil to the sink.
    pub fn clear(&self) {
        {
            let mut st = self.state.lock().unwrap();
            st.ip = None;
        }
        if let Some(sink) = self.sink() {
            sink.set_device_endpoint_ip(None);
        }
        mm_common::logging::verbose_log("[minimuxer] device endpoint cleared -> nil");
    }

    /// Swift `isInitialized`.
    pub fn is_initialized(&self) -> bool {
        self.state.lock().unwrap().ip.is_some()
    }
}
