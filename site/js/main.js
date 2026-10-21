// Boots the page. The language (i18n.js) and the download links (downloads.js) start on every
// visit. The decorative 3D scenes start only when the inline script in <head> found WebGL 2 and
// added the `webgl` class to <html>; three.js is downloaded when a scene is about to become
// visible. Each figure keeps its static design until its scene has drawn a first frame, and goes
// back to it for good if the scene fails (see scene-state.js), so a broken scene never leaves an
// empty canvas behind.

import { initDownloads } from "./downloads.js";
import { initI18n } from "./i18n.js";
import { nextSceneState } from "./scene-state.js";

// Language first, so the copy is settled before anything else renders.
try {
  initI18n();
} catch (error) {
  console.warn("BandEar: language switch unavailable.", error);
}

// The download links already open the releases page; this only makes them direct.
initDownloads().catch((error) => console.warn("BandEar: download links left on the releases page.", error));

const root = document.documentElement;
const mounted = new Set();

// Moves the figure around `host` to its next state (see scene-state.js).
function setState(host, event) {
  const container = host.closest("[data-3d-host]");
  if (!container) return;
  const next = nextSceneState(container.dataset.scene, event);
  if (next !== undefined) container.dataset.scene = next;
}

function start(host, load) {
  let scene = null;
  let failed = false;
  // A load error, a mount error, a frame that throws or a lost GPU context.
  const fail = (error) => {
    if (failed) return;
    failed = true;
    if (scene) {
      mounted.delete(scene);
      scene.dispose();
      scene = null;
    }
    setState(host, "failed");
    // The page background has no static twin: just remove it.
    if (!host.closest("[data-3d-host]")) host.remove();
    console.warn("BandEar: 3D scene disabled.", error);
  };
  return load()
    .then(({ mount }) => {
      const handle = mount(host, { onRendered: () => setState(host, "rendered"), onFail: fail });
      // The first frame runs inside mount, so it may already have failed.
      if (failed) handle.dispose();
      else {
        scene = handle;
        mounted.add(scene);
      }
    })
    .catch(fail);
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
