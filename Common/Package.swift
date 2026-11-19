// swift-tools-version: 5.9
import PackageDescription

// MinimuxerCommon — Rust-backed shell.
//
// The binaryTarget below is the Rust core (all crates, staticlib)
// published by the minimuxer-rust repo's own release flow
// (DESIGN section 6.5): tag rust-v<semver> -> Release asset
// MinimuxerRust.xcframework.zip; the checksum must match that
// Release asset exactly.
//
// MinimuxerRustBindings (the checked-in swift-bridge generated
// code in Bindings/) is exposed as a product only so the sibling
// shell packages (DeviceGateway, Minimuxer) can share the single
// copy; it is not part of the App-facing surface.
//
// (A dynamic bindings product was tried and is impossible:
// the target is also in the static products' closures, and
// Xcode refuses to build one target both ways.)
//
// The libimobiledevice binaryTarget mirrors the Swift package's
// DeviceGateway manifest: the Rust core's limd backend
// (rusty_libimobiledevice, dynamic-link configuration) binds
// the C library (afc/lockdownd/instproxy/plist/usbmuxd), so the
// vendored xcframework must be in the link, which is what the
// bindings target's dependency on it arranges.
let package = Package(
    name: "MinimuxerCommon",
    platforms: [
        .iOS(.v14),
        .macOS(.v11),
        .tvOS(.v14)
    ],
    products: [
        .library(
            name: "MinimuxerCommon",
            targets: ["MinimuxerCommon"]
        ),
        .library(
            name: "MinimuxerRustBindings",
            targets: ["MinimuxerRustBindings"]
        )
    ],
    dependencies: [
        // Same package and product the Swift package's
        // LibimobiledeviceGateway links for the C library's
        // OpenSSL symbols (pair-record key/cert generation).
        .package(url: "https://github.com/mahee96/RemotePairingKit.git", branch: "main")
    ],
    targets: [
        .binaryTarget(
            name: "MinimuxerRust",
            url: "https://github.com/yongshuai1013/minimuxer-rust/releases/download/rust-v0.1.4/MinimuxerRust.xcframework.zip",
            checksum: "dcc76daad3e4d1814bd4e48478425ed7f78677ecdbde2aaf7c5b28f50c441c7a"
        ),
        .binaryTarget(
            name: "libimobiledevice",
            url: "https://github.com/SideStore/libimobiledevice-xcframework/releases/download/1.4.0-ss-0f88f7b/libimobiledevice.xcframework.zip#DeviceGateway",
            checksum: "7ccbdd56b074807461fc43d2e32ba92f20df2501a6c14cf9f64a917e7f3fe6e7"
        ),
        .target(
            name: "MinimuxerRustBindings",
            dependencies: [
                "MinimuxerRust",
                "libimobiledevice",
                .product(name: "OpenSSL", package: "RemotePairingKit")
            ],
            path: "Bindings"
        ),
        .target(
            name: "MinimuxerCommon",
            dependencies: ["MinimuxerRustBindings"],
            path: "Sources"
        )
    ]
)
