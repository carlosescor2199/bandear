// The download button: finds the latest GitHub release, points every download link at the real
// file and offers the visitor's own system first. The markup already links everything to the
// releases page, so any failure here (offline, rate limit, an unexpected answer) leaves a page
// that still works.
//
// The pure helpers (platform detection, asset matching, the hero choice) touch no browser
// globals and are tested with node.

import { LANGUAGE_EVENT, t } from "./i18n.js";

export const RELEASES_PAGE = "https://github.com/carlosescor2199/bandear/releases/latest";
export const RELEASE_API = "https://api.github.com/repos/carlosescor2199/bandear/releases/latest";

// One entry per installer, keyed by the suffix of its file name.
export const ASSETS = Object.freeze({
  windows: "_x64-setup.exe",
  macArm: "_aarch64.dmg",
  macIntel: "_x64.dmg",
  appImage: "_amd64.AppImage",
  deb: "_amd64.deb",
  android: "_android.apk",
});

const CACHE_KEY = "bandear.release";
const CACHE_MS = 30 * 60 * 1000; // The API allows 60 anonymous requests per hour per address.
const TIMEOUT_MS = 6000;
const HINTS_TIMEOUT_MS = 800; // client hints usually answer at once; never hold the button back

// Which system the visitor is on, from navigator-like data:
//   { userAgent, platform, maxTouchPoints, uaPlatform, architecture }
// uaPlatform and architecture come from navigator.userAgentData when the browser has it.
// Returns { os: "windows" | "mac" | "linux" | "android" | "ios" | "unknown", arch: "arm" | "x86" | null }.
export function detectPlatform({ userAgent = "", platform = "", maxTouchPoints = 0, uaPlatform = "", architecture = "" } = {}) {
  const ua = userAgent;
  const hint = uaPlatform.toLowerCase();
  const arch = architecture === "arm" || architecture === "x86" ? architecture : null;

  if (/iPhone|iPad|iPod/.test(ua) || hint === "ios") return { os: "ios", arch: null };
  // iPadOS asks for desktop sites and reports itself as a Mac with a touch screen.
  if (/Macintosh/.test(ua) && maxTouchPoints > 1) return { os: "ios", arch: null };
  if (/Android/.test(ua) || hint === "android") return { os: "android", arch: null };
  if (/CrOS/.test(ua) || hint === "chrome os") return { os: "unknown", arch: null };
  if (/Windows/.test(ua) || hint === "windows" || /^Win/.test(platform)) return { os: "windows", arch };
  if (/Macintosh|Mac OS X/.test(ua) || hint === "macos" || /^Mac/.test(platform)) return { os: "mac", arch };
  if (/Linux|X11/.test(ua) || hint === "linux" || /Linux/.test(platform)) {
    // Firefox and others name an ARM CPU in the user agent ("Linux aarch64", "armv7l").
    const armInUa = /\b(aarch64|arm64|armv\d+\w*)\b/i.test(`${ua} ${platform}`);
    return { os: "linux", arch: arch ?? (armInUa ? "arm" : null) };
  }
  return { os: "unknown", arch: null };
}

// Maps each known installer to { name, url } from a GitHub release's asset list.
export function matchAssets(assets = []) {
  const found = {};
  for (const [key, suffix] of Object.entries(ASSETS)) {
    const asset = assets.find(
      (item) => typeof item?.name === "string" && item.name.endsWith(suffix) && typeof item.browser_download_url === "string",
    );
    if (asset) found[key] = { name: asset.name, url: asset.browser_download_url };
  }
  return found;
}

// What the hero offers a platform:
//   { label: [key, vars], href, card: key | null, alt?: { text, link?, href? } }
// `label` is a dictionary key and its placeholders, `card` the download card to highlight and
// `alt` an optional line under the button (a text key, plus a link key and its href).
// Without a matching file the button still names the system and opens the releases page.
export function heroChoice({ os, arch }, found = {}) {
  const url = (key) => found[key]?.url ?? RELEASES_PAGE;
  const forOs = (osKey) => ["dl.hero.for", { os: osKey }];
  switch (os) {
    case "windows":
      return { label: forOs("dl.os.windows"), href: url("windows"), card: "windows" };
    case "mac":
      if (arch === "arm") return { label: forOs("dl.os.mac"), href: url("macArm"), card: "macArm" };
      if (arch === "x86") return { label: forOs("dl.os.mac"), href: url("macIntel"), card: "macIntel" };
      // Unknown chip (Safari, Firefox): Apple Silicon first, with the Intel build right below.
      return {
        label: forOs("dl.os.macArm"),
        href: url("macArm"),
        card: "macArm",
        alt: { text: "dl.alt.intel", link: "dl.alt.intelLink", href: url("macIntel") },
      };
    case "linux":
      // Only x86-64 Linux builds exist: never hand an ARM machine the amd64 AppImage.
      if (arch === "arm") {
        return { label: ["dl.hero.generic", {}], href: RELEASES_PAGE, card: null, alt: { text: "dl.alt.linuxArm" } };
      }
      return {
        label: forOs("dl.os.linux"),
        href: url("appImage"),
        card: "appImage",
        alt: { text: "dl.alt.deb", link: "dl.alt.debLink", href: url("deb") },
      };
    case "android":
      return { label: forOs("dl.os.android"), href: url("android"), card: "android" };
    case "ios":
      return { label: ["dl.hero.ios", {}], href: "#downloads", card: "ios", alt: { text: "dl.alt.ios" } };
    default:
      return { label: ["dl.hero.generic", {}], href: RELEASES_PAGE, card: null };
  }
}

