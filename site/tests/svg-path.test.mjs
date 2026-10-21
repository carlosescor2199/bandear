// Tests for the three-free helpers behind the 3D scenes.
// Run: powershell.exe -NoProfile -Command "fnm exec --using=default node --test 'site/tests/*.test.mjs'"

import { test } from "node:test";
import assert from "node:assert/strict";

import { arcToCenter, parsePathData, samplePath } from "../js/svg-path.js";
import { capPixelRatio, clamp, damp, particleBudget, sectionProgress } from "../js/motion.js";

const near = (actual, expected, tolerance = 1e-6) =>
  assert.ok(Math.abs(actual - expected) <= tolerance, `${actual} is not within ${tolerance} of ${expected}`);
const dist = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]);

// The six strokes of the BandEar mark (1024 grid).
const MARK = [
  "M520 224 H344 A48 48 0 0 0 296 272 V738 A70 70 0 0 0 366 808 H556 A186 186 0 0 0 692 528",
  "M736 446 Q720 486 678 514",
  "M314 522 C354 482 404 470 476 470 H606",
  "M402 708 H562 A71 71 0 0 0 562 566 H474 A72 72 0 0 0 402 638",
  "M586 302 A92 92 0 0 1 662 386",
  "M584 224 A162 162 0 0 1 738 386",
];

test("parsePathData turns relative and shorthand commands into absolute segments", () => {
  const segments = parsePathData("m10 10 h20 v20 l-5 5 z");
  assert.deepEqual(
    segments.map((s) => s.type),
    ["move", "line", "line", "line", "line"],
  );
  assert.deepEqual(segments[1].to, [30, 10]);
  assert.deepEqual(segments[2].to, [30, 30]);
  assert.deepEqual(segments[3].to, [25, 35]);
  // "z" closes back to the subpath start.
  assert.deepEqual(segments[4].to, [10, 10]);
});

test("parsePathData splits compact arc flags", () => {
  const [, arc] = parsePathData("M0 0a5 5 0 016 6");
  assert.equal(arc.type, "arc");
  assert.equal(arc.largeArc, false);
  assert.equal(arc.sweep, true);
  assert.deepEqual(arc.to, [6, 6]);
});

test("parsePathData rejects commands it does not implement", () => {
  assert.throws(() => parsePathData("M0 0 S1 1 2 2"), /unsupported/i);
});

test("arcToCenter finds the centre of a quarter circle", () => {
  // Quarter circle of radius 10 around (0, 0), from (10, 0) to (0, 10), sweeping with the y-down clock.
  const arc = arcToCenter([10, 0], 10, 10, 0, false, true, [0, 10]);
  near(arc.cx, 0);
  near(arc.cy, 0);
  near(arc.deltaAngle, Math.PI / 2);
});

test("arcToCenter scales up radii that are too small", () => {
  const arc = arcToCenter([0, 0], 1, 1, 0, false, true, [10, 0]);
  near(arc.rx, 5);
  near(arc.cx, 5);
  near(arc.cy, 0);
});

test("arcToCenter has no centre for the arcs SVG treats as lines or omits", () => {
  assert.equal(arcToCenter([0, 0], 0, 10, 0, false, true, [10, 0]), null);
  assert.equal(arcToCenter([0, 0], 10, 0, 0, false, true, [10, 0]), null);
  assert.equal(arcToCenter([5, 5], 10, 10, 0, false, true, [5, 5]), null);
});

test("samplePath draws an arc with a zero radius as a straight line", () => {
  for (const d of ["M0 0 A0 10 0 0 1 40 0", "M0 0 A10 0 30 1 0 40 0"]) {
    const [points] = samplePath(d, { step: 8 });
    assert.ok(points.length > 2, d);
    for (const [x, y] of points) {
      assert.ok(Number.isFinite(x) && Number.isFinite(y), `non-finite point in ${d}`);
      near(y, 0, 1e-9);
    }
    assert.deepEqual(points.at(-1), [40, 0]);
  }
});

