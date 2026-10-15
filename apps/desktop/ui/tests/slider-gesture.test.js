import test from "node:test";
import assert from "node:assert/strict";

import { SliderGesture } from "../app/lib/slider-gesture.js";
import { FADER, PAN, snapToCentre } from "../app/lib/values.js";

function fader(value) {
  return new SliderGesture({ range: FADER, value });
}

const input = (value) => ({ type: "input", value });
const change = (value) => ({ type: "change", value });

test("a press on the track jumps there, a drag follows, and one change goes out at the end", () => {
  const gesture = fader(50);

  assert.deepEqual(gesture.press(0.8, { onThumb: false }), [input(80)]);
  assert.equal(gesture.dragging, true);
  assert.deepEqual(gesture.move(0.85), [input(85)]);
  assert.deepEqual(gesture.move(0.851), [], "no event when the value does not change");
  assert.deepEqual(gesture.move(0.9), [input(90)]);
  assert.deepEqual(gesture.release(), [change(90)]);
  assert.equal(gesture.value, 90);
  assert.equal(gesture.dragging, false);
});

test("grabbing the thumb keeps the grab point, so the value never jumps under the pointer", () => {
  const gesture = fader(50);

  assert.deepEqual(gesture.press(0.53, { onThumb: true }), []);
  assert.deepEqual(gesture.move(0.63), [input(60)]);
  assert.deepEqual(gesture.release(), [change(60)]);
});

test("a drag that ends where it started reports no change", () => {
  const gesture = fader(50);

  gesture.press(0.5, { onThumb: true });
  gesture.move(0.7);
  gesture.move(0.5);

  assert.deepEqual(gesture.release(), []);
});

test("moving or releasing without a press does nothing", () => {
  const gesture = fader(50);

  assert.deepEqual(gesture.move(0.9), []);
  assert.deepEqual(gesture.release(), []);
  assert.equal(gesture.value, 50);
});

test("a value set from outside during a drag waits, and shows before the drag's change goes out", () => {
  const gesture = fader(50);
  gesture.press(0.6, { onThumb: false });

  assert.equal(gesture.set(20), false, "the pointer wins while it drags");
  assert.equal(gesture.value, 60);
  gesture.move(0.7);
  assert.deepEqual(gesture.release(), [change(70)], "the change is the dragged value");
  assert.equal(gesture.value, 20, "the value set during the drag shows once it has ended");
  assert.equal(gesture.set(70), true, "after the drag, a value from outside shows at once");
  assert.equal(gesture.value, 70);
});

test("a value set from outside is clamped, and setting the same value changes nothing", () => {
  const gesture = fader(50);

  assert.equal(gesture.set(140), true);
  assert.equal(gesture.value, 100);
  assert.equal(gesture.set(100), false);
});

test("each key step is an input and a change of its own", () => {
  const gesture = fader(62);

  assert.deepEqual(gesture.key("ArrowUp"), [input(63), change(63)]);
  assert.deepEqual(gesture.key("PageDown"), [input(53), change(53)]);
  assert.deepEqual(gesture.key("Home"), [input(0), change(0)]);
  assert.deepEqual(gesture.key("End"), [input(100), change(100)]);
  assert.deepEqual(gesture.key("ArrowUp"), [], "at the top a step changes nothing");
  assert.equal(gesture.key("Enter"), null, "a key the slider does not use is left to the page");
});

test("a double-click resets to the default, and is quiet when the value is already there", () => {
  const gesture = fader(40);

  assert.deepEqual(gesture.reset(100), [input(100), change(100)]);
  assert.deepEqual(gesture.reset(100), []);
});

test("a pan drag lands on the centre within the detent, and the keyboard stops there", () => {
  const gesture = new SliderGesture({
    range: PAN,
    value: 30,
    snap: (value) => snapToCentre(value, PAN),
  });

  assert.deepEqual(gesture.press(0.51, { onThumb: false }), [input(50)]);
  assert.deepEqual(gesture.move(0.53), [input(53)], "outside the detent the drag is free");
  assert.deepEqual(gesture.release(), [change(53)]);
  gesture.set(45);
  assert.deepEqual(gesture.key("PageUp"), [input(50), change(50)]);
});
