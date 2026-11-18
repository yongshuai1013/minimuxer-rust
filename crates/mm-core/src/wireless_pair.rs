//! `WirelessPairService` (Swift, subsystem 9) — the
//! wireless pairing service the facade exposes as
//! `wirelessPair` (behavior list
//! `docs/m8-wireless-behavior.md` §A, diff list
//! `docs/m8-wireless-diff.md`).
//!
//! The service owns: the single start slot and the
//! keyed trigger-task registry, the Bonjour
//! advertisement's lifecycle (mm-rp's dns-sd route),
//! and the bridge between the gateway's event surface
//! and the App-facing handlers the shell installs.
//! The pairing operations themselves are the current
//! gateway's (Swift: `deviceProvider.gateway`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use mm_common::constants::{
    DEFAULT_AD_DOMAIN, REMOTE_PAIRING_PAIRABLE_HOST_SERVICE_TYPE,
};
use mm_common::MmErrorInfo;
use mm_gateway::{
    GatewayFuture, PairCancelToken, PairedDeviceRecord, WirelessPairEvents,
    WirelessPairStart, WirelessPairTrigger,
};
use mm_rp::Advertisement;
use tokio::sync::oneshot;

use crate::core::CoreGatewayProvider;

/// The App-facing handlers (Swift WirelessPairService's
/// three callback properties, plus the per-call
/// `resolveFileName` closure carried as an object
/// method across the FFI — diff D-M8-3). Installed by
/// the shell through [`WirelessPairService::
/// set_app_events`].
pub trait WirelessPairAppEvents: Send + Sync {
    /// Swift `onPinReceived`.
    fn on_pin_received(&self, pin: &str);

    /// Swift `onReadyToPair` (the port crosses as i32,
    /// Swift's `Int`).
    fn on_ready_to_pair(&self, service_id: &str, port: i32);

    /// Swift `onRequestPin` presence + issuance:
    /// returns true when the App has a PIN handler and
    /// the request was issued (the answer comes back
    /// through `complete_pin`); false when no handler
    /// is set — the service then answers "000000"
    /// itself (behavior W6).
    fn request_pin(&self, request_id: u64) -> bool;

    /// Swift's `resolveFileName(name, model)` closure.
    fn resolve_file_name(&self, name: &str, model: &str) -> Option<String>;
}

/// Pending PIN answers, keyed by request id (the M2
/// `request_service_port` registry shape).
type PinRequests = Mutex<HashMap<u64, oneshot::Sender<String>>>;

struct TaskSlot {
    id: u64,
    cancel: PairCancelToken,
}

/// Swift `WirelessPairService`.
pub struct WirelessPairService {
    provider: Arc<dyn CoreGatewayProvider>,
    app_events: Mutex<Option<Arc<dyn WirelessPairAppEvents>>>,
    pin_requests: Arc<PinRequests>,
    next_pin_request: AtomicU64,
    next_task: AtomicU64,
    start_task: Mutex<Option<TaskSlot>>,
    trigger_tasks: Mutex<HashMap<String, TaskSlot>>,
    advertisement: Mutex<Option<Advertisement>>,
}

impl WirelessPairService {
    pub fn new(provider: Arc<dyn CoreGatewayProvider>) -> Arc<Self> {
        Arc::new(Self {
            provider,
            app_events: Mutex::new(None),
            pin_requests: Arc::new(Mutex::new(HashMap::new())),
            next_pin_request: AtomicU64::new(0),
            next_task: AtomicU64::new(0),
            start_task: Mutex::new(None),
            trigger_tasks: Mutex::new(HashMap::new()),
            advertisement: Mutex::new(None),
        })
    }

    /// Installs (or clears) the App-facing handlers.
    pub fn set_app_events(&self, events: Option<Arc<dyn WirelessPairAppEvents>>) {
        *self.app_events.lock().unwrap() = events;
    }

    /// Delivers the App's PIN answer for a pending
    /// request (the FFI completion call). Returns
    /// false when no request with that id is pending.
    pub fn complete_pin(&self, request_id: u64, pin: String) -> bool {
        let sender = self.pin_requests.lock().unwrap().remove(&request_id);
        match sender {
            Some(sender) => sender.send(pin).is_ok(),
            None => false,
        }
    }

