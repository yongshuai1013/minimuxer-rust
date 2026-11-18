//! DeviceEndpoint semantics (INVENTORY 2.6): single source of
//! truth, uninitialized error, gateway-sink mirroring.

use mm_net::{DeviceEndpoint, EndpointIpSink};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct RecordingSink {
    calls: Mutex<Vec<Option<String>>>,
}

impl EndpointIpSink for RecordingSink {
    fn set_device_endpoint_ip(&self, ip: Option<&str>) {
        self.calls.lock().unwrap().push(ip.map(|s| s.to_string()));
    }
}

#[test]
fn uninitialized_ip_read_fails_with_internal_error() {
    let ep = DeviceEndpoint::new();
    assert!(!ep.is_initialized());
    let err = ep.ip().expect_err("must fail while uninitialized");
    assert_eq!(err.code(), "deviceEndpointNotInitialized");
    assert_eq!(err.to_info().domain, "MinimuxerInternalError");
}

#[test]
fn update_and_clear_mirror_to_sink() {
    let ep = DeviceEndpoint::new();
    let sink = Arc::new(RecordingSink::default());
    ep.attach_sink(sink.clone());
    // Attach pushes the current (nil) value.
    assert_eq!(sink.calls.lock().unwrap().clone(), vec![None]);

    ep.update("10.7.0.1");
    assert!(ep.is_initialized());
    assert_eq!(ep.ip().unwrap(), "10.7.0.1");

    ep.clear();
    assert!(!ep.is_initialized());
    assert!(ep.ip().is_err());

    assert_eq!(
        sink.calls.lock().unwrap().clone(),
        vec![None, Some("10.7.0.1".to_string()), None]
    );
}

#[test]
fn late_attach_pushes_current_value() {
    let ep = DeviceEndpoint::new();
    ep.update("10.7.0.9");
    let sink = Arc::new(RecordingSink::default());
    ep.attach_sink(sink.clone());
    assert_eq!(
        sink.calls.lock().unwrap().clone(),
        vec![Some("10.7.0.9".to_string())]
    );
}
