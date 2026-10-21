// SVG path sampling for the 3D scenes. Pure functions with no three.js or DOM dependency,
// so they run under `node --test` as well as in the browser.
//
// Supported commands: M L H V C Q A Z, absolute and relative. Anything else throws, which
// is enough for the BandEar mark and keeps this small.

const NUMBER = /[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/y;

// A cursor over path data that reads commands, numbers and single-character arc flags.
function createScanner(d) {
  let index = 0;
  const skip = () => {
    while (index < d.length && /[\s,]/.test(d[index])) index += 1;
  };
  return {
    done() {
      skip();
      return index >= d.length;
    },
    command() {
      skip();
      const char = d[index];
      if (char !== undefined && /[a-zA-Z]/.test(char)) {
        index += 1;
        return char;
      }
      return null;
    },
    number() {
      skip();
      NUMBER.lastIndex = index;
      const match = NUMBER.exec(d);
      if (match === null) throw new Error(`expected a number at ${index} in "${d}"`);
      index = NUMBER.lastIndex;
      return Number(match[0]);
    },
    // Arc flags are one character each, so "016" means flags 0 and 1 followed by 6.
    flag() {
      skip();
      const char = d[index];
      if (char !== "0" && char !== "1") throw new Error(`expected an arc flag at ${index} in "${d}"`);
      index += 1;
      return char === "1";
    },
  };
}

// Parses path data into absolute segments: { type: "move" | "line" | "quad" | "cubic" | "arc", ... }.
export function parsePathData(d) {
  const scan = createScanner(d);
  const segments = [];
  let current = [0, 0];
  let start = [0, 0];
  let command = null;

  while (!scan.done()) {
    const explicit = scan.command();
    if (explicit !== null) {
      command = explicit;
    } else if (command === null || command === "z" || command === "Z") {
      throw new Error(`path data must start with a command: "${d}"`);
    } else if (command === "M") {
      command = "L"; // Extra coordinate pairs after a move are implicit line-tos.
    } else if (command === "m") {
      command = "l";
    }

    const relative = command === command.toLowerCase();
    const from = current;
    const point = () => {
      const x = scan.number();
      const y = scan.number();
      return relative ? [from[0] + x, from[1] + y] : [x, y];
    };

    switch (command.toUpperCase()) {
      case "M": {
        const to = point();
        segments.push({ type: "move", to });
        current = to;
        start = to;
        break;
      }
      case "L": {
        const to = point();
        segments.push({ type: "line", from, to });
        current = to;
        break;
      }
      case "H": {
        const x = scan.number();
        const to = [relative ? from[0] + x : x, from[1]];
        segments.push({ type: "line", from, to });
        current = to;
        break;
      }
      case "V": {
        const y = scan.number();
        const to = [from[0], relative ? from[1] + y : y];
        segments.push({ type: "line", from, to });
        current = to;
        break;
      }
      case "Q": {
        const control = point();
        const to = point();
        segments.push({ type: "quad", from, control, to });
        current = to;
        break;
      }
      case "C": {
        const control1 = point();
        const control2 = point();
        const to = point();
        segments.push({ type: "cubic", from, control1, control2, to });
        current = to;
        break;
      }
      case "A": {
        const rx = scan.number();
        const ry = scan.number();
        const rotation = scan.number();
        const largeArc = scan.flag();
        const sweep = scan.flag();
        const to = point();
        segments.push({ type: "arc", from, rx, ry, rotation, largeArc, sweep, to });
        current = to;
        break;
      }
      case "Z": {
        segments.push({ type: "line", from, to: start });
        current = start;
        break;
      }
      default:
        throw new Error(`unsupported path command "${command}"`);
    }
  }
  return segments;
}

// Converts an SVG endpoint arc to centre form (SVG 1.1 implementation notes, F.6.5).
// Angles are in radians in the path's own (y-down) coordinates. Returns null for the
// out-of-range cases of F.6.2, which have no centre: identical endpoints (the arc is omitted)
// and a zero radius (the arc is drawn as a straight line).
export function arcToCenter(from, rx, ry, rotationDeg, largeArc, sweep, to) {
  const [x1, y1] = from;
  const [x2, y2] = to;
  if ((x1 === x2 && y1 === y2) || rx === 0 || ry === 0) return null;
  const phi = (rotationDeg * Math.PI) / 180;
  const cos = Math.cos(phi);
  const sin = Math.sin(phi);
  let rX = Math.abs(rx);
  let rY = Math.abs(ry);

  const dx = (x1 - x2) / 2;
  const dy = (y1 - y2) / 2;
  const x1p = cos * dx + sin * dy;
  const y1p = -sin * dx + cos * dy;

  // Radii too small to reach the endpoint are scaled up just enough.
  const lambda = (x1p * x1p) / (rX * rX) + (y1p * y1p) / (rY * rY);
  if (lambda > 1) {
    const scale = Math.sqrt(lambda);
    rX *= scale;
    rY *= scale;
  }

  const numerator = rX * rX * rY * rY - rX * rX * y1p * y1p - rY * rY * x1p * x1p;
  const denominator = rX * rX * y1p * y1p + rY * rY * x1p * x1p;
  let coefficient = denominator === 0 ? 0 : Math.sqrt(Math.max(0, numerator / denominator));
  if (largeArc === sweep) coefficient = -coefficient;
  const cxp = (coefficient * rX * y1p) / rY;
  const cyp = (-coefficient * rY * x1p) / rX;

  const cx = cos * cxp - sin * cyp + (x1 + x2) / 2;
  const cy = sin * cxp + cos * cyp + (y1 + y2) / 2;
  const startAngle = Math.atan2((y1p - cyp) / rY, (x1p - cxp) / rX);
  let deltaAngle = Math.atan2((-y1p - cyp) / rY, (-x1p - cxp) / rX) - startAngle;
  if (sweep && deltaAngle < 0) deltaAngle += 2 * Math.PI;
  if (!sweep && deltaAngle > 0) deltaAngle -= 2 * Math.PI;

  return { cx, cy, rx: rX, ry: rY, rotation: phi, startAngle, deltaAngle };
}

// A function t ∈ [0, 1] → [x, y] for one segment; t = 1 returns the exact endpoint.
function segmentFunction(segment) {
  const { from, to } = segment;
  const finish = (fn) => (t) => (t >= 1 ? [to[0], to[1]] : fn(t));
  const line = () => finish((t) => [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t]);
  switch (segment.type) {
    case "line":
      return line();
    case "quad": {
      const c = segment.control;
      return finish((t) => {
        const u = 1 - t;
        return [
          u * u * from[0] + 2 * u * t * c[0] + t * t * to[0],
          u * u * from[1] + 2 * u * t * c[1] + t * t * to[1],
        ];
      });
    }
    case "cubic": {
      const a = segment.control1;
      const b = segment.control2;
      return finish((t) => {
        const u = 1 - t;
        const w0 = u * u * u;
        const w1 = 3 * u * u * t;
        const w2 = 3 * u * t * t;
        const w3 = t * t * t;
        return [
          w0 * from[0] + w1 * a[0] + w2 * b[0] + w3 * to[0],
          w0 * from[1] + w1 * a[1] + w2 * b[1] + w3 * to[1],
        ];
      });
    }
    case "arc": {
      const arc = arcToCenter(from, segment.rx, segment.ry, segment.rotation, segment.largeArc, segment.sweep, to);
      // A zero radius draws a straight line; identical endpoints give a zero-length line,
      // which sampling skips.
      if (arc === null) return line();
      const cos = Math.cos(arc.rotation);
      const sin = Math.sin(arc.rotation);
      return finish((t) => {
        const angle = arc.startAngle + arc.deltaAngle * t;
        const ex = arc.rx * Math.cos(angle);
        const ey = arc.ry * Math.sin(angle);
        return [arc.cx + ex * cos - ey * sin, arc.cy + ex * sin + ey * cos];
      });
    }
    default:
      throw new Error(`cannot sample segment "${segment.type}"`);
  }
}

// Points along one segment, evenly spaced by arc length (the start point excluded).
function sampleSegment(fn, step) {
  const FINE = 64;
  const fine = [fn(0)];
  const lengths = [0];
  for (let i = 1; i <= FINE; i += 1) {
    const p = fn(i / FINE);
    const q = fine[i - 1];
    fine.push(p);
    lengths.push(lengths[i - 1] + Math.hypot(p[0] - q[0], p[1] - q[1]));
  }
  const total = lengths[FINE];
  if (total < 1e-9) return [];

  const count = Math.max(1, Math.ceil(total / step));
  const points = [];
  let cursor = 1;
  for (let k = 1; k <= count; k += 1) {
    if (k === count) {
      points.push(fn(1));
      break;
    }
    const target = (total * k) / count;
    while (lengths[cursor] < target) cursor += 1;
    const span = lengths[cursor] - lengths[cursor - 1];
    const local = span === 0 ? 0 : (target - lengths[cursor - 1]) / span;
    points.push(fn((cursor - 1 + local) / FINE));
  }
  return points;
}

// Samples path data into subpaths of [x, y] points spaced roughly `step` units apart.
export function samplePath(d, { step = 8 } = {}) {
  const subpaths = [];
  let points = null;
  for (const segment of parsePathData(d)) {
    if (segment.type === "move") {
      points = [[segment.to[0], segment.to[1]]];
      subpaths.push(points);
      continue;
    }
    if (points === null) {
      points = [[segment.from[0], segment.from[1]]];
      subpaths.push(points);
    }
    points.push(...sampleSegment(segmentFunction(segment), step));
  }
  return subpaths.filter((subpath) => subpath.length > 1);
}
