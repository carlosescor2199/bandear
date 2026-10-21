// Tests for the download button's pure helpers: platform detection, asset matching and the
// choice the hero offers each system.
// Run: powershell.exe -NoProfile -Command "fnm exec --using=default node --test 'site/tests/*.test.mjs'"

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  ASSETS,
  detectPlatform,
  heroChoice,
  matchAssets,
  readArchitecture,
  versionOf,
} from "../js/downloads.js";
import { DICTIONARY, LANGUAGES } from "../js/i18n.js";
import { RELEASES_PAGE } from "../js/links.js";

const UA = {
  windows: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
  macSafari: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15",
  macChrome: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
  linux: "Mozilla/5.0 (X11; Linux x86_64; rv:144.0) Gecko/20100101 Firefox/144.0",
  android: "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Mobile Safari/537.36",
  iphone: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.6 Mobile/15E148 Safari/604.1",
  chromebook: "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
};

// The v0.2.0 release as the GitHub API lists it (names trimmed to what matters).
const RELEASE = {
  tag_name: "v0.2.0",
  assets: [
    "BandEar_0.2.0_aarch64.dmg",
    "BandEar_0.2.0_amd64.AppImage",
    "BandEar_0.2.0_amd64.deb",
    "BandEar_0.2.0_android.apk",
    "BandEar_0.2.0_x64-setup.exe",
    "BandEar_0.2.0_x64.dmg",
    "BandEar_aarch64.app.tar.gz",
    "BandEar_x64.app.tar.gz",
    "latest.json",
  ].map((name) => ({ name, browser_download_url: `https://github.com/carlosescor2199/bandear/releases/download/v0.2.0/${name}` })),
};

test("detects each desktop system from its user agent", () => {
  assert.deepEqual(detectPlatform({ userAgent: UA.windows }), { os: "windows", arch: null });
  assert.deepEqual(detectPlatform({ userAgent: UA.macSafari }), { os: "mac", arch: null });
  assert.deepEqual(detectPlatform({ userAgent: UA.linux }), { os: "linux", arch: null });
});

test("tells phones apart from the desktops their user agents mention", () => {
  // Android user agents also say "Linux"; iPhones say "like Mac OS X".
  assert.deepEqual(detectPlatform({ userAgent: UA.android }), { os: "android", arch: null });
  assert.deepEqual(detectPlatform({ userAgent: UA.iphone }), { os: "ios", arch: null });
});

test("an iPad asking for the desktop site is still iOS", () => {
  assert.equal(detectPlatform({ userAgent: UA.macSafari, platform: "MacIntel", maxTouchPoints: 5 }).os, "ios");
  assert.equal(detectPlatform({ userAgent: UA.macSafari, platform: "MacIntel", maxTouchPoints: 0 }).os, "mac");
});

test("uses the Mac chip from client hints when the browser shares it", () => {
  assert.deepEqual(detectPlatform({ userAgent: UA.macChrome, uaPlatform: "macOS", architecture: "arm" }), { os: "mac", arch: "arm" });
  assert.deepEqual(detectPlatform({ userAgent: UA.macChrome, uaPlatform: "macOS", architecture: "x86" }), { os: "mac", arch: "x86" });
  assert.deepEqual(detectPlatform({ userAgent: UA.macChrome, architecture: "riscv" }), { os: "mac", arch: null });
});

test("falls back to platform hints and treats Chromebooks and unknowns generically", () => {
  assert.equal(detectPlatform({ userAgent: "", uaPlatform: "Windows" }).os, "windows");
  assert.equal(detectPlatform({ userAgent: "", platform: "Win32" }).os, "windows");
  assert.equal(detectPlatform({ userAgent: UA.chromebook }).os, "unknown");
  assert.equal(detectPlatform({ userAgent: "SomeBot/1.0" }).os, "unknown");
  assert.equal(detectPlatform().os, "unknown");
});

