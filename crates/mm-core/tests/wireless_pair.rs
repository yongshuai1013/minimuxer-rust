//! M8 host tests for the WirelessPairService layer
//! (behavior list `docs/m8-wireless-behavior.md`
//! W1–W7): task registry semantics, PIN request
//! routing, and event forwarding, over a fake gateway.

use std::sync::{Arc, Mutex, Weak};

use mm_common::{MmErrorInfo, PairingProtocol};
use mm_core::{CoreGatewayProvider, WirelessPairAppEvents, WirelessPairService};
use mm_gateway::gateway::{
    WirelessPairStart, WirelessPairTrigger,
};
use mm_gateway::{
    DeviceGateway, GatewayBackend, GatewayCore, GatewayFuture, GatewayProvider,
    PairedDeviceRecord,
};

const LOCKDOWN_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n<key>WiFiMACAddress</key>\n<string>AA:BB:CC:DD:EE:FF</string>\n<key>SystemBUID</key>\n<string>BUID-1</string>\n<key>RootPrivateKey</key>\n<data>AAECAw==</data>\n<key>HostPrivateKey</key>\n<data>AAECAw==</data>\n<key>HostID</key>\n<string>H1</string>\n<key>RootCertificate</key>\n<data>AAECAw==</data>\n<key>UDID</key>\n<string>UDID-1</string>\n<key>EscrowBag</key>\n<data>AAECAw==</data>\n<key>HostCertificate</key>\n<data>AAECAw==</data>\n<key>DeviceCertificate</key>\n<data>AAECAw==</data>\n</dict>\n</plist>\n";

fn record() -> PairedDeviceRecord {
    let pairing_file =
        mm_common::parse_pairing_content(LOCKDOWN_XML, None).expect("parse");
    PairedDeviceRecord {
        name: "Peer".to_string(),
        model: "Model".to_string(),
        pairing_file_path: "/tmp/Peer_Model_rp.plist".to_string(),
        pairing_file,
    }
}

fn cancelled() -> MmErrorInfo {
    mm_gateway::gateway::gw_error(
        mm_common::DeviceGatewayCode::ServiceError,
        "Pairing failed or cancelled",
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Block until the cancel token fires, then fail
    /// the way the real gateway does.
    WaitForCancel,
    /// Fire onReady (start only), then complete.
    Complete,
    /// Ask for a PIN via the events, record it, then
    /// complete (trigger only).
    RequestPin,
}

struct FakeGateway {
    core: GatewayCore,
    start_mode: Mutex<Mode>,
    trigger_mode: Mutex<Mode>,
    start_calls: Mutex<u32>,
    trigger_keys: Mutex<Vec<String>>,
    pins_seen: Arc<Mutex<Vec<String>>>,
}

impl FakeGateway {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            core: GatewayCore::new(),
            start_mode: Mutex::new(Mode::WaitForCancel),
            trigger_mode: Mutex::new(Mode::WaitForCancel),
            start_calls: Mutex::new(0),
            trigger_keys: Mutex::new(Vec::new()),
            pins_seen: Arc::new(Mutex::new(Vec::new())),
        })
    }
}

impl DeviceGateway for FakeGateway {
    fn backend(&self) -> GatewayBackend {
        GatewayBackend::Idevice
    }

    fn core(&self) -> &GatewayCore {
        &self.core
    }

    fn set_logging(&self, enabled: bool) {
        self.core.set_logging(enabled);
    }

