// Tests for the page copy: both languages cover the same keys, and the markup only names keys
// the dictionary has.
// Run: powershell.exe -NoProfile -Command "fnm exec --using=default node --test 'site/tests/*.test.mjs'"

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { DICTIONARY, LANGUAGES, keysInHtml, parseAttrSpec, pickLanguage, translate } from "../js/i18n.js";

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const sceneFlow = readFileSync(new URL("../js/scene-flow.js", import.meta.url), "utf8");

test("English and Spanish define exactly the same keys", () => {
  const en = Object.keys(DICTIONARY.en).sort();
  for (const lang of LANGUAGES) {
    assert.deepEqual(Object.keys(DICTIONARY[lang]).sort(), en, `${lang} keys differ from en`);
  }
});

test("no string is empty in either language", () => {
  for (const lang of LANGUAGES) {
    for (const [key, value] of Object.entries(DICTIONARY[lang])) {
      assert.ok(typeof value === "string" && value.trim().length > 0, `${lang}.${key} is empty`);
    }
  }
});

test("every key index.html names exists in the dictionary", () => {
  const keys = keysInHtml(html);
  assert.ok(keys.size > 80, `expected the page to be tagged, found ${keys.size} keys`);
  for (const key of keys) {
    for (const lang of LANGUAGES) assert.ok(key in DICTIONARY[lang], `${key} is missing from ${lang}`);
  }
});

test("the 3D scene's labels name dictionary keys", () => {
  const block = sceneFlow.match(/const LABELS = \[([^\]]+)\]/);
  assert.ok(block, "LABELS not found in scene-flow.js");
  const keys = [...block[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
  assert.equal(keys.length, 5);
  for (const key of keys) assert.ok(key in DICTIONARY.en && key in DICTIONARY.es, `${key} is missing`);
});

test("the English dictionary matches the copy shipped in the markup", () => {
  for (const [, key, text] of html.matchAll(/data-i18n="([^"]+)"[^>]*>([^<]+)</g)) {
    assert.equal(DICTIONARY.en[key], text.trim(), `index.html and en.${key} disagree`);
  }
});

test("pickLanguage prefers a stored choice, then a Spanish browser", () => {
  assert.equal(pickLanguage("es", ["en-US"]), "es");
  assert.equal(pickLanguage("en", ["es-MX"]), "en");
  assert.equal(pickLanguage(null, ["es-419", "en"]), "es");
  assert.equal(pickLanguage(null, ["ES"]), "es");
  assert.equal(pickLanguage(null, ["en-GB", "es"]), "en");
  assert.equal(pickLanguage("fr", ["de-DE"]), "en");
  assert.equal(pickLanguage(null, []), "en");
  assert.equal(pickLanguage(null, [undefined]), "en");
});

test("translate fills placeholders and falls back to English, then the key", () => {
  assert.equal(translate("es", "dl.hero.for", { os: "Linux" }), "Descargar para Linux");
  assert.equal(translate("en", "dl.hero.for", { os: "Linux" }), "Download for Linux");
  assert.equal(translate("en", "dl.hero.for"), "Download for {os}");
  assert.equal(translate("xx", "nav.faq"), "FAQ");
  assert.equal(translate("es", "no.such.key"), "no.such.key");
});

test("parseAttrSpec reads attribute=key pairs", () => {
  assert.deepEqual(parseAttrSpec("aria-label=a.b; lang=c"), [
    ["aria-label", "a.b"],
    ["lang", "c"],
  ]);
  assert.deepEqual(parseAttrSpec(""), []);
});
