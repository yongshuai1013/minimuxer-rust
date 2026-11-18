//! Unified error bus — port of the Swift error surface:
//! `MinimuxerError` (46 cases), `DeviceGatewayError` (8 codes),
//! `PairingError` (4), `EMProxyError` (10), `MinimuxerInternalError` (3)
//! and the `MinimuxerServiceError` wrapper (2 components) — 73 entries in
//! `error-map.toml`, the single source of truth mirrored by the generated
//! [`ERROR_CASES`] table.
//!
//! Across the FFI boundary only [`MmErrorInfo`] travels:
//! `{ domain, code, reason }` where `code` is the Swift case name verbatim.
//! Mapping *decisions* (e.g. `DeviceGatewayError::as_minimuxer_error`,
//! `is_vpn_drop`) live here in Rust, never in the Swift shell.
//!
//! Payload encoding inside `reason` (documented in error-map.toml):
//! protocol-carrying cases use `"<protocol>\u{1}<reason>"`; the pairing
//! `incomplete` case uses `"<protocol>\u{1}<key1,key2,...>"`;
//! `EMProxyError::UnknownError` stores the bare i32.

use crate::pairing::PairingProtocol;

/// One row of the generated error table.
pub struct ErrorCaseDef {
    pub domain: &'static str,
    pub code: &'static str,
    pub description: &'static str,
    pub description_empty_reason: Option<&'static str>,
}

include!(concat!(env!("OUT_DIR"), "/error_cases_generated.rs"));

/// Looks up a (domain, code) pair in the generated table.
pub fn lookup_error_case(domain: &str, code: &str) -> Option<&'static ErrorCaseDef> {
    ERROR_CASES
        .iter()
        .find(|c| c.domain == domain && c.code == code)
}

/// Renders a Swift-style `description` from the table. `payload` carries
/// the decoded extras for the placeholder kinds: protocol raw value,
/// missing keys (already joined), component raw value, numeric code.
pub fn render_description(
    domain: &str,
    code: &str,
    reason: &str,
    protocol: Option<&str>,
    missing_keys: Option<&str>,
    numeric_code: Option<i32>,
) -> Option<String> {
    let def = lookup_error_case(domain, code)?;
    if reason.is_empty() {
        if let Some(empty) = def.description_empty_reason {
            return Some(empty.to_string());
        }
    }
    let mut out = def.description.to_string();
    out = out.replace("{protocol}", protocol.unwrap_or(""));
    out = out.replace("{missing_keys}", missing_keys.unwrap_or(""));
    out = out.replace("{component}", code);
    out = out.replace(
        "{code}",
        &numeric_code.map(|n| n.to_string()).unwrap_or_else(|| reason.to_string()),
    );
    out = out.replace("{reason}", reason);
    Some(out)
}

const PAYLOAD_SEP: char = '\u{1}';

fn encode_payload(payload: &str, reason: &str) -> String {
    format!("{payload}{PAYLOAD_SEP}{reason}")
}

fn decode_payload(reason: &str) -> Option<(&str, &str)> {
    reason.split_once(PAYLOAD_SEP)
}

/// The single error shape that crosses the FFI boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmErrorInfo {
    pub domain: String,
    pub code: String,
    pub reason: String,
}

impl MmErrorInfo {
    pub fn new(domain: &str, code: &str, reason: &str) -> Self {
        Self {
            domain: domain.to_string(),
            code: code.to_string(),
            reason: reason.to_string(),
        }
    }

