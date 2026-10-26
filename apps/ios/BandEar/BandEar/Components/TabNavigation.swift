import BandEarScreens
import SwiftUI

extension AppTab {
    /// The SF Symbol of the tab: faders for Mix, a bookmark for the saved Mixes, a gear for Settings,
    /// close to the design's line icons.
    var symbol: String {
        switch self {
        case .mix: "slider.vertical.3"
        case .mixes: "bookmark"
        case .settings: "gearshape"
        }
    }
}

/// The bar along the bottom of the screen that switches between the ``AppTab``s in portrait
/// (Android's `BottomTabBar`), on the bar colour. The selected tab is in the accent and bolder, so
/// it is not told by colour alone.
///
/// VoiceOver hears each tab as a selectable button, the selected one as selected; from iOS 17 the
/// bar is also a tab bar, so each says its place ("tab, 1 of 3"). Every tab is at least 48 pt tall.
/// The bar runs on under the home indicator; put it at the bottom of the screen.
struct BottomTabBar: View {
    @Binding var selection: AppTab

    var body: some View {
        VStack(spacing: 0) {
            Rectangle()
                .fill(BandEarColors.border)
                .frame(height: 1)
            HStack(spacing: 0) {
                ForEach(AppTab.allCases, id: \.self) { tab in
                    let selected = tab == selection
                    Button {
                        selection = tab
                    } label: {
                        VStack(spacing: 4) {
                            Image(systemName: tab.symbol)
                                .font(.system(size: 20, weight: .medium))
                                .frame(height: 22)
                            Text(tab.label)
                                .bandEarFont(
                                    BandEarType.labelSmall.with(weight: selected ? .semibold : .medium))
                                .lineLimit(1)
                        }
                        .foregroundStyle(selected ? BandEarColors.accent : BandEarColors.textSecondary)
                        .frame(maxWidth: .infinity, minHeight: 48)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(PressedDimStyle())
                    .tabItemAccessibility(label: tab.label, selected: selected)
                }
            }
            .padding(.horizontal, 12)
            .padding(.top, 8)
            .padding(.bottom, 4)
        }
        .background(BandEarColors.bar.ignoresSafeArea(edges: .bottom))
        .dynamicTypeSize(...DynamicTypeSize.xxLarge)
        .tabBarAccessibility()
    }
}

/// The rail down the leading edge of the screen that switches between the ``AppTab``s in landscape,
/// so the faders keep the screen's height: the same three tabs on the bar colour, the selected one
/// in the accent on the accent's tint.
///
/// VoiceOver hears it as ``BottomTabBar``. Every item is 60 by 64 pt. The rail runs on under the
/// leading safe area and the top and bottom edges; put it at the leading edge of the screen.
struct NavigationRail: View {
    @Binding var selection: AppTab

    var body: some View {
        VStack(spacing: 8) {
            ForEach(AppTab.allCases, id: \.self) { tab in
                let selected = tab == selection
                Button {
                    selection = tab
                } label: {
                    VStack(spacing: 4) {
                        Image(systemName: tab.symbol)
                            .font(.system(size: 20, weight: .medium))
                            .frame(height: 22)
                        Text(tab.label)
                            .bandEarFont(
                                BandEarType.labelSmall.with(size: 10, weight: selected ? .semibold : .medium))
                            .lineLimit(1)
                            .minimumScaleFactor(0.8)
                    }
                    .foregroundStyle(selected ? BandEarColors.accent : BandEarColors.textSecondary)
                    .frame(width: 60)
                    .frame(minHeight: 64)
                    .background(
                        selected ? BandEarColors.amberTint : .clear,
                        in: BandEarShapes.rounded(BandEarShapes.card))
                    .contentShape(Rectangle())
                }
                .buttonStyle(PressedDimStyle())
                .tabItemAccessibility(label: tab.label, selected: selected)
            }
        }
        .padding(.leading, 4)
        .padding(.trailing, 4)
        .frame(maxHeight: .infinity)
        .background {
            BandEarColors.bar
                .overlay(alignment: .trailing) {
                    Rectangle()
                        .fill(BandEarColors.border)
                        .frame(width: 1)
                }
                .ignoresSafeArea(edges: [.leading, .vertical])
        }
        .dynamicTypeSize(...DynamicTypeSize.xxLarge)
        .tabBarAccessibility()
    }
}

extension View {
    /// A tab of a ``BottomTabBar`` or a ``NavigationRail``: a button named by its tab, selected while
    /// it is the one shown, and shown large when held under the larger accessibility text sizes.
    fileprivate func tabItemAccessibility(label: String, selected: Bool) -> some View {
        self
            .accessibilityLabel(label)
            .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
            .accessibilityShowsLargeContentViewer()
    }

    /// The tabs' container: one group for VoiceOver, a tab bar where iOS 17 can say so.
    @ViewBuilder fileprivate func tabBarAccessibility() -> some View {
        if #available(iOS 17.0, *) {
            accessibilityElement(children: .contain).accessibilityAddTraits(.isTabBar)
        } else {
            accessibilityElement(children: .contain)
        }
    }
}
