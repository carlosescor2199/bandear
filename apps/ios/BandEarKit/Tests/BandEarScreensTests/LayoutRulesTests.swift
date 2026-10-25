import BandEarScreens
import Testing

/// Which way a space is laid out. The Android client pins the same rule in `LayoutOrientationTest`
/// (`apps/android/app/src/test/java/com/bandear/android/ui/components/LayoutOrientationTest.kt`).
@Suite("Layout rules: orientation")
struct LayoutOrientationRuleTests {
    @Test func aPhoneOnItsSideIsLandscape() {
        #expect(LayoutRules.isLandscape(width: 844, height: 390))
    }

    @Test func anUprightPhoneIsPortrait() {
        #expect(!LayoutRules.isLandscape(width: 390, height: 844))
    }

    @Test func aSquareSpaceIsPortrait() {
        #expect(!LayoutRules.isLandscape(width: 600, height: 600))
    }

    @Test func aSpaceNotMeasuredYetIsPortrait() {
        #expect(!LayoutRules.isLandscape(width: 0, height: 0))
    }
}

/// Whose a drag is: the fader's, when the finger moves along its axis, or whatever is behind it
/// (the row of strips that scrolls), when it moves across.
@Suite("Layout rules: drag direction")
struct DragDirectionRuleTests {
    /// A drag that has moved `along` and `across` the axis, with no velocity to fall back on.
    private func begins(along: Double, across: Double) -> Bool {
        LayoutRules.dragBegins(movedAlong: along, movedAcross: across, velocityAlong: 0, velocityAcross: 0)
    }

    @Test func aDragAlongTheAxisBegins() {
        #expect(begins(along: 10, across: 2))
        // Up a vertical fader or left along a horizontal one: the sign says nothing about the axis.
        #expect(begins(along: -10, across: 2))
        #expect(begins(along: 10, across: -2))
    }

    @Test func aDragAcrossTheAxisDeclines() {
        #expect(!begins(along: 2, across: 10))
        #expect(!begins(along: -2, across: -10))
    }

    @Test func anExactDiagonalDeclines() {
        #expect(!begins(along: 5, across: 5))
        #expect(!begins(along: 5, across: -5))
        #expect(
            !LayoutRules.dragBegins(movedAlong: 0, movedAcross: 0, velocityAlong: 80, velocityAcross: -80))
    }

    @Test func noMovementAtAllDeclines() {
        #expect(!LayoutRules.dragBegins(movedAlong: 0, movedAcross: 0, velocityAlong: 0, velocityAcross: 0))
    }

    @Test func theDistanceMovedDecidesOverTheVelocity() {
        #expect(
            !LayoutRules.dragBegins(movedAlong: 1, movedAcross: 5, velocityAlong: 400, velocityAcross: 0))
        #expect(
            LayoutRules.dragBegins(movedAlong: 5, movedAcross: 1, velocityAlong: 0, velocityAcross: 400))
        // Moved only across: the finger has moved, so its velocity along the axis does not count.
        #expect(
            !LayoutRules.dragBegins(movedAlong: 0, movedAcross: 3, velocityAlong: 400, velocityAcross: 0))
    }

    @Test func beforeTheFingerMovesTheVelocityDecides() {
        #expect(
            LayoutRules.dragBegins(movedAlong: 0, movedAcross: 0, velocityAlong: 50, velocityAcross: 10))
        #expect(
            !LayoutRules.dragBegins(movedAlong: 0, movedAcross: 0, velocityAlong: 10, velocityAcross: 50))
    }
}

/// How tall a modal panel's content is drawn: as tall as it would like, up to the panel's cap and
/// the height offered.
@Suite("Layout rules: panel content height")
struct PanelContentHeightRuleTests {
    @Test func theBottomSheetMayTakeThreeQuartersOfTheSpace() {
        #expect(LayoutRules.panelContentMaxHeight(spaceHeight: 800, isLandscape: false) == 600)
    }

    @Test func theSidePanelHasNoCapOfItsOwn() {
        #expect(LayoutRules.panelContentMaxHeight(spaceHeight: 390, isLandscape: true) == .infinity)
    }

    @Test func aShortChildHugsItsContent() {
        #expect(LayoutRules.panelContentHeight(ideal: 200, maxHeight: 600, proposed: 800) == 200)
    }

    @Test func aTallChildStopsAtTheCap() {
        #expect(LayoutRules.panelContentHeight(ideal: 1000, maxHeight: 600, proposed: 800) == 600)
    }

    @Test func aTallChildStopsAtTheHeightOfferedWhenThatIsLess() {
        #expect(LayoutRules.panelContentHeight(ideal: 1000, maxHeight: 600, proposed: 400) == 400)
        // The side panel: no cap, so only the height offered stops it.
        #expect(LayoutRules.panelContentHeight(ideal: 1000, maxHeight: .infinity, proposed: 390) == 390)
    }

    @Test func withNoHeightProposedTheChildAndTheCapDecide() {
        #expect(LayoutRules.panelContentHeight(ideal: 300, maxHeight: 600, proposed: nil) == 300)
        #expect(LayoutRules.panelContentHeight(ideal: 900, maxHeight: 600, proposed: nil) == 600)
        #expect(LayoutRules.panelContentHeight(ideal: 900, maxHeight: .infinity, proposed: nil) == 900)
    }
}

/// How tall the Mix screen's console is: what the parts above it leave of the space, but never less
/// than its least height, so a short window scrolls rather than leaving the faders no travel.
@Suite("Layout rules: console height")
struct ConsoleHeightRuleTests {
    @Test func theConsoleFillsWhatThePartsAboveLeave() {
        #expect(LayoutRules.consoleHeight(spaceHeight: 360, topHeight: 52, least: 240) == 308)
    }

    @Test func theConsoleKeepsItsLeastHeightWhenTheSpaceIsShort() {
        // Two banners under a landscape header: the screen scrolls instead.
        #expect(LayoutRules.consoleHeight(spaceHeight: 360, topHeight: 186, least: 240) == 240)
    }

    @Test func exactlyTheLeastHeightLeftIsKept() {
        #expect(LayoutRules.consoleHeight(spaceHeight: 360, topHeight: 120, least: 240) == 240)
    }

    @Test func partsTallerThanTheSpaceStillLeaveTheLeastHeight() {
        #expect(LayoutRules.consoleHeight(spaceHeight: 200, topHeight: 260, least: 240) == 240)
    }
}

/// Whether letting go of a drag down a portrait bottom sheet's top closes it, or the sheet springs
/// back.
@Suite("Layout rules: bottom sheet drag")
struct SheetDragRuleTests {
    @Test func aLongPullCloses() {
        #expect(LayoutRules.sheetDragCloses(predictedPull: 180))
    }

    @Test func aShortPullSpringsBack() {
        #expect(!LayoutRules.sheetDragCloses(predictedPull: 40))
    }

    @Test func thePullMustPassTheCloseDistance() {
        #expect(!LayoutRules.sheetDragCloses(predictedPull: LayoutRules.sheetCloseDistance))
        #expect(LayoutRules.sheetDragCloses(predictedPull: LayoutRules.sheetCloseDistance + 1))
    }

    @Test func aDragPushedBackUpSpringsBack() {
        #expect(!LayoutRules.sheetDragCloses(predictedPull: -60))
        #expect(!LayoutRules.sheetDragCloses(predictedPull: 0))
    }
}
