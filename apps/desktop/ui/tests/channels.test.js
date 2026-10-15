import test from "node:test";
import assert from "node:assert/strict";

import {
  CHANNEL_KINDS,
  MAX_CHANNEL_NAME_LENGTH,
  balanceToPans,
  channelLabel,
  checkChannels,
  fitChannels,
  keepPairsEqual,
  pansToBalance,
} from "../app/lib/channels.js";
import { defaultMix } from "../app/lib/mix.js";

function detail(name, kind = "other", { hidden = false, link = false } = {}) {
  return { name, kind, hidden, link };
}

test("the instrument kinds are the engine's, in the order a picker offers them", () => {
  assert.deepEqual(CHANNEL_KINDS, [
    "voice",
    "guitar",
    "bass",
    "keys",
    "drums",
    "percussion",
    "horns",
    "strings",
    "playback",
    "click",
    "other",
  ]);
});

test("a channel is called by its name, or by its number when it has none", () => {
  assert.equal(channelLabel({ number: 1, name: "Kick" }), "Kick");
  assert.equal(channelLabel({ number: 3, name: "" }), "CH 3");
  assert.equal(channelLabel({ number: 4, name: "   " }), "CH 4");
  assert.equal(channelLabel({ number: 5 }), "CH 5", "a status from before channel names");
});

test("a pair's balance writes both pans, and the two pans read back as the balance", () => {
  assert.deepEqual(balanceToPans(50), [0, 100], "centre is the full stereo width");
  assert.deepEqual(balanceToPans(0), [0, 0]);
  assert.deepEqual(balanceToPans(100), [100, 100]);
  assert.deepEqual(balanceToPans(70), [40, 100]);
  assert.deepEqual(balanceToPans(30), [0, 60]);
  assert.deepEqual(balanceToPans(130), [100, 100], "clamped like any level");
  for (let balance = 0; balance <= 100; balance += 1) {
    assert.equal(pansToBalance(...balanceToPans(balance)), balance, `balance ${balance}`);
  }
  assert.equal(pansToBalance(0, 100), 50, "the pans a link sets read as centre");
});

test("channel details are checked as the engine checks them", () => {
  assert.doesNotThrow(() =>
    checkChannels([
      detail("Kick", "drums"),
      detail("Keys L", "keys", { link: true }),
      detail("Keys R", "keys"),
      detail("Spare L", "other", { hidden: true, link: true }),
      detail("Spare R", "other", { hidden: true }),
    ]),
  );

  assert.doesNotThrow(
    () => checkChannels([{ kind: "keys", link: true }, { kind: "keys" }, {}]),
    "an entry may leave out its name (and its kind, which is other)",
  );

  for (const [refused, named] of [
    [[detail("Kazoo", "kazoo")], /kazoo/],
    [Array.from({ length: 33 }, () => detail("")), /33/],
    [[detail("x".repeat(MAX_CHANNEL_NAME_LENGTH + 1))], /CH 1/],
    [[detail("Kick", "drums"), detail("Pad", "keys", { link: true })], /CH 2/],
    [
      [
        detail("A", "keys", { link: true }),
        detail("B", "keys", { link: true }),
        detail("C", "keys"),
      ],
      /CH 2/,
    ],
    [[detail("L", "keys", { link: true }), detail("R", "keys", { hidden: true })], /CH 1/],
  ]) {
    assert.throws(() => checkChannels(refused), named);
  }
});

test("channel details fit a device: kept where it has the input, defaults past them, no half pair", () => {
  const details = [
    detail("Kick", "drums"),
    detail("Spare", "other", { hidden: true }),
    detail("Keys L", "keys", { link: true }),
    detail("Keys R", "keys"),
  ];

  assert.deepEqual(fitChannels(details, 4), details);
  assert.deepEqual(fitChannels(details, 3), [
    detail("Kick", "drums"),
    detail("Spare", "other", { hidden: true }),
    detail("Keys L", "keys"),
  ]);
  assert.deepEqual(fitChannels(details.slice(0, 1), 2), [detail("Kick", "drums"), detail("")]);
  assert.equal(details[2].link, true, "the details given are left as they were");
});

test("both sides of a pair keep one level, mute and solo, as the engine keeps them", () => {
  const channels = [detail("Keys L", "keys", { link: true }), detail("Keys R", "keys"), detail("")];
  const before = defaultMix();
  const keep = (changes) => keepPairsEqual(before, { ...before, ...changes }, channels);

  assert.deepEqual(
    keep({ channelGains: [80, ...before.channelGains.slice(1)] }).channelGains.slice(0, 3),
    [80, 80, 100],
    "the first side moves the second",
  );
  assert.deepEqual(
    keep({ channelGains: [100, 40, ...before.channelGains.slice(2)] }).channelGains.slice(0, 2),
    [40, 40],
    "the second side moves the first",
  );
  assert.deepEqual(
    keep({ channelGains: [30, 60, ...before.channelGains.slice(2)] }).channelGains.slice(0, 2),
    [30, 30],
    "both changed to different values: the first channel wins",
  );
  assert.deepEqual(
    keep({ channelMuted: [false, true, ...before.channelMuted.slice(2)] }).channelMuted.slice(0, 2),
    [true, true],
  );
  assert.deepEqual(
    keep({ channelSolo: [true, ...before.channelSolo.slice(1)] }).channelSolo.slice(0, 2),
    [true, true],
  );
  const panned = keep({ pans: [20, 70, ...before.pans.slice(2)] });
  assert.deepEqual(panned.pans.slice(0, 2), [20, 70], "a balance writes both pans as they are");
  const outside = keep({ channelGains: [100, 100, 50, ...before.channelGains.slice(3)] });
  assert.deepEqual(outside.channelGains.slice(2, 4), [50, 100], "CH 3 is no pair");
});
