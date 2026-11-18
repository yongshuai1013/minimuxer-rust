//! Pairing parser fixtures: both plist shapes, complete/missing-key/
//! malformed variants, mirroring Swift PairingFileParser semantics.

use mm_common::error::PairingError;
use mm_common::pairing::{
    parse_pairing_bytes, parse_pairing_content, validate_pairing_file, PairingFile,
    PairingProtocol,
};
use plist::{Dictionary, Value};

fn s(v: &str) -> String {
    format!("<string>{v}</string>")
}
fn d() -> String {
    "<data>AAECAw==</data>".to_string()
}

fn plist_xml(entries: &[(&str, String)]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n",
    );
    for (k, v) in entries {
        out.push_str(&format!("<key>{k}</key>\n{v}\n"));
    }
    out.push_str("</dict>\n</plist>\n");
    out
}

fn lockdown_entries() -> Vec<(&'static str, String)> {
    vec![
        ("WiFiMACAddress", s("AA:BB:CC:DD:EE:FF")),
        ("SystemBUID", s("BUID-1234")),
        ("RootPrivateKey", d()),
        ("HostPrivateKey", d()),
        ("HostID", s("HOST-ID-1")),
        ("RootCertificate", d()),
        ("UDID", s("00008030-001234567890402E")),
        ("EscrowBag", d()),
        ("HostCertificate", d()),
        ("DeviceCertificate", d()),
    ]
}

fn rp_entries() -> Vec<(&'static str, String)> {
    vec![
        ("private_key", d()),
        ("public_key", d()),
        ("identifier", s("rp-device-1")),
    ]
}

fn to_dict(xml: &str) -> Dictionary {
    match Value::from_reader(std::io::Cursor::new(xml.as_bytes())).unwrap() {
        Value::Dictionary(d) => d,
        other => panic!("expected dict, got {other:?}"),
    }
}

#[test]
fn valid_lockdown_parses_and_extracts() {
    let xml = plist_xml(&lockdown_entries());
    let file = parse_pairing_content(&xml, None).expect("valid lockdown");
    assert_eq!(file.mode(), PairingProtocol::Lockdown);
    match file {
        PairingFile::Lockdown(f) => {
            assert_eq!(f.udid, "00008030-001234567890402E");
            assert_eq!(f.system_buid, "BUID-1234");
            assert_eq!(f.host_id.as_deref(), Some("HOST-ID-1"));
            assert_eq!(f.wifi_mac_address.as_deref(), Some("AA:BB:CC:DD:EE:FF"));
            assert_eq!(f.host_certificate.as_deref(), Some(&[0, 1, 2, 3][..]));
            assert_eq!(f.raw_content, xml);
        }
        other => panic!("expected lockdown, got {other:?}"),
    }
    let dict = to_dict(&xml);
    assert_eq!(
        validate_pairing_file(Some(&dict), None).unwrap(),
        PairingProtocol::Lockdown
    );
}

#[test]
fn valid_rp_parses_and_extracts() {
    let xml = plist_xml(&rp_entries());
    let file = parse_pairing_content(&xml, None).expect("valid rp");
    assert_eq!(file.mode(), PairingProtocol::RpPairing);
    match file {
        PairingFile::Rp(f) => {
            assert_eq!(f.identifier, "rp-device-1");
            assert_eq!(f.private_key.as_deref(), Some(&[0, 1, 2, 3][..]));
            assert_eq!(f.public_key.as_deref(), Some(&[0, 1, 2, 3][..]));
        }
        other => panic!("expected rp, got {other:?}"),
    }
}

#[test]
fn both_complete_is_ambiguous_without_preference() {
    let mut entries = lockdown_entries();
    entries.extend(rp_entries());
    let xml = plist_xml(&entries);
    let dict = to_dict(&xml);
    let err = validate_pairing_file(Some(&dict), None).unwrap_err();
    assert_eq!(
        err,
        PairingError::Ambiguous(
            "The pairing file contains credentials for both Remote Pairing and Lockdown protocols. Specify a preferred protocol to resolve ambiguity."
                .to_string()
        )
    );
    // A preference resolves the ambiguity, both ways.
    assert_eq!(
        validate_pairing_file(Some(&dict), Some(PairingProtocol::Lockdown)).unwrap(),
        PairingProtocol::Lockdown
    );
    assert_eq!(
        parse_pairing_content(&xml, Some(PairingProtocol::RpPairing))
            .unwrap()
            .mode(),
        PairingProtocol::RpPairing
    );
}

