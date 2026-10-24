import BandEarScreens
import SwiftUI
import UIKit

extension View {
    /// Shows `content` in a modal panel while `isPresented` is true: a bottom sheet with a grabber in
    /// portrait, and a panel from the trailing edge in landscape (the design's "Channel panel"
    /// boards), over a dimmed scrim. iOS 16 has neither inspectors nor presentation sizing, so the
    /// panel is drawn here rather than presented.
    ///
    /// The panel's top row holds `header` (the panel's title, such as the channel's name) and a
    /// `doneTitle` button (``ChannelSheetWords/done``). Tapping the scrim, the Done button or
    /// VoiceOver's escape gesture (a two-finger Z) sets `isPresented` to false. The content scrolls
    /// when it is taller than the panel; in portrait the sheet is as tall as its content, which may
    /// take up to three quarters of the space.
    ///
    /// For VoiceOver the panel is modal: the views beneath are hidden while it is up, and focus
    /// moves into it when it opens and back when it closes.
    ///
    /// The panel and its content keep their place in the view tree whichever way the space is laid
    /// out, so the content's own state survives a rotation.
    ///
    /// The panel covers the view it is presented from. Where an ancestor installed
    /// ``modalPanelHost()``, it covers that ancestor instead, so a panel opened from the Mix screen
    /// also dims the tab bar or the rail beside it.
    func modalPanel<Header: View, Content: View>(
        isPresented: Binding<Bool>,
        doneTitle: String,
        @ViewBuilder header: @escaping () -> Header,
        @ViewBuilder content: @escaping () -> Content
    ) -> some View {
        modifier(
            ModalPanelModifier(
                isPresented: isPresented, doneTitle: doneTitle, header: header, panel: content))
    }

    /// Shows the ``modalPanel(isPresented:doneTitle:header:content:)`` panels of the views inside
    /// over this whole view: put it on the root, around the screen and its tab bar or rail.
    ///
    /// The panels reach it as SwiftUI preferences, which may not cross a `TabView`, a sheet or a
    /// navigation stack: install it where none stands between it and the presenting views, or a
    /// panel presented beyond one would not show.
    func modalPanelHost() -> some View {
        modifier(ModalPanelHostModifier())
    }
}

/// Whether a ``ModalPanelHostModifier`` above shows the panels of the views inside.
private struct ModalPanelHostedKey: EnvironmentKey {
    nonisolated static var defaultValue: Bool { false }
}

extension EnvironmentValues {
    fileprivate var modalPanelHosted: Bool {
        get { self[ModalPanelHostedKey.self] }
        set { self[ModalPanelHostedKey.self] = newValue }
    }
}

/// A panel handed up to the host: one per presenting modifier, whether it is open or not, so it
/// can still animate out.
private struct ModalPanelEntry: Identifiable {
    let id: UUID
    let layer: AnyView
}

/// The panels of the views inside a host, in the order they are declared.
private struct ModalPanelEntries: PreferenceKey {
    nonisolated static var defaultValue: [ModalPanelEntry] { [] }

    nonisolated static func reduce(value: inout [ModalPanelEntry], nextValue: () -> [ModalPanelEntry]) {
        value.append(contentsOf: nextValue())
    }
}

/// Whether any panel inside a host is open, so the host can hide what is beneath from VoiceOver.
/// Apart from ``ModalPanelEntries`` because it compares, and the entries must not.
private struct ModalPanelShown: PreferenceKey {
    nonisolated static var defaultValue: Bool { false }

    nonisolated static func reduce(value: inout Bool, nextValue: () -> Bool) {
        value = value || nextValue()
    }
}

private struct ModalPanelHostModifier: ViewModifier {
    @State private var shown = false

    func body(content: Content) -> some View {
        content
            .environment(\.modalPanelHosted, true)
            .accessibilityHidden(shown)
            .onPreferenceChange(ModalPanelShown.self) { shown = $0 }
            .overlayPreferenceValue(ModalPanelEntries.self) { entries in
                ZStack {
                    ForEach(entries) { $0.layer }
                }
            }
    }
}

private struct ModalPanelModifier<Header: View, Panel: View>: ViewModifier {
    @Binding var isPresented: Bool
    let doneTitle: String
    let header: () -> Header
    let panel: () -> Panel

    @Environment(\.modalPanelHosted) private var hosted
    @State private var id = UUID()

    func body(content: Content) -> some View {
        let layer = ModalPanelLayer(
            isPresented: $isPresented, doneTitle: doneTitle, header: header, panel: panel)
        if hosted {
            let entry = ModalPanelEntry(id: id, layer: AnyView(layer))
            let open = isPresented
            content
                .transformPreference(ModalPanelEntries.self) { $0.append(entry) }
                .transformPreference(ModalPanelShown.self) { $0 = $0 || open }
        } else {
            content
                .accessibilityHidden(isPresented)
                .overlay { layer }
        }
    }
}

/// The scrim and the panel over the space they are given, laid out by that space's orientation.
private struct ModalPanelLayer<Header: View, Panel: View>: View {
    @Binding var isPresented: Bool
    let doneTitle: String
    let header: () -> Header
    let panel: () -> Panel

