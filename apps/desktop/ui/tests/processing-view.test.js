// The channel processing editor's pure logic (processing-view.js): the equaliser graph's scales and
// handles, the words every value is shown in, the sliders' positions, the compressor's static
// curve, the gain-reduction meters, the channel choices and the A/B bypass.
import test from "node:test";
import assert from "node:assert/strict";

import { RANGES, defaultStrip } from "../app/lib/processing.js";
import {
  BAND_NAMES,
  abBypass,
  abRestore,
  abSettle,
  bandHandles,
  compressorOutputDb,
  curveDescription,
  curvePath,
  curveSource,
  dragHandle,
  fieldPosition,
  fieldValue,
  formatDb,
  formatGainDb,
  formatHz,
  formatMs,
  formatQ,
  formatRatio,
  freqToX,
  gainToY,
  gridLines,
  handleKey,
  nudgeQ,
  presetOptions,
  reductionView,
  stepChoice,
  stripChoices,
  transferPath,
  withSetting,
  xToFreq,
  yToGain,
} from "../app/lib/processing-view.js";

const WIDTH = 600;
const HEIGHT = 200;

const near = (actual, expected, tolerance = 1e-6) =>
  assert.ok(Math.abs(actual - expected) <= tolerance, `${actual} is not ${expected}`);

test("the graph's frequency scale is logarithmic from 20 Hz to 20 kHz and reads back", () => {
  near(freqToX(20, WIDTH), 0);
  near(freqToX(20000, WIDTH), WIDTH);
  near(freqToX(632.4555, WIDTH), WIDTH / 2, 1e-3);
  near(xToFreq(WIDTH / 2, WIDTH), 632.4555, 1e-3);
  near(xToFreq(freqToX(2500, WIDTH), WIDTH), 2500, 1e-6);
  assert.equal(xToFreq(-50, WIDTH), 20, "held to the span");
  assert.equal(xToFreq(WIDTH + 50, WIDTH), 20000);
});

test("the graph's gain scale spans ±18 dB, +18 at the top, and reads back", () => {
  near(gainToY(18, HEIGHT), 0);
  near(gainToY(0, HEIGHT), HEIGHT / 2);
  near(gainToY(-18, HEIGHT), HEIGHT);
  near(yToGain(gainToY(4.5, HEIGHT), HEIGHT), 4.5);
  assert.equal(yToGain(-10, HEIGHT), 18);
  assert.equal(yToGain(HEIGHT + 10, HEIGHT), -18);
  assert.equal(gainToY(40, HEIGHT), 0, "a curve beyond the scale is drawn at its edge");
});

test("values are written in words the engineer reads", () => {
  assert.equal(formatHz(80), "80 Hz");
  assert.equal(formatHz(99.6), "100 Hz");
  assert.equal(formatHz(2500), "2.5 kHz");
  assert.equal(formatHz(1000), "1 kHz");
  assert.equal(formatHz(12500), "12.5 kHz");
  assert.equal(formatGainDb(3), "+3.0 dB");
  assert.equal(formatGainDb(-2), "−2.0 dB");
  assert.equal(formatGainDb(0), "0.0 dB");
  assert.equal(formatGainDb(-0.01), "0.0 dB", "no minus zero");
  assert.equal(formatDb(-18), "−18 dB");
  assert.equal(formatDb(-2.5), "−2.5 dB");
  assert.equal(formatDb(6), "6 dB");
  assert.equal(formatQ(0.7071), "0.71");
  assert.equal(formatQ(10), "10.0");
  assert.equal(formatMs(0.5), "0.5 ms");
  assert.equal(formatMs(150), "150 ms");
  assert.equal(formatMs(1500), "1.5 s");
  assert.equal(formatRatio(3), "3:1");
  assert.equal(formatRatio(2.5), "2.5:1");
});

