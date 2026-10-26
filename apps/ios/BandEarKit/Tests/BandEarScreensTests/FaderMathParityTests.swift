import BandEarScreens
import Testing

/// Android compares positions to 0.001 px (`assertEquals(expected, actual, 0.001f)`).
private func isClose(_ actual: Double, _ expected: Double) -> Bool {
    abs(actual - expected) <= 0.001
}

/// A 24 px thumb on a 224 px fader: the thumb's edge travels 200 px, two pixels a step
/// (`FaderGestureTest.kt:10-11`).
private func laidOut(length: Double = 224) -> FaderGesture {
    var gesture = FaderGesture(thumbLength: 24)
    gesture.length = length
    return gesture
}

/// The same 24 px thumb laid left to right on a 224 px fader (`HorizontalFaderGestureTest.kt:10-11`).
private func laidOutHorizontally(width: Double = 224) -> HorizontalFaderGesture {
    var gesture = HorizontalFaderGesture(thumbWidth: 24)
    gesture.width = width
    return gesture
}

/// A 24 px thumb on a 224 px pan control, so the thumb's edge travels the same 200 px
/// (`PanScaleTest.kt:104-105`).
private func panGesture() -> HorizontalFaderGesture {
    var gesture = HorizontalFaderGesture(thumbWidth: 24, scale: .pan)
    gesture.width = 224
    return gesture
}

/// The Android client's `FaderMathTest` (`apps/android/app/src/test/java/com/bandear/android/ui/
/// components/FaderMathTest.kt`): where a point on a vertical track reads, and where a level sits.
@Suite("Fader maths: Android parity")
struct FaderMathParityTests {

    /// `FaderMathTest.kt:7-11`
    @Test func topOfTheTrackIsFullAndBottomIsSilent() {
        #expect(FaderMath.value(at: 0, trackHeight: 200) == 100)
        #expect(FaderMath.value(at: 200, trackHeight: 200) == 0)
    }

    /// `FaderMathTest.kt:13-18`
    @Test func valueFollowsThePointerDownTheTrack() {
        #expect(FaderMath.value(at: 50, trackHeight: 200) == 75)
        #expect(FaderMath.value(at: 100, trackHeight: 200) == 50)
        #expect(FaderMath.value(at: 150, trackHeight: 200) == 25)
    }

    /// `FaderMathTest.kt:20-25`
    @Test func valueRoundsToTheNearestStep() {
        // 0.4 of a step below 50 still reads 50; 0.6 of a step reads 49.
        #expect(FaderMath.value(at: 100.8, trackHeight: 200) == 50)
        #expect(FaderMath.value(at: 101.2, trackHeight: 200) == 49)
    }

    /// `FaderMathTest.kt:27-31`
    @Test func pointerOutsideTheTrackClampsToTheNearestEnd() {
        #expect(FaderMath.value(at: -30, trackHeight: 200) == 100)
        #expect(FaderMath.value(at: 260, trackHeight: 200) == 0)
    }

    /// `FaderMathTest.kt:33-37`
    @Test func trackWithNoHeightReadsAsTheBottom() {
        #expect(FaderMath.value(at: 0, trackHeight: 0) == 0)
        #expect(FaderMath.value(at: 12, trackHeight: -4) == 0)
    }

    /// `FaderMathTest.kt:39-44`
    @Test func offsetPlacesTheThumbFromTheTop() {
        #expect(isClose(FaderMath.offset(of: 100, trackHeight: 200), 0))
        #expect(isClose(FaderMath.offset(of: 25, trackHeight: 200), 150))
        #expect(isClose(FaderMath.offset(of: 0, trackHeight: 200), 200))
    }

    /// `FaderMathTest.kt:46-50`
    @Test func offsetClampsValuesOutsideTheRange() {
        #expect(isClose(FaderMath.offset(of: 140, trackHeight: 200), 0))
        #expect(isClose(FaderMath.offset(of: -5, trackHeight: 200), 200))
    }

    /// `FaderMathTest.kt:52-59`
    @Test func offsetIsTheInverseOfTheValueForEveryStep() {
        // An odd height, as a real track in pixels would be, so the rounding is exercised.
        let trackHeight = 212.0
        for value in 0...FaderMath.max {
            let offset = FaderMath.offset(of: value, trackHeight: trackHeight)
            #expect(FaderMath.value(at: offset, trackHeight: trackHeight) == value, "value \(value)")
        }
    }
}

/// The Android client's `FaderGestureTest` (`apps/android/app/src/test/java/com/bandear/android/ui/
/// components/FaderGestureTest.kt`): a vertical fader's taps, drags and screen-reader steps.
@Suite("Fader gesture: Android parity")
struct FaderGestureParityTests {

