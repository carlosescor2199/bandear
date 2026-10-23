import BandEarScreens
import SwiftUI

/// The Mixes tab: its title and introduction, and the empty state.
///
/// Follows the Android client's `SavedMixesScreen.kt`, words from ``SavedMixesWords``. Its layout
/// and empty state only: the save field, the list and what the store reports come with the saved
/// mixes themselves (T9).
struct MixesView: View {
    var body: some View {
        List {
            Section {
                Text(SavedMixesWords.screenIntro)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets(top: 0, leading: 4, bottom: 0, trailing: 4))
            }
            Section {
                VStack(alignment: .leading, spacing: 4) {
                    Text(SavedMixesWords.emptyTitle)
                        .font(.headline)
                    Text(SavedMixesWords.emptyDetail)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
                .accessibilityElement(children: .combine)
            }
        }
        .navigationTitle(SavedMixesWords.screenTitle)
    }
}
