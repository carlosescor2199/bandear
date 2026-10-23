import SwiftUI

/// The Mix tab: the musician's mix while a session runs.
///
/// A placeholder with the signature the app shell uses; T8d3 builds the screen.
struct MixView: View {
    @ObservedObject var session: SessionModel

    var body: some View {
        Text("Mix")
    }
}