    /// Swift `description` for this error, rendered from the table with
    /// payloads decoded per the documented encoding. Falls back to the
    /// raw reason when the (domain, code) pair is unknown.
    pub fn description(&self) -> String {
        let mut protocol: Option<String> = None;
        let mut missing: Option<String> = None;
        let mut numeric: Option<i32> = None;
        let mut reason: &str = &self.reason;
        match (self.domain.as_str(), self.code.as_str()) {
            ("MinimuxerError", "invalidPairing") | ("MinimuxerError", "mount") => {
                if let Some((proto, rest)) = decode_payload(&self.reason) {
                    protocol = Some(proto.to_string());
                    reason = rest;
                }
            }
            ("PairingError", "incomplete") => {
                if let Some((proto, keys)) = decode_payload(&self.reason) {
                    protocol = Some(proto.to_string());
                    // Stored comma-joined without spaces; Swift renders
                    // the description with ", " between keys.
                    missing = Some(
                        keys.split(',')
                            .filter(|k| !k.is_empty())
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    reason = "";
                }
            }
            ("EMProxyError", "unknownError") => {
                numeric = self.reason.trim().parse::<i32>().ok();
                reason = "";
            }
            _ => {}
        }
        render_description(
            &self.domain,
            &self.code,
            reason,
            protocol.as_deref(),
            missing.as_deref(),
            numeric,
        )
        .unwrap_or_else(|| self.reason.clone())
    }
}

impl std::fmt::Display for MmErrorInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl std::error::Error for MmErrorInfo {}

/// Swift `PairingError` (MinimuxerCommon).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingError {
    Unreadable(String),
    InvalidPlist(String),
    Incomplete {
        protocol: PairingProtocol,
        missing_keys: Vec<String>,
    },
    Ambiguous(String),
}

impl PairingError {
    pub fn code(&self) -> &'static str {
        match self {
            PairingError::Unreadable(_) => "unreadable",
            PairingError::InvalidPlist(_) => "invalidPlist",
            PairingError::Incomplete { .. } => "incomplete",
            PairingError::Ambiguous(_) => "ambiguous",
        }
    }

    pub fn description(&self) -> String {
        self.to_info().description()
    }

    pub fn to_info(&self) -> MmErrorInfo {
        let reason = match self {
            PairingError::Unreadable(r)
            | PairingError::InvalidPlist(r)
            | PairingError::Ambiguous(r) => r.clone(),
            PairingError::Incomplete {
                protocol,
                missing_keys,
            } => encode_payload(protocol.raw_value(), &missing_keys.join(",")),
        };
        MmErrorInfo::new("PairingError", self.code(), &reason)
    }

    pub fn from_info(info: &MmErrorInfo) -> Option<Self> {
        if info.domain != "PairingError" {
            return None;
        }
        match info.code.as_str() {
            "unreadable" => Some(PairingError::Unreadable(info.reason.clone())),
            "invalidPlist" => Some(PairingError::InvalidPlist(info.reason.clone())),
            "ambiguous" => Some(PairingError::Ambiguous(info.reason.clone())),
            "incomplete" => {
                let (proto, keys) = decode_payload(&info.reason)?;
                Some(PairingError::Incomplete {
                    protocol: PairingProtocol::from_raw(proto)
                        .unwrap_or(PairingProtocol::Unknown),
                    missing_keys: keys
                        .split(',')
                        .filter(|k| !k.is_empty())
                        .map(|k| k.to_string())
                        .collect(),
                })
            }
            _ => None,
        }
    }
}

impl std::fmt::Display for PairingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl std::error::Error for PairingError {}

/// Swift `MinimuxerError` (46 cases, declaration order preserved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MinimuxerError {
    NoDevice(String),
    NoConnection(String),
    NotReachable(String),
    NoVPN(String),
    NotStarted(String),
    PairingNotLoaded(String),
    ConnectionModeNotConfigured(String),
    RestartAlreadyInProgressError(String),
    InvalidVPN(String),
    InvalidPairing {
        protocol: PairingProtocol,
        reason: String,
    },
    MuxerNotListening(String),
    CreateDebug(String),
    CreateInstproxy(String),
    CreateLockdown(String),
    CreateCoreDevice(String),
    CreateSoftwareTunnel(String),
    CreateRemoteServer(String),
    CreateProcessControl(String),
    GetLockdownValue(String),
    FetchUDID(String),
    Connect(String),
    Close(String),
    XpcHandshake(String),
    NoService(String),
    InvalidProductVersion(String),
    LookupApps(String),
    FindApp(String),
    BundlePath(String),
    MaxPacket(String),
    WorkingDirectory(String),
    Argv(String),
    LaunchSuccess(String),
    Detach(String),
    Attach(String),
    CreateAfc(String),
    RwAfc(String),
    InstallApp(String),
    UninstallApp(String),
    CreateMisagent(String),
    ProfileInstall(String),
    ProfileRemove(String),
    CreateFolder(String),
    DownloadImage(String),
    ImageLookup(String),
    ImageRead(String),
    Mount {
        protocol: PairingProtocol,
        reason: String,
    },
}