    /// State query for the FFI: whether a start
    /// operation is in flight, and how many trigger
    /// operations are.
    pub fn state(&self) -> (bool, usize) {
        (
            self.start_task.lock().unwrap().is_some(),
            self.trigger_tasks.lock().unwrap().len(),
        )
    }

    fn app_events(&self) -> Option<Arc<dyn WirelessPairAppEvents>> {
        self.app_events.lock().unwrap().clone()
    }

    fn gateway(&self) -> Arc<dyn mm_gateway::DeviceGateway> {
        self.provider
            .current_gateway()
            .expect("wireless pair gateway provider must always yield a gateway")
    }

    // -- start (behavior W1–W4) ----------------------------------------------

    /// Swift `start(hostName:hostModel:outPath:
    /// resolveFileName:completion:)` — the completion
    /// is this future's return value across the port.
    pub async fn start(
        self: &Arc<Self>,
        host_name: &str,
        host_model: &str,
        out_path: &str,
    ) -> Result<PairedDeviceRecord, MmErrorInfo> {
        let id = self.next_task.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = PairCancelToken::new();
        // W1: re-entry cancels the previous start.
        if let Some(old) = self
            .start_task
            .lock()
            .unwrap()
            .replace(TaskSlot { id, cancel: cancel.clone() })
        {
            old.cancel.cancel();
        }
        let (tx, rx) = oneshot::channel();
        let gateway = self.gateway();
        let events = self.events_bridge();
        let host_name = host_name.to_string();
        let host_model = host_model.to_string();
        let out_path = out_path.to_string();
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let result = {
                let args = WirelessPairStart {
                    host_name: &host_name,
                    host_model: &host_model,
                    out_path: &out_path,
                    events,
                    cancel,
                };
                gateway.start_wireless_pair(args).await
            };
            if let Some(service) = weak.upgrade() {
                // W2: advertising stops before the
                // completion is delivered, on both the
                // success and failure paths.
                service.stop_advertising();
                service.clear_start_slot(id);
            }
            let _ = tx.send(result);
        });
        match rx.await {
            Ok(result) => result,
            Err(_) => Err(mm_gateway::unsupported("startWirelessPair")),
        }
    }

    fn clear_start_slot(&self, id: u64) {
        let mut slot = self.start_task.lock().unwrap();
        if slot.as_ref().map(|s| s.id) == Some(id) {
            *slot = None;
        }
    }

    // -- trigger (behavior W5–W6) ----------------------------------------------

    /// Swift `trigger(targetIp:targetPort:hostName:
    /// hostModel:outPath:resolveFileName:completion:)`.
    pub async fn trigger(
        self: &Arc<Self>,
        target_ip: &str,
        target_port: u16,
        host_name: &str,
        host_model: &str,
        out_path: &str,
    ) -> Result<PairedDeviceRecord, MmErrorInfo> {
        let key = format!("{target_ip}:{target_port}");
        let id = self.next_task.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = PairCancelToken::new();
        // W5: same-key re-entry cancels the old task;
        // different keys run in parallel.
        if let Some(old) = self.trigger_tasks.lock().unwrap().insert(
            key.clone(),
            TaskSlot {
                id,
                cancel: cancel.clone(),
            },
        ) {
            old.cancel.cancel();
        }
        let (tx, rx) = oneshot::channel();
        let gateway = self.gateway();
        let events = self.events_bridge();
        let host_name = host_name.to_string();
        let host_model = host_model.to_string();
        let out_path = out_path.to_string();
        let target_ip = target_ip.to_string();
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let result = {
                let args = WirelessPairTrigger {
                    target_ip: &target_ip,
                    target_port,
                    host_name: &host_name,
                    host_model: &host_model,
                    out_path: &out_path,
                    events,
                    cancel,
                };
                gateway.trigger_wireless_pair(args).await
            };
            if let Some(service) = weak.upgrade() {
                service.clear_trigger_slot(&key, id);
            }
            let _ = tx.send(result);
        });
        match rx.await {
            Ok(result) => result,
            Err(_) => Err(mm_gateway::unsupported("triggerWirelessPair")),
        }
    }

    fn clear_trigger_slot(&self, key: &str, id: u64) {
        let mut tasks = self.trigger_tasks.lock().unwrap();
        if tasks.get(key).map(|s| s.id) == Some(id) {
            tasks.remove(key);
        }
    }

    // -- stop (behavior W7) ------------------------------------------------------

    /// Swift `stop()`: cancels the start task and all
    /// trigger tasks and stops advertising.
    pub fn stop(&self) {
        if let Some(slot) = self.start_task.lock().unwrap().take() {
            slot.cancel.cancel();
        }
        let triggers: Vec<TaskSlot> = self
            .trigger_tasks
            .lock()
            .unwrap()
            .drain()
            .map(|(_, slot)| slot)
            .collect();
        for slot in triggers {
            slot.cancel.cancel();
        }
        self.stop_advertising();
    }

    // -- advertising (behavior W3/B1) --------------------------------------------

    fn start_advertising(
        &self,
        service_id: &str,
        port: u16,
        txt_records: &[(String, String)],
    ) {
        // B1: at most one advertisement; the old one
        // stops first (Swift startAdvertising).
        self.stop_advertising();
        match Advertisement::register(
            service_id,
            REMOTE_PAIRING_PAIRABLE_HOST_SERVICE_TYPE,
            DEFAULT_AD_DOMAIN,
            port,
            txt_records,
        ) {
            Ok(ad) => {
                *self.advertisement.lock().unwrap() = Some(ad);
            }
            Err(e) => {
                // Swift's publish() surfaces nothing
                // (no delegate is set); log and carry
                // on — onReadyToPair still fires.
                mm_common::logging::debug_log(&format!(
                    "[WirelessPairService] advertising failed to start: {e}"
                ));
            }
        }
    }

    fn stop_advertising(&self) {
        if let Some(ad) = self.advertisement.lock().unwrap().take() {
            ad.stop();
        }
    }

    /// Whether an advertisement is currently live
    /// (host tests; the dns-sd route only exists on
    /// Apple targets).
    pub fn is_advertising(&self) -> bool {
        self.advertisement.lock().unwrap().is_some()
    }

    // -- gateway events bridge -----------------------------------------------------

    fn events_bridge(self: &Arc<Self>) -> Arc<dyn WirelessPairEvents> {
        Arc::new(GatewayEventsBridge {
            service: Arc::downgrade(self),
        })
    }
}

