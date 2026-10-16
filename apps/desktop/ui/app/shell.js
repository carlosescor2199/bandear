// The console shell: the sidebar, hash routing (#/mix by default, #/network) and the main region.
//
// Each section renders into the main region through a screen: a function `(main, route)` that
// builds the section and may return a cleanup function, called when the engineer leaves it.
// Screens are containers: they use the console app (lib/console-app.js), which the shell creates
// once for the window, and compose the organisms. Until a section has its screen, a placeholder
// heading stands in.
//
// In the MixLink app the console talks to the engine through Tauri. Opened in a browser as
// `index.html?fake` (served by dev/serve.mjs), it runs against the simulated engine of
// lib/fake-bridge.js instead.

import { createTauriBridge } from "./lib/bridge.js";
import { createConsoleApp } from "./lib/console-app.js";
import { createFakeBridge } from "./lib/fake-bridge.js";
import { ROUTES, documentTitle, resolveRoute } from "./lib/router.js";
import { mountMixScreen } from "./screens/mix-screen.js";
import { mountNetworkScreen } from "./screens/network-screen.js";

const ICONS = Object.freeze({
  mix: `<path d="M6 4v16M12 4v16M18 4v16"/><path d="M4 14h4M10 8h4M16 12h4"/>`,
  network: `<path d="M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0"/><circle cx="12" cy="19.5" r="1"/>`,
});

const PLACEHOLDERS = Object.freeze({
  mix: "Each musician's channel strips, groups and master appear here.",
  network: "Tune each phone's connection, watch its health and look after the server.",
});

function icon(id) {
  return `<svg class="nav-icon" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${ICONS[id]}</svg>`;
}

function renderSidebar(nav) {
  const links = ROUTES.map(
    (route) =>
      `<li><a class="nav-link" href="${route.hash}" data-route="${route.id}" title="${route.label}">${icon(route.id)}<span class="nav-label">${route.label}</span></a></li>`,
  ).join("");
  nav.innerHTML = `<div class="brand"><span class="brand-mark" aria-hidden="true"></span><span class="brand-name">MixLink</span></div><ul class="nav-list">${links}</ul>`;
}

/** The stand-in for a section whose screen is not built yet. */
export function placeholderScreen(main, route) {
  const header = document.createElement("header");
  header.className = "screen-header";
  const heading = document.createElement("h1");
  heading.tabIndex = -1;
  heading.textContent = route.title;
  const note = document.createElement("p");
  note.textContent = PLACEHOLDERS[route.id] ?? "";
  header.append(heading, note);
  main.append(header);
}

/**
 * Mounts the shell on `nav` and `main` and shows the section the location hash names. `screens`
 * maps a route id to its screen. Returns a function that unmounts it.
 */
export function mountShell({ nav, main, skipLink = null, screens = {} }) {
  renderSidebar(nav);
  let leave = null;
  let first = true;

  function show() {
    const route = resolveRoute(window.location.hash);
    if (window.location.hash !== route.hash) {
      // An empty or unknown hash shows the default section under its own address.
      history.replaceState(null, "", route.hash);
    }
    for (const link of nav.querySelectorAll("a[data-route]")) {
      if (link.dataset.route === route.id) {
        link.setAttribute("aria-current", "page");
      } else {
        link.removeAttribute("aria-current");
      }
    }
    document.title = documentTitle(route);
    leave?.();
    main.replaceChildren();
    const screen = screens[route.id] ?? placeholderScreen;
    leave = screen(main, route) ?? null;
    // After a navigation, focus moves to the new section's heading so a screen reader starts
    // there; the first paint leaves focus where the window puts it.
    if (!first) {
      (main.querySelector("h1") ?? main).focus();
    }
    first = false;
  }

  // The skip link must not change the hash, which would navigate.
  function skip(event) {
    event.preventDefault();
    main.focus();
  }

  window.addEventListener("hashchange", show);
  skipLink?.addEventListener("click", skip);
  show();
  return () => {
    window.removeEventListener("hashchange", show);
    skipLink?.removeEventListener("click", skip);
    leave?.();
  };
}

/** Shown in place of a screen when there is no engine to talk to. */
function unavailableScreen(main) {
  const header = document.createElement("header");
  header.className = "screen-header";
  const heading = document.createElement("h1");
  heading.tabIndex = -1;
  heading.textContent = "No engine to talk to";
  const note = document.createElement("p");
  note.append(
    "This console runs inside the MixLink app. To preview it in a browser with a simulated " +
      "engine, open ",
  );
  const link = document.createElement("a");
  link.href = "?fake";
  link.textContent = "index.html?fake";
  note.append(link, ".");
  header.append(heading, note);
  main.append(header);
}

// The window's engine connection: Tauri in the app, the simulated engine with `?fake`, none in a
// plain browser.
function connectEngine() {
  if (new URLSearchParams(window.location.search).has("fake")) {
    const fake = createFakeBridge();
    fake.startDemo();
    return { app: createConsoleApp({ bridge: fake }), simulated: true };
  }
  try {
    return { app: createConsoleApp({ bridge: createTauriBridge() }), simulated: false };
  } catch {
    return { app: null, simulated: false };
  }
}

const nav = document.querySelector("[data-shell-nav]");
const main = document.querySelector("[data-shell-main]");
if (nav && main) {
  const { app, simulated } = connectEngine();
  mountShell({
    nav,
    main,
    skipLink: document.querySelector("[data-skip-link]"),
    screens: {
      mix: app ? (screenMain) => mountMixScreen(screenMain, app) : unavailableScreen,
      network: app ? (screenMain) => mountNetworkScreen(screenMain, app) : unavailableScreen,
    },
  });
  if (simulated) {
    const note = document.createElement("p");
    note.className = "sidebar-note";
    note.textContent = "Simulated engine";
    nav.append(note);
  }
}
