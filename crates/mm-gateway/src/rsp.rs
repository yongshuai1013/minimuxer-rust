//! RSP (GDB Remote Serial Protocol) tools — port of the helpers
//! on Swift `BaseDeviceGateway` (yongshuai1013/minimuxer
//! @714aafa3): `RSPStopReply`, `RSPProcessInfo`, `stringToHex`,
//! `decodeHexASCII`, and the `findProcessPID` search, plus the
//! debug-proxy wire client used by gw-idevice's pre-17 launch
//! path. Behaviour is catalogued in `docs/m5-install-ddi-debug-diff.md`
//! §2 and D-M5-5.

use std::collections::HashMap;
use std::future::Future;

use idevice::IdeviceError;
use mm_common::MmErrorInfo;

// ---------------------------------------------------------------------------
// RSPStopReply (Swift `RSPStopReply`)
// ---------------------------------------------------------------------------

/// A parsed debugserver stop reply (the `T` packet sent after a
/// successful attach).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RspStopReply {
    pub raw_packet: String,
    pub signal: Option<u8>,
    pub thread_id: Option<String>,
    pub registers: HashMap<String, String>,
    pub metadata: HashMap<String, String>,
}

impl RspStopReply {
    /// Swift's failable init: the packet must start with `T`.
    pub fn parse(raw_packet: &str) -> Option<Self> {
        if !raw_packet.starts_with('T') {
            return None;
        }
        // The two hex digits after 'T' (fewer when the packet is
        // truncated — Swift takes `prefix(2)` of what remains).
        let hex_sig: String = raw_packet.chars().skip(1).take(2).collect();
        let signal = u8::from_str_radix(&hex_sig, 16).ok();

        let body_start = 1 + hex_sig.len();
        let body: String = raw_packet.chars().skip(body_start).collect();

        let mut thread_id = None;
        let mut registers = HashMap::new();
        let mut metadata = HashMap::new();
        for pair in body.split(';') {
            let mut parts = pair.splitn(2, ':');
            let (Some(key), Some(val)) = (parts.next(), parts.next()) else {
                continue;
            };
            if key == "thread" {
                thread_id = Some(val.to_string());
            } else if key.chars().all(|c| c.is_ascii_hexdigit())
                || ["pc", "sp", "fp", "lr", "cpsr"].contains(&key)
            {
                registers.insert(key.to_string(), val.to_string());
            } else {
                metadata.insert(key.to_string(), val.to_string());
            }
        }

        Some(Self {
            raw_packet: raw_packet.to_string(),
            signal,
            thread_id,
            registers,
            metadata,
        })
    }
}

// ---------------------------------------------------------------------------
// RSPProcessInfo (Swift `RSPProcessInfo`)
// ---------------------------------------------------------------------------

/// A parsed `qProcessInfo`-style response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RspProcessInfo {
    pub pid: u32,
    pub name: String,
    pub executable: String,
}

impl RspProcessInfo {
    /// Swift's failable init: empty responses and error replies
    /// (starting with `E`) do not parse, and a pid is required.
    pub fn parse(raw_response: &str) -> Option<Self> {
        if raw_response.is_empty() || raw_response.starts_with('E') {
            return None;
        }
        let mut pid: Option<u32> = None;
        let mut name = String::new();
        let mut executable = String::new();
        for pair in raw_response.split(';') {
            let mut parts = pair.splitn(2, ':');
            let (Some(key), Some(val)) = (parts.next(), parts.next()) else {
                continue;
            };
            let key = key.trim();
            let val = val.trim();
            match key {
                "pid" => {
                    pid = u32::from_str_radix(val, 16)
                        .ok()
                        .or_else(|| val.parse::<u32>().ok());
                }
                "name" => {
                    name = decode_hex_ascii(val).unwrap_or_else(|| val.to_string());
                }
                "executable" => {
                    executable =
                        decode_hex_ascii(val).unwrap_or_else(|| val.to_string());
                }
                _ => {}
            }
        }
        Some(Self {
            pid: pid?,
            name,
            executable,
        })
    }

    /// Swift `matches(term:)`: case-insensitive substring on the
    /// name, or on the executable when one was reported.
    pub fn matches(&self, term: &str) -> bool {
        let term = term.to_lowercase();
        self.name.to_lowercase().contains(&term)
            || (!self.executable.is_empty()
                && self.executable.to_lowercase().contains(&term))
    }
}