test("a handle sits at its band's frequency and gain; the high-pass one on the 0 dB line", () => {
  const strip = defaultStrip();
  strip.eq.bands[1] = { ...strip.eq.bands[1], gainDb: 6 };
  const handles = bandHandles(strip.eq, WIDTH, HEIGHT);
  assert.deepEqual(
    handles.map((handle) => handle.target),
    ["highPass", 0, 1, 2, 3],
  );
  assert.deepEqual(BAND_NAMES, ["Low shelf", "Low peak", "High peak", "High shelf"]);
  const [highPass, , lowPeak] = handles;
  near(highPass.x, freqToX(80, WIDTH));
  near(highPass.y, HEIGHT / 2);
  assert.equal(highPass.enabled, false);
  assert.equal(highPass.label, "High-pass, off, 80 Hz");
  near(lowPeak.x, freqToX(400, WIDTH));
  near(lowPeak.y, gainToY(6, HEIGHT));
  assert.equal(lowPeak.label, "Low peak, on, 400 Hz, +6.0 dB, Q 1.00");
});

test("dragging a handle moves its band, in range, and turns it on", () => {
  const eq = defaultStrip().eq;
  eq.bands[2].enabled = false;
  const moved = dragHandle(eq, 2, freqToX(1000, WIDTH), gainToY(-4, HEIGHT), WIDTH, HEIGHT);
  near(moved.bands[2].frequencyHz, 1000, 1e-6);
  near(moved.bands[2].gainDb, -4, 1e-9);
  assert.equal(moved.bands[2].enabled, true);
  assert.equal(eq.bands[2].enabled, false, "the strip it came from is left alone");
  const far = dragHandle(eq, 0, -40, -40, WIDTH, HEIGHT);
  assert.equal(far.bands[0].frequencyHz, 20);
  assert.equal(far.bands[0].gainDb, 18);
  const highPass = dragHandle(eq, "highPass", freqToX(5000, WIDTH), 0, WIDTH, HEIGHT);
  assert.equal(highPass.highPass.frequencyHz, RANGES.highPassFrequencyHz[1], "held to 2 kHz");
  assert.equal(highPass.highPass.enabled, true);
  assert.deepEqual(highPass.bands, eq.bands, "only frequency moves for the high-pass");
});

test("the wheel and the keys change Q, frequency, gain and on/off, held in range", () => {
  const eq = defaultStrip().eq;
  near(nudgeQ(eq, 1, 1).bands[1].q, 1.12, 1e-9);
  near(nudgeQ(eq, 1, -1).bands[1].q, 1 / 1.12, 1e-9);
  assert.equal(nudgeQ(eq, 1, 100).bands[1].q, RANGES.bandQ[1]);
  assert.equal(nudgeQ(eq, "highPass", -100).highPass.q, RANGES.highPassQ[0]);

  near(handleKey(eq, 1, "ArrowRight").bands[1].frequencyHz, 400 * 2 ** (1 / 12), 1e-9);
  near(handleKey(eq, 1, "ArrowLeft", { shift: true }).bands[1].frequencyHz, 400 / 2, 1e-9);
  assert.equal(handleKey(eq, 1, "ArrowUp").bands[1].gainDb, 0.5);
  assert.equal(handleKey(eq, 1, "ArrowDown", { shift: true }).bands[1].gainDb, -3);
  near(handleKey(eq, 1, "PageUp").bands[1].q, 1.12, 1e-9);
  const boosted = handleKey(eq, 1, "ArrowUp");
  assert.equal(handleKey(boosted, 1, "Home").bands[1].gainDb, 0);
  assert.equal(handleKey(eq, 1, "Enter").bands[1].enabled, false);
  assert.equal(handleKey(eq, "highPass", " ").highPass.enabled, true);
  assert.equal(handleKey(eq, "highPass", "ArrowUp"), null, "the high-pass has no gain");
  assert.equal(handleKey(eq, 1, "a"), null);
});

test("the curve is a path through every point, drawn inside the scale", () => {
  const path = curvePath(
    [
      { freqHz: 20, db: 0 },
      { freqHz: 20000, db: 30 },
    ],
    WIDTH,
    HEIGHT,
  );
  assert.equal(path, "M0.0 100.0L600.0 0.0");
  assert.equal(curvePath([], WIDTH, HEIGHT), "");
});