/// The gateway-facing event surface of one operation,
/// bridging to the service (advertising + App events).
struct GatewayEventsBridge {
    service: Weak<WirelessPairService>,
}

impl WirelessPairEvents for GatewayEventsBridge {
    fn on_ready(
        &self,
        service_id: &str,
        port: u16,
        txt_records: &[(String, String)],
    ) {
        let Some(service) = self.service.upgrade() else {
            return;
        };
        // W3: advertise first, then tell the App.
        service.start_advertising(service_id, port, txt_records);
        if let Some(events) = service.app_events() {
            events.on_ready_to_pair(service_id, port as i32);
        }
    }

    fn on_pin(&self, pin: &str) {
        let Some(service) = self.service.upgrade() else {
            return;
        };
        if let Some(events) = service.app_events() {
            events.on_pin_received(pin);
        }
    }

    fn request_pin<'a>(&'a self) -> GatewayFuture<'a, String> {
        Box::pin(async move {
            let Some(service) = self.service.upgrade() else {
                return Ok("000000".to_string());
            };
            let Some(events) = service.app_events() else {
                // W6: no App events at all — the default
                // PIN, as when onRequestPin is unset.
                return Ok("000000".to_string());
            };
            let request_id =
                service.next_pin_request.fetch_add(1, Ordering::Relaxed) + 1;
            let (tx, rx) = oneshot::channel();
            service
                .pin_requests
                .lock()
                .unwrap()
                .insert(request_id, tx);
            if !events.request_pin(request_id) {
                // No App PIN handler: answer immediately
                // (behavior W6).
                service.pin_requests.lock().unwrap().remove(&request_id);
                return Ok("000000".to_string());
            }
            match rx.await {
                Ok(pin) => Ok(pin),
                Err(_) => Ok("000000".to_string()),
            }
        })
    }

    fn resolve_file_name(&self, name: &str, model: &str) -> Option<String> {
        let service = self.service.upgrade()?;
        let events = service.app_events()?;
        events.resolve_file_name(name, model)
    }
}
