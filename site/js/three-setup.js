// Shared plumbing for the decorative three.js scenes: renderer creation, a render loop that
// pauses when the canvas is off-screen or the tab is hidden, a soft glow texture and disposal.

import * as THREE from "../vendor/three/three.module.min.js";
import { capPixelRatio } from "./motion.js";

export const AMBER = 0xffbf43;

export const prefersReducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// A phone or small tablet: coarse pointer and a short screen side.
export const isPhone = () =>
  window.matchMedia("(pointer: coarse)").matches && Math.min(window.screen.width, window.screen.height) < 768;

export function createRenderer(canvas, { maxPixelRatio = Infinity } = {}) {
  const phone = isPhone();
  const renderer = new THREE.WebGLRenderer({
    canvas,
    alpha: true,
    antialias: !phone,
    powerPreference: "default",
  });
  renderer.setPixelRatio(Math.min(maxPixelRatio, capPixelRatio(window.devicePixelRatio, phone)));
  renderer.setClearColor(0x000000, 0);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  return renderer;
}

// Creates the renderer for the canvas in `host` and returns `build(renderer)`. A host without a
// canvas throws before any WebGL context exists; when `build` throws, the renderer and its
// context are released before the error propagates, so a failed mount leaks no GPU context.
export function withRenderer(host, options, build) {
  const canvas = host.querySelector("canvas");
  if (!canvas) throw new Error("the 3D host has no <canvas>");
  const renderer = createRenderer(canvas, options);
  try {
    return build(renderer);
  } catch (error) {
    renderer.dispose();
    renderer.forceContextLoss();
    throw error;
  }
}

// A white radial gradient on transparent, for additive "bloom" sprites and soft particles.
export function glowTexture(size = 128) {
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext("2d");
  const gradient = context.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
  gradient.addColorStop(0, "rgba(255,255,255,1)");
  gradient.addColorStop(0.25, "rgba(255,255,255,0.45)");
  gradient.addColorStop(0.6, "rgba(255,255,255,0.1)");
  gradient.addColorStop(1, "rgba(255,255,255,0)");
  context.fillStyle = gradient;
  context.fillRect(0, 0, size, size);
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  return texture;
}

// Frees every geometry, material and texture under `root`.
export function disposeObject(root) {
  root.traverse((object) => {
    object.geometry?.dispose();
    const materials = Array.isArray(object.material) ? object.material : [object.material];
    for (const material of materials) {
      if (!material) continue;
      for (const value of Object.values(material)) {
        if (value && value.isTexture) value.dispose();
      }
      for (const uniform of Object.values(material.uniforms ?? {})) {
        if (uniform.value && uniform.value.isTexture) uniform.value.dispose();
      }
      material.dispose();
    }
  });
}

// Drives one canvas: sizes the renderer to `host`, renders frames only while the host is on
// screen and the tab is visible, and with reduced motion renders a single still frame (again
// after each resize). `onFrame(elapsedSeconds, dtSeconds)` draws; `onResize(width, height)`
// updates cameras. `alwaysOn` skips the visibility observer (for a fixed full-page canvas).
// `onRendered()` runs once, after the first frame drew without throwing. `onFail(error)` runs
// once when a frame or a resize throws or the GPU context is lost; the stage stops drawing.
export function createStage({ host, renderer, onResize, onFrame, onRendered, onFail, fps = 60, alwaysOn = false }) {
  const still = prefersReducedMotion();
  const minInterval = 1000 / fps - 2;
  const size = { width: 0, height: 0 };
  let visible = alwaysOn;
  let running = false;
  let disposed = false;
  let failed = false;
  let frame = 0;
  let last = 0;
  let lastDraw = 0;
  let elapsed = 0;

  function fail(error) {
    if (failed || disposed) return;
    failed = true;
    update();
    onFail?.(error);
  }

  function resize() {
    const width = host.clientWidth;
    const height = host.clientHeight;
    if (failed || width === 0 || height === 0 || (width === size.width && height === size.height)) return false;
    size.width = width;
    size.height = height;
    try {
      renderer.setSize(width, height, false); // CSS owns the canvas box, so no layout shift.
      onResize(width, height);
    } catch (error) {
      fail(error);
      return false;
    }
    return true;
  }

  function draw(dt) {
    if (failed || size.width === 0) return;
    try {
      onFrame(elapsed, dt);
    } catch (error) {
      fail(error);
      return;
    }
    if (host.dataset.rendered !== "true") {
      host.dataset.rendered = "true";
      onRendered?.();
    }
  }

  function tick(now) {
    frame = running ? requestAnimationFrame(tick) : 0;
    if (!running || now - lastDraw < minInterval) return;
    const dt = last === 0 ? 0 : Math.min(0.1, (now - last) / 1000);
    last = now;
    lastDraw = now;
    elapsed += dt;
    draw(dt);
  }

  function update() {
    const shouldRun = !disposed && !failed && !still && visible && !document.hidden;
    if (shouldRun && !running) {
      running = true;
      last = 0;
      frame = requestAnimationFrame(tick);
    } else if (!shouldRun && running) {
      running = false;
      cancelAnimationFrame(frame);
      frame = 0;
    }
  }

  const resizeObserver = new ResizeObserver(() => {
    if (resize() && !running) draw(0);
  });
  resizeObserver.observe(host);

  const intersectionObserver = alwaysOn
    ? null
    : new IntersectionObserver(
        (entries) => {
          visible = entries[entries.length - 1].isIntersecting;
          update();
        },
        { rootMargin: "64px 0px" },
      );
  intersectionObserver?.observe(host);

  document.addEventListener("visibilitychange", update);

  const lost = (event) => {
    event.preventDefault();
    fail(new Error("WebGL context lost"));
  };
  renderer.domElement.addEventListener("webglcontextlost", lost);

  resize();
  draw(0);
  update();

  return {
    still,
    size,
    dispose() {
      disposed = true;
      update();
      resizeObserver.disconnect();
      intersectionObserver?.disconnect();
      document.removeEventListener("visibilitychange", update);
      renderer.domElement.removeEventListener("webglcontextlost", lost);
    },
  };
}