test("the graph's grid marks decades and gains with their words", () => {
  const { freqs, gains } = gridLines(WIDTH, HEIGHT);
  assert.deepEqual(
    freqs.map((line) => line.label),
    ["50 Hz", "100 Hz", "200 Hz", "500 Hz", "1 kHz", "2 kHz", "5 kHz", "10 kHz"],
  );
  near(freqs[1].x, freqToX(100, WIDTH));
  assert.deepEqual(
    gains.map((line) => line.label),
    ["+12", "+6", "0", "−6", "−12"],
  );
  near(gains[2].y, HEIGHT / 2);
});

test("the curve's text alternative says every part of the equaliser", () => {
  const eq = defaultStrip().eq;
  eq.highPass.enabled = true;
  eq.bands[1].gainDb = 3;
  eq.bands[3].enabled = false;
  assert.equal(
    curveDescription(eq),
    "Equaliser response. High-pass on at 80 Hz. Low shelf on: 100 Hz, 0.0 dB, Q 0.71. " +
      "Low peak on: 400 Hz, +3.0 dB, Q 1.00. High peak on: 2.5 kHz, 0.0 dB, Q 1.00. " +
      "High shelf off.",
  );
});

test("the desk's curve is drawn only for the equaliser it was asked for, else the local one", () => {
  const eq = defaultStrip().eq;
  const desk = [{ freqHz: 20, db: 1 }];
  assert.equal(curveSource({ asked: eq, eq: structuredClone(eq), desk }).points, desk);
  const changed = structuredClone(eq);
  changed.bands[0].gainDb = 2;
  const local = curveSource({ asked: eq, eq: changed, desk, points: 8 });
  assert.equal(local.points.length, 8);
  assert.equal(local.fromDesk, false);
  assert.equal(curveSource({ asked: null, eq, desk: null, points: 4 }).points.length, 4);
});

test("a slider moves along a field's scale: linear, or logarithmic for times", () => {
  const threshold = { key: "thresholdDb", range: [-60, 0], scale: "linear" };
  assert.equal(fieldPosition(threshold, -30), 500);
  assert.equal(fieldValue(threshold, 250), -45);
  const attack = { key: "attackMs", range: [0.1, 200], scale: "log" };
  assert.equal(fieldPosition(attack, 0.1), 0);
  assert.equal(fieldPosition(attack, 200), 1000);
  near(fieldValue(attack, fieldPosition(attack, 10)), 10, 0.1);
  assert.equal(fieldPosition(attack, 1e6), 1000, "held to the scale");
  const hold = { key: "holdMs", range: [0, 2000], scale: "log" };
  assert.equal(fieldValue(hold, 0), 0, "a scale that starts at 0 still reaches it");
  near(fieldValue(hold, fieldPosition(hold, 50)), 50, 0.5);
});

test("a setting is changed on a copy of the strip, by its path", () => {
  const strip = defaultStrip();
  const next = withSetting(strip, "eq.bands.1.gainDb", 4);
  assert.equal(next.eq.bands[1].gainDb, 4);
  assert.equal(strip.eq.bands[1].gainDb, 0);
  assert.equal(withSetting(strip, "compressor.enabled", true).compressor.enabled, true);
});

test("the compressor's static curve: straight below the knee, the ratio above, makeup on top", () => {
  const compressor = { thresholdDb: -20, ratio: 4, kneeDb: 0, makeupDb: 0 };
  assert.equal(compressorOutputDb(compressor, -40), -40);
  assert.equal(compressorOutputDb(compressor, 0), -15);
  assert.equal(compressorOutputDb({ ...compressor, makeupDb: 5 }, 0), -10);
  const soft = { ...compressor, kneeDb: 10 };
  assert.equal(compressorOutputDb(soft, -26), -26, "below the knee");
  near(compressorOutputDb(soft, -20), -20 + (0.25 - 1) * 25 / 20);
  assert.equal(compressorOutputDb(soft, -10), -17.5, "above the knee");
  const path = transferPath(compressor, 60);
  assert.ok(path.startsWith("M0.0 60.0"), path);
  assert.ok(path.endsWith("L60.0 15.0"), path);
});

