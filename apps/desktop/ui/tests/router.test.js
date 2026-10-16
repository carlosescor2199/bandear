import test from "node:test";
import assert from "node:assert/strict";

import { DEFAULT_ROUTE, ROUTES, documentTitle, resolveRoute } from "../app/lib/router.js";

test("the sidebar holds the sections built so far, in the design's order, Mix first", () => {
  assert.deepEqual(
    ROUTES.map(({ id, hash, label }) => [id, hash, label]),
    [
      ["mix", "#/mix", "Mix"],
      ["channels", "#/channels", "Channels and groups"],
      ["network", "#/network", "Network and system"],
    ],
  );
  assert.equal(DEFAULT_ROUTE.id, "mix");
});

test("a hash opens its section", () => {
  assert.equal(resolveRoute("#/mix").id, "mix");
  assert.equal(resolveRoute("#/channels").id, "channels");
  assert.equal(resolveRoute("#/network").id, "network");
});

test("a hash written a little differently still opens its section", () => {
  assert.equal(resolveRoute("#/network/").id, "network");
  assert.equal(resolveRoute("#/Network").id, "network");
  assert.equal(resolveRoute("#network").id, "network");
});

test("no hash, or one that names no section, opens the Mix", () => {
  for (const hash of ["", "#", "#/", "#/record", "#main", undefined]) {
    assert.equal(resolveRoute(hash).id, "mix", String(hash));
  }
});

test("the window title names the section", () => {
  assert.equal(documentTitle(resolveRoute("#/network")), "Network and system · MixLink");
});