test("matches every installer by its suffix and ignores the updater files", () => {
  const found = matchAssets(RELEASE.assets);
  assert.deepEqual(Object.keys(found).sort(), Object.keys(ASSETS).sort());
  assert.equal(found.windows.name, "BandEar_0.2.0_x64-setup.exe");
  assert.equal(found.macArm.name, "BandEar_0.2.0_aarch64.dmg");
  assert.equal(found.macIntel.name, "BandEar_0.2.0_x64.dmg");
  assert.equal(found.appImage.name, "BandEar_0.2.0_amd64.AppImage");
  assert.equal(found.deb.name, "BandEar_0.2.0_amd64.deb");
  assert.equal(found.android.name, "BandEar_0.2.0_android.apk");
  assert.match(found.android.url, /\/download\/v0\.2\.0\/BandEar_0\.2\.0_android\.apk$/);
});

test("matching tolerates missing or malformed assets", () => {
  assert.deepEqual(matchAssets(), {});
  assert.deepEqual(matchAssets([null, { name: 3 }, { name: "x_android.apk" }]), {});
  assert.deepEqual(Object.keys(matchAssets([{ name: "a_android.apk", browser_download_url: "u" }])), ["android"]);
});

test("the hero offers each system its own installer", () => {
  const found = matchAssets(RELEASE.assets);
  const windows = heroChoice({ os: "windows", arch: null }, found);
  assert.deepEqual(windows.label, ["dl.hero.for", { os: "dl.os.windows" }]);
  assert.equal(windows.href, found.windows.url);
  assert.equal(heroChoice({ os: "android", arch: null }, found).href, found.android.url);
  assert.equal(heroChoice({ os: "mac", arch: "arm" }, found).href, found.macArm.url);
  assert.equal(heroChoice({ os: "mac", arch: "x86" }, found).href, found.macIntel.url);

  const linux = heroChoice({ os: "linux", arch: null }, found);
  assert.equal(linux.href, found.appImage.url);
  assert.equal(linux.alt.href, found.deb.url);
});

test("reads an ARM CPU from a Linux user agent when client hints are missing", () => {
  const firefoxArm = "Mozilla/5.0 (X11; Linux aarch64; rv:144.0) Gecko/20100101 Firefox/144.0";
  assert.deepEqual(detectPlatform({ userAgent: firefoxArm }), { os: "linux", arch: "arm" });
  assert.deepEqual(detectPlatform({ userAgent: UA.linux, platform: "Linux armv7l" }), { os: "linux", arch: "arm" });
  assert.deepEqual(detectPlatform({ userAgent: UA.linux, uaPlatform: "Linux", architecture: "arm" }), { os: "linux", arch: "arm" });
  // A client hint wins over the user agent.
  assert.deepEqual(detectPlatform({ userAgent: firefoxArm, architecture: "x86" }), { os: "linux", arch: "x86" });
});

test("ARM Linux is sent to the releases page, never to the x86-64 AppImage", () => {
  const found = matchAssets(RELEASE.assets);
  const arm = heroChoice({ os: "linux", arch: "arm" }, found);
  assert.equal(arm.href, RELEASES_PAGE);
  assert.notEqual(arm.href, found.appImage.url);
  assert.deepEqual(arm.label, ["dl.hero.generic", {}]);
  assert.equal(arm.card, null);
  assert.deepEqual(arm.alt, { text: "dl.alt.linuxArm" });
  // x86-64 (or an unknown CPU) still gets the AppImage.
  assert.equal(heroChoice({ os: "linux", arch: "x86" }, found).href, found.appImage.url);
});

