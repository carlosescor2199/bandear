import test from "node:test";
import assert from "node:assert/strict";

import {
  FADER,
  PAN,
  clampLevel,
  groupTone,
  keyValue,
  levelFraction,
  levelFromFraction,
  levelToDb,
  meterFraction,
  meterZone,
  panLabel,
  panValueText,
  pointerFraction,
  snapToCentre,
} from "../app/lib/values.js";

test("levels are whole numbers from 0 to 100", () => {
  assert.equal(clampLevel(62.4), 62);
  assert.equal(clampLevel(62.5), 63);
  assert.equal(clampLevel(-3), 0);
  assert.equal(clampLevel(140), 100);
  assert.equal(clampLevel(Number.NaN), 0);
  assert.equal(clampLevel(undefined), 0);
});

test("a fader's position and its percent map onto each other in a straight line", () => {
  for (let percent = 0; percent <= 100; percent += 1) {
    assert.equal(levelFromFraction(levelFraction(percent)), percent);
  }
  assert.equal(levelFraction(62), 0.62);
  assert.equal(levelFromFraction(0.624), 62);
  assert.equal(levelFromFraction(1.2), 100);
  assert.equal(levelFromFraction(-0.1), 0);
});

test("a vertical pointer position counts from the bottom of the track", () => {
  const vertical = { reverse: true };

  assert.equal(pointerFraction(300, 100, 200, vertical), 0);
  assert.equal(pointerFraction(100, 100, 200, vertical), 1);
  assert.equal(pointerFraction(200, 100, 200, vertical), 0.5);
  assert.equal(pointerFraction(20, 100, 200, vertical), 1, "above the track is the top");
  assert.equal(pointerFraction(150, 100, 200), 0.25, "horizontal counts from the start");
  assert.equal(pointerFraction(150, 100, 0), 0, "a track with no length has no position");
});

test("the keyboard moves a fader by a step, a page or to either end", () => {
  const at = (key, value) => keyValue(key, value, FADER);

  assert.equal(at("ArrowUp", 62), 63);
  assert.equal(at("ArrowRight", 62), 63);
  assert.equal(at("ArrowDown", 62), 61);
  assert.equal(at("ArrowLeft", 62), 61);
  assert.equal(at("PageUp", 62), 72);
  assert.equal(at("PageDown", 62), 52);
  assert.equal(at("Home", 62), 0);
  assert.equal(at("End", 62), 100);
  assert.equal(at("ArrowUp", 100), 100);
  assert.equal(at("PageDown", 4), 0);
  assert.equal(at("Enter", 62), null, "keys a fader does not use are left alone");
  assert.equal(at("a", 62), null);
});

test("the keyboard stops a pan at the centre on the way across", () => {
  const at = (key, value) => keyValue(key, value, PAN);

  assert.equal(at("PageUp", 45), 50);
  assert.equal(at("PageUp", 50), 60);
  assert.equal(at("PageDown", 55), 50);
  assert.equal(at("ArrowRight", 49), 50);
  assert.equal(at("ArrowLeft", 50), 49);
  assert.equal(at("Home", 70), 0);
  assert.equal(at("End", 30), 100);
});

test("a dragged pan near the centre snaps to it", () => {
  assert.equal(snapToCentre(52, PAN), 50);
  assert.equal(snapToCentre(48, PAN), 50);
  assert.equal(snapToCentre(53, PAN), 53);
  assert.equal(snapToCentre(47, PAN), 47);
  assert.equal(snapToCentre(0, PAN), 0);
});

test("a pan reads as L, C or R and how far", () => {
  assert.equal(panLabel(50), "C");
  assert.equal(panLabel(40), "L20");
  assert.equal(panLabel(65), "R30");
  assert.equal(panLabel(0), "L100");
  assert.equal(panLabel(100), "R100");
  assert.equal(panLabel(51), "R2");
});

test("a pan's spoken value names the side in words", () => {
  assert.equal(panValueText(50), "Centre");
  assert.equal(panValueText(40), "20% left");
  assert.equal(panValueText(65), "30% right");
});

test("a meter level converts to decibels below full scale", () => {
  assert.equal(levelToDb(100), 0);
  assert.ok(Math.abs(levelToDb(50) - -6.0206) < 1e-3);
  assert.equal(levelToDb(0), -Infinity);
  assert.equal(levelToDb(-4), -Infinity);
});

test("a meter bar spans 60 dB, so quiet signals still show", () => {
  assert.equal(meterFraction(0), 0);
  assert.equal(meterFraction(100), 1);
  assert.equal(meterFraction(150), 1);
  assert.ok(Math.abs(meterFraction(50) - 0.8997) < 1e-3);
  assert.ok(Math.abs(meterFraction(1) - 1 / 3) < 1e-3);
  assert.equal(meterFraction(0.05), 0, "below the 60 dB floor the bar is empty");
});

test("a meter is hot from 6 dB below full scale and clips at full scale", () => {
  assert.equal(meterZone(0), "safe");
  assert.equal(meterZone(50), "safe");
  assert.equal(meterZone(51), "hot");
  assert.equal(meterZone(99), "hot");
  assert.equal(meterZone(100), "clip");
});

test("groups take the chip tones in turn, and no group is neutral", () => {
  assert.deepEqual(
    [0, 1, 2, 3, 4].map((index) => groupTone(index)),
    ["teal", "blue", "violet", "orange", "teal"],
  );
  assert.equal(groupTone(null), "none");
  assert.equal(groupTone(-1), "none");
});
