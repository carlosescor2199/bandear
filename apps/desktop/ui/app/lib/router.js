// The console's sections and hash routing. Pure functions, no DOM.
//
// Only the sections this phase builds are listed; each later phase adds its own.

export const ROUTES = Object.freeze([
  Object.freeze({ id: "mix", hash: "#/mix", label: "Mix", title: "Mix" }),
  Object.freeze({
    id: "network",
    hash: "#/network",
    label: "Network and system",
    title: "Network and system",
  }),
]);

export const DEFAULT_ROUTE = ROUTES[0];

/**
 * The section a location hash opens. A trailing slash, a missing slash or other letter case still
 * find their section; no hash, or one that names no section, opens the Mix.
 */
export function resolveRoute(hash) {
  const id = String(hash ?? "")
    .replace(/^#\/?/, "")
    .replace(/\/+$/, "")
    .toLowerCase();
  return ROUTES.find((route) => route.id === id) ?? DEFAULT_ROUTE;
}

/** The window title for a section: "Mix · MixLink". */
export function documentTitle(route) {
  return `${route.title} · MixLink`;
}
