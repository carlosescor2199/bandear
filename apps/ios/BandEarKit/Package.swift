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
        .library(name: "BandEarPlayback", targets: ["BandEarPlayback"]),
        .library(name: "BandEarDiscovery", targets: ["BandEarDiscovery"]),
        .library(name: "BandEarMix", targets: ["BandEarMix"]),
        .library(name: "BandEarSession", targets: ["BandEarSession"]),
    ],
    targets: [
        .target(name: "BandEarProtocol"),
        .testTarget(name: "BandEarProtocolTests", dependencies: ["BandEarProtocol"]),
        // The lock-free ring between the receive loop and the render callback. In C because iOS 16
        // has no `Synchronization.Atomic`, and C11 atomics need no package to fetch.
        .target(name: "BandEarRing"),
        // When a packet is played, and how the phone measures and plays it. It follows the desk's
        // playback settings and packets, so it depends on the protocol; nothing there depends on it.
        // It plays through the ring.
        .target(name: "BandEarPlayback", dependencies: ["BandEarProtocol", "BandEarRing"]),
        .testTarget(
            name: "BandEarPlaybackTests", dependencies: ["BandEarPlayback", "BandEarProtocol"]),
        .target(name: "BandEarDiscovery"),
        .testTarget(name: "BandEarDiscoveryTests", dependencies: ["BandEarDiscovery"]),
        // The mix speaks the desk's published vocabulary (its `config` and `mix_*` values), so it
        // depends on the protocol module; nothing in the protocol depends on the mix.
        .target(name: "BandEarMix", dependencies: ["BandEarProtocol"]),
        .testTarget(name: "BandEarMixTests", dependencies: ["BandEarMix", "BandEarProtocol"]),
        // The control session joins the other four: it connects to a desk found by discovery,
        // speaks the protocol over the connection, runs the mix through it, and plays the audio
        // the desk sends by the session's settings.
        .target(
            name: "BandEarSession",
            dependencies: ["BandEarProtocol", "BandEarMix", "BandEarDiscovery", "BandEarPlayback"]),
        .testTarget(
            name: "BandEarSessionTests",
            dependencies: ["BandEarSession", "BandEarProtocol", "BandEarMix", "BandEarDiscovery"]),
    ]
)
