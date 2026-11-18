//! Error bus verification: the generated table (from error-map.toml) must
//! cover every Rust error enum case exactly once, round-trip through
//! MmErrorInfo, and render Swift-identical descriptions.

use mm_common::error::*;
use mm_common::pairing::PairingProtocol;
use std::collections::HashSet;

#[test]
fn table_has_73_cases_with_expected_domain_counts() {
    assert_eq!(ERROR_CASES.len(), 73);
    let count = |d: &str| ERROR_CASES.iter().filter(|c| c.domain == d).count();
    assert_eq!(count("MinimuxerError"), 46);
    assert_eq!(count("DeviceGatewayError"), 8);
    assert_eq!(count("PairingError"), 4);
    assert_eq!(count("EMProxyError"), 10);
    assert_eq!(count("MinimuxerInternalError"), 3);
    assert_eq!(count("MinimuxerServiceError"), 2);
    let mut seen = HashSet::new();
    for c in ERROR_CASES {
        assert!(
            seen.insert((c.domain, c.code)),
            "duplicate case {} / {}",
            c.domain,
            c.code
        );
    }
}

fn all_minimuxer_errors() -> Vec<MinimuxerError> {
    use MinimuxerError::*;
    let r = || "reason".to_string();
    vec![
        NoDevice(r()),
        NoConnection(r()),
        NotReachable(r()),
        NoVPN(r()),
        NotStarted(r()),
        PairingNotLoaded(r()),
        ConnectionModeNotConfigured(r()),
        RestartAlreadyInProgressError(r()),
        InvalidVPN(r()),
        InvalidPairing {
            protocol: PairingProtocol::Lockdown,
            reason: r(),
        },
        MuxerNotListening(r()),
        CreateDebug(r()),
        CreateInstproxy(r()),
        CreateLockdown(r()),
        CreateCoreDevice(r()),
        CreateSoftwareTunnel(r()),
        CreateRemoteServer(r()),
        CreateProcessControl(r()),
        GetLockdownValue(r()),
        FetchUDID(r()),
        Connect(r()),
        Close(r()),
        XpcHandshake(r()),
        NoService(r()),
        InvalidProductVersion(r()),
        LookupApps(r()),
        FindApp(r()),
        BundlePath(r()),
        MaxPacket(r()),
        WorkingDirectory(r()),
        Argv(r()),
        LaunchSuccess(r()),
        Detach(r()),
        Attach(r()),
        CreateAfc(r()),
        RwAfc(r()),
        InstallApp(r()),
        UninstallApp(r()),
        CreateMisagent(r()),
        ProfileInstall(r()),
        ProfileRemove(r()),
        CreateFolder(r()),
        DownloadImage(r()),
        ImageLookup(r()),
        ImageRead(r()),
        Mount {
            protocol: PairingProtocol::RpPairing,
            reason: r(),
        },
    ]
}

#[test]
fn minimuxer_error_round_trips_and_is_in_table() {
    let all = all_minimuxer_errors();
    assert_eq!(all.len(), 46);
    for e in all {
        let info = e.to_info();
        assert!(
            lookup_error_case(&info.domain, &info.code).is_some(),
            "missing table entry for {}",
            info.code
        );
        assert_eq!(MinimuxerError::from_info(&info), Some(e.clone()));
        assert!(!info.description().is_empty());
    }
}

#[test]
fn other_domains_round_trip() {
    use DeviceGatewayCode::*;
    for code in [
        InvalidPairingFile,
        ConnectionFailed,
        ServiceError,
        NoConnection,
        NotInitialized,
        DeviceEndpointIpNotAvailable,
        InvalidTargetEndpoint,
        UnsupportedOperation,
    ] {
        let e = DeviceGatewayError::new(code, "why");
        let info = e.to_info();
        assert!(lookup_error_case(&info.domain, &info.code).is_some());
        assert_eq!(DeviceGatewayError::from_info(&info), Some(e));
    }

    for e in [
        PairingError::Unreadable("u".into()),
        PairingError::InvalidPlist("i".into()),
        PairingError::Incomplete {
            protocol: PairingProtocol::Lockdown,
            missing_keys: vec!["EscrowBag".into(), "UDID".into()],
        },
        PairingError::Ambiguous("a".into()),
    ] {
        let info = e.to_info();
        assert!(lookup_error_case(&info.domain, &info.code).is_some());
        assert_eq!(PairingError::from_info(&info), Some(e));
    }

    for e in [
        EmProxyError::InvalidBindAddressPointer,
        EmProxyError::InvalidUtf8String,
        EmProxyError::InvalidSocketAddress("bad".into()),
        EmProxyError::SocketBindFailed,
        EmProxyError::CryptoInitFailed,
        EmProxyError::ServerNotRunning,
        EmProxyError::StopSignalFailed,
        EmProxyError::ThreadJoinFailed,
        EmProxyError::HandshakeClientNotConfigured,
        EmProxyError::UnknownError(-3),
    ] {
        let info = e.to_info();
        assert!(lookup_error_case(&info.domain, &info.code).is_some());
        assert_eq!(EmProxyError::from_info(&info), Some(e));
    }

    for e in [
        InternalError::DeviceEndpointNotInitialized,
        InternalError::NetworkIfaceNotRefreshed,
        InternalError::PairingFailed("p".into()),
    ] {
        let info = e.to_info();
        assert!(lookup_error_case(&info.domain, &info.code).is_some());
        assert_eq!(InternalError::from_info(&info), Some(e));
    }

    for comp in [MinimuxerComponent::Heartbeat, MinimuxerComponent::Mounter] {
        let e = MinimuxerServiceError {
            component: comp,
            inner_description: "inner".into(),
        };
        let info = e.to_info();
        assert!(lookup_error_case(&info.domain, &info.code).is_some());
        assert_eq!(MinimuxerServiceError::from_info(&info), Some(e));
    }
}