    /// `FaderGestureTest.kt:13-20`
    @Test func aTapPutsTheThumbCentreUnderTheFinger() {
        let gesture = laidOut()
        // The thumb centre sits half a thumb below its edge, so a tap 12 px down is the very top.
        #expect(gesture.tap(at: 12, current: 50) == 100)
        #expect(gesture.tap(at: 62, current: 50) == 75)
        #expect(gesture.tap(at: 212, current: 50) == 0)
    }

    /// `FaderGestureTest.kt:22-27`
    @Test func aTapPastEitherEndReadsAsThatEnd() {
        let gesture = laidOut()
        #expect(gesture.tap(at: 0, current: 50) == 100)
        #expect(gesture.tap(at: 224, current: 50) == 0)
    }

    /// `FaderGestureTest.kt:29-32`
    @Test func aTapOnTheCurrentLevelReportsNothing() {
        #expect(laidOut().tap(at: 112, current: 50) == nil)
    }

    /// `FaderGestureTest.kt:34-42`
    @Test func aDragThatStartsAtAnyLevelWithZeroDeltaReportsNothing() {
        // An odd length, as a real fader in pixels would be, so the seed's rounding is exercised.
        var gesture = laidOut(length: 236.5)
        for value in 0...FaderMath.max {
            gesture.startDrag(current: value)
            #expect(gesture.drag(by: 0, current: value) == nil, "level \(value)")
        }
    }

    /// `FaderGestureTest.kt:44-51`
    @Test func aDragMovesTheLevelFromWhereItWasNotFromWhereTheFingerLanded() {
        var gesture = laidOut()
        gesture.startDrag(current: 50)
        #expect(gesture.drag(by: 20, current: 50) == 40)
        #expect(gesture.drag(by: 20, current: 40) == 30)
        #expect(gesture.drag(by: -50, current: 30) == 55)
    }

    /// `FaderGestureTest.kt:53-60`
    @Test func aDragKeepsFollowingTheFingerWhenTheLevelWasNotApplied() {
        var gesture = laidOut()
        gesture.startDrag(current: 50)
        #expect(gesture.drag(by: 20, current: 50) == 40)
        // The caller kept 50; the thumb still follows the finger, not the refused level.
        #expect(gesture.drag(by: 20, current: 50) == 30)
    }

    /// `FaderGestureTest.kt:62-68`
    @Test func aDragSmallerThanHalfAStepReportsNothing() {
        var gesture = laidOut()
        gesture.startDrag(current: 50)
        #expect(gesture.drag(by: 0.8, current: 50) == nil)
        #expect(gesture.drag(by: 0.4, current: 50) == 49)
    }

    /// `FaderGestureTest.kt:70-76`
    @Test func aDragPastEitherEndClampsToThatEnd() {
        var gesture = laidOut()
        gesture.startDrag(current: 90)
        #expect(gesture.drag(by: -500, current: 90) == FaderMath.max)
        #expect(gesture.drag(by: 1000, current: 100) == 0)
    }

    /// `FaderGestureTest.kt:78-83`
    @Test func aStepRoundsToAWholeLevel() {
        let gesture = laidOut()
        #expect(gesture.step(to: 42.6, current: 10) == 43)
        #expect(gesture.step(to: 42.4, current: 10) == 42)
    }

    /// `FaderGestureTest.kt:85-90`
    @Test func aStepOutsideTheRangeClampsToTheNearestEnd() {
        let gesture = laidOut()
        #expect(gesture.step(to: 140, current: 10) == FaderMath.max)
        #expect(gesture.step(to: -3, current: 10) == 0)
    }

    /// `FaderGestureTest.kt:92-97`
    @Test func aStepToTheCurrentLevelOrToNoNumberReportsNothing() {
        let gesture = laidOut()
        #expect(gesture.step(to: 50.2, current: 50) == nil)
        #expect(gesture.step(to: .nan, current: 50) == nil)
    }

    /// `FaderGestureTest.kt:99-111`
    @Test func zeroTravelReportsNothing() {
        // A fader laid out no taller than its thumb has nowhere to move it: no input may silence it.
        for length in [0.0, 10, 24] {
            var gesture = laidOut(length: length)
            #expect(!gesture.acceptsInput, "length \(length)")
            #expect(gesture.tap(at: 200, current: 80) == nil)
            gesture.startDrag(current: 80)
            #expect(gesture.drag(by: 0, current: 80) == nil)
            #expect(gesture.drag(by: 30, current: 80) == nil)
            #expect(gesture.step(to: 0, current: 80) == nil)
        }
    }

