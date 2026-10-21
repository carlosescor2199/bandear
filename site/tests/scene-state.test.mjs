// Tests for the 3D figure states: the static design only gives way to a scene that has drawn,
// and a failure always brings it back for good.
// Run: node --test 'site/tests/*.test.mjs'

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { nextSceneState } from "../js/scene-state.js";

test("a loading figure goes live on its first frame", () => {
  assert.equal(nextSceneState(undefined, "rendered"), "live");
  assert.equal(nextSceneState("live", "rendered"), "live");
});

test("a failure brings the static design back, before or after the first frame", () => {
  assert.equal(nextSceneState(undefined, "failed"), "static");
  assert.equal(nextSceneState("live", "failed"), "static");
});

test("static is final: a late frame cannot hide the static design again", () => {
  assert.equal(nextSceneState("static", "rendered"), "static");
  assert.equal(nextSceneState("static", "failed"), "static");
});

test("unknown events leave the state alone", () => {
  assert.equal(nextSceneState(undefined, "resized"), undefined);
  assert.equal(nextSceneState("live", undefined), "live");
});

test("the stylesheet hides a static visual only once its scene is live", () => {
  const css = readFileSync(new URL("../style.css", import.meta.url), "utf8");
  // No rule may take the screenshot or the SVG diagram away just because WebGL exists.
  for (const [, selectors, body] of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const hides = /display:\s*none|opacity:\s*0\s*;|visibility:\s*hidden/.test(body);
    if (!hides) continue;
    for (const selector of selectors.split(",").map((part) => part.trim())) {
      if (/\.window\b|>\s*svg\b/.test(selector)) {
        assert.match(selector, /\[data-scene="live"\]/, `"${selector}" hides a static visual too early`);
      }
    }
  }
});
