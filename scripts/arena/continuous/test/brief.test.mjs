import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  BRIEF_MAX_BYTES,
  buildBrief,
  sampleModelBattles,
} from '../brief.mjs';

const ME = 'vendor/model-a';

function model(overrides = {}) {
  return {
    model_id: ME,
    rating: 42,
    wins: 6,
    losses: 9,
    draws: 1,
    artifact: { version: 2 },
    ...overrides,
  };
}

function rec(overrides = {}) {
  return {
    mode: 'arena',
    me: ME,
    opponent: 'vendor/weak',
    winner: ME,
    draw: false,
    counts: { idle: 1, attack: 5, defend: 2, charge: 1, support: 1 },
    faults: { trap: 0, fuel: 0, fallback: 0 },
    m: 1000,
    ...overrides,
  };
}

function lossRecord(opponent, mode, m) {
  return rec({ opponent, mode, winner: opponent, m });
}

function winRecord(opponent, mode, m) {
  return rec({ opponent, mode, winner: ME, m });
}

// 20 games: ctf 1W-8L (loses 89%), arena 8W-3L (loses 27%); rivals:
// strong 1W-7L, mid 3W-3L, weak 5W-1L. Two traps and one fuel error.
function fixtureRecords() {
  const records = [];
  let m = 100;
  const push = (record) => { m += 1; records.push({ ...record, m }); };
  for (let i = 0; i < 7; i += 1) push(lossRecord('vendor/strong', 'ctf'));
  push(winRecord('vendor/strong', 'ctf'));
  for (let i = 0; i < 3; i += 1) push(lossRecord('vendor/mid', 'arena'));
  for (let i = 0; i < 3; i += 1) push(winRecord('vendor/mid', 'arena'));
  push(lossRecord('vendor/weak', 'ctf'));
  for (let i = 0; i < 5; i += 1) push(winRecord('vendor/weak', 'arena'));
  records[0].faults = { trap: 2, fuel: 1, fallback: 0 };
  return records;
}

test('buildBrief reports fingerprint, per-mode weakness, worst matchups, faults', () => {
  const brief = buildBrief({ model: model(), records: fixtureRecords() });
  assert.ok(Buffer.byteLength(brief, 'utf8') <= BRIEF_MAX_BYTES);
  assert.match(brief, /improvement brief — vendor\/model-a/);
  assert.match(brief, /artifact v2/);
  assert.match(brief, /Sampled 20 recent battles: 9W-11L-0D\./);
  assert.match(brief, /Behavior fingerprint: idle 10%, attack 50%, defend 20%, charge 10%, support 10%/);
  assert.match(brief, /aggression 0\.60/);
  assert.match(brief, /ctf 1-8-0 \(loses 89%\)/);
  assert.match(brief, /arena 8-3-0 \(loses 27%\)/);
  // Worst matchup first: vendor/strong at 88% losses.
  assert.match(brief, /Worst matchups: vs vendor\/strong 1-7-0 \(loses 88%\); vs vendor\/mid 3-3-0 \(loses 50%\); vs vendor\/weak 5-1-0 \(loses 17%\)\./);
  assert.match(brief, /Runtime faults in sample: traps 2, fuel errors 1, fallbacks 0\./);
  assert.match(brief, /weakest mode is ctf \(loses 89%\)/);
  assert.match(brief, /you lose most games against vendor\/strong/);
  assert.match(brief, /eliminate the 3 runtime traps\/fuel errors\/fallbacks/);
});

test('buildBrief stays under the byte cap with many rivals and long ids', () => {
  const longId = `vendor/${'x'.repeat(120)}`;
  const records = [];
  for (let i = 0; i < 60; i += 1) {
    records.push(rec({
      me: longId,
      opponent: `vendor/opponent-${i}-${'y'.repeat(100)}`,
      winner: `vendor/opponent-${i}-${'y'.repeat(100)}`,
      mode: `mode-${i}`,
      m: i,
    }));
  }
  const brief = buildBrief({ model: model({ model_id: longId }), records });
  assert.ok(Buffer.byteLength(brief, 'utf8') <= BRIEF_MAX_BYTES);
});

test('buildBrief handles an empty sample', () => {
  const brief = buildBrief({ model: model(), records: [] });
  assert.match(brief, /No sampled battles yet/);
  assert.ok(Buffer.byteLength(brief, 'utf8') <= BRIEF_MAX_BYTES);
});

