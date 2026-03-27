// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "PagerunnerBar",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "PagerunnerBar", targets: ["PagerunnerBar"]),
        .library(name: "PagerunnerCore", targets: ["PagerunnerCore"]),
    ],
    dependencies: [
        .package(url: "https://github.com/sparkle-project/Sparkle", from: "2.6.0"),
        .package(url: "https://github.com/sindresorhus/KeyboardShortcuts", from: "2.2.0"),
    ],
    targets: [
        .target(
            name: "PagerunnerCore",
            dependencies: [],
            path: "Sources/PagerunnerCore",
            swiftSettings: [.unsafeFlags(["-strict-concurrency=complete"])]
        ),
        .executableTarget(
            name: "PagerunnerBar",
            dependencies: [
                "PagerunnerCore",
                .product(name: "Sparkle", package: "Sparkle"),
                .product(name: "KeyboardShortcuts", package: "KeyboardShortcuts"),
            ],
            path: "Sources/PagerunnerBar",
            swiftSettings: [.unsafeFlags(["-strict-concurrency=complete"])]
        ),
        .testTarget(
            name: "PagerunnerCoreTests",
            dependencies: ["PagerunnerCore"],
            path: "Tests/PagerunnerCoreTests"
        ),
    ]
)
