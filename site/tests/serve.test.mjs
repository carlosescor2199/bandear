// Tests for the preview server's request resolver: it never serves a file outside site/, and
// answers bad methods and malformed URLs plainly.
// Run: node --test 'site/tests/*.test.mjs'

import { test } from "node:test";
import assert from "node:assert/strict";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { resolveRequest } from "../dev/resolve.mjs";

const ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));

test("serves files under the root, with index.html for folders", () => {
  assert.deepEqual(resolveRequest("GET", "/", ROOT), { file: join(ROOT, "index.html"), pathname: "/", directory: true });
  assert.deepEqual(resolveRequest("HEAD", "/js/main.js?v=1", ROOT), {
    file: join(ROOT, "js", "main.js"),
    pathname: "/js/main.js",
    directory: false,
  });
  assert.equal(resolveRequest("GET", "/assets/", ROOT).file, join(ROOT, "assets", "index.html"));
  // Percent-encoded names are decoded.
  assert.equal(resolveRequest("GET", "/a%20b.txt", ROOT).file, join(ROOT, "a b.txt"));
});

test("a folder without its trailing slash is reported as a file path, for the redirect", () => {
  const target = resolveRequest("GET", "/assets", ROOT);
  assert.equal(target.directory, false);
  assert.equal(target.file, join(ROOT, "assets"));
  assert.equal(target.pathname, "/assets");
});

test("never resolves outside the root", () => {
  for (const url of ["/..%2F..%2Fsecret.txt", "/%2e%2e/%2e%2e/secret.txt", "/..%5C..%5Csecret.txt", "/js/..%2F..%2F..%2Fx"]) {
    const target = resolveRequest("GET", url, ROOT);
    if (target.file !== undefined) {
      assert.ok(target.file.startsWith(ROOT), `${url} resolved to ${target.file}`);
    } else {
      assert.equal(target.status, 404, url);
    }
  }
  assert.deepEqual(resolveRequest("GET", "/..%2F..%2Fsecret.txt", ROOT), { status: 404 });
});

test("malformed percent-encoding and NUL bytes are bad requests", () => {
  assert.deepEqual(resolveRequest("GET", "/%E0%A4%A", ROOT), { status: 400 });
  assert.deepEqual(resolveRequest("GET", "/%", ROOT), { status: 400 });
  assert.deepEqual(resolveRequest("GET", "/index.html%00.png", ROOT), { status: 400 });
});

test("only GET and HEAD are allowed", () => {
  for (const method of ["POST", "PUT", "DELETE", "OPTIONS", "PATCH"]) {
    assert.deepEqual(resolveRequest(method, "/", ROOT), { status: 405 }, method);
  }
});