// ---------------------------------------------------------------------------
// Hex helpers (Swift `stringToHex` / `decodeHexASCII`)
// ---------------------------------------------------------------------------

/// Swift `stringToHex`: UTF-8 bytes as lowercase `%02x`.
pub fn string_to_hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

/// Swift `decodeHexASCII`: pairs of hex digits → bytes → string.
/// `None` on odd length or any non-hex character (Swift then
/// falls back to an ASCII decode of the same bytes; for the
/// byte strings debugserver produces the two agree, and callers
/// here fall back to the raw value like Swift's `?? val`).
pub fn decode_hex_ascii(hex: &str) -> Option<String> {
    let bytes = hex.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let hi = hex_val(pair[0])?;
        let lo = hex_val(pair[1])?;
        out.push((hi << 4) | lo);
    }
    String::from_utf8(out).ok()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// findProcessPID (Swift `findProcessPID`)
// ---------------------------------------------------------------------------

/// The search-term list of Swift `findProcessPID`, as a pure
/// function (the decision table is unit-tested): executable name
/// first, then the bundle path's stem, then the app id's
/// components (with the skip rules), then the dotted prefix,
/// then the full app id.
pub fn process_search_terms(
    app_id: &str,
    bundle_path: Option<&str>,
    executable_name: Option<&str>,
) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();

    if let Some(executable_name) = executable_name {
        if !executable_name.is_empty() {
            terms.push(executable_name.to_string());
        }
    }

    if let Some(bundle_path) = bundle_path {
        if !bundle_path.is_empty() {
            // `URL.deletingPathExtension().lastPathComponent`.
            let last = bundle_path.rsplit('/').next().unwrap_or(bundle_path);
            let stem = match last.rfind('.') {
                Some(idx) if idx > 0 => &last[..idx],
                _ => last,
            };
            if !stem.is_empty() && stem != "App" && !terms.iter().any(|t| t == stem) {
                terms.push(stem.to_string());
            }
        }
    }

    for comp in app_id.split(|c| c == '.' || c == '-' || c == '_') {
        if comp.is_empty() {
            continue;
        }
        let str_comp = comp;
        if ["com", "org", "net", "io", "app", "ios"]
            .contains(&str_comp.to_lowercase().as_str())
        {
            continue;
        }
        // Team-id-shaped component: 10 chars, all alphanumeric,
        // already fully uppercase.
        if str_comp.chars().count() == 10
            && str_comp.chars().all(|c| c.is_alphanumeric())
            && str_comp.to_uppercase() == str_comp
        {
            continue;
        }
        let stripped: String = str_comp
            .trim_matches(|c: char| c.is_ascii_digit())
            .to_string();
        if !stripped.is_empty()
            && stripped != str_comp
            && !terms.iter().any(|t| t == &stripped)
        {
            terms.push(stripped);
        }
        if !terms.iter().any(|t| t == str_comp) {
            terms.push(str_comp.to_string());
        }
    }

    let dot_components: Vec<&str> = app_id.split('.').filter(|s| !s.is_empty()).collect();
    if dot_components.len() > 1 {
        let without_suffix = dot_components[..dot_components.len() - 1].join(".");
        if !without_suffix.is_empty() && !terms.iter().any(|t| t == &without_suffix) {
            terms.push(without_suffix);
        }
    }

    if !terms.iter().any(|t| t == app_id) {
        terms.push(app_id.to_string());
    }

    terms
}

/// Swift `findProcessPID`: tries `vAttachName;<hex>` for every
/// search term; a non-empty, non-`E` response means the attach
/// succeeded in-place — the helper sends `D` and returns
/// `Some(0)`. Command errors are swallowed per attempt (Swift
/// `try?`). `None` when no term attaches.
pub async fn find_process_pid<S, F>(
    app_id: &str,
    bundle_path: Option<&str>,
    executable_name: Option<&str>,
    mut send_command: S,
) -> Option<u32>
where
    S: FnMut(String, Vec<String>) -> F,
    F: Future<Output = Result<Option<String>, MmErrorInfo>>,
{
    let terms = process_search_terms(app_id, bundle_path, executable_name);
    for term in terms {
        let attach_cmd = format!("vAttachName;{}", string_to_hex(&term));
        let response = send_command(attach_cmd, Vec::new()).await;
        let response = match response {
            Ok(Some(resp)) => resp,
            _ => continue,
        };
        if response.is_empty() || response.starts_with('E') {
            continue;
        }
        let _ = send_command("D".to_string(), Vec::new()).await;
        return Some(0);
    }
    None
}

