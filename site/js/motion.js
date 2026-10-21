// Small pure helpers for animation, scroll mapping and performance budgets.
// No three.js or DOM dependency, so they are covered by `node --test`.

export const clamp = (value, min, max) => Math.min(max, Math.max(min, value));

export const lerp = (a, b, t) => a + (b - a) * t;

// Frame-rate independent easing toward a target: same result for one 1/30 s step or two 1/60 s steps.
export const damp = (current, target, lambda, dt) => current + (target - current) * (1 - Math.exp(-lambda * dt));

// How far a section has travelled through the viewport: 0 when its top meets the bottom of the
// viewport, 1 when its bottom leaves the top. `top` is getBoundingClientRect().top.
export function sectionProgress(top, height, viewportHeight) {
  const travel = viewportHeight + height;
  if (travel <= 0) return 0;
  return clamp((viewportHeight - top) / travel, 0, 1);
}

// Device pixel ratio cap: rendering above 2x costs a lot and shows little; phones stop at 1.5x.
export const capPixelRatio = (devicePixelRatio, isPhone) => Math.min(devicePixelRatio || 1, isPhone ? 1.5 : 2);

// Particle count for a viewport width: the full budget from 1280 px up, never below 35 % of it.
export const particleBudget = (viewportWidth, full) => Math.round(full * clamp(viewportWidth / 1280, 0.35, 1));
