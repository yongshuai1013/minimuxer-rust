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
// The bindings product is DYNAMIC on purpose: the generated code
// contains the @_cdecl shims for the Swift-implemented bridge
// types (MmEventSink, MmConnectionConfig, MmWirelessPairSink)
// that the Rust staticlib calls back into. In a static product
// those shims never reach the App link (the integration build
// failed with exactly those 20 symbols undefined), because
// nothing on the Swift side references them and archive member
// pulling drops the object that defines them. A dynamic
// framework links all of the target's objects unconditionally
// and exports the shims.
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
            type: .dynamic,
            targets: ["MinimuxerRustBindings"]
        )
    ],
    targets: [
        .binaryTarget(
            name: "MinimuxerRust",
            url: "https://github.com/yongshuai1013/minimuxer-rust/releases/download/rust-v0.1.2/MinimuxerRust.xcframework.zip",
            checksum: "d494410d36312671001c7ff5d20dd6e4e92fc7c6d224e2b8cb2f1e1d7e88b192"
        ),
        .binaryTarget(
            name: "libimobiledevice",
            url: "https://github.com/SideStore/libimobiledevice-xcframework/releases/download/1.4.0-ss-0f88f7b/libimobiledevice.xcframework.zip#DeviceGateway",
            checksum: "7ccbdd56b074807461fc43d2e32ba92f20df2501a6c14cf9f64a917e7f3fe6e7"
        ),
        .target(
            name: "MinimuxerRustBindings",
            dependencies: ["MinimuxerRust", "libimobiledevice"],
            path: "Bindings"
        ),
        .target(
            name: "MinimuxerCommon",
            dependencies: ["MinimuxerRustBindings"],
            path: "Sources"
        )
    ]
)