// ---------------------------------------------------------------------------
// RspClient — the debug-proxy wire client (gw-idevice)
// ---------------------------------------------------------------------------

/// Hex-encodes bytes as uppercase — the idevice crate's
/// `DebugserverCommand` argument encoding (and the FFI's).
fn hex_encode_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn rsp_checksum(data: &str) -> String {
    let sum = data.bytes().fold(0u8, |acc, b| acc.wrapping_add(b));
    format!("{sum:02x}")
}

/// An RSP client over a raw device stream (the usbmuxd
/// device stream of Swift's `connectDebugProxy`, produced by
/// `crate::rawconn::mux_device_stream`). Written in-crate
/// because idevice 0.1.29's `Idevice` exposes neither its
/// socket (for the crate's `DebugProxyClient`) nor raw I/O,
/// and the crate's `read_response` does not consume the
/// response checksum (D-M5-5). Command formatting matches
/// the crate exactly (name + uppercase-hex argv, `$…#cc`
/// framing, ack mode); the response reader consumes the two
/// checksum characters per the protocol.
pub struct RspClient {
    stream: Box<dyn idevice::ReadWrite>,
}

impl RspClient {
    pub fn new(stream: Box<dyn idevice::ReadWrite>) -> Self {
        Self { stream }
    }

    /// Sends one command and reads its response; `Ok(None)`
    /// when the peer does not ack/frame a response (the
    /// crate's "No + ack" / "No $ response" outcomes).
    pub async fn send_command(
        &mut self,
        name: &str,
        args: &[String],
    ) -> Result<Option<String>, IdeviceError> {
        use tokio::io::AsyncWriteExt as _;
        let mut packet_data = name.to_string();
        for arg in args {
            packet_data.push_str(&hex_encode_upper(arg.as_bytes()));
        }
        let frame = format!("${}#{}", packet_data, rsp_checksum(&packet_data));
        self.stream.write_all(frame.as_bytes()).await?;
        self.stream.flush().await?;
        self.read_response().await
    }

    async fn read_response(&mut self) -> Result<Option<String>, IdeviceError> {
        use tokio::io::AsyncReadExt as _;
        let mut one = [0u8; 1];
        self.stream.read_exact(&mut one).await?;
        if one[0] != b'+' {
            return Ok(None);
        }
        self.stream.read_exact(&mut one).await?;
        if one[0] != b'$' {
            return Ok(None);
        }
        let mut buffer = Vec::new();
        loop {
            self.stream.read_exact(&mut one).await?;
            if one[0] == b'#' {
                break;
            }
            buffer.push(one[0]);
        }
        // Consume the two checksum characters (D-M5-5).
        let mut checksum = [0u8; 2];
        self.stream.read_exact(&mut checksum).await?;
        use tokio::io::AsyncWriteExt as _;
        self.stream.write_all(b"+").await?;
        self.stream.flush().await?;
        Ok(Some(String::from_utf8(buffer)?))
    }

