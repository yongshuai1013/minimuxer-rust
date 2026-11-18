// swift-tools-version: 5.9
import PackageDescription

// MinimuxerGateway — Rust-backed shell.
//
// Only the DeviceGatewayAPI product remains: the two concrete
// gateway products of the Swift package (IdeviceGateway,
// LibimobiledeviceGateway and their -Dynamic variants) were the
// backend implementations, which now live in the Rust core
// (mm-gateway, selected at runtime through the gateway hub).
// Nothing in the App or in the root package referenced those
// products directly.
let package = Package(
    name: "MinimuxerGateway",
    platforms: [
        .iOS(.v14),
        .macOS(.v11),
        .tvOS(.v14)
    ],
    products: [
        .library(
            name: "DeviceGatewayAPI",
            targets: ["DeviceGatewayAPI"]
        )
    ],
    dependencies: [
        .package(path: "../Common")
    ],
    targets: [
        .target(
            name: "DeviceGatewayAPI",
            dependencies: [
                .product(name: "MinimuxerCommon", package: "Common"),
                .product(name: "MinimuxerRustBindings", package: "Common")
            ],
            path: "Sources"
        )
    ]
)
