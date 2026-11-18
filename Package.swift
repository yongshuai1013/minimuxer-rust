// swift-tools-version: 5.9
import PackageDescription

// Minimuxer — Rust-backed shell (DESIGN D2: package identity and
// the three product faces are unchanged from the Swift package).
// All behavior lives in the Rust core; this package and its
// Common/ + DeviceGateway/ sub-packages are the Swift binding
// layer only (protocol conformance, type/error conversion, event
// presentation — DESIGN D3).
//
// Differences from the Swift package's manifest, by design:
// - No EMProxy binaryTarget: EMProxy is part of the Rust core
//   (mm-emproxy embeds the em_proxy crate, the same source the
//   binary target shipped).
// - No ZIPFoundation dependency: profile dump / DDI zip handling
//   is done by the Rust core.
// - No IdeviceGateway/LibimobiledeviceGateway/RemotePairingKit
//   dependencies: both backends live in the Rust core.
let package = Package(
    name: "Minimuxer",
    platforms: [
        .iOS(.v14),
        .macOS(.v11),
        .tvOS(.v14)
    ],
    products: [
        .library(
            name: "Minimuxer",
            targets: ["Minimuxer"]
        )
    ],
    dependencies: [
        .package(path: "Common"),
        .package(path: "DeviceGateway")
    ],
    targets: [
        .target(
            name: "Minimuxer",
            dependencies: [
                .product(name: "MinimuxerCommon", package: "Common"),
                .product(name: "MinimuxerRustBindings", package: "Common"),
                .product(name: "DeviceGatewayAPI", package: "DeviceGateway")
            ],
            path: "Shell/Sources"
        )
    ]
)