    /// `FaderGestureTest.kt:113-116`
    @Test func aFaderTallerThanItsThumbAcceptsInput() {
        #expect(laidOut(length: 25).acceptsInput)
    }

    /// `FaderGestureTest.kt:118-123`
    @Test func beforeLayoutTheFaderAcceptsNoInput() {
        let gesture = FaderGesture(thumbLength: 24)
        #expect(!gesture.acceptsInput)
        #expect(gesture.tap(at: 0, current: 30) == nil)
    }
}

/// The Android client's `HorizontalFaderGestureTest` (`apps/android/app/src/test/java/com/bandear/
/// android/ui/components/HorizontalFaderGestureTest.kt`): the same gesture laid left to right.
@Suite("Horizontal fader gesture: Android parity")
struct HorizontalFaderGestureParityTests {

    /// `HorizontalFaderGestureTest.kt:13-19`
    @Test func aRightwardDragRaisesTheLevel() {
        var gesture = laidOutHorizontally()
        gesture.startDrag(current: 50)
        #expect(gesture.drag(byX: 20, current: 50) == 60)
        #expect(gesture.drag(byX: 20, current: 60) == 70)
    }

    /// `HorizontalFaderGestureTest.kt:21-26`
    @Test func aLeftwardDragLowersTheLevel() {
        var gesture = laidOutHorizontally()
        gesture.startDrag(current: 50)
        #expect(gesture.drag(byX: -20, current: 50) == 40)
    }

    /// `HorizontalFaderGestureTest.kt:28-33`
    @Test func aDragPastTheRightEndReadsAsTheTop() {
        var gesture = laidOutHorizontally()
        gesture.startDrag(current: 90)
        #expect(gesture.drag(byX: 500, current: 90) == FaderMath.max)
    }

    /// `HorizontalFaderGestureTest.kt:35-42`
    @Test func aTapNearTheRightEndRaisesTheLevel() {
        let gesture = laidOutHorizontally()
        // 4 px from the right edge is inside the thumb's half-width at the loud end: the top.
        #expect(gesture.tap(atX: 220, current: 50) == FaderMath.max)
        // 62 px from the right edge, as a tap 62 px down a vertical fader of the same length.
        #expect(gesture.tap(atX: 162, current: 50) == 75)
    }

    /// `HorizontalFaderGestureTest.kt:44-49`
    @Test func aTapNearTheLeftEndLowersTheLevel() {
        let gesture = laidOutHorizontally()
        #expect(gesture.tap(atX: 4, current: 50) == 0)
        #expect(gesture.tap(atX: 62, current: 50) == 25)
    }

    /// `HorizontalFaderGestureTest.kt:51-54`
    @Test func aTapInTheMiddleOnTheCurrentLevelReportsNothing() {
        #expect(laidOutHorizontally().tap(atX: 112, current: 50) == nil)
    }

    /// `HorizontalFaderGestureTest.kt:56-59`
    @Test func aScreenReaderStepIsTheSameOnEitherAxis() {
        #expect(laidOutHorizontally().step(to: 42.6, current: 10) == 43)
    }

    /// `HorizontalFaderGestureTest.kt:61-69`
    @Test func beforeLayoutItIgnoresInput() {
        var gesture = HorizontalFaderGesture(thumbWidth: 24)
        #expect(!gesture.acceptsInput)
        #expect(gesture.tap(atX: 200, current: 30) == nil)
        gesture.startDrag(current: 30)
        #expect(gesture.drag(byX: 40, current: 30) == nil)
        #expect(laidOutHorizontally().acceptsInput)
    }
}

/// The pan control's gesture cases of the Android client's `PanScaleTest` (`apps/android/app/src/
/// test/java/com/bandear/android/ui/components/PanScaleTest.kt`, lines 104-142): the horizontal
/// gesture over the pan's scale, with its centre detent. The scale's own cases are in
/// `PanScaleParityTests`.
@Suite("Pan gesture: Android parity")
struct PanGestureParityTests {

    /// `PanScaleTest.kt:107-112`
    @Test func aTapNearTheMiddleLandsOnTheCentre() {
        let gesture = panGesture()
        #expect(gesture.tap(atX: 112, current: 30) == PanScale.centre)
        #expect(gesture.tap(atX: 121, current: 80) == PanScale.centre)
    }

