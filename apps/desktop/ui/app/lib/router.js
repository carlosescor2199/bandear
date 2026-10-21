// The console's sections and hash routing. Pure functions, no DOM.
//
// Only the sections built so far are listed, in the design's order; each later phase adds its own.
// The processing editor's hash may name the channel it opens on: "#/processing/5".

export const ROUTES = Object.freeze([
  Object.freeze({ id: "mix", hash: "#/mix", label: "Mix", title: "Mix" }),
  Object.freeze({ id: "record", hash: "#/record", label: "Record", title: "Record" }),
  Object.freeze({
    id: "channels",
    hash: "#/channels",
    label: "Channels and groups",
    title: "Channels and groups",
  }),
  Object.freeze({ id: "processing", hash: "#/processing", label: "Processing", title: "Processing" }),
  Object.freeze({ id: "effects", hash: "#/effects", label: "Effects", title: "Effects" }),
  Object.freeze({
    id: "network",
    hash: "#/network",
    label: "Network and system",
    title: "Network and system",
  }),
]);

export const DEFAULT_ROUTE = ROUTES[0];

/** The highest channel the processing editor's hash may name. */
const LAST_CHANNEL = 32;

// The hash's path, lower case, without its leading "#/" or trailing slashes, split at "/".
function pathOf(hash) {
  return String(hash ?? "")
    .replace(/^#\/?/, "")
    .replace(/\/+$/, "")
    .toLowerCase()
    .split("/");
}

/**
 * The section a location hash opens. A trailing slash, a missing slash or other letter case still
 * find their section; no hash, or one that names no section, opens the Mix. Only the processing
 * editor takes a channel after its name.
 */
export function resolveRoute(hash) {
  const [id, ...rest] = pathOf(hash);
  if (rest.length > 0 && id !== "processing") {
    return DEFAULT_ROUTE;
  }
  return ROUTES.find((route) => route.id === id) ?? DEFAULT_ROUTE;
}

/** The channel (1-32) the processing editor's hash names, or null. */
export function routeChannel(hash) {
  const [id, channel, ...rest] = pathOf(hash);
  if (id !== "processing" || rest.length > 0 || !/^\d+$/.test(channel ?? "")) {
    return null;
  }
  const number = Number(channel);
  return number >= 1 && number <= LAST_CHANNEL ? number : null;
}

/** The processing editor's hash opened on `channel` (1-based). */
export function processingHash(channel) {
  return `#/processing/${channel}`;
}

/** The address a hash is shown under: its section's, with the editor's channel when it names one. */
export function canonicalHash(hash) {
  const route = resolveRoute(hash);
  const channel = route.id === "processing" ? routeChannel(hash) : null;
  return channel === null ? route.hash : processingHash(channel);
}

/** The window title for a section: "Mix · BandEar". */
export function documentTitle(route) {
  return `${route.title} · BandEar`;
}