    fn start<'a>(
        &'a self,
        _content: &'a str,
        _preferred: Option<PairingProtocol>,
    ) -> GatewayFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn stop<'a>(&'a self) -> GatewayFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn fetch_udid<'a>(&'a self) -> GatewayFuture<'a, String> {
        Box::pin(async { Ok("UDID-1".to_string()) })
    }

    fn get_lockdown_value<'a>(&'a self, _key: &'a str) -> GatewayFuture<'a, String> {
        Box::pin(async { Err(mm_gateway::unsupported("getLockdownValue")) })
    }

    fn start_session<'a>(&'a self) -> GatewayFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn stop_session<'a>(&'a self) -> GatewayFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn open_service<'a>(
        &'a self,
        _name: &'a str,
    ) -> GatewayFuture<'a, mm_gateway::OpenedService> {
        Box::pin(async { Err(mm_gateway::unsupported("openService")) })
    }

    fn start_wireless_pair<'a>(
        &'a self,
        args: WirelessPairStart<'a>,
    ) -> GatewayFuture<'a, PairedDeviceRecord> {
        *self.start_calls.lock().unwrap() += 1;
        let mode = *self.start_mode.lock().unwrap();
        Box::pin(async move {
            match mode {
                Mode::WaitForCancel => {
                    args.cancel.cancelled().await;
                    Err(cancelled())
                }
                Mode::Complete => {
                    args.events.on_ready(
                        "svc-id",
                        4242,
                        &[("name".to_string(), "SideStore".to_string())],
                    );
                    args.events.on_pin("123456");
                    Ok(record())
                }
                Mode::RequestPin => {
                    Err(mm_gateway::unsupported("startWirelessPair"))
                }
            }
        })
    }

    fn trigger_wireless_pair<'a>(
        &'a self,
        args: WirelessPairTrigger<'a>,
    ) -> GatewayFuture<'a, PairedDeviceRecord> {
        self.trigger_keys
            .lock()
            .unwrap()
            .push(format!("{}:{}", args.target_ip, args.target_port));
        let mode = *self.trigger_mode.lock().unwrap();
        let pins_seen = self.pins_seen.clone();
        Box::pin(async move {
            match mode {
                Mode::WaitForCancel => {
                    args.cancel.cancelled().await;
                    Err(cancelled())
                }
                Mode::Complete => Ok(record()),
                Mode::RequestPin => {
                    let pin = args.events.request_pin().await?;
                    pins_seen.lock().unwrap().push(pin);
                    Ok(record())
                }
            }
        })
    }
}

struct FakeProvider {
    gateway: Arc<FakeGateway>,
}

impl GatewayProvider for FakeProvider {
    fn current_gateway(&self) -> Option<Arc<dyn DeviceGateway>> {
        Some(self.gateway.clone())
    }
}

impl CoreGatewayProvider for FakeProvider {
    fn backend(&self) -> GatewayBackend {
        GatewayBackend::Idevice
    }

    fn set_backend(
        &self,
        _backend: GatewayBackend,
    ) -> Arc<dyn DeviceGateway> {
        self.gateway.clone()
    }
}

fn make_service() -> (Arc<WirelessPairService>, Arc<FakeGateway>) {
    let gateway = FakeGateway::new();
    let provider: Arc<dyn CoreGatewayProvider> =
        Arc::new(FakeProvider { gateway: gateway.clone() });
    (WirelessPairService::new(provider), gateway)
}

