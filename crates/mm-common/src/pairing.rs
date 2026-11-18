//! Pairing-file model and parser — port of Swift `PairingProtocol`,
//! `PairingFile`, `RPPairingFile`, `LockdownPairingFile` and
//! `PairingFileParser` (MinimuxerCommon). Source: yongshuai1013/minimuxer
//! @714aafa3, `Common/PairingProtocol.swift` + `Common/PairingFile.swift`.
//!
//! Semantics replicated exactly, including error strings: presence of a
//! required key is all that is checked (values are not type-validated),
//! and validation order / ambiguity rules match the Swift implementation.

use crate::constants;
use crate::error::PairingError;
use plist::{Dictionary, Value};

/// Swift `PairingProtocol` (String raw values, verbatim).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PairingProtocol {
    RpPairing,
    Lockdown,
    Unknown,
}

impl PairingProtocol {
    pub fn raw_value(&self) -> &'static str {
        match self {
            PairingProtocol::RpPairing => "rppairing",
            PairingProtocol::Lockdown => "lockdown",
            PairingProtocol::Unknown => "unknown",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        match raw {
            "rppairing" => Some(PairingProtocol::RpPairing),
            "lockdown" => Some(PairingProtocol::Lockdown),
            "unknown" => Some(PairingProtocol::Unknown),
            _ => None,
        }
    }

    /// Swift `defaultPort`. The `unknown` case depends on
    /// `#available(iOS 17, *)`; the caller supplies that availability
    /// (on iOS it is an OS-version check, done by the owning layer).
    pub fn default_port(&self, ios17_available: bool) -> u16 {
        match self {
            PairingProtocol::RpPairing => constants::REMOTE_PAIRING_PORT,
            PairingProtocol::Lockdown => constants::LOCKDOWND_PORT,
            PairingProtocol::Unknown => {
                if ios17_available {
                    constants::REMOTE_PAIRING_PORT
                } else {
                    constants::LOCKDOWND_PORT
                }
            }
        }
    }
}

impl std::fmt::Display for PairingProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.raw_value())
    }
}

/// Required keys for a RemotePairing pairing file (Swift order).
pub const RP_REQUIRED_KEYS: [&str; 3] = ["private_key", "public_key", "identifier"];

/// Required keys for a lockdown pairing file (Swift order).
pub const LOCKDOWN_REQUIRED_KEYS: [&str; 10] = [
    "WiFiMACAddress",
    "SystemBUID",
    "RootPrivateKey",
    "HostPrivateKey",
    "HostID",
    "RootCertificate",
    "UDID",
    "EscrowBag",
    "HostCertificate",
    "DeviceCertificate",
];

fn missing_keys(plist: &Dictionary, required: &[&str]) -> Vec<String> {
    required
        .iter()
        .filter(|k| !plist.contains_key(**k))
        .map(|k| k.to_string())
        .collect()
}

/// Swift `RPPairingFile.missingKeys(in:)`.
pub fn rp_missing_keys(plist: &Dictionary) -> Vec<String> {
    missing_keys(plist, &RP_REQUIRED_KEYS)
}

/// Swift `LockdownPairingFile.missingKeys(in:)`.
pub fn lockdown_missing_keys(plist: &Dictionary) -> Vec<String> {
    missing_keys(plist, &LOCKDOWN_REQUIRED_KEYS)
}

