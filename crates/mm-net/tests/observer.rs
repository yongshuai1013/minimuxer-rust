//! PollingObserver: scripted scan sources, start/stop
//! idempotence, change emission after settle, and the signature's
//! default-route component.

mod common;

use common::*;
use mm_common::PairingProtocol;
use mm_net::{
    DeviceConnectionManager, DeviceEndpoint, NetStack, PollingObserver, RouteTable, ScanSnapshot,
    ScanSource, TablePortSource,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct ScriptedSource {
    snaps: Vec<ScanSnapshot>,
    current: Mutex<usize>,
}

impl ScriptedSource {
    fn advance(&self) {
        let mut cur = self.current.lock().unwrap();
        if *cur + 1 < self.snaps.len() {
            *cur += 1;
        }
    }
}

impl ScanSource for ScriptedSource {
    fn snapshot(&self) -> ScanSnapshot {
        let cur = *self.current.lock().unwrap();
        self.snaps[cur].clone()
    }
}

fn snap_without_tunnel() -> ScanSnapshot {
    let records = vec![v4_rec("en0", IFF_UP | IFF_RUNNING, "192.168.1.5", "255.255.255.0")];
    build(&records, &[], &[("en0", 4)])
}

fn snap_with_tunnel() -> ScanSnapshot {
    let records = vec![
        v4_rec("en0", IFF_UP | IFF_RUNNING, "192.168.1.5", "255.255.255.0"),
        v4_rec("utun0", IFF_UP | IFF_RUNNING, "10.7.0.10", "255.255.255.0"),
    ];
    build(&records, &wireguard_blob(9), &[("en0", 4), ("utun0", 9)])
}

fn make_stack(source: Arc<dyn ScanSource>) -> Arc<NetStack> {
    let ps = Arc::new(TablePortSource::default());
    ps.set_pairing_file_type(PairingProtocol::Lockdown);
    let manager = Arc::new(DeviceConnectionManager::new(ps, source.clone()));
    Arc::new(NetStack::new(
        manager,
        Arc::new(DeviceEndpoint::new()),
        source,
    ))
}

fn counting_handler(counter: Arc<AtomicUsize>) -> mm_net::NetChangeHandler {
    Arc::new(move || -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let counter = counter.clone();
        Box::pin(async move {
            counter.fetch_add(1, Ordering::SeqCst);
        })
    })
}

#[tokio::test]
async fn observer_emits_on_change_and_start_stop_are_idempotent() {
    let source = Arc::new(ScriptedSource {
        snaps: vec![snap_without_tunnel(), snap_with_tunnel()],
        current: Mutex::new(0),
    });
    let stack = make_stack(source.clone());
    let changes = Arc::new(AtomicUsize::new(0));
    stack.add_change_handler(counting_handler(changes.clone()));

    let observer = PollingObserver::with_timing(
        stack,
        source.clone(),
        Duration::from_millis(40),
        Duration::from_millis(10),
    );
    let handle = tokio::runtime::Handle::current();
    assert!(observer.start(&handle));
    assert!(!observer.start(&handle), "second start is a no-op");

    // Let the baseline snapshot settle.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(changes.load(Ordering::SeqCst), 0, "no change yet");

    source.advance();
    // Poll (40ms) + settle (10ms) + slack.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        changes.load(Ordering::SeqCst) >= 1,
        "tunnel appearance must emit a change"
    );

    assert!(observer.stop());
    assert!(!observer.stop(), "second stop is a no-op");
    assert!(observer.start(&handle), "restart works");
    assert!(observer.stop());
}

#[tokio::test]
async fn observer_signature_reacts_to_default_route_move_alone() {
    // Two snapshots with IDENTICAL interface sets; only the route
    // table's default route moves (en0 → utun0). The signature's
    // default-route component must still flag a change (the Swift
    // path monitor would fire for this).
    let base = {
        let records = vec![
            v4_rec("en0", IFF_UP, "192.168.1.5", "255.255.255.0"),
            v4_rec("utun0", IFF_UP, "10.7.0.10", "255.255.255.0"),
        ];
        let blob = rt_msg(
            4,
            RTF_GATEWAY,
            &[
                (RTAX_DST, sa_v4([0, 0, 0, 0])),
                (RTAX_GATEWAY, sa_v4([192, 168, 1, 1])),
                (RTAX_NETMASK, sa_mask(&[])),
                (RTAX_IFA, sa_v4([192, 168, 1, 5])),
            ],
        );
        build(&records, &blob, &[("en0", 4), ("utun0", 9)])
    };
    let mut moved = base.clone();
    moved.routes = RouteTable::new(mm_net::parse_route_dump(&wireguard_blob(9)));
    assert!(mm_net::net_info_sets_equal(
        &base.interfaces,
        &moved.interfaces
    ));

    let source = Arc::new(ScriptedSource {
        snaps: vec![base, moved],
        current: Mutex::new(0),
    });
    let stack = make_stack(source.clone());
    let changes = Arc::new(AtomicUsize::new(0));
    stack.add_change_handler(counting_handler(changes.clone()));
    let observer = PollingObserver::with_timing(
        stack,
        source.clone(),
        Duration::from_millis(40),
        Duration::from_millis(10),
    );
    let handle = tokio::runtime::Handle::current();
    assert!(observer.start(&handle));
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(changes.load(Ordering::SeqCst), 0);
    source.advance();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        changes.load(Ordering::SeqCst) >= 1,
        "default-route move alone must emit"
    );
    assert!(observer.stop());
}