    var body: some View {
        GeometryReader { proxy in
            let orientation = LayoutOrientation(proxy.size)
            let alignment: Alignment = orientation.isLandscape ? .trailing : .bottom
            ZStack(alignment: alignment) {
                if isPresented {
                    Color.black.opacity(0.5)
                        .ignoresSafeArea()
                        .contentShape(Rectangle())
                        .onTapGesture { isPresented = false }
                        .accessibilityHidden(true)
                        .transition(.opacity)
                    ModalPanelSurface(
                        orientation: orientation, available: proxy.size, doneTitle: doneTitle,
                        dismiss: { isPresented = false }, header: header, panel: panel
                    )
                    .transition(.move(edge: orientation.isLandscape ? .trailing : .bottom))
                }
            }
            .frame(width: proxy.size.width, height: proxy.size.height, alignment: alignment)
        }
        .animation(.easeOut(duration: 0.25), value: isPresented)
    }
}

/// The panel itself: the grabber in portrait, the header row with Done, and the content.
private struct ModalPanelSurface<Header: View, Panel: View>: View {
    let orientation: LayoutOrientation
    let available: CGSize
    let doneTitle: String
    let dismiss: @MainActor () -> Void
    let header: () -> Header
    let panel: () -> Panel

    /// The landscape panel's width inside the safe area; its background runs on past the edge.
    private static var sideWidth: CGFloat { 360 }

    var body: some View {
        let landscape = orientation.isLandscape
        VStack(alignment: .leading, spacing: landscape ? 12 : 18) {
            if !landscape {
                Capsule()
                    .fill(BandEarColors.border)
                    .frame(width: 36, height: 5)
                    .frame(maxWidth: .infinity)
                    .accessibilityHidden(true)
            }
            HStack(spacing: 8) {
                header()
                    .frame(maxWidth: .infinity, alignment: .leading)
                Button {
                    dismiss()
                } label: {
                    Text(doneTitle)
                        .bandEarFont(BandEarType.bodyLarge.with(weight: .semibold))
                        .foregroundStyle(BandEarColors.accent)
                        .frame(minWidth: 44, minHeight: 44, alignment: .trailing)
                        .contentShape(Rectangle())
                }
                .buttonStyle(PressedDimStyle())
            }
            let maxHeight = LayoutRules.panelContentMaxHeight(
                spaceHeight: Double(available.height), isLandscape: landscape)
            ContentHeightLayout(maxHeight: CGFloat(maxHeight)) {
                ScrollView {
                    panel()
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
        .padding(.top, landscape ? 16 : 10)
        .padding(.leading, landscape ? 24 : 20)
        .padding(.trailing, landscape ? 16 : 20)
        .padding(.bottom, landscape ? 20 : 16)
        .frame(width: landscape ? min(Self.sideWidth, available.width) : nil)
        .frame(maxWidth: landscape ? nil : .infinity, maxHeight: landscape ? .infinity : nil, alignment: .top)
        .background {
            let shape = PanelShape(attachedEdge: landscape ? .trailing : .bottom, radius: BandEarShapes.pill)
            shape.fill(BandEarColors.surface)
                .overlay(shape.stroke(BandEarColors.border, lineWidth: 1))
                // Past the screen's edge, so the attached side's outline never shows.
                .padding(landscape ? .trailing : .bottom, -80)
                .flipsForRightToLeftLayoutDirection(true)
                .ignoresSafeArea()
        }
        .accessibilityElement(children: .contain)
        .accessibilityAddTraits(.isModal)
        .accessibilityAction(.escape) { dismiss() }
        .onAppear { UIAccessibility.post(notification: .screenChanged, argument: nil) }
        .onDisappear { UIAccessibility.post(notification: .screenChanged, argument: nil) }
    }
}

/// Sizes its one child, a scroll view, to the height of what it scrolls, up to `maxHeight` and the
/// height offered: a short panel hugs its content and a tall one scrolls
/// (``LayoutRules/panelContentHeight(ideal:maxHeight:proposed:)``).
private struct ContentHeightLayout: Layout {
    let maxHeight: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard let child = subviews.first else { return .zero }
        let ideal = child.sizeThatFits(ProposedViewSize(width: proposal.width, height: nil))
        let height = LayoutRules.panelContentHeight(
            ideal: Double(ideal.height), maxHeight: Double(maxHeight),
            proposed: proposal.height.map(Double.init))
        return CGSize(width: proposal.width ?? ideal.width, height: CGFloat(height))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        subviews.first?.place(at: bounds.origin, proposal: ProposedViewSize(bounds.size))
    }
}

/// A rectangle with the two corners away from `attachedEdge` rounded: the top corners of a bottom
/// sheet, the leading corners of a side panel. iOS 16 has no uneven rounded rectangle.
private struct PanelShape: Shape {
    let attachedEdge: Edge
    let radius: CGFloat

    nonisolated func path(in rect: CGRect) -> Path {
        let r = min(radius, rect.width / 2, rect.height / 2)
        // The corners clockwise from the top leading one, each with its radius.
        let corners: [(point: CGPoint, radius: CGFloat)] = [
            (CGPoint(x: rect.minX, y: rect.minY), r),
            (CGPoint(x: rect.maxX, y: rect.minY), attachedEdge == .bottom ? r : 0),
            (CGPoint(x: rect.maxX, y: rect.maxY), 0),
            (CGPoint(x: rect.minX, y: rect.maxY), attachedEdge == .bottom ? 0 : r),
        ]
        var path = Path()
        path.move(to: CGPoint(x: rect.minX + r, y: rect.minY))
        for index in 1...corners.count {
            let corner = corners[index % corners.count]
            let next = corners[(index + 1) % corners.count].point
            path.addArc(tangent1End: corner.point, tangent2End: next, radius: corner.radius)
        }
        path.closeSubpath()
        return path
    }
}
