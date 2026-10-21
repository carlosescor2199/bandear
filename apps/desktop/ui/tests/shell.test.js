// The sidebar's brand row (brandMarkup, shell.js): the BandEar mark, as a decorative image whose
// file is there relative to index.html. shell.js loads the custom elements and looks for the page
// when it is imported, so it is imported in a child process with just enough of a DOM stubbed in,
// which keeps those stubs out of the other suites.
import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

const UI = new URL("../", import.meta.url);
const SHELL = new URL("app/shell.js", UI);

function brandMarkup() {
  const script = `
    globalThis.HTMLElement = class {};
    globalThis.CSSStyleSheet = class { replaceSync() {} };
    globalThis.customElements = { get: () => undefined, define: () => {} };
    globalThis.document = { querySelector: () => null };
    globalThis.window = { location: { hash: "", search: "" }, addEventListener: () => {} };
    const shell = await import(${JSON.stringify(SHELL.href)});
    process.stdout.write(shell.brandMarkup());
  `;
  return execFileSync(process.execPath, ["--input-type=module", "-e", script], {
    encoding: "utf8",
  });
}

test("the sidebar brand row shows the BandEar mark as a decorative image", () => {
  const markup = brandMarkup();
  const images = markup.match(/<img\b[^>]*>/g) ?? [];
  assert.equal(images.length, 1, markup);
  const [image] = images;
  assert.match(image, /\ssrc="app\/brand\/bandear-mark\.svg"/);
  assert.match(image, /\salt=""/, "the name beside it says it, so the mark is decorative");
  assert.match(markup, /<span class="brand-name">BandEar<\/span>/);
});

test("the mark's file is there relative to index.html", () => {
  const src = brandMarkup().match(/<img\b[^>]*\ssrc="([^"]+)"/)[1];
  assert.ok(existsSync(new URL("index.html", UI)), "index.html is the page the shell runs in");
  assert.ok(existsSync(fileURLToPath(new URL(src, UI))), src);
});
