// Boots the page: the language (i18n.js), then the decorative 3D layers. The inline script in <head> adds the `webgl` class to <html>
// before first paint when WebGL 2 is available, so the CSS swaps the static visuals for canvas
// hosts without layout shifts. Without that class this module does nothing and the static
// design stays. three.js is only downloaded when a scene is about to become visible.

import { initI18n } from "./i18n.js";

// Language first, so the copy is settled before anything else renders.
try {
  initI18n();
} catch (error) {
  console.warn("BandEar: language switch unavailable.", error);
}

const root = document.documentElement;
const mounted = new Set();

// Puts one host back to its static design (load failure or lost GPU context).
function fallBack(host, error) {
  const container = host.closest("[data-3d-host]");
  if (container) container.classList.add("is-static");
  else host.remove();
  if (error) console.warn("BandEar: 3D scene disabled.", error);
}

function start(host, load) {
  return load()
    .then(({ mount }) => {
      let scene = null;
      scene = mount(host, {
        onLost() {
          scene?.dispose();
          mounted.delete(scene);
          fallBack(host);
        },
      });
      mounted.add(scene);
    })
    .catch((error) => fallBack(host, error));
}

// Lazy init: mount a scene once its host comes within reach of the viewport.
function mountWhenNear(selector, load) {
  const host = document.querySelector(selector);
  if (!host) return;
  const observer = new IntersectionObserver(
    (entries) => {
      if (!entries.some((entry) => entry.isIntersecting)) return;
      observer.disconnect();
      start(host, load);
    },
    { rootMargin: "300px 0px" },
  );
  observer.observe(host);
}

// The page background is fixed behind everything; start it once the page has settled.
function mountBackground() {
  const host = document.createElement("div");
  host.className = "bg-3d";
  host.setAttribute("aria-hidden", "true");
  host.append(document.createElement("canvas"));
  document.body.prepend(host);
  start(host, () => import("./scene-background.js"));
}

if (root.classList.contains("webgl")) {
  mountWhenNear(".hero-3d", () => import("./scene-hero.js"));
  mountWhenNear(".flow-3d", () => import("./scene-flow.js"));
  const idle = window.requestIdleCallback ?? ((callback) => setTimeout(callback, 600));
  idle(mountBackground, { timeout: 2000 });

  // Free GPU resources when the page is really going away (not when it enters the back/forward cache).
  window.addEventListener("pagehide", (event) => {
    if (event.persisted) return;
    for (const scene of mounted) scene.dispose();
    mounted.clear();
  });
}