impl MinimuxerError {
    pub fn code(&self) -> &'static str {
        use MinimuxerError::*;
        match self {
            NoDevice(_) => "noDevice",
            NoConnection(_) => "noConnection",
            NotReachable(_) => "notReachable",
            NoVPN(_) => "noVPN",
            NotStarted(_) => "notStarted",
            PairingNotLoaded(_) => "pairingNotLoaded",
            ConnectionModeNotConfigured(_) => "connectionModeNotConfigured",
            RestartAlreadyInProgressError(_) => "restartAlreadyInProgressError",
            InvalidVPN(_) => "invalidVPN",
            InvalidPairing { .. } => "invalidPairing",
            MuxerNotListening(_) => "muxerNotListening",
            CreateDebug(_) => "createDebug",
            CreateInstproxy(_) => "createInstproxy",
            CreateLockdown(_) => "createLockdown",
            CreateCoreDevice(_) => "createCoreDevice",
            CreateSoftwareTunnel(_) => "createSoftwareTunnel",
            CreateRemoteServer(_) => "createRemoteServer",
            CreateProcessControl(_) => "createProcessControl",
            GetLockdownValue(_) => "getLockdownValue",
            FetchUDID(_) => "fetchUDID",
            Connect(_) => "connect",
            Close(_) => "close",
            XpcHandshake(_) => "xpcHandshake",
            NoService(_) => "noService",
            InvalidProductVersion(_) => "invalidProductVersion",
            LookupApps(_) => "lookupApps",
            FindApp(_) => "findApp",
            BundlePath(_) => "bundlePath",
            MaxPacket(_) => "maxPacket",
            WorkingDirectory(_) => "workingDirectory",
            Argv(_) => "argv",
            LaunchSuccess(_) => "launchSuccess",
            Detach(_) => "detach",
            Attach(_) => "attach",
            CreateAfc(_) => "createAfc",
            RwAfc(_) => "rwAfc",
            InstallApp(_) => "installApp",
            UninstallApp(_) => "uninstallApp",
            CreateMisagent(_) => "createMisagent",
            ProfileInstall(_) => "profileInstall",
            ProfileRemove(_) => "profileRemove",
            CreateFolder(_) => "createFolder",
            DownloadImage(_) => "downloadImage",
            ImageLookup(_) => "imageLookup",
            ImageRead(_) => "imageRead",
            Mount { .. } => "mount",
        }
    }

    /// The plain reason string (payload cases return their reason part).
    pub fn reason(&self) -> &str {
        use MinimuxerError::*;
        match self {
            InvalidPairing { reason, .. } | Mount { reason, .. } => reason,
            NoDevice(r)
            | NoConnection(r)
            | NotReachable(r)
            | NoVPN(r)
            | NotStarted(r)
            | PairingNotLoaded(r)
            | ConnectionModeNotConfigured(r)
            | RestartAlreadyInProgressError(r)
            | InvalidVPN(r)
            | MuxerNotListening(r)
            | CreateDebug(r)
            | CreateInstproxy(r)
            | CreateLockdown(r)
            | CreateCoreDevice(r)
            | CreateSoftwareTunnel(r)
            | CreateRemoteServer(r)
            | CreateProcessControl(r)
            | GetLockdownValue(r)
            | FetchUDID(r)
            | Connect(r)
            | Close(r)
            | XpcHandshake(r)
            | NoService(r)
            | InvalidProductVersion(r)
            | LookupApps(r)
            | FindApp(r)
            | BundlePath(r)
            | MaxPacket(r)
            | WorkingDirectory(r)
            | Argv(r)
            | LaunchSuccess(r)
            | Detach(r)
            | Attach(r)
            | CreateAfc(r)
            | RwAfc(r)
            | InstallApp(r)
            | UninstallApp(r)
            | CreateMisagent(r)
            | ProfileInstall(r)
            | ProfileRemove(r)
            | CreateFolder(r)
            | DownloadImage(r)
            | ImageLookup(r)
            | ImageRead(r) => r,
        }
    }
}

impl MinimuxerError {
    pub fn description(&self) -> String {
        self.to_info().description()
    }