// "v0.2.0" from a release, or null when the answer has no usable tag.
export function versionOf(release) {
  const tag = typeof release?.tag_name === "string" ? release.tag_name.trim() : "";
  if (!/^v?\d+(\.\d+)*([-.+][\w.]+)?$/.test(tag)) return null;
  return tag.startsWith("v") ? tag : `v${tag}`;
}

// The CPU architecture from client hints ("arm", "x86"…), or "" when the browser has none,
// declines or takes longer than `timeoutMs` to answer. Takes navigator.userAgentData.
export async function readArchitecture(userAgentData, timeoutMs = HINTS_TIMEOUT_MS) {
  if (typeof userAgentData?.getHighEntropyValues !== "function") return "";
  let timer;
  const timeout = new Promise((resolve) => {
    timer = setTimeout(() => resolve(null), timeoutMs);
  });
  try {
    const hints = await Promise.race([userAgentData.getHighEntropyValues(["architecture"]), timeout]);
    return typeof hints?.architecture === "string" ? hints.architecture : "";
  } catch {
    return ""; // The browser declined: the hero offers both Mac builds.
  } finally {
    clearTimeout(timer);
  }
}

// ---------- Browser side ----------

function readPlatform(architecture = "") {
  const data = navigator.userAgentData;
  return detectPlatform({
    userAgent: navigator.userAgent,
    platform: navigator.platform,
    maxTouchPoints: navigator.maxTouchPoints,
    uaPlatform: data?.platform ?? "",
    architecture,
  });
}

function readCache() {
  try {
    const cached = JSON.parse(window.localStorage.getItem(CACHE_KEY));
    if (cached && Date.now() - cached.at < CACHE_MS) return cached.release;
  } catch {
    // Nothing cached, or storage is blocked.
  }
  return null;
}

function writeCache(release) {
  try {
    window.localStorage.setItem(CACHE_KEY, JSON.stringify({ at: Date.now(), release }));
  } catch {
    // Storage is blocked: the next visit asks the API again.
  }
}

async function fetchRelease() {
  const cached = readCache();
  if (cached) return cached;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), TIMEOUT_MS);
  try {
    const response = await fetch(RELEASE_API, {
      headers: { Accept: "application/vnd.github+json" },
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`GitHub API answered ${response.status}`);
    const body = await response.json();
    // Keep only what the page uses.
    const release = {
      tag_name: body.tag_name,
      assets: (body.assets ?? []).map(({ name, browser_download_url }) => ({ name, browser_download_url })),
    };
    writeCache(release);
    return release;
  } finally {
    clearTimeout(timer);
  }
}

function renderHero(choice) {
  const button = document.querySelector("[data-download-hero]");
  if (button) {
    button.href = choice.href;
    const [key, vars] = choice.label;
    button.querySelector("[data-download-label]").textContent = t(key, vars.os ? { os: t(vars.os) } : {});
  }
  const alt = document.querySelector("[data-download-alt]");
  if (!alt) return;
  alt.replaceChildren();
  alt.hidden = !choice.alt;
  if (!choice.alt) return;
  alt.append(t(choice.alt.text));
  if (choice.alt.link) {
    const link = document.createElement("a");
    link.href = choice.alt.href;
    link.textContent = t(choice.alt.link);
    alt.append(" ", link);
  }
}

function renderRelease(found, version) {
  for (const [key, asset] of Object.entries(found)) {
    for (const link of document.querySelectorAll(`[data-asset="${key}"]`)) link.href = asset.url;
    for (const label of document.querySelectorAll(`[data-asset-name="${key}"]`)) label.textContent = asset.name;
  }
  if (version) {
    for (const element of document.querySelectorAll("[data-version]")) element.textContent = version;
  }
}

function markCard(card) {
  for (const element of document.querySelectorAll("[data-card]")) {
    element.classList.toggle("is-yours", element.dataset.card === card);
  }
}

export async function initDownloads() {
  // Name the visitor's system straight away from the user agent; the client hints (the Mac
  // chip, an ARM Linux) and the release data refine the choice when they arrive.
  let platform = readPlatform();
  let found = {};
  let choice = heroChoice(platform, found);
  const render = () => {
    renderHero(choice);
    markCard(choice.card);
  };
  render();
  document.addEventListener(LANGUAGE_EVENT, () => renderHero(choice));

  const refined = readArchitecture(navigator.userAgentData).then((architecture) => {
    if (!architecture) return;
    platform = readPlatform(architecture);
    choice = heroChoice(platform, found);
    render();
  });

  try {
    const release = await fetchRelease();
    found = matchAssets(release.assets);
    renderRelease(found, versionOf(release));
    choice = heroChoice(platform, found);
    render();
  } catch (error) {
    // Every link already opens the releases page.
    console.info("BandEar: using the releases page for downloads.", error?.message ?? error);
  }
  await refined;
}