async fn wait_until(mut cond: impl FnMut() -> bool) {
    for _ in 0..200 {
        if cond() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("condition not met in time");
}

/// W1 + W7: a second start cancels the first; stop()
/// cancels the second; the registry drains.
#[tokio::test]
async fn start_reentry_cancels_previous_and_stop_cancels_current() {
    let (service, gateway) = make_service();
    let first = {
        let service = service.clone();
        tokio::spawn(
            async move { service.start("SideStore", "Mac17,7", "/tmp").await },
        )
    };
    wait_until(|| *gateway.start_calls.lock().unwrap() == 1).await;
    assert_eq!(service.state(), (true, 0));

    let second = {
        let service = service.clone();
        tokio::spawn(
            async move { service.start("SideStore", "Mac17,7", "/tmp").await },
        )
    };
    let first_result = first.await.expect("first task");
    assert_eq!(
        first_result.expect_err("first start must be cancelled").reason,
        "Pairing failed or cancelled"
    );
    wait_until(|| *gateway.start_calls.lock().unwrap() == 2).await;
    assert_eq!(service.state(), (true, 0));

    service.stop();
    let second_result = second.await.expect("second task");
    assert!(second_result.is_err());
    assert_eq!(service.state(), (false, 0));
}

/// W5: same-key trigger re-entry cancels the old task;
/// a different key runs in parallel.
#[tokio::test]
async fn trigger_keyed_tasks() {
    let (service, _gateway) = make_service();
    let t1 = {
        let service = service.clone();
        tokio::spawn(async move {
            service
                .trigger("10.0.0.1", 49152, "SideStore", "Mac17,7", "/tmp")
                .await
        })
    };
    wait_until(|| service.state().1 == 1).await;
    let t2 = {
        let service = service.clone();
        tokio::spawn(async move {
            service
                .trigger("10.0.0.2", 49152, "SideStore", "Mac17,7", "/tmp")
                .await
        })
    };
    wait_until(|| service.state().1 == 2).await;
    // Same key as t1: cancels t1, keeps the count at 2.
    let t3 = {
        let service = service.clone();
        tokio::spawn(async move {
            service
                .trigger("10.0.0.1", 49152, "SideStore", "Mac17,7", "/tmp")
                .await
        })
    };
    assert!(t1.await.expect("t1 task").is_err());
    assert_eq!(service.state().1, 2);
    service.stop();
    assert!(t2.await.expect("t2 task").is_err());
    assert!(t3.await.expect("t3 task").is_err());
    assert_eq!(service.state(), (false, 0));
}

/// W6: with no App events installed, a PIN request is
/// answered "000000" by the service itself.
#[tokio::test]
async fn trigger_pin_defaults_without_app_handler() {
    let (service, gateway) = make_service();
    *gateway.trigger_mode.lock().unwrap() = Mode::RequestPin;
    let result = service
        .trigger("10.0.0.9", 49152, "SideStore", "Mac17,7", "/tmp")
        .await;
    assert!(result.is_ok());
    assert_eq!(
        gateway.pins_seen.lock().unwrap().as_slice(),
        ["000000".to_string()]
    );
}

/// Fake App events: records ready/pin deliveries and
/// answers PIN requests by completing them through
/// the service (the FFI shape).
struct FakeAppEvents {
    service: Mutex<Option<Weak<WirelessPairService>>>,
    has_pin_handler: bool,
    answer_pin: String,
    ready: Mutex<Vec<(String, i32)>>,
    pins_received: Mutex<Vec<String>>,
    pin_request_ids: Mutex<Vec<u64>>,
}

impl WirelessPairAppEvents for FakeAppEvents {
    fn on_pin_received(&self, pin: &str) {
        self.pins_received.lock().unwrap().push(pin.to_string());
    }

    fn on_ready_to_pair(&self, service_id: &str, port: i32) {
        self.ready
            .lock()
            .unwrap()
            .push((service_id.to_string(), port));
    }

    fn request_pin(&self, request_id: u64) -> bool {
        self.pin_request_ids.lock().unwrap().push(request_id);
        if !self.has_pin_handler {
            return false;
        }
        let service = self
            .service
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|w| w.upgrade());
        if let Some(service) = service {
            let pin = self.answer_pin.clone();
            tokio::spawn(async move {
                service.complete_pin(request_id, pin);
            });
        }
        true
    }

    fn resolve_file_name(&self, _name: &str, _model: &str) -> Option<String> {
        None
    }
}

/// W3/W4: onReady and onPin reach the App handlers;
/// W6: a handled PIN request is answered by id.
#[tokio::test]
async fn app_events_forwarding_and_pin_by_request_id() {
    let (service, gateway) = make_service();
    let app = Arc::new(FakeAppEvents {
        service: Mutex::new(Some(Arc::downgrade(&service))),
        has_pin_handler: true,
        answer_pin: "654321".to_string(),
        ready: Mutex::new(Vec::new()),
        pins_received: Mutex::new(Vec::new()),
        pin_request_ids: Mutex::new(Vec::new()),
    });
    service.set_app_events(Some(app.clone()));

    // Start flow: onReady → onReadyToPair, onPin →
    // onPinReceived.
    *gateway.start_mode.lock().unwrap() = Mode::Complete;
    let result = service.start("SideStore", "Mac17,7", "/tmp").await;
    assert!(result.is_ok());
    assert_eq!(
        app.ready.lock().unwrap().as_slice(),
        [("svc-id".to_string(), 4242)]
    );
    assert_eq!(
        app.pins_received.lock().unwrap().as_slice(),
        ["123456".to_string()]
    );

    // Trigger flow: the PIN the gateway receives is
    // the App's answer, routed by request id.
    *gateway.trigger_mode.lock().unwrap() = Mode::RequestPin;
    let result = service
        .trigger("10.0.0.10", 49152, "SideStore", "Mac17,7", "/tmp")
        .await;
    assert!(result.is_ok());
    assert_eq!(
        gateway.pins_seen.lock().unwrap().as_slice(),
        ["654321".to_string()]
    );
    assert_eq!(app.pin_request_ids.lock().unwrap().len(), 1);
    // An unknown id completes nothing.
    assert!(!service.complete_pin(999_999, "000000".to_string()));
}
