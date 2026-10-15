// Value mapping for the console's controls: faders, pans and meters. Pure functions, no DOM.
//
// Every level the engine takes or reports is a whole number from 0 to 100. A fader is linear in
// that percent, exactly as the engine applies it and the phone shows it. A meter's peak is a linear
// share of full scale, so the bar is drawn on a decibel scale for quiet signals to stay visible.

/** A fader: 0-100 %, one step per arrow key, ten per page key, double-click back to unity. */
export const FADER = Object.freeze({ min: 0, max: 100, step: 1, page: 10, reset: 100 });

/** A pan: 0 is hard left, 50 the centre, 100 hard right. Drags snap to the centre within 2. */
export const PAN = Object.freeze({
  min: 0,
  max: 100,
  step: 1,
  page: 10,
  centre: 50,
  detent: 2,
  reset: 50,
});

/** The quietest level a meter bar shows, in dB below full scale. */
export const METER_FLOOR_DB = -60;

/** From this many dB below full scale a meter shows the hot colour. */
export const HOT_DB = -6;

/** The group chip tones, taken in turn by the configured groups. */
export const GROUP_TONES = Object.freeze(["teal", "blue", "violet", "orange"]);

/** A whole level from `min` to `max`; anything that is not a finite number is `min`. */
export function clampLevel(value, min = 0, max = 100) {
  const number = Number(value);
  if (!Number.isFinite(number)) {
    return min;
  }
  return Math.min(max, Math.max(min, Math.round(number)));
}

/** A fader's position along its track (0 at the bottom, 1 at the top) for a percent. */
export function levelFraction(percent) {
  return clampLevel(percent) / 100;
}

/** The percent for a position along a fader's track. */
export function levelFromFraction(fraction) {
  return clampLevel(Number(fraction) * 100);
}

/**
 * Where a pointer sits along a track, from 0 to 1. `start` and `length` are the track's usable
 * span in the same coordinates as `position`; `reverse` counts from the end, as a vertical fader
 * counts from the bottom while screen coordinates grow downwards.
 */
export function pointerFraction(position, start, length, { reverse = false } = {}) {
  if (!(length > 0)) {
    return 0;
  }
  const fraction = Math.min(1, Math.max(0, (position - start) / length));
  return reverse ? 1 - fraction : fraction;
}

/**
 * The value a slider key moves to, or `null` for a key the slider does not use.
 *
 * Arrows move one `step` (up and right raise it), Page Up and Page Down one `page`, Home and End go
 * to either end. With a `centre`, a step or page that would cross it stops on it, so the keyboard
 * finds a pan's centre as easily as a drag does.
 */
export function keyValue(key, value, { min = 0, max = 100, step = 1, page = 10, centre } = {}) {
  let target;
  switch (key) {
    case "ArrowUp":
    case "ArrowRight":
      target = value + step;
      break;
    case "ArrowDown":
    case "ArrowLeft":
      target = value - step;
      break;
    case "PageUp":
      target = value + page;
      break;
    case "PageDown":
      target = value - page;
      break;
    case "Home":
      return min;
    case "End":
      return max;
    default:
      return null;
  }
  target = Math.min(max, Math.max(min, target));
  if (centre !== undefined && (value - centre) * (target - centre) < 0) {
    return centre;
  }
  return target;
}

/** A dragged value within `detent` of the centre lands on it. */
export function snapToCentre(value, { centre = PAN.centre, detent = PAN.detent } = {}) {
  return Math.abs(value - centre) <= detent ? centre : value;
}

function panSide(pan) {
  const value = clampLevel(pan);
  // The amount is the share of a full swing to one side: 40 is 20 % of the way to hard left.
  return { side: Math.sign(value - PAN.centre), amount: Math.abs(value - PAN.centre) * 2 };
}

/** The short pan readout: "C", "L20", "R30". */
export function panLabel(pan) {
  const { side, amount } = panSide(pan);
  if (side === 0) {
    return "C";
  }
  return `${side < 0 ? "L" : "R"}${amount}`;
}

/** The pan as a screen reader says it: "Centre", "20% left", "30% right". */
export function panValueText(pan) {
  const { side, amount } = panSide(pan);
  if (side === 0) {
    return "Centre";
  }
  return `${amount}% ${side < 0 ? "left" : "right"}`;
}

/** A 0-100 peak in dB below full scale; silence is minus infinity. */
export function levelToDb(level) {
  const value = Math.min(100, Number(level) || 0);
  if (value <= 0) {
    return -Infinity;
  }
  return 20 * Math.log10(value / 100);
}

/** How much of a meter bar a peak fills, from 0 to 1, on a `floorDb`-to-0 dB scale. */
export function meterFraction(level, floorDb = METER_FLOOR_DB) {
  const db = levelToDb(level);
  if (db === -Infinity) {
    return 0;
  }
  return Math.min(1, Math.max(0, (db - floorDb) / -floorDb));
}

/** The meter colour band for a peak: "safe", "hot" from -6 dB, "clip" at full scale. */
export function meterZone(level) {
  if ((Number(level) || 0) >= 100) {
    return "clip";
  }
  return levelToDb(level) >= HOT_DB ? "hot" : "safe";
}

/** The chip tone of the group at `index` in the engine's group list; no group is "none". */
export function groupTone(index) {
  return Number.isInteger(index) && index >= 0 ? GROUP_TONES[index % GROUP_TONES.length] : "none";
}
