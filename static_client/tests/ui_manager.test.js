import test from "node:test";
import assert from "node:assert/strict";

import {
  hydrateBoundedIdentitySet,
  rememberBoundedIdentity,
} from "../client_logic/UIManager.js";

test("rememberBoundedIdentity caps set size and keeps newest identities", () => {
  const identities = new Set();

  for (let i = 0; i < 300; i += 1) {
    rememberBoundedIdentity(identities, `player-${i}`);
  }

  assert.equal(identities.size, 256);
  assert.equal(identities.has("player-0"), false);
  assert.equal(identities.has("player-43"), false);
  assert.equal(identities.has("player-44"), true);
  assert.equal(identities.has("player-299"), true);
});

test("rememberBoundedIdentity normalizes names and refreshes recency", () => {
  const identities = new Set();
  rememberBoundedIdentity(identities, " Alpha ");
  rememberBoundedIdentity(identities, "beta");
  rememberBoundedIdentity(identities, "alpha");

  assert.deepEqual(Array.from(identities), ["beta", "alpha"]);
});

test("hydrateBoundedIdentitySet keeps only the most recent bounded identities", () => {
  const identities = new Set();
  const raw = Array.from({ length: 260 }, (_, index) => ` Player-${index} `);

  hydrateBoundedIdentitySet(identities, raw, { normalize: true, limit: 4 });

  assert.deepEqual(Array.from(identities), [
    "player-256",
    "player-257",
    "player-258",
    "player-259",
  ]);
});
