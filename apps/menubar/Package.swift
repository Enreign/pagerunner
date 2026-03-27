// swift-tools-version: 6.0
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
        .package(url: "https://github.com/sindresorhus/KeyboardShortcuts", exact: "1.15.0"),
    ],
    targets: [
        .target(
            name: "PagerunnerCore",
            dependencies: [],
            path: "Sources/PagerunnerCore"
        ),
        .executableTarget(
            name: "PagerunnerBar",
            dependencies: [
                "PagerunnerCore",
                .product(name: "Sparkle", package: "Sparkle"),
                .product(name: "KeyboardShortcuts", package: "KeyboardShortcuts"),
            ],
            path: "Sources/PagerunnerBar",
            exclude: ["Info.plist"]
        ),
        .testTarget(
            name: "PagerunnerCoreTests",
            dependencies: ["PagerunnerCore"],
            path: "Tests/PagerunnerCoreTests"
        ),
    ],
    swiftLanguageModes: [.v6]
)