    pub fn to_info(&self) -> MmErrorInfo {
        let reason = match self {
            MinimuxerError::InvalidPairing { protocol, reason }
            | MinimuxerError::Mount { protocol, reason } => {
                encode_payload(protocol.raw_value(), reason)
            }
            other => other.reason().to_string(),
        };
        MmErrorInfo::new("MinimuxerError", self.code(), &reason)
    }

    pub fn from_info(info: &MmErrorInfo) -> Option<Self> {
        if info.domain != "MinimuxerError" {
            return None;
        }
        let r = info.reason.clone();
        use MinimuxerError::*;
        Some(match info.code.as_str() {
            "noDevice" => NoDevice(r),
            "noConnection" => NoConnection(r),
            "notReachable" => NotReachable(r),
            "noVPN" => NoVPN(r),
            "notStarted" => NotStarted(r),
            "pairingNotLoaded" => PairingNotLoaded(r),
            "connectionModeNotConfigured" => ConnectionModeNotConfigured(r),
            "restartAlreadyInProgressError" => RestartAlreadyInProgressError(r),
            "invalidVPN" => InvalidVPN(r),
            "invalidPairing" => {
                let (proto, reason) = decode_payload(&r)?;
                InvalidPairing {
                    protocol: PairingProtocol::from_raw(proto)
                        .unwrap_or(PairingProtocol::Unknown),
                    reason: reason.to_string(),
                }
            }
            "muxerNotListening" => MuxerNotListening(r),
            "createDebug" => CreateDebug(r),
            "createInstproxy" => CreateInstproxy(r),
            "createLockdown" => CreateLockdown(r),
            "createCoreDevice" => CreateCoreDevice(r),
            "createSoftwareTunnel" => CreateSoftwareTunnel(r),
            "createRemoteServer" => CreateRemoteServer(r),
            "createProcessControl" => CreateProcessControl(r),
            "getLockdownValue" => GetLockdownValue(r),
            "fetchUDID" => FetchUDID(r),
            "connect" => Connect(r),
            "close" => Close(r),
            "xpcHandshake" => XpcHandshake(r),
            "noService" => NoService(r),
            "invalidProductVersion" => InvalidProductVersion(r),
            "lookupApps" => LookupApps(r),
            "findApp" => FindApp(r),
            "bundlePath" => BundlePath(r),
            "maxPacket" => MaxPacket(r),
            "workingDirectory" => WorkingDirectory(r),
            "argv" => Argv(r),
            "launchSuccess" => LaunchSuccess(r),
            "detach" => Detach(r),
            "attach" => Attach(r),
            "createAfc" => CreateAfc(r),
            "rwAfc" => RwAfc(r),
            "installApp" => InstallApp(r),
            "uninstallApp" => UninstallApp(r),
            "createMisagent" => CreateMisagent(r),
            "profileInstall" => ProfileInstall(r),
            "profileRemove" => ProfileRemove(r),
            "createFolder" => CreateFolder(r),
            "downloadImage" => DownloadImage(r),
            "imageLookup" => ImageLookup(r),
            "imageRead" => ImageRead(r),
            "mount" => {
                let (proto, reason) = decode_payload(&r)?;
                Mount {
                    protocol: PairingProtocol::from_raw(proto)
                        .unwrap_or(PairingProtocol::Unknown),
                    reason: reason.to_string(),
                }
            }
            _ => return None,
        })
    }
}

impl std::fmt::Display for MinimuxerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl std::error::Error for MinimuxerError {}

/// Swift `DeviceGatewayError.Code` (string raw values, verbatim).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceGatewayCode {
    InvalidPairingFile,
    ConnectionFailed,
    ServiceError,
    NoConnection,
    NotInitialized,
    DeviceEndpointIpNotAvailable,
    InvalidTargetEndpoint,
    UnsupportedOperation,
}