fn as_string(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn as_data(v: Option<&Value>) -> Option<Vec<u8>> {
    match v {
        Some(Value::Data(d)) => Some(d.clone()),
        _ => None,
    }
}

/// Swift `RPPairingFile`.
#[derive(Debug, Clone)]
pub struct RpPairingFile {
    pub raw_content: String,
    pub raw_data: Vec<u8>,
    pub plist: Dictionary,
    pub identifier: String,
    pub public_key: Option<Vec<u8>>,
    pub private_key: Option<Vec<u8>>,
}

impl RpPairingFile {
    pub fn new(content: String, plist: Dictionary, data: Vec<u8>) -> Result<Self, PairingError> {
        let missing = rp_missing_keys(&plist);
        if !missing.is_empty() {
            return Err(PairingError::Incomplete {
                protocol: PairingProtocol::RpPairing,
                missing_keys: missing,
            });
        }
        Ok(Self {
            identifier: as_string(plist.get("identifier")).unwrap_or_default(),
            public_key: as_data(plist.get("public_key")),
            private_key: as_data(plist.get("private_key")),
            raw_content: content,
            raw_data: data,
            plist,
        })
    }

    pub fn mode(&self) -> PairingProtocol {
        PairingProtocol::RpPairing
    }
}

/// Swift `LockdownPairingFile`.
#[derive(Debug, Clone)]
pub struct LockdownPairingFile {
    pub raw_content: String,
    pub raw_data: Vec<u8>,
    pub plist: Dictionary,
    pub udid: String,
    pub system_buid: String,
    pub host_id: Option<String>,
    pub wifi_mac_address: Option<String>,
    pub host_certificate: Option<Vec<u8>>,
    pub root_certificate: Option<Vec<u8>>,
    pub device_certificate: Option<Vec<u8>>,
}

impl LockdownPairingFile {
    pub fn new(content: String, plist: Dictionary, data: Vec<u8>) -> Result<Self, PairingError> {
        let missing = lockdown_missing_keys(&plist);
        if !missing.is_empty() {
            return Err(PairingError::Incomplete {
                protocol: PairingProtocol::Lockdown,
                missing_keys: missing,
            });
        }
        Ok(Self {
            udid: as_string(plist.get("UDID")).unwrap_or_default(),
            system_buid: as_string(plist.get("SystemBUID")).unwrap_or_default(),
            host_id: as_string(plist.get("HostID")),
            wifi_mac_address: as_string(plist.get("WiFiMACAddress")),
            host_certificate: as_data(plist.get("HostCertificate")),
            root_certificate: as_data(plist.get("RootCertificate")),
            device_certificate: as_data(plist.get("DeviceCertificate")),
            raw_content: content,
            raw_data: data,
            plist,
        })
    }

    pub fn mode(&self) -> PairingProtocol {
        PairingProtocol::Lockdown
    }
}

/// Swift `PairingFile` protocol, as an enum over the two concrete shapes.
#[derive(Debug, Clone)]
pub enum PairingFile {
    Rp(RpPairingFile),
    Lockdown(LockdownPairingFile),
}

impl PairingFile {
    pub fn mode(&self) -> PairingProtocol {
        match self {
            PairingFile::Rp(f) => f.mode(),
            PairingFile::Lockdown(f) => f.mode(),
        }
    }

    pub fn raw_content(&self) -> &str {
        match self {
            PairingFile::Rp(f) => &f.raw_content,
            PairingFile::Lockdown(f) => &f.raw_content,
        }
    }

    pub fn raw_data(&self) -> &[u8] {
        match self {
            PairingFile::Rp(f) => &f.raw_data,
            PairingFile::Lockdown(f) => &f.raw_data,
        }
    }

    pub fn plist(&self) -> &Dictionary {
        match self {
            PairingFile::Rp(f) => &f.plist,
            PairingFile::Lockdown(f) => &f.plist,
        }
    }
}

/// Formats a string list the way Swift interpolates `[String]` into a
/// string: `["a", "b"]`. Used by the invalidPlist reason below, which is
/// part of the Swift error surface and must match byte-for-byte.
fn swift_array_description(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| format!("\"{s}\"")).collect();
    format!("[{}]", inner.join(", "))
}