#[test]
fn both_incomplete_is_invalid_plist_listing_all_missing() {
    let xml = plist_xml(&[]);
    let dict = to_dict(&xml);
    let err = validate_pairing_file(Some(&dict), None).unwrap_err();
    let expected = "Unrecognized pairing file format.   \u{2022} Missing .rppairing attributes: [\"private_key\", \"public_key\", \"identifier\"];   \u{2022} Missing .lockdown  attributes: [\"WiFiMACAddress\", \"SystemBUID\", \"RootPrivateKey\", \"HostPrivateKey\", \"HostID\", \"RootCertificate\", \"UDID\", \"EscrowBag\", \"HostCertificate\", \"DeviceCertificate\"].";
    assert_eq!(err, PairingError::InvalidPlist(expected.to_string()));
}

#[test]
fn missing_one_lockdown_key_with_preference_is_incomplete() {
    let entries: Vec<_> = lockdown_entries()
        .into_iter()
        .filter(|(k, _)| *k != "EscrowBag")
        .collect();
    let xml = plist_xml(&entries);
    let dict = to_dict(&xml);
    let err = validate_pairing_file(Some(&dict), Some(PairingProtocol::Lockdown)).unwrap_err();
    assert_eq!(
        err,
        PairingError::Incomplete {
            protocol: PairingProtocol::Lockdown,
            missing_keys: vec!["EscrowBag".to_string()],
        }
    );
    assert_eq!(
        err.description(),
        "The pairing file is incomplete for .lockdown. Missing keys: EscrowBag."
    );
    // Without a preference the same file is an invalidPlist (neither
    // shape is complete), not an incomplete.
    let err2 = validate_pairing_file(Some(&dict), None).unwrap_err();
    assert!(matches!(err2, PairingError::InvalidPlist(_)));
}

#[test]
fn missing_one_rp_key_with_preference_is_incomplete() {
    let entries: Vec<_> = rp_entries()
        .into_iter()
        .filter(|(k, _)| *k != "public_key")
        .collect();
    let xml = plist_xml(&entries);
    let err = parse_pairing_content(&xml, Some(PairingProtocol::RpPairing)).unwrap_err();
    assert_eq!(
        err,
        PairingError::Incomplete {
            protocol: PairingProtocol::RpPairing,
            missing_keys: vec!["public_key".to_string()],
        }
    );
}

#[test]
fn malformed_and_non_dict_and_none_inputs() {
    // Not a plist at all.
    let err = parse_pairing_content("this is not a plist", None).unwrap_err();
    assert_eq!(
        err,
        PairingError::InvalidPlist("PropertyListSerialization failed".to_string())
    );
    // Valid plist, root is an array, not a dictionary.
    let arr = "<?xml version=\"1.0\"?><plist version=\"1.0\"><array><string>x</string></array></plist>";
    let err = parse_pairing_content(arr, None).unwrap_err();
    assert_eq!(
        err,
        PairingError::InvalidPlist("PropertyListSerialization failed".to_string())
    );
    // validate(None).
    let err = validate_pairing_file(None, None).unwrap_err();
    assert_eq!(
        err,
        PairingError::InvalidPlist(
            "The file could not be parsed as a property list (plist).".to_string()
        )
    );
    // preferred = .unknown.
    let xml = plist_xml(&lockdown_entries());
    let dict = to_dict(&xml);
    let err = validate_pairing_file(Some(&dict), Some(PairingProtocol::Unknown)).unwrap_err();
    assert_eq!(
        err,
        PairingError::InvalidPlist("Unknown pairing protocol specified".to_string())
    );
    // Non-UTF8 bytes.
    let err = parse_pairing_bytes(&[0xFF, 0xFE, 0x00], None).unwrap_err();
    assert_eq!(
        err,
        PairingError::Unreadable("UTF-8 encoding failed".to_string())
    );
}