#[test]
fn descriptions_match_swift() {
    assert_eq!(
        MinimuxerError::NoDevice("gone".into()).description(),
        "NoDevice: gone"
    );
    assert_eq!(
        MinimuxerError::InstallApp("boom".into()).description(),
        "InstallApp(boom)"
    );
    assert_eq!(
        MinimuxerError::InvalidPairing {
            protocol: PairingProtocol::Lockdown,
            reason: "stale".into(),
        }
        .description(),
        "InvalidPairing(protocol: lockdown, reason: stale)"
    );
    assert_eq!(
        MinimuxerError::Mount {
            protocol: PairingProtocol::RpPairing,
            reason: "no dmg".into(),
        }
        .description(),
        "Mount(protocol: rppairing, reason: no dmg)"
    );
    // DeviceGatewayError empty-reason fallbacks.
    assert_eq!(
        DeviceGatewayError::new(DeviceGatewayCode::NoConnection, "").description(),
        "No connection to the device."
    );
    assert_eq!(
        DeviceGatewayError::new(DeviceGatewayCode::NotInitialized, "").description(),
        "DeviceGateway not initialized. start() should be called first."
    );
    assert_eq!(
        DeviceGatewayError::new(DeviceGatewayCode::DeviceEndpointIpNotAvailable, "")
            .description(),
        "Device endpoint IP is not available."
    );
    assert_eq!(
        DeviceGatewayError::new(DeviceGatewayCode::InvalidTargetEndpoint, "").description(),
        "Invalid target endpoint IP or port."
    );
    assert_eq!(
        DeviceGatewayError::new(DeviceGatewayCode::UnsupportedOperation, "wipe").description(),
        "Operation 'wipe' is not supported."
    );
    assert_eq!(
        DeviceGatewayError::new(DeviceGatewayCode::InvalidPairingFile, "bad plist")
            .description(),
        "The pairing file is invalid: bad plist"
    );
    assert_eq!(EmProxyError::UnknownError(-3).description(), "EMProxy error code: -3");
    assert_eq!(
        EmProxyError::InvalidSocketAddress("x".into()).description(),
        "Invalid IPv4 socket address format: x"
    );
    assert_eq!(
        InternalError::PairingFailed("nope".into()).description(),
        "PairingFailed(nope)"
    );
    assert_eq!(
        PairingError::Incomplete {
            protocol: PairingProtocol::RpPairing,
            missing_keys: vec!["public_key".into()],
        }
        .description(),
        "The pairing file is incomplete for .rppairing. Missing keys: public_key."
    );
}

#[test]
fn gateway_mapping_functions_match_swift() {
    use DeviceGatewayCode::*;
    let broken_pipe = DeviceGatewayError::new(ConnectionFailed, "Broken pipe (os error 32)");
    assert!(broken_pipe.is_vpn_drop());
    assert!(!broken_pipe.is_retryable());
    assert_eq!(
        broken_pipe.as_minimuxer_error(PairingProtocol::Lockdown, MinimuxerError::Connect),
        MinimuxerError::InvalidVPN("Broken pipe (os error 32)".into())
    );

    let early_eof = DeviceGatewayError::new(ServiceError, "early eof");
    assert!(early_eof.is_vpn_drop());

    // Code gate: noConnection is never a VPN drop even with a pipe reason.
    let wrong_code = DeviceGatewayError::new(NoConnection, "broken pipe");
    assert!(!wrong_code.is_vpn_drop());
    assert!(wrong_code.is_retryable());

    let timeout = DeviceGatewayError::new(ConnectionFailed, "timed out");
    assert!(!timeout.is_vpn_drop());
    assert!(timeout.is_retryable());
    assert_eq!(
        timeout.as_minimuxer_error(PairingProtocol::Lockdown, MinimuxerError::Connect),
        MinimuxerError::Connect("timed out".into())
    );

    let bad_pairing = DeviceGatewayError::new(InvalidPairingFile, "corrupt");
    assert_eq!(
        bad_pairing.as_minimuxer_error(PairingProtocol::RpPairing, MinimuxerError::Connect),
        MinimuxerError::InvalidPairing {
            protocol: PairingProtocol::RpPairing,
            reason: "corrupt".into(),
        }
    );
}