    /// `PanScaleTest.kt:114-119`
    @Test func aTapAtEitherEndIsHardLeftOrHardRight() {
        let gesture = panGesture()
        #expect(gesture.tap(atX: 4, current: PanScale.centre) == PanScale.left)
        #expect(gesture.tap(atX: 220, current: PanScale.centre) == PanScale.right)
    }

    /// `PanScaleTest.kt:121-128`
    @Test func aDragFromTheCentreHoldsThereUntilItLeavesTheDetent() {
        var gesture = panGesture()
        gesture.startDrag(current: PanScale.centre)

        #expect(gesture.drag(byX: 8, current: PanScale.centre) == nil, "still inside the detent")
        #expect(gesture.drag(byX: 4, current: PanScale.centre) == 51)
    }

    /// `PanScaleTest.kt:130-137`
    @Test func aDragAcrossTheMiddleStopsOnTheCentre() {
        var gesture = panGesture()
        gesture.startDrag(current: 40)

        // From 18 px left of the detent, 28 px to the right lands 10 px inside it.
        #expect(gesture.drag(byX: 28, current: 40) == PanScale.centre)
    }

    /// `PanScaleTest.kt:139-142`
    @Test func aScreenReaderStepIgnoresTheDetent() {
        #expect(panGesture().step(to: 48, current: PanScale.centre) == 48)
    }
}

/// The Android client's `FaderMarksTest` (`apps/android/app/src/test/java/com/bandear/android/ui/
/// components/FaderMarksTest.kt`): where a fader draws its cap, its ticks and the engineer's limit,
/// so a touch on what is drawn reads back as what it shows.
@Suite("Fader marks: Android parity")
struct FaderMarksParityTests {
    /// A 214 px fader with a 14 px cap: the cap's centre travels 200 px, from 7 px to 207 px
    /// (`FaderMarksTest.kt:9-10`).
    private let length = 214.0
    private let cap = 14.0

    /// `FaderMarksTest.kt:12-17`
    @Test func theCapsCentreRunsFromHalfACapBelowTheTopToHalfACapAboveTheBottom() {
        #expect(isClose(FaderMath.centre(of: FaderMath.max, length: length, cap: cap), 7))
        #expect(isClose(FaderMath.centre(of: 0, length: length, cap: cap), 207))
        #expect(isClose(FaderMath.centre(of: 75, length: length, cap: cap), 57))
    }

    /// `FaderMarksTest.kt:19-23`
    @Test func aLevelOutsideTheRangeIsDrawnAtTheNearestEnd() {
        #expect(isClose(FaderMath.centre(of: 140, length: length, cap: cap), 7))
        #expect(isClose(FaderMath.centre(of: -5, length: length, cap: cap), 207))
    }

    /// `FaderMarksTest.kt:25-28`
    @Test func aFaderNoTallerThanItsCapKeepsTheCapAtTheTop() {
        #expect(isClose(FaderMath.centre(of: 30, length: 10, cap: cap), 7))
    }

    /// `FaderMarksTest.kt:30-36`
    @Test func aTapOnTheDrawnCapReadsBackAsItsLevel() {
        var gesture = FaderGesture(thumbLength: cap)
        gesture.length = length
        for level in 0...FaderMath.max {
            let centre = FaderMath.centre(of: level, length: length, cap: cap)
            #expect(gesture.tap(at: centre, current: -1) == level, "level \(level)")
        }
    }

    /// `FaderMarksTest.kt:38-42`
    @Test func theTicksMarkTheTopEveryQuarterAndTheBottom() {
        #expect(FaderMath.tickLevels == [100, 75, 50, 25, 0])
        #expect(FaderMath.tickCentres(length: length, cap: cap) == [7, 57, 107, 157, 207])
    }

    /// `FaderMarksTest.kt:44-47`
    @Test func noLimitDrawsNoMarker() {
        #expect(FaderMath.limitCentre(nil, length: length, cap: cap) == nil)
    }

    /// `FaderMarksTest.kt:49-53`
    @Test func theLimitMarkerSitsWhereTheCapsCentreWouldBeAtThatLevel() {
        #expect(isClose(FaderMath.limitCentre(85, length: length, cap: cap) ?? .nan, 37))
        #expect(
            isClose(
                FaderMath.limitCentre(40, length: length, cap: cap) ?? .nan,
                FaderMath.centre(of: 40, length: length, cap: cap)))
    }

    /// `FaderMarksTest.kt:55-59`
    @Test func aLimitOutsideTheRangeIsDrawnAtTheNearestEnd() {
        #expect(isClose(FaderMath.limitCentre(120, length: length, cap: cap) ?? .nan, 7))
        #expect(isClose(FaderMath.limitCentre(-1, length: length, cap: cap) ?? .nan, 207))
    }
}