/** In-memory IO fixture for sampleModelBattles. */
function fakeIo(files) {
  // files: Map<dirPath, Array<{ name, mtime, json }>>
  return {
    readdir: async (dir) => {
      const listing = files.get(dir);
      if (!listing) {
        const error = new Error(`ENOENT: ${dir}`);
        error.code = 'ENOENT';
        throw error;
      }
      return listing.map((entry) => entry.name);
    },
    statMtimeMs: async (dir, name) => {
      const entry = files.get(dir).find((candidate) => candidate.name === name);
      return entry.mtime;
    },
    readJson: async (file) => {
      for (const listing of files.values()) {
        const entry = listing.find((candidate) => file.endsWith(candidate.name));
        if (entry) return entry.json;
      }
      throw new Error(`ENOENT: ${file}`);
    },
  };
}

function generationCheckpoint(arenaId, providerModel) {
  return { model_id: arenaId, provider_model: providerModel };
}

function battleCheckpoint(aId, bId, { mode = 'arena', winner = aId, draw = false } = {}) {
  return {
    model_a_id: aId,
    model_b_id: bId,
    simulation: {
      mode,
      winner_model_id: winner,
      draw,
      team_a_action_counts: { idle: 1, attack: 4, defend: 2, charge: 2, support: 1 },
      team_b_action_counts: { idle: 3, attack: 2, defend: 3, charge: 0, support: 2 },
      trap_count: 0,
      fuel_error_count: 0,
      fallback_count: 0,
    },
  };
}

test('sampleModelBattles maps arena ids, orders newest-first, respects the cap', async () => {
  const seasons = '/seasons';
  const day0 = `${seasons}/continuous-cml-test-day0`;
  const day1 = `${seasons}/continuous-cml-test-day1`;
  const files = new Map([
    [`${day0}/generations`, [
      { name: 'd0-a.json', mtime: 10, json: generationCheckpoint('d0-a', ME) },
      { name: 'd0-b.json', mtime: 10, json: generationCheckpoint('d0-b', 'vendor/b') },
    ]],
    [`${day0}/battles`, [
      { name: 'old.json', mtime: 20, json: battleCheckpoint('d0-a', 'd0-b') },
      { name: 'older.json', mtime: 10, json: battleCheckpoint('d0-b', 'd0-a', { winner: 'd0-a' }) },
      { name: 'unrelated.json', mtime: 30, json: battleCheckpoint('d0-x', 'd0-y') },
    ]],
    [`${day1}/generations`, [
      { name: 'd1-a.json', mtime: 40, json: generationCheckpoint('d1-a', ME) },
      { name: 'd1-b.json', mtime: 40, json: generationCheckpoint('d1-b', 'vendor/b') },
    ]],
    [`${day1}/battles`, [
      { name: 'new.json', mtime: 60, json: battleCheckpoint('d1-b', 'd1-a', { mode: 'ctf', winner: 'd1-b' }) },
      { name: 'mid.json', mtime: 50, json: battleCheckpoint('d1-a', 'd1-b', { mode: 'ctf' }) },
    ]],
  ]);

  const records = await sampleModelBattles({
    seasonsDirectory: seasons,
    leagueId: 'cml-test',
    dayIndex: 2,
    modelId: ME,
    io: fakeIo(files),
  });

  // Newest day first, newest file first within the day; the unrelated battle
  // (models absent from the id map) is skipped.
  assert.deepEqual(records.map((record) => record.m), [60, 50, 20, 10]);
  assert.deepEqual(records.map((record) => record.mode), ['ctf', 'ctf', 'arena', 'arena']);
  assert.ok(records.every((record) => record.me === ME && record.opponent === 'vendor/b'));
  // The day-1 loss is attributed to the provider id, not the arena id.
  assert.equal(records[0].winner, 'vendor/b');
  assert.equal(records[1].winner, ME);
  // Counts come from the side the model played.
  assert.equal(records[0].counts.idle, 3); // model was team b in new.json
  assert.equal(records[1].counts.idle, 1); // model was team a in mid.json

  const capped = await sampleModelBattles({
    seasonsDirectory: seasons,
    leagueId: 'cml-test',
    dayIndex: 2,
    modelId: ME,
    perModelLimit: 2,
    io: fakeIo(files),
  });
  assert.deepEqual(capped.map((record) => record.m), [60, 50]);
});
