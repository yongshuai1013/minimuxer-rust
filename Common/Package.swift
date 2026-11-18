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
    targets: [
        .binaryTarget(
            name: "MinimuxerRust",
            url: "https://github.com/yongshuai1013/minimuxer-rust/releases/download/rust-v0.1.0/MinimuxerRust.xcframework.zip",
            checksum: "__XCFRAMEWORK_CHECKSUM__"
        ),
        .target(
            name: "MinimuxerRustBindings",
            dependencies: ["MinimuxerRust"],
            path: "Bindings"
        ),
        .target(
            name: "MinimuxerCommon",
            dependencies: ["MinimuxerRustBindings"],
            path: "Sources"
        )
    ]
)
