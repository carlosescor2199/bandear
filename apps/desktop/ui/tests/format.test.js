import test from "node:test";
import assert from "node:assert/strict";

import {
  formatBuffer,
  formatCaptureFormat,
  formatDb,
  formatPercent,
  formatSampleFormat,
  formatSampleRate,
  linkState,
  musicianLabel,
  needsAttention,
} from "../app/lib/format.js";

const MINUS = "−";

function reporting(extra = {}) {
  return {
    address: "192.168.1.3:50000",
    controlConnected: true,
    linkHealth: "good",
    packetsPerSecond: 200,
    underrunsPerSecond: 0,
    lossPercent: 0,
    receiveTimeoutsPerSecond: 0,
    ...extra,
  };
}

const NO_REPORT = {
  packetsPerSecond: null,
  underrunsPerSecond: null,
  lossPercent: null,
  receiveTimeoutsPerSecond: null,
};

test("percent and decibel readouts", () => {
  assert.equal(formatPercent(85), "85%");
  assert.equal(formatPercent(62.4), "62%");
  assert.equal(formatDb(-6.02), `${MINUS}6 dBFS`);
  assert.equal(formatDb(0), "0 dBFS");
  assert.equal(formatDb(-0.3), "0 dBFS");
  assert.equal(formatDb(-Infinity), `${MINUS}∞ dBFS`);
});

test("the capture format reads like the design's top bar", () => {
  assert.equal(formatSampleRate(48000), "48 kHz");
  assert.equal(formatSampleRate(44100), "44.1 kHz");
  assert.equal(formatSampleRate(96000), "96 kHz");
  assert.equal(formatSampleFormat("I16"), "16-bit");
  assert.equal(formatSampleFormat("I24"), "24-bit");
  assert.equal(formatSampleFormat("F32"), "32-bit float");
  assert.equal(formatSampleFormat("U8"), "8-bit unsigned");
  assert.equal(formatSampleFormat("DSD"), "DSD", "an unknown format is shown as it came");
  assert.equal(formatBuffer("Fixed(256)"), "256");
  assert.equal(formatBuffer("device default"), "default buffer");
  assert.equal(
    formatCaptureFormat({
      channels: 4,
      sampleRate: 48000,
      sampleFormat: "I24",
      bufferSize: "Fixed(256)",
    }),
    "48 kHz · 24-bit · 256",
  );
});

test("a reporting phone's link reads Good, Warning or Poor with its numbers", () => {
  assert.deepEqual(linkState(reporting()), {
    level: "good",
    label: "Good",
    detail: "Loss 0.00% · Glitches 0.0/s · Timeouts 0.0/s",
  });
  const warning = linkState(
    reporting({ linkHealth: "warning", lossPercent: 0.82, underrunsPerSecond: 2 }),
  );
  assert.equal(warning.level, "warning");
  assert.equal(warning.label, "Warning");
  assert.equal(warning.detail, "Loss 0.82% · Glitches 2.0/s · Timeouts 0.0/s");
  assert.equal(linkState(reporting({ linkHealth: "bad", packetsPerSecond: 150 })).label, "Poor");
});

test("a bad link without reports is lost, and one that hears nothing has no audio", () => {
  const lost = linkState(reporting({ linkHealth: "bad", ...NO_REPORT }));
  assert.equal(lost.level, "bad");
  assert.equal(lost.label, "Link lost");

  const silent = linkState(reporting({ linkHealth: "bad", packetsPerSecond: 0 }));
  assert.equal(silent.level, "bad");
  assert.equal(silent.label, "No audio");
});

test("a phone with no reports yet says whether its app is connected", () => {
  assert.equal(linkState(reporting({ linkHealth: "unknown", ...NO_REPORT })).label, "No reports");
  assert.equal(
    linkState(reporting({ linkHealth: "unknown", controlConnected: false, ...NO_REPORT })).label,
    "Not connected",
  );
  assert.equal(
    linkState(reporting({ linkHealth: "sideways", ...NO_REPORT })).level,
    "unknown",
    "a health the window does not know reads as unknown",
  );
});

test("only a warning or bad link needs attention", () => {
  assert.equal(needsAttention(reporting()), false);
  assert.equal(needsAttention(reporting({ linkHealth: "warning" })), true);
  assert.equal(needsAttention(reporting({ linkHealth: "bad", ...NO_REPORT })), true);
  assert.equal(needsAttention(reporting({ linkHealth: "unknown", ...NO_REPORT })), false);
});

test("a musician is called by name, or by the phone's IP when there is none", () => {
  assert.equal(musicianLabel({ name: "Bass", address: "192.168.1.12:50000" }), "Bass");
  assert.equal(musicianLabel({ name: null, address: "192.168.1.12:50000" }), "192.168.1.12");
  assert.equal(musicianLabel({ name: "", address: "[fe80::1]:50000" }), "fe80::1");
});
