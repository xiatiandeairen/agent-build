// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Core",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [.library(name: "Core", targets: ["Core"])],
    targets: [
        .target(name: "CoreObjC"),
        .target(name: "Core", dependencies: ["CoreObjC"]),
        .testTarget(name: "CoreTests", dependencies: ["Core"]),
    ]
)
