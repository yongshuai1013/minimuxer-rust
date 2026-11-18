//! `GatewayHub` — the FFI-side owner of the current device
//! gateway (Swift: `DeviceProvider` plus the backend-switching
//! half of `Minimuxer.set`).
//!
//! Backend semantics, preserved from Swift
//! (`MinimuxerApi.swift`): the App picks a backend
//! (`idevice` by default); switching builds a *fresh* gateway
//! instance for the new backend, first carrying the current
//! remote-pairing port into it, then swaps it in as current.
//! The outgoing instance becomes unreachable (diff list D-8).
//!
//! The hub is also the seam where the M2 stand-ins retire:
//! - as mm-net's `ServicePortSource`, the connection manager's
//!   port queries now resolve against the *real* gateway (the
//!   `TablePortSource` stand-in is gone from the wiring),
//! - as mm-net's `EndpointIpSink`, endpoint updates discovered
//!   by the network layer flow into the current gateway
//!   (`BaseDeviceGateway.deviceEndpointIP`, whose changes
//!   invalidate cached gateway connections),
//! - as mm-gateway's `GatewayProvider`, the usbmuxd proxy reads
//!   whichever gateway is current, per packet.

use std::sync::{Arc, Mutex};

use mm_common::PairingProtocol;
use mm_gateway::{DeviceGateway, GatewayBackend, GatewayProvider};
use mm_net::{EndpointIpSink, ServicePortSource};

pub struct GatewayHub {
    backend: Mutex<GatewayBackend>,
    current: Mutex<Option<Arc<dyn DeviceGateway>>>,
}

impl Default for GatewayHub {
    fn default() -> Self {
        Self::new()
    }
}

impl GatewayHub {
    pub fn new() -> Self {
        Self {
            backend: Mutex::new(GatewayBackend::default()),
            current: Mutex::new(None),
        }
    }

    pub fn backend(&self) -> GatewayBackend {
        *self.backend.lock().unwrap()
    }

    /// The current gateway, creating the default-backend
    /// instance on first use (Swift's DeviceProvider starts
    /// empty; the gateway FFI entry points materialize it so the
    /// shell never deals in Options).
    pub fn ensure_gateway(&self) -> Arc<dyn DeviceGateway> {
        let mut slot = self.current.lock().unwrap();
        if let Some(gw) = slot.as_ref() {
            return gw.clone();
        }
        let gw = mm_gateway::make_gateway(self.backend());
        *slot = Some(gw.clone());
        gw
    }

    /// `Minimuxer.set(backend:)`: build a fresh instance for the
    /// new backend, carry the outgoing gateway's remote-pairing
    /// port into it, then swap.
    pub fn set_backend(&self, backend: GatewayBackend) -> Arc<dyn DeviceGateway> {
        let new_gw = mm_gateway::make_gateway(backend);
        if let Some(old) = self.current_gateway() {
            let rp_port = old.get_port(PairingProtocol::RpPairing);
            new_gw.set_port(rp_port, PairingProtocol::RpPairing);
        }
        *self.backend.lock().unwrap() = backend;
        *self.current.lock().unwrap() = Some(new_gw.clone());
        new_gw
    }
}

impl GatewayProvider for GatewayHub {
    fn current_gateway(&self) -> Option<Arc<dyn DeviceGateway>> {
        self.current.lock().unwrap().clone()
    }
}

impl EndpointIpSink for GatewayHub {
    fn set_device_endpoint_ip(&self, ip: Option<&str>) {
        // No gateway yet: the push is dropped, exactly like
        // Swift writing through a nil currentGateway.
        if let Some(gw) = self.current_gateway() {
            gw.set_device_endpoint_ip(ip);
        }
    }
}

impl ServicePortSource for GatewayHub {
    fn pairing_file_type(&self) -> PairingProtocol {
        self.current_gateway()
            .map(|gw| gw.pairing_file_type())
            .unwrap_or(PairingProtocol::Unknown)
    }

    fn service_port(&self) -> u16 {
        let protocol = self.pairing_file_type();
        self.current_gateway()
            .map(|gw| gw.get_port(protocol))
            .unwrap_or_else(|| protocol.default_port(false))
    }

    fn set_port(&self, port: u16, for_protocol: PairingProtocol) {
        if let Some(gw) = self.current_gateway() {
            gw.set_port(port, for_protocol);
        }
    }
}
