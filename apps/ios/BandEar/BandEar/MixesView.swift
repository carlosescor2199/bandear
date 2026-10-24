import BandEarMix
import BandEarScreens
import SwiftUI

/// The Mixes tab: the mixes saved on this phone while a session runs.
///
/// The container: it reads the saved mixes and hands their values to ``MixesContent``, which lays
/// them out, and sends every action back through ``MixesModel``.
struct MixesView: View {
    @ObservedObject var mixes: MixesModel

    var body: some View {
        MixesContent(
            names: mixes.mixes.map(\.name),
            activeName: mixes.activeName,
            notice: mixes.notice,
            save: { mixes.save(as: $0) },
            recall: { mixes.recall($0) },
            rename: { mixes.rename($0, to: $1) },
            delete: { mixes.delete($0) })
    }
}
