// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "AuraCapture",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "AuraCapture", type: .static, targets: ["AuraCapture"])
    ],
    targets: [
        .target(name: "AuraCapture"),
        .testTarget(name: "AuraCaptureTests", dependencies: ["AuraCapture"]),
    ],
    swiftLanguageModes: [.v5]
)