/// The drawn-thumb cases of the Android client's `PanScaleTest` (`PanScaleTest.kt`, lines 144-169):
/// where the strip pan draws its dot, so a touch on it reads back as its pan.
@Suite("Pan marks: Android parity")
struct PanMarksParityTests {

    /// `PanScaleTest.kt:144-150`
    @Test func theThumbsCentreRunsFromHalfAThumbInToHalfAThumbFromTheRightEnd() {
        // A 10 px dot on a 210 px strip pan: its centre travels 200 px, from 5 px to 205 px.
        #expect(isClose(PanScale.thumbCentre(of: PanScale.left, length: 210, thumb: 10), 5))
        #expect(isClose(PanScale.thumbCentre(of: PanScale.right, length: 210, thumb: 10), 205))
        #expect(isClose(PanScale.thumbCentre(of: 49, length: 210, thumb: 10), 5 + 90 - 1.8))
    }

    /// `PanScaleTest.kt:152-156`
    @Test func theCentreIsDrawnInTheMiddleOfTheControl() {
        #expect(isClose(PanScale.thumbCentre(of: PanScale.centre, length: 210, thumb: 10), 105))
        #expect(isClose(PanScale.thumbCentre(of: PanScale.centre, length: 74, thumb: 10), 37))
    }

    /// `PanScaleTest.kt:158-161`
    @Test func aControlNoWiderThanItsThumbKeepsTheThumbAtTheStart() {
        #expect(isClose(PanScale.thumbCentre(of: PanScale.right, length: 8, thumb: 10), 5))
    }

    /// `PanScaleTest.kt:163-169`
    @Test func aTapOnTheDrawnThumbReadsBackAsItsPan() {
        var gesture = HorizontalFaderGesture(thumbWidth: 10, scale: .pan)
        gesture.width = 210
        for pan in PanScale.left...PanScale.right {
            let centre = PanScale.thumbCentre(of: pan, length: 210, thumb: 10)
            #expect(gesture.tap(atX: centre, current: -1) == pan, "pan \(pan)")
        }
    }
}

/// What iOS adds to the ported gesture: the size of a VoiceOver swipe, and inputs the platform can
/// hand over that Android's arithmetic never meets.
@Suite("Fader gesture: iOS rules")
struct FaderGestureIOSTests {

    /// Compose's slider semantics move a `0..100` range a twentieth per screen-reader swipe
    /// (`AccessibilitySliderStepsCount`), so a VoiceOver swipe moves a level or a pan by 5.
    @Test func aScreenReaderSwipeMovesFiveEitherWay() {
        #expect(FaderMath.screenReaderStep == 5)
        let gesture = laidOut()
        #expect(gesture.increment(current: 72) == 77)
        #expect(gesture.decrement(current: 72) == 67)
        let pan = panGesture()
        #expect(pan.increment(current: PanScale.centre) == 55)
        #expect(pan.decrement(current: PanScale.centre) == 45)
    }

    /// A swipe past an end stops there, and a swipe at an end, or before layout, reports nothing.
    @Test func aScreenReaderSwipeStopsAtTheEnds() {
        let gesture = laidOut()
        #expect(gesture.increment(current: 98) == FaderMath.max)
        #expect(gesture.increment(current: FaderMath.max) == nil)
        #expect(gesture.decrement(current: 3) == 0)
        #expect(gesture.decrement(current: 0) == nil)
        #expect(FaderGesture(thumbLength: 24).increment(current: 50) == nil)
        #expect(HorizontalFaderGesture(thumbWidth: 24).decrement(current: 50) == nil)
    }

    /// A touch that is not a number, or a drag of no finite length, reports nothing and leaves the
    /// drag where it was, instead of reading as an end (Kotlin would throw on `NaN`).
    @Test func aNonFiniteTouchReportsNothing() {
        var gesture = laidOut()
        #expect(gesture.tap(at: .nan, current: 50) == nil)
        #expect(gesture.tap(at: .infinity, current: 50) == nil)
        gesture.startDrag(current: 50)
        #expect(gesture.drag(by: .nan, current: 50) == nil)
        #expect(gesture.drag(by: -.infinity, current: 50) == nil)
        #expect(gesture.drag(by: 20, current: 50) == 40)
        #expect(FaderMath.value(at: .nan, trackHeight: 200) == 0)
        #expect(FaderMath.value(at: -1e300, trackHeight: 200) == FaderMath.max)
        #expect(gesture.step(to: .infinity, current: 50) == FaderMath.max)
    }
}
