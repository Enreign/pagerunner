// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "PagerunnerMobile",
    platforms: [.iOS(.v17)],
    products: [
        .library(name: "PagerunnerKit", targets: ["PagerunnerKit"]),
    ],
    targets: [
        .target(
            name: "PagerunnerKit",
            path: "Sources/PagerunnerKit",
            swiftSettings: [.swiftLanguageMode(.v6)]
        ),
        .executableTarget(
            name: "Pagerunner",
            dependencies: ["PagerunnerKit"],
            path: "Sources/Pagerunner",
            swiftSettings: [.swiftLanguageMode(.v6)]
        ),
        .testTarget(
            name: "PagerunnerKitTests",
            dependencies: ["PagerunnerKit"],
            path: "Tests/PagerunnerKitTests",
            swiftSettings: [.swiftLanguageMode(.v6)]
        ),
    ]
)
