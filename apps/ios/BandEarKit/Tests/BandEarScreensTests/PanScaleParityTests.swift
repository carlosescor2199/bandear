import BandEarScreens
import Testing

/// A 200 px travel, as in Android's test: the detent holds the middle 20 px (90 to 110) at the
/// centre, and each side maps 90 px onto 50 values, 1.8 px a step.
private let travel = 200.0

/// Android compares positions to 0.001 px (`assertEquals(expected, actual, 0.001f)`).
private func isClose(_ actual: Double, _ expected: Double) -> Bool {
    abs(actual - expected) <= 0.001
}

/// The Android client's `PanScaleTest` (`apps/android/app/src/test/java/com/bandear/android/ui/
/// components/PanScaleTest.kt`): the pan's words and its centre detent.
///
/// Its last five cases (`aTapNearTheMiddleLandsOnTheCentre` through
/// `aScreenReaderStepIgnoresTheDetent`, lines 104-142) drive Android's own fader gesture
/// (`HorizontalFaderGesture`); they are ported with that gesture, in `FaderMathParityTests`.
@Suite("Pan scale: Android parity")
struct PanScaleParityTests {

    /// `PanScaleTest.kt:15-21`
    @Test func theEndsAreHardLeftAndHardRight() {
        #expect(PanScale.value(at: 0, travel: travel) == PanScale.left)
        #expect(PanScale.value(at: travel, travel: travel) == PanScale.right)
        #expect(PanScale.value(at: -30, travel: travel) == PanScale.left)
        #expect(PanScale.value(at: 260, travel: travel) == PanScale.right)
    }

    /// `PanScaleTest.kt:23-28`
    @Test func theMiddleTenthOfTheTravelIsTheCentre() {
        #expect(PanScale.value(at: 100, travel: travel) == PanScale.centre)
        #expect(PanScale.value(at: 91, travel: travel) == PanScale.centre)
        #expect(PanScale.value(at: 109, travel: travel) == PanScale.centre)
    }

    /// `PanScaleTest.kt:30-34`
    @Test func justOutsideTheDetentThePanLeavesTheCentre() {
        #expect(PanScale.value(at: 88, travel: travel) == 49)
        #expect(PanScale.value(at: 112, travel: travel) == 51)
    }

    /// `PanScaleTest.kt:36-41`
    @Test func everyPanCanBeReachedAndReadsBackAsItself() {
        for pan in PanScale.left...PanScale.right {
            #expect(
                PanScale.value(at: PanScale.position(of: pan, travel: travel), travel: travel) == pan,
                "pan \(pan)")
        }
    }

    /// `PanScaleTest.kt:43-53`
    @Test func movingRightNeverMovesThePanLeft() {
        var previous = PanScale.left
        var position = 0.0
        while position <= travel {
            let pan = PanScale.value(at: position, travel: travel)
            #expect(pan >= previous, "at \(position) px the pan went from \(previous) to \(pan)")
            previous = pan
            position += 0.5
        }
    }

    /// `PanScaleTest.kt:55-63`
    @Test func theCentreIsDrawnInTheMiddleAndTheEndsAtTheEnds() {
        #expect(isClose(PanScale.position(of: PanScale.centre, travel: travel), 100))
        #expect(isClose(PanScale.position(of: PanScale.left, travel: travel), 0))
        #expect(isClose(PanScale.position(of: PanScale.right, travel: travel), travel))
        // Next to the centre the thumb sits at the detent's edge, so it snaps out of the middle.
        #expect(isClose(PanScale.position(of: 49, travel: travel), 90 - 1.8))
        #expect(isClose(PanScale.position(of: 51, travel: travel), 110 + 1.8))
    }

    /// `PanScaleTest.kt:65-69`
    @Test func aPanOutOfRangeIsDrawnAtTheNearestEnd() {
        #expect(isClose(PanScale.position(of: -10, travel: travel), 0))
        #expect(isClose(PanScale.position(of: 150, travel: travel), travel))
    }

    /// `PanScaleTest.kt:71-74`
    @Test func withNoTravelItReadsTheCentre() {
        #expect(PanScale.value(at: 0, travel: 0) == PanScale.centre)
    }

    /// `PanScaleTest.kt:76-79`
    @Test func theCentreReadsC() {
        #expect(PanScale.text(PanScale.centre) == "C")
    }

    /// `PanScaleTest.kt:81-89`
    @Test func aSideReadsHowFarTowardIt() {
        #expect(PanScale.text(35) == "L30")
        #expect(PanScale.text(60) == "R20")
        #expect(PanScale.text(49) == "L2")
        #expect(PanScale.text(51) == "R2")
        #expect(PanScale.text(PanScale.left) == "L100")
        #expect(PanScale.text(PanScale.right) == "R100")
    }

    /// `PanScaleTest.kt:91-95`
    @Test func aPanOutOfRangeReadsAsTheNearestEnd() {
        #expect(PanScale.text(-5) == "L100")
        #expect(PanScale.text(130) == "R100")
    }

    /// `PanScaleTest.kt:97-102`
    @Test func aScreenReaderHearsTheSideInWords() {
        #expect(PanScale.spokenText(PanScale.centre) == "Centre")
        #expect(PanScale.spokenText(35) == "Left 30")
        #expect(PanScale.spokenText(PanScale.right) == "Right 100")
    }
}
