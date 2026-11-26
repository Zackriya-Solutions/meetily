// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "Meetily",
    platforms: [
        .iOS(.v17),
        .macOS(.v14)
    ],
    products: [
        .executable(name: "Meetily", targets: ["Meetily"])
    ],
    targets: [
        .executableTarget(
            name: "Meetily",
            path: "Sources/Meetily"
        )
    ]
)