    /// Swift's `debug_proxy_set_argv`: the `A` packet built
    /// exactly like the idevice crate's `set_argv` (decimal
    /// hex-length prefixes, uppercase hex data, leading byte
    /// overwritten with 'A'), sent framed like every other
    /// command. Returns the response text ("" when none).
    pub async fn set_argv(&mut self, argv: &[String]) -> Result<String, IdeviceError> {
        use tokio::io::AsyncWriteExt as _;
        if argv.is_empty() {
            return Err(IdeviceError::InvalidArgument);
        }
        let mut pkt = Vec::new();
        for (i, arg) in argv.iter().enumerate() {
            let prefix = format!(",{},{},", arg.len() * 2, i);
            pkt.extend_from_slice(prefix.as_bytes());
            pkt.extend_from_slice(hex_encode_upper(arg.as_bytes()).as_bytes());
        }
        pkt[0] = b'A';
        let packet_data = String::from_utf8(pkt)?;
        let frame = format!("${}#{}", packet_data, rsp_checksum(&packet_data));
        self.stream.write_all(frame.as_bytes()).await?;
        self.stream.flush().await?;
        Ok(self.read_response().await?.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_reply_parses_signal_thread_registers_metadata() {
        let reply = RspStopReply::parse(
            "T05thread:1a2b;pc:0000000100003abc;sp:000000016fdfeb30;name:4d6f636b;",
        )
        .unwrap();
        assert_eq!(reply.signal, Some(5));
        assert_eq!(reply.thread_id.as_deref(), Some("1a2b"));
        assert_eq!(
            reply.registers.get("pc").map(String::as_str),
            Some("0000000100003abc")
        );
        assert_eq!(
            reply.registers.get("sp").map(String::as_str),
            Some("000000016fdfeb30")
        );
        // Hex-shaped keys land in registers too (Swift's rule).
        let reply = RspStopReply::parse("T11aa:bb;zz:top;").unwrap();
        assert_eq!(reply.signal, Some(0x11));
        assert_eq!(reply.registers.get("aa").map(String::as_str), Some("bb"));
        assert_eq!(reply.metadata.get("zz").map(String::as_str), Some("top"));
    }

    #[test]
    fn stop_reply_rejects_non_t_and_tolerates_truncation() {
        assert!(RspStopReply::parse("OK").is_none());
        assert!(RspStopReply::parse("").is_none());
        let reply = RspStopReply::parse("T").unwrap();
        assert_eq!(reply.signal, None);
        let reply = RspStopReply::parse("T5").unwrap();
        assert_eq!(reply.signal, Some(5));
        let reply = RspStopReply::parse("Tzz").unwrap();
        assert_eq!(reply.signal, None);
    }

    #[test]
    fn process_info_parses_hex_pid_and_hex_name() {
        let info =
            RspProcessInfo::parse("pid:1f3;name:4d6f636b417070;executable:4d6f636b;")
                .unwrap();
        assert_eq!(info.pid, 0x1f3);
        assert_eq!(info.name, "MockApp");
        assert_eq!(info.executable, "Mock");
        assert!(info.matches("mockapp"));
        assert!(info.matches("MOCK"));
        assert!(!info.matches("other"));
    }

    #[test]
    fn process_info_decimal_pid_and_failures() {
        // Hex parse wins first ("10" is 0x10, not ten) — Swift's
        // `UInt32(val, radix: 16) ?? UInt32(val)` order.
        let info = RspProcessInfo::parse("pid:10;name:Plain;").unwrap();
        assert_eq!(info.pid, 16);
        assert_eq!(info.name, "Plain");
        assert!(RspProcessInfo::parse("").is_none());
        assert!(RspProcessInfo::parse("E01").is_none());
        assert!(RspProcessInfo::parse("name:4e6f7065;").is_none());
    }

    #[test]
    fn hex_helpers() {
        assert_eq!(string_to_hex("App"), "417070");
        assert_eq!(string_to_hex(""), "");
        assert_eq!(decode_hex_ascii("4d6f636b").as_deref(), Some("Mock"));
        assert_eq!(decode_hex_ascii("4D6F636B").as_deref(), Some("Mock"));
        assert_eq!(decode_hex_ascii("abc"), None);
        assert_eq!(decode_hex_ascii("zz"), None);
        assert_eq!(decode_hex_ascii(""), Some(String::new()));
    }

    #[test]
    fn search_terms_decision_table() {
        // Executable name wins; bundle stem next; components in
        // order with skip rules; dotted prefix; full id last.
        let terms = process_search_terms(
            "com.example.MyApp",
            Some("/private/var/containers/Bundle/Application/UUID/MyApp.app"),
            Some("MyApp"),
        );
        assert_eq!(
            terms,
            vec!["MyApp", "example", "com.example", "com.example.MyApp"]
        );

        // No executable / bundle path: components only.
        let terms = process_search_terms("org.foo.Bar2", None, None);
        assert_eq!(terms, vec!["foo", "Bar", "Bar2", "org.foo", "org.foo.Bar2"]);

        // Team-id-shaped component (10 chars, alphanumeric,
        // uppercase) is skipped entirely.
        let terms = process_search_terms("com.ABCDE12345.Widget", None, None);
        assert_eq!(
            terms,
            vec!["Widget", "com.ABCDE12345", "com.ABCDE12345.Widget"]
        );

        // "App" bundle stem is not added; digits are stripped
        // before the raw component.
        let terms = process_search_terms(
            "io.demo.3DGame",
            Some("/x/App.app/App"),
            None,
        );
        assert_eq!(
            terms,
            vec!["demo", "DGame", "3DGame", "io.demo", "io.demo.3DGame"]
        );
    }
}