/// Swift `PairingFileParser.validatePairingFile(from:preferred:)`.
pub fn validate_pairing_file(
    plist: Option<&Dictionary>,
    preferred: Option<PairingProtocol>,
) -> Result<PairingProtocol, PairingError> {
    let plist = match plist {
        Some(p) => p,
        None => {
            return Err(PairingError::InvalidPlist(
                "The file could not be parsed as a property list (plist).".to_string(),
            ))
        }
    };

    if let Some(preferred) = preferred {
        match preferred {
            PairingProtocol::RpPairing => {
                let missing = rp_missing_keys(plist);
                if !missing.is_empty() {
                    return Err(PairingError::Incomplete {
                        protocol: PairingProtocol::RpPairing,
                        missing_keys: missing,
                    });
                }
                return Ok(PairingProtocol::RpPairing);
            }
            PairingProtocol::Lockdown => {
                let missing = lockdown_missing_keys(plist);
                if !missing.is_empty() {
                    return Err(PairingError::Incomplete {
                        protocol: PairingProtocol::Lockdown,
                        missing_keys: missing,
                    });
                }
                return Ok(PairingProtocol::Lockdown);
            }
            PairingProtocol::Unknown => {
                return Err(PairingError::InvalidPlist(
                    "Unknown pairing protocol specified".to_string(),
                ));
            }
        }
    }

    let missing_rp = rp_missing_keys(plist);
    let missing_lockdown = lockdown_missing_keys(plist);
    let has_rp = missing_rp.is_empty();
    let has_lockdown = missing_lockdown.is_empty();

    if has_rp && has_lockdown {
        return Err(PairingError::Ambiguous(
            "The pairing file contains credentials for both Remote Pairing and Lockdown protocols. Specify a preferred protocol to resolve ambiguity.".to_string(),
        ));
    }
    if has_rp {
        return Ok(PairingProtocol::RpPairing);
    }
    if has_lockdown {
        return Ok(PairingProtocol::Lockdown);
    }

    Err(PairingError::InvalidPlist(format!(
        "Unrecognized pairing file format.   \u{2022} Missing .{} attributes: {};   \u{2022} Missing .{}  attributes: {}.",
        PairingProtocol::RpPairing.raw_value(),
        swift_array_description(&missing_rp),
        PairingProtocol::Lockdown.raw_value(),
        swift_array_description(&missing_lockdown),
    )))
}

/// Swift `PairingFileParser.parse(content:preferred:)`, from raw bytes.
/// The Swift entry point takes a `String`; a non-UTF-8 byte source maps to
/// the same `unreadable("UTF-8 encoding failed")` outcome Swift produces
/// when the content cannot be represented.
pub fn parse_pairing_bytes(
    bytes: &[u8],
    preferred: Option<PairingProtocol>,
) -> Result<PairingFile, PairingError> {
    let content = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            return Err(PairingError::Unreadable(
                "UTF-8 encoding failed".to_string(),
            ))
        }
    };
    parse_pairing_content(&content, preferred)
}

/// Swift `PairingFileParser.parse(content:preferred:)`.
pub fn parse_pairing_content(
    content: &str,
    preferred: Option<PairingProtocol>,
) -> Result<PairingFile, PairingError> {
    let data = content.as_bytes().to_vec();
    let dict = match parse_plist_dict(&data) {
        Some(d) => d,
        None => {
            return Err(PairingError::InvalidPlist(
                "PropertyListSerialization failed".to_string(),
            ))
        }
    };
    let mode = validate_pairing_file(Some(&dict), preferred)?;
    match mode {
        PairingProtocol::RpPairing => Ok(PairingFile::Rp(RpPairingFile::new(
            content.to_string(),
            dict,
            data,
        )?)),
        PairingProtocol::Lockdown => Ok(PairingFile::Lockdown(LockdownPairingFile::new(
            content.to_string(),
            dict,
            data,
        )?)),
        PairingProtocol::Unknown => Err(PairingError::InvalidPlist(
            "Unknown pairing file format".to_string(),
        )),
    }
}

/// Parses plist bytes into a root dictionary; `None` mirrors Swift's
/// `PropertyListSerialization ... as? [String: Any]` failing (malformed
/// plist, or a valid plist whose root is not a dictionary).
fn parse_plist_dict(data: &[u8]) -> Option<Dictionary> {
    let value = Value::from_reader(std::io::Cursor::new(data)).ok()?;
    match value {
        Value::Dictionary(d) => Some(d),
        _ => None,
    }
}