impl DeviceGatewayCode {
    pub fn raw_value(&self) -> &'static str {
        use DeviceGatewayCode::*;
        match self {
            InvalidPairingFile => "invalidPairingFile",
            ConnectionFailed => "connectionFailed",
            ServiceError => "serviceError",
            NoConnection => "noConnection",
            NotInitialized => "notInitialized",
            DeviceEndpointIpNotAvailable => "deviceEndpointIpNotAvailable",
            InvalidTargetEndpoint => "invalidTargetEndpoint",
            UnsupportedOperation => "unsupportedOperation",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        use DeviceGatewayCode::*;
        Some(match raw {
            "invalidPairingFile" => InvalidPairingFile,
            "connectionFailed" => ConnectionFailed,
            "serviceError" => ServiceError,
            "noConnection" => NoConnection,
            "notInitialized" => NotInitialized,
            "deviceEndpointIpNotAvailable" => DeviceEndpointIpNotAvailable,
            "invalidTargetEndpoint" => InvalidTargetEndpoint,
            "unsupportedOperation" => UnsupportedOperation,
            _ => return None,
        })
    }
}

/// Swift `DeviceGatewayError` (code + reason pair).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceGatewayError {
    pub code: DeviceGatewayCode,
    pub reason: String,
}

impl DeviceGatewayError {
    pub fn new(code: DeviceGatewayCode, reason: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
        }
    }

    pub fn description(&self) -> String {
        self.to_info().description()
    }

    pub fn to_info(&self) -> MmErrorInfo {
        MmErrorInfo::new("DeviceGatewayError", self.code.raw_value(), &self.reason)
    }

    pub fn from_info(info: &MmErrorInfo) -> Option<Self> {
        if info.domain != "DeviceGatewayError" {
            return None;
        }
        Some(Self {
            code: DeviceGatewayCode::from_raw(&info.code)?,
            reason: info.reason.clone(),
        })
    }

    /// Swift `DeviceGatewayError.isVPNDrop`: connection/service failures
    /// whose reason names a dropped VPN tunnel.
    pub fn is_vpn_drop(&self) -> bool {
        if self.code != DeviceGatewayCode::ConnectionFailed
            && self.code != DeviceGatewayCode::ServiceError
        {
            return false;
        }
        let lower = self.reason.to_lowercase();
        [
            "broken pipe",
            "brokenpipe",
            "connection reset",
            "connectionreset",
            "early eof",
            "unexpectedeof",
            "no route to host",
            "connection refused",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
    }

    /// Swift `DeviceGatewayError.isRetryable`.
    pub fn is_retryable(&self) -> bool {
        (self.code == DeviceGatewayCode::ConnectionFailed
            || self.code == DeviceGatewayCode::NoConnection)
            && !self.is_vpn_drop()
    }

    /// Swift `DeviceGatewayError.asMinimuxerError(protocol:catchAll:)`.
    pub fn as_minimuxer_error(
        &self,
        active_protocol: PairingProtocol,
        catch_all: impl Fn(String) -> MinimuxerError,
    ) -> MinimuxerError {
        if self.code == DeviceGatewayCode::InvalidPairingFile {
            return MinimuxerError::InvalidPairing {
                protocol: active_protocol,
                reason: self.reason.clone(),
            };
        }
        if self.is_vpn_drop() {
            return MinimuxerError::InvalidVPN(self.reason.clone());
        }
        catch_all(self.reason.clone())
    }
}

impl std::fmt::Display for DeviceGatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl std::error::Error for DeviceGatewayError {}

/// Swift `EMProxyError` (10 cases).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmProxyError {
    InvalidBindAddressPointer,
    InvalidUtf8String,
    InvalidSocketAddress(String),
    SocketBindFailed,
    CryptoInitFailed,
    ServerNotRunning,
    StopSignalFailed,
    ThreadJoinFailed,
    HandshakeClientNotConfigured,
    UnknownError(i32),
}

