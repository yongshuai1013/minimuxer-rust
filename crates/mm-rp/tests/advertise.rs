//! M8 Bonjour spike + TXT format tests (behavior list
//! `docs/m8-wireless-behavior.md` B2, diff list
//! D-M8-9).
//!
//! The TXT wire format is pure and runs everywhere.
//! The registration spike runs the real dns-sd route
//! on Apple hosts (macOS CI is authoritative); on
//! other hosts the capability gate asserts the stub's
//! PlatformUnsupported instead (M6/M7 pattern).

use mm_rp::advertise::build_txt_record;
use mm_rp::{AdvertiseError, Advertisement};
use mm_common::constants::REMOTE_PAIRING_PAIRABLE_HOST_SERVICE_TYPE;

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Behavior B2: length-prefixed `key=value` entries,
/// the exact bytes NetService.data(fromTXTRecord:)
/// produces.
#[test]
fn txt_record_wire_format() {
    let bytes = build_txt_record(&pairs(&[
        ("name", "SideStore"),
        ("flags", "1"),
        ("identifier", "abc"),
    ]))
    .expect("build");
    let mut expected = Vec::new();
    expected.push(14u8);
    expected.extend_from_slice(b"name=SideStore");
    expected.push(7u8);
    expected.extend_from_slice(b"flags=1");
    expected.push(14u8);
    expected.extend_from_slice(b"identifier=abc");
    assert_eq!(bytes, expected);
}

#[test]
fn txt_record_empty_value_is_bare_key() {
    let bytes = build_txt_record(&pairs(&[("k", "")])).expect("build");
    assert_eq!(bytes, vec![1u8, b'k']);
}

#[test]
fn txt_record_rejects_oversize_entry() {
    let long = "x".repeat(300);
    let err = build_txt_record(&pairs(&[("k", &long)]))
        .expect_err("oversize entry must fail");
    assert!(matches!(err, AdvertiseError::TxtEntryTooLong(_)));
}

/// The E2 spike: register the pairable-host service
/// type with dns-sd and observe the daemon's answer.
#[test]
fn spike_register_pairable_host_service() {
    #[cfg(target_vendor = "apple")]
    {
        let name = format!("mm-spike-{}", std::process::id());
        let ad = Advertisement::register(
            &name,
            REMOTE_PAIRING_PAIRABLE_HOST_SERVICE_TYPE,
            "",
            49152,
            &pairs(&[("name", "spike"), ("flags", "1")]),
        )
        .expect("DNSServiceRegister must succeed on an Apple host");
        // The daemon answers asynchronously via the
        // pump; kDNSServiceErr_NoError = 0.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut result = None;
        while std::time::Instant::now() < deadline {
            result = ad.callback_result();
            if result.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(
            result,
            Some(0),
            "registration callback must report success"
        );
        ad.stop();
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        // Host stub (this sandbox): the route reports
        // the platform gate; macOS CI runs the real
        // spike above.
        let err = Advertisement::register(
            "mm-spike",
            REMOTE_PAIRING_PAIRABLE_HOST_SERVICE_TYPE,
            "",
            49152,
            &pairs(&[("name", "spike")]),
        )
        .expect_err("non-Apple hosts have no dns-sd");
        assert_eq!(err, AdvertiseError::PlatformUnsupported);
        println!("SKIP: dns-sd spike requires an Apple host (capability gate)");
    }
}