test("samplePath skips an arc whose endpoints are the same", () => {
  const [points] = samplePath("M0 0 A10 10 0 0 1 0 0 L20 0", { step: 8 });
  for (const [x, y] of points) assert.ok(Number.isFinite(x) && Number.isFinite(y));
  assert.deepEqual(points[0], [0, 0]);
  assert.deepEqual(points.at(-1), [20, 0]);
  // Only the line contributes points: no zero-length step at the start.
  assert.ok(dist(points[0], points[1]) > 0.5);
  // A path made only of such an arc has nothing to draw.
  assert.deepEqual(samplePath("M3 3 A10 10 0 0 1 3 3"), []);
});

test("samplePath keeps every arc point on its circle", () => {
  const [points] = samplePath("M586 302 A92 92 0 0 1 662 386", { step: 6 });
  const arc = arcToCenter([586, 302], 92, 92, 0, false, true, [662, 386]);
  for (const point of points) {
    near(dist(point, [arc.cx, arc.cy]), 92, 1e-6);
  }
});

test("samplePath starts and ends on the path endpoints for every mark stroke", () => {
  for (const d of MARK) {
    const subpaths = samplePath(d, { step: 8 });
    assert.equal(subpaths.length, 1, d);
    const points = subpaths[0];
    const [, sx, sy] = d.match(/^M(\d+) (\d+)/).map(Number);
    assert.deepEqual(points[0], [sx, sy]);
    const end = parsePathData(d).at(-1).to;
    near(points.at(-1)[0], end[0], 1e-6);
    near(points.at(-1)[1], end[1], 1e-6);
  }
});

test("samplePath spaces points close to the requested step without duplicates", () => {
  for (const d of MARK) {
    const [points] = samplePath(d, { step: 8 });
    for (let i = 1; i < points.length; i += 1) {
      const gap = dist(points[i - 1], points[i]);
      assert.ok(gap > 0.5, `duplicate point in ${d}`);
      assert.ok(gap <= 8 * 1.1, `gap ${gap} too large in ${d}`);
    }
  }
});

test("samplePath keeps straight runs straight", () => {
  const [points] = samplePath("M314 522 C354 482 404 470 476 470 H606", { step: 8 });
  const run = points.filter((p) => p[0] >= 476);
  assert.ok(run.length > 10);
  for (const p of run) near(p[1], 470, 1e-9);
});

test("clamp limits a value to a range", () => {
  assert.equal(clamp(-1, 0, 1), 0);
  assert.equal(clamp(2, 0, 1), 1);
  assert.equal(clamp(0.25, 0, 1), 0.25);
});

test("sectionProgress maps a section's trip through the viewport to 0..1", () => {
  // Section 600px tall in an 800px viewport.
  assert.equal(sectionProgress(800, 600, 800), 0); // top edge at the bottom of the viewport
  assert.equal(sectionProgress(-600, 600, 800), 1); // bottom edge at the top of the viewport
  near(sectionProgress(100, 600, 800), 0.5); // centred
  assert.equal(sectionProgress(5000, 600, 800), 0);
  assert.equal(sectionProgress(-5000, 600, 800), 1);
  assert.equal(sectionProgress(0, 0, 0), 0);
});

test("damp eases toward the target independent of frame rate", () => {
  const oneStep = damp(0, 1, 4, 1 / 30);
  const twoSteps = damp(damp(0, 1, 4, 1 / 60), 1, 4, 1 / 60);
  near(oneStep, twoSteps, 1e-12);
  assert.equal(damp(0.3, 0.3, 4, 1 / 60), 0.3);
  assert.ok(damp(0, 1, 4, 10) > 0.999);
});

test("capPixelRatio caps phones lower than laptops", () => {
  assert.equal(capPixelRatio(3, false), 2);
  assert.equal(capPixelRatio(3, true), 1.5);
  assert.equal(capPixelRatio(1, true), 1);
  assert.equal(capPixelRatio(undefined, false), 1);
});

test("particleBudget scales with the viewport width", () => {
  assert.ok(particleBudget(360, 200) < particleBudget(1440, 200));
  assert.equal(particleBudget(1440, 200), 200);
  assert.ok(particleBudget(320, 200) >= Math.round(200 * 0.35));
});