impl EmProxyError {
    pub fn code(&self) -> &'static str {
        use EmProxyError::*;
        match self {
            InvalidBindAddressPointer => "invalidBindAddressPointer",
            InvalidUtf8String => "invalidUTF8String",
            InvalidSocketAddress(_) => "invalidSocketAddress",
            SocketBindFailed => "socketBindFailed",
            CryptoInitFailed => "cryptoInitFailed",
            ServerNotRunning => "serverNotRunning",
            StopSignalFailed => "stopSignalFailed",
            ThreadJoinFailed => "threadJoinFailed",
            HandshakeClientNotConfigured => "handshakeClientNotConfigured",
            UnknownError(_) => "unknownError",
        }
    }

    pub fn description(&self) -> String {
        self.to_info().description()
    }

    pub fn to_info(&self) -> MmErrorInfo {
        let reason = match self {
            EmProxyError::InvalidSocketAddress(addr) => addr.clone(),
            EmProxyError::UnknownError(code) => code.to_string(),
            _ => String::new(),
        };
        MmErrorInfo::new("EMProxyError", self.code(), &reason)
    }

    pub fn from_info(info: &MmErrorInfo) -> Option<Self> {
        if info.domain != "EMProxyError" {
            return None;
        }
        use EmProxyError::*;
        Some(match info.code.as_str() {
            "invalidBindAddressPointer" => InvalidBindAddressPointer,
            "invalidUTF8String" => InvalidUtf8String,
            "invalidSocketAddress" => InvalidSocketAddress(info.reason.clone()),
            "socketBindFailed" => SocketBindFailed,
            "cryptoInitFailed" => CryptoInitFailed,
            "serverNotRunning" => ServerNotRunning,
            "stopSignalFailed" => StopSignalFailed,
            "threadJoinFailed" => ThreadJoinFailed,
            "handshakeClientNotConfigured" => HandshakeClientNotConfigured,
            "unknownError" => UnknownError(info.reason.trim().parse().ok()?),
            _ => return None,
        })
    }
}

impl std::fmt::Display for EmProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl std::error::Error for EmProxyError {}

/// Swift `MinimuxerInternalError` (3 cases).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalError {
    DeviceEndpointNotInitialized,
    NetworkIfaceNotRefreshed,
    PairingFailed(String),
}

impl InternalError {
    pub fn code(&self) -> &'static str {
        use InternalError::*;
        match self {
            DeviceEndpointNotInitialized => "deviceEndpointNotInitialized",
            NetworkIfaceNotRefreshed => "networkIfaceNotRefreshed",
            PairingFailed(_) => "pairingFailed",
        }
    }

    pub fn description(&self) -> String {
        self.to_info().description()
    }

    pub fn to_info(&self) -> MmErrorInfo {
        let reason = match self {
            InternalError::PairingFailed(r) => r.clone(),
            _ => String::new(),
        };
        MmErrorInfo::new("MinimuxerInternalError", self.code(), &reason)
    }

    pub fn from_info(info: &MmErrorInfo) -> Option<Self> {
        if info.domain != "MinimuxerInternalError" {
            return None;
        }
        use InternalError::*;
        Some(match info.code.as_str() {
            "deviceEndpointNotInitialized" => DeviceEndpointNotInitialized,
            "networkIfaceNotRefreshed" => NetworkIfaceNotRefreshed,
            "pairingFailed" => PairingFailed(info.reason.clone()),
            _ => return None,
        })
    }
}

impl std::fmt::Display for InternalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl std::error::Error for InternalError {}

/// Swift `MinimuxerComponent` (raw values of the service wrapper).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MinimuxerComponent {
    Heartbeat,
    Mounter,
}

impl MinimuxerComponent {
    pub fn raw_value(&self) -> &'static str {
        match self {
            MinimuxerComponent::Heartbeat => "heartbeat",
            MinimuxerComponent::Mounter => "mounter",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        match raw {
            "heartbeat" => Some(MinimuxerComponent::Heartbeat),
            "mounter" => Some(MinimuxerComponent::Mounter),
            _ => None,
        }
    }
}

/// Swift `MinimuxerServiceError`: a component tag wrapped around an inner
/// error's description (`"[<component>] <inner description>"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinimuxerServiceError {
    pub component: MinimuxerComponent,
    /// The wrapped error's description text.
    pub inner_description: String,
}

impl MinimuxerServiceError {
    pub fn description(&self) -> String {
        format!("[{}] {}", self.component.raw_value(), self.inner_description)
    }

    pub fn to_info(&self) -> MmErrorInfo {
        MmErrorInfo::new(
            "MinimuxerServiceError",
            self.component.raw_value(),
            &self.inner_description,
        )
    }

    pub fn from_info(info: &MmErrorInfo) -> Option<Self> {
        if info.domain != "MinimuxerServiceError" {
            return None;
        }
        Some(Self {
            component: MinimuxerComponent::from_raw(&info.code)?,
            inner_description: info.reason.clone(),
        })
    }
}

impl std::fmt::Display for MinimuxerServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl std::error::Error for MinimuxerServiceError {}