test("a gain-reduction meter fills from the right in proportion to what is taken off", () => {
  assert.deepEqual(reductionView(6, 24), { fraction: 0.25, text: "−6.0 dB" });
  assert.deepEqual(reductionView(0.01, 24), { fraction: 0, text: "0.0 dB" });
  assert.deepEqual(reductionView(90, 24), { fraction: 1, text: "−90.0 dB" });
  assert.deepEqual(reductionView(undefined, 24), { fraction: 0, text: "0.0 dB" });
});

test("the channel choices are the device's inputs, a stereo pair once, and prev/next step them", () => {
  const status = {
    sourceChannels: 4,
    channels: [
      { number: 1, name: "Kick", link: false },
      { number: 2, name: "", link: false },
      { number: 3, name: "Keys", link: true },
      { number: 4, name: "Keys R", link: false },
      { number: 5, name: "Spare", link: false },
    ],
  };
  assert.deepEqual(stripChoices(status), [
    { channel: 1, sides: [1], label: "CH 1 · Kick" },
    { channel: 2, sides: [2], label: "CH 2" },
    { channel: 3, sides: [3, 4], label: "CH 3–4 · Keys" },
  ]);
  const choices = stripChoices(status);
  assert.equal(stepChoice(choices, 1, 1), 2);
  assert.equal(stepChoice(choices, 4, -1), 2, "the second side steps from its pair");
  assert.equal(stepChoice(choices, 3, 1), 3, "the last stays the last");
  assert.equal(stepChoice(choices, 1, -1), 1);
  assert.equal(stepChoice([], 1, 1), 1);
  assert.deepEqual(stripChoices(null), []);
});

test("the presets are listed with the built-in ones marked and never deletable", () => {
  assert.deepEqual(
    presetOptions([
      { name: "Vocal", factory: true, settings: {} },
      { name: "Mine", factory: false, settings: {} },
    ]),
    [
      { name: "Vocal", label: "Vocal (built-in)", factory: true },
      { name: "Mine", label: "Mine", factory: false },
    ],
  );
  assert.deepEqual(presetOptions(null), []);
});

test("A/B keeps the strip, sends a neutral one, and restores the kept one", () => {
  const strip = defaultStrip();
  strip.compressor.enabled = true;
  const bypassed = abBypass({}, 2, strip);
  assert.deepEqual(bypassed.send, defaultStrip());
  assert.deepEqual(bypassed.ab, { 2: strip });
  const restored = abRestore(bypassed.ab, 2);
  assert.deepEqual(restored.send, strip);
  assert.deepEqual(restored.ab, {});
  assert.equal(abRestore({}, 2).send, null, "nothing kept, nothing to send");

  // The desk's strip changed by someone else (a load, a preset) while bypassed: the bypass ends.
  assert.deepEqual(abSettle(bypassed.ab, 2, { active: true, pending: false }), {});
  assert.equal(abSettle(bypassed.ab, 2, { active: false, pending: false }), bypassed.ab);
  assert.equal(abSettle(bypassed.ab, 2, { active: true, pending: true }), bypassed.ab);
});

test("every slider of the editor covers its part's whole range and says its value", async () => {
  const view = await import("../app/lib/processing-view.js");
  const strip = defaultStrip();
  const parts = [
    [view.HIGH_PASS_FIELDS, strip.eq.highPass, "highPass"],
    [view.BAND_FIELDS, strip.eq.bands[1], "band"],
    [view.GATE_FIELDS, strip.gate, "gate"],
    [view.COMPRESSOR_FIELDS, strip.compressor, "compressor"],
  ];
  for (const [fields, part, prefix] of parts) {
    for (const entry of fields) {
      assert.ok(entry.key in part, `${prefix}.${entry.key}`);
      const rangeKey = `${prefix}${entry.key[0].toUpperCase()}${entry.key.slice(1)}`;
      if (rangeKey in RANGES) {
        assert.deepEqual(entry.range, RANGES[rangeKey], rangeKey);
      }
      assert.equal(fieldValue(entry, 0), entry.range[0], rangeKey);
      near(fieldValue(entry, 1000), entry.range[1], 1e-9);
      assert.equal(typeof entry.format(part[entry.key]), "string");
    }
  }
  assert.equal(view.COMPRESSOR_FIELDS[1].format(4), "4:1");
});