test("every string the hero can show exists in both languages", () => {
  const found = matchAssets(RELEASE.assets);
  const platforms = ["windows", "mac", "linux", "android", "ios", "unknown"].flatMap((os) =>
    [null, "arm", "x86"].map((arch) => ({ os, arch })),
  );
  for (const platform of platforms) {
    const { label, alt } = heroChoice(platform, found);
    const keys = [label[0], label[1].os, alt?.text, alt?.link].filter(Boolean);
    for (const key of keys) {
      for (const lang of LANGUAGES) assert.ok(key in DICTIONARY[lang], `${lang} lacks ${key} (${platform.os}/${platform.arch})`);
    }
  }
});

test("a Mac with an unknown chip gets Apple Silicon first and Intel right below", () => {
  const found = matchAssets(RELEASE.assets);
  const mac = heroChoice({ os: "mac", arch: null }, found);
  assert.deepEqual(mac.label, ["dl.hero.for", { os: "dl.os.macArm" }]);
  assert.equal(mac.href, found.macArm.url);
  assert.equal(mac.alt.href, found.macIntel.url);
});

test("iPhones are told iOS is coming and sent to the downloads section", () => {
  const ios = heroChoice({ os: "ios", arch: null }, matchAssets(RELEASE.assets));
  assert.deepEqual(ios.label, ["dl.hero.ios", {}]);
  assert.equal(ios.href, "#downloads");
});

test("without release data every choice opens the releases page", () => {
  for (const os of ["windows", "mac", "linux", "android", "unknown"]) {
    assert.equal(heroChoice({ os, arch: null }).href, RELEASES_PAGE, os);
  }
  assert.deepEqual(heroChoice({ os: "unknown", arch: null }).label, ["dl.hero.generic", {}]);
});

test("readArchitecture returns the client hint, or nothing rather than waiting", async () => {
  assert.equal(await readArchitecture(undefined), "");
  assert.equal(await readArchitecture({}), "");
  assert.equal(await readArchitecture({ getHighEntropyValues: async () => ({ architecture: "arm" }) }), "arm");
  assert.equal(await readArchitecture({ getHighEntropyValues: async () => ({}) }), "");
  assert.equal(await readArchitecture({ getHighEntropyValues: () => Promise.reject(new Error("denied")) }), "");
  assert.equal(
    await readArchitecture({
      getHighEntropyValues() {
        throw new Error("sync failure");
      },
    }),
    "",
  );
  const started = Date.now();
  const never = { getHighEntropyValues: () => new Promise(() => {}) };
  assert.equal(await readArchitecture(never, 30), "");
  assert.ok(Date.now() - started < 1000, "the timeout did not cut the wait short");
});

test("reads the version from the release tag", () => {
  assert.equal(versionOf(RELEASE), "v0.2.0");
  assert.equal(versionOf({ tag_name: "1.4.0-rc.1" }), "v1.4.0-rc.1");
  assert.equal(versionOf({ tag_name: "<script>" }), null);
  assert.equal(versionOf({}), null);
  assert.equal(versionOf(null), null);
});

test("index.html tags a link and a file name for every installer", () => {
  const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
  for (const key of Object.keys(ASSETS)) {
    assert.match(html, new RegExp(`data-asset="${key}"`), `no link for ${key}`);
    assert.match(html, new RegExp(`data-asset-name="${key}"`), `no file name for ${key}`);
  }
  assert.match(html, /data-download-hero/);
  assert.match(html, /data-download-label/);
});

test("the markup's fallback download links are the releases page from links.js", () => {
  const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
  const notFound = readFileSync(new URL("../404.html", import.meta.url), "utf8");
  const fallbacks = [...html.matchAll(/<a [^>]*\b(?:data-asset|data-download-hero)\b[^>]*>/g)].map((match) => match[0]);
  assert.ok(fallbacks.length >= Object.keys(ASSETS).length + 1, "download links not found");
  for (const tag of fallbacks) assert.match(tag, new RegExp(`href="${RELEASES_PAGE}"`), tag);
  assert.ok(notFound.includes(`href="${RELEASES_PAGE}"`), "404.html download link differs from links.js");
});
