import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_ROUTE,
  ROUTES,
  canonicalHash,
  documentTitle,
  processingHash,
  resolveRoute,
  routeChannel,
} from "../app/lib/router.js";

test("the sidebar holds the sections built so far, in the design's order, Mix first", () => {
  assert.deepEqual(
    ROUTES.map(({ id, hash, label }) => [id, hash, label]),
    [
      ["mix", "#/mix", "Mix"],
      ["record", "#/record", "Record"],
      ["channels", "#/channels", "Channels and groups"],
      ["processing", "#/processing", "Processing"],
      ["effects", "#/effects", "Effects"],
      ["network", "#/network", "Network and system"],
    ],
  );
  assert.equal(DEFAULT_ROUTE.id, "mix");
});

test("a hash opens its section", () => {
  assert.equal(resolveRoute("#/mix").id, "mix");
  assert.equal(resolveRoute("#/record").id, "record");
  assert.equal(resolveRoute("#/channels").id, "channels");
  assert.equal(resolveRoute("#/processing").id, "processing");
  assert.equal(resolveRoute("#/effects").id, "effects");
  assert.equal(resolveRoute("#/network").id, "network");
});

test("a hash written a little differently still opens its section", () => {
  assert.equal(resolveRoute("#/network/").id, "network");
  assert.equal(resolveRoute("#/Network").id, "network");
  assert.equal(resolveRoute("#network").id, "network");
});

test("no hash, or one that names no section, opens the Mix", () => {
  for (const hash of ["", "#", "#/", "#/reverb", "#main", "#/mix/3", undefined]) {
    assert.equal(resolveRoute(hash).id, "mix", String(hash));
  }
});

test("the processing editor's hash names the channel it opens on", () => {
  assert.equal(processingHash(5), "#/processing/5");
  assert.equal(resolveRoute("#/processing/5").id, "processing");
  assert.equal(routeChannel("#/processing/5"), 5);
  assert.equal(routeChannel("#/Processing/12/"), 12);
  for (const hash of ["#/processing", "#/processing/0", "#/processing/33", "#/processing/x", "#/mix"]) {
    assert.equal(routeChannel(hash), null, hash);
  }
  assert.equal(canonicalHash("#/processing/5/"), "#/processing/5");
  assert.equal(canonicalHash("#/processing/99"), "#/processing");
  assert.equal(canonicalHash("#/Network"), "#/network");
  assert.equal(canonicalHash(""), "#/mix");
});

test("the window title names the section", () => {
  assert.equal(documentTitle(resolveRoute("#/network")), "Network and system · BandEar");
});
