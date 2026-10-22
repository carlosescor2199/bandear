// swift-tools-version: 6.0
import PackageDescription

/// The parts of the iOS client that have nothing to do with iOS: the wire format the desk speaks,
/// the timing policy that decides when a packet is played, how a desk is found, and the mix the
/// musician is editing. Keeping them here means `swift test` proves them from the command line,
/// with no Xcode project and no simulator, and keeps the app shell thin enough to read in one
/// sitting.
let package = Package(
    name: "BandEarKit",
    platforms: [.iOS(.v16), .macOS(.v13)],
    products: [
        .library(name: "BandEarProtocol", targets: ["BandEarProtocol"]),
        .library(name: "BandEarDiscovery", targets: ["BandEarDiscovery"]),
        .library(name: "BandEarMix", targets: ["BandEarMix"]),
    ],
    targets: [
        .target(name: "BandEarProtocol"),
        .testTarget(name: "BandEarProtocolTests", dependencies: ["BandEarProtocol"]),
        .target(name: "BandEarDiscovery"),
        .testTarget(name: "BandEarDiscoveryTests", dependencies: ["BandEarDiscovery"]),
        // The mix speaks the desk's published vocabulary (its `config` and `mix_*` values), so it
        // depends on the protocol module; nothing in the protocol depends on the mix.
        .target(name: "BandEarMix", dependencies: ["BandEarProtocol"]),
        .testTarget(name: "BandEarMixTests", dependencies: ["BandEarMix", "BandEarProtocol"]),
    ]
)
