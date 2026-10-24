#if DEBUG
    import BandEarScreens
    import SwiftUI

    /// One sample strip's state, for the previews only.
    private struct SampleStrip: Identifiable {
        let id: Int
        var name: String
        var group: String
        var tone: ScribbleStripTone
        var level: Int
        var pan: Int
        var muted = false
        var soloed = false
        var mine = false
        var held: HeldTag.Reason?
        var stereo = false
    }

    /// A console laid out from the shared components alone, with the tab bar in portrait and the
    /// rail in landscape, so the previews show both looks and every control moves. The Mix screen
    /// itself builds its strips from the session (T2); this only exercises the parts.
    private struct ConsoleSample: View {
        @State private var strips = [
            SampleStrip(
                id: 0, name: "Kick", group: "Drums", tone: .group(.teal), level: 75, pan: 50, held: .locked),
            SampleStrip(id: 1, name: "Snare", group: "Drums", tone: .group(.teal), level: 70, pan: 50),
            SampleStrip(id: 2, name: "Bass", group: "No group", tone: .plain, level: 64, pan: 40),
            SampleStrip(
                id: 3, name: "Keys", group: "Band", tone: .group(.violet), level: 58, pan: 50, stereo: true),
            SampleStrip(
                id: 4, name: "Lead vox", group: "My channel", tone: .mine, level: 100, pan: 50, mine: true),
            SampleStrip(
                id: 5, name: "Click", group: "No group", tone: .plain, level: 45, pan: 50, muted: true),
        ]
        @State private var master = 80
        @State private var masterMuted = false
        @State private var row = MixRow.channels
        @State private var moreOfMe = true
        @State private var tab = AppTab.mix
        @State private var panelOpen: Bool

        init(panelOpen: Bool = false) {
            _panelOpen = State(initialValue: panelOpen)
        }

        var body: some View {
            OrientationReader { orientation in
                let layout =
                    orientation.isLandscape
                    ? AnyLayout(HStackLayout(spacing: 0)) : AnyLayout(VStackLayout(spacing: 0))
                layout {
                    if orientation.isLandscape { NavigationRail(selection: $tab) }
                    VStack(alignment: .leading, spacing: 10) {
                        header(landscape: orientation.isLandscape)
                        HStack(spacing: 8) {
                            ScrollView(.horizontal, showsIndicators: false) {
                                HStack(spacing: 6) {
                                    ForEach($strips) { strip in
                                        stripView(strip, width: orientation.isLandscape ? 80 : 72)
                                    }
                                }
                            }
                            masterStrip(width: orientation.isLandscape ? 112 : 88)
                        }
                    }
                    .padding(.horizontal, orientation.isLandscape ? 12 : 20)
                    .padding(.vertical, 10)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    if !orientation.isLandscape { BottomTabBar(selection: $tab) }
                }
            }
            .background(BandEarColors.ground.ignoresSafeArea())
            .modalPanel(isPresented: $panelOpen, doneTitle: "Done") {
                HStack(spacing: 8) {
                    Text("Keys")
                        .bandEarFont(BandEarType.titleMedium)
                    Image(systemName: ScribbleStrip.stereoSymbol)
                        .foregroundStyle(BandEarColors.textSecondary)
                }
                .foregroundStyle(BandEarColors.text)
                .accessibilityAddTraits(.isHeader)
            } content: {
                PanelSample()
            }
            .modalPanelHost()
        }

        /// The header on one line in landscape; in portrait the choice of row goes under it.
        private func header(landscape: Bool) -> some View {
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 10) {
                    VStack(alignment: .leading, spacing: 0) {
                        Text("Ana")
                            .bandEarFont(landscape ? BandEarType.titleSmall : BandEarType.titleLarge)
                            .foregroundStyle(BandEarColors.text)
                        Text("BandEar on Studio-Mac")
                            .bandEarFont(BandEarType.labelMedium)
                            .foregroundStyle(BandEarColors.textSecondary)
                    }
                    Spacer(minLength: 8)
                    if landscape { rowChoice }
                    StatusPill(
                        tone: .green, text: "Connected", voiceOverLabel: "Connection status: Connected")
                }
                if !landscape { rowChoice }
            }
        }

        private var rowChoice: some View {
            SegmentedChoice(options: MixRow.allCases, selection: $row) { $0.label }
        }

        private func stripView(_ strip: Binding<SampleStrip>, width: CGFloat) -> some View {
            let value = strip.wrappedValue
            let fill = BandEarColors.faderFill(
                StripWords.fill(muted: value.muted, mine: value.mine, moreOfMeOn: moreOfMe))
            return VStack(spacing: 0) {
                if let held = value.held {
                    HeldTag(reason: held)
                } else {
                    HStack(spacing: 4) {
                        ConsoleKey(
                            text: "M", kind: .mute, isOn: value.muted, voiceOverLabel: "Mute \(value.name)"
                        ) { strip.wrappedValue.muted = $0 }
                        ConsoleKey(
                            text: "S", kind: .solo, isOn: value.soloed, voiceOverLabel: "Solo \(value.name)"
                        ) { strip.wrappedValue.soloed = $0 }
                    }
                }
                Group {
                    StripPan(
                        pan: value.pan, mine: value.mine, voiceOverLabel: "\(value.name) balance",
                        voiceOverValue: PanScale.spokenText(value.pan)
                    ) { strip.wrappedValue.pan = $0 }
                    .padding(.top, 8)
                    VerticalFader(
                        value: value.level, fill: fill, muted: value.muted,
                        voiceOverLabel: "\(value.name) level",
                        voiceOverValue: StripWords.valueText(level: value.level, muted: value.muted)
                    ) { strip.wrappedValue.level = $0 }
                    .padding(.top, 8)
                    .padding(.bottom, 4)
                    Text(StripWords.valueText(level: value.level, muted: value.muted))
                        .bandEarFont(BandEarType.number.with(size: 13))
                        .foregroundStyle(value.mine ? BandEarColors.accent : BandEarColors.text)
                        .frame(height: 18)
                }
                .disabled(value.held != nil)
                ScribbleStrip(
                    title: value.name, subtitle: value.group, stereo: value.stereo, tone: value.tone,
                    voiceOverLabel: "Open the channel's settings: \(value.name)"
                ) { panelOpen = true }
                .padding(.top, 6)
            }
            .padding(.vertical, 8)
            .padding(.horizontal, 6)
            .frame(width: width)
            .background(
                value.mine ? BandEarColors.surfaceSelected : BandEarColors.surface,
                in: BandEarShapes.rounded(BandEarShapes.panel)
            )
            .overlay(
                BandEarShapes.rounded(BandEarShapes.panel)
                    .strokeBorder(value.mine ? BandEarColors.amberTintBorder : .clear, lineWidth: 1))
        }

        private func masterStrip(width: CGFloat) -> some View {
            VStack(spacing: 0) {
                Text("MASTER")
                    .bandEarFont(BandEarType.caps.with(size: 11))
                    .foregroundStyle(BandEarColors.textSecondary)
                ConsoleKey(
                    text: "Mute", kind: .mute, size: .word, isOn: masterMuted, voiceOverLabel: "Mute master"
                ) { masterMuted = $0 }
                .padding(.top, 6)
                VerticalFader(
                    value: master, fill: BandEarColors.textSecondary, muted: masterMuted, limit: 85,
                    voiceOverLabel: "Master level", voiceOverValue: "\(master)%"
                ) { master = $0 }
                .padding(.top, 8)
                .padding(.bottom, 4)
                Text("\(master)%")
                    .bandEarFont(BandEarType.number.with(size: 13))
                    .foregroundStyle(BandEarColors.text)
                    .frame(height: 18)
                ScribbleStrip(
                    title: "Level cap 90%", subtitle: "Limited to 85%", tone: .plain,
                    voiceOverLabel: "Level cap"
                ) { panelOpen = true }
                .padding(.top, 6)
            }
            .padding(.vertical, 8)
            .padding(.horizontal, 6)
            .frame(width: width)
            .background(BandEarColors.surface, in: BandEarShapes.rounded(BandEarShapes.panel))
            .overlay(
                BandEarShapes.rounded(BandEarShapes.panel).strokeBorder(BandEarColors.border, lineWidth: 1))
        }
    }

    /// The content of a sample panel: the shared buttons, the switch and the pills, with state of
    /// its own so a rotation can show it survives.
    private struct PanelSample: View {
        @State private var on = true
        @State private var taps = 0

        var body: some View {
            VStack(alignment: .leading, spacing: 14) {
                Toggle(isOn: $on) {
                    Text("More of me")
                        .bandEarFont(BandEarType.bodyLarge.with(weight: .semibold))
                        .foregroundStyle(BandEarColors.text)
                }
                .toggleStyle(BandEarSwitchStyle())
                .frame(minHeight: 44)
                Text("Tapped \(taps) times")
                    .bandEarFont(BandEarType.numberSmall)
                    .foregroundStyle(BandEarColors.textSecondary)
                HStack(spacing: 10) {
                    PrimaryButton(title: "Save", fillsWidth: true) { taps += 1 }
                    SecondaryButton(title: "Recall", fillsWidth: true) { taps += 1 }
                }
                DangerButton(title: "Disconnect", fillsWidth: true) { taps += 1 }
                PrimaryButton(title: "Connect") {}
                    .disabled(true)
                HStack(spacing: 8) {
                    StatusPill(tone: .amber, text: "Reconnecting", voiceOverLabel: "Reconnecting")
                    StatusPill(tone: .neutral, text: "Not connected", voiceOverLabel: "Not connected")
                }
                Text("CHANNELS · 6")
                    .bandEarFont(BandEarType.caps)
                    .foregroundStyle(BandEarColors.textSecondary)
            }
        }
    }

    #Preview("Components · portrait") {
        ConsoleSample()
    }

    @available(iOS 17.0, *)
    #Preview("Components · landscape", traits: .landscapeLeft) {
        ConsoleSample()
    }

    #Preview("Modal panel · portrait") {
        ConsoleSample(panelOpen: true)
    }

    @available(iOS 17.0, *)
    #Preview("Modal panel · landscape", traits: .landscapeLeft) {
        ConsoleSample(panelOpen: true)
    }

    #Preview("Panel content · portrait") {
        ScrollView {
            PanelSample()
                .padding(20)
        }
        .background(BandEarColors.surface.ignoresSafeArea())
    }

    @available(iOS 17.0, *)
    #Preview("Panel content · landscape", traits: .landscapeLeft) {
        ScrollView {
            PanelSample()
                .padding(20)
        }
        .background(BandEarColors.surface.ignoresSafeArea())
    }
#endif
