import { test } from 'node:test';
import assert from 'node:assert/strict';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { mascotFor } from '../../mascots.mjs';
import { migrateTop40ToLive } from '../migrate_top40_to_live.mjs';
import { writeFighterRecord, readFighterRecord } from '../generation.mjs';
import { TRACKS } from '../league.mjs';
import { createState, statePathFor, validateState, writeState } from '../state.mjs';

const sha = (ch) => ch.repeat(64);
const NOW = new Date('2026-08-31T12:00:00.000Z');

function rosterEntry(modelId, overrides = {}) {
  return {
    model_id: modelId,
    slug: `${modelId}-20260801`,
    mascot: mascotFor(modelId),
    joined_at: '2026-08-24T00:00:00.000Z',
    submissions_used: 1,
    artifact: {
      wasm_sha256: sha('a'),
      source_sha256: sha('b'),
      prompt_sha256: sha('c'),
      version: 1,
      parent_version: null,
    },
    rating: 50,
    wins: 10,
    losses: 8,
    draws: 2,
    matches: 20,
    days_in_league: 7,
    status: 'active',
    ...overrides,
  };
}

async function fighterFor(dir, trackId, modelId) {
  await writeFighterRecord(path.join(dir, 'tracks', trackId), modelId, {
    checkpoint: {
      schema_version: 2,
      stage: 'compiled',
      provider_model: modelId,
      wasm_sha256: sha('d'),
      source_sha256: sha('e'),
      prompt_sha256: sha('f'),
    },
    source: `fn bot_tick_v2() { /* ${modelId} */ }\n`,
    meta: { model_id: modelId, slug: `${modelId}-20260801`, model_name: modelId },
  });
}

async function fixtureLeagues({ liveModels, shadowExtra }) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cml-migrate-'));
  const shadowDir = path.join(root, 'shadow');
  const liveDir = path.join(root, 'live');
  const shadow = createState({ now: NOW, leagueId: 'cml-shadow-test' });
  const live = createState({ now: NOW, leagueId: 'cml-live-test' });
  for (const trackId of TRACKS) {
    for (const id of liveModels) live.tracks[trackId].roster.push(rosterEntry(id));
    for (const id of [...liveModels, ...shadowExtra]) {
      shadow.tracks[trackId].roster.push(rosterEntry(id));
      await fighterFor(shadowDir, trackId, id);
    }
  }
  await writeState(shadowDir, shadow);
  await writeState(liveDir, live);
  return { root, shadowDir, liveDir };
}

test('migration adds only the shadow-imported models, fresh entries, fighter records copied', async () => {
  const { shadowDir, liveDir } = await fixtureLeagues({
    liveModels: ['vendor/keep-1', 'vendor/keep-2'],
    shadowExtra: ['vendor/new-1', 'vendor/new-2', 'vendor/new-3'],
  });

  const { migrated, rosterCounts, state } = await migrateTop40ToLive({
    shadowDirectory: shadowDir,
    liveDirectory: liveDir,
    now: NOW,
    log: () => {},
  });

  assert.deepEqual(migrated.sort(), ['vendor/new-1', 'vendor/new-2', 'vendor/new-3']);
  assert.deepEqual(rosterCounts, { L0: 5, L1: 5, L2: 5, L3: 5 });
  validateState(state);

  for (const trackId of TRACKS) {
    const roster = state.tracks[trackId].roster;
    // Existing entries untouched (evolved ratings/records preserved).
    const kept = roster.find((entry) => entry.model_id === 'vendor/keep-1');
    assert.equal(kept.rating, 50);
    assert.equal(kept.matches, 20);
    // New entries: fresh league bookkeeping, artifact digests from the paid artifact.
    const added = roster.find((entry) => entry.model_id === 'vendor/new-1');
    assert.equal(added.submissions_used, 1);
    assert.equal(added.artifact.version, 1);
    assert.equal(added.artifact.parent_version, null);
    assert.equal(added.artifact.wasm_sha256, sha('d'));
    assert.equal(added.rating, 50);
    assert.equal(added.wins + added.losses + added.draws + added.matches, 0);
    assert.equal(added.joined_at, NOW.toISOString());
    const fighter = await readFighterRecord(path.join(liveDir, 'tracks', trackId), 'vendor/new-1');
    assert.equal(fighter.checkpoint.provider_model, 'vendor/new-1');
  }

  // Persisted on disk and validates.
  const onDisk = JSON.parse(await fs.readFile(statePathFor(liveDir), 'utf8'));
  assert.equal(onDisk.tracks.L2.roster.length, 5);
  validateState(onDisk);
});

test('migration is idempotent: a second run adds nothing', async () => {
  const { shadowDir, liveDir } = await fixtureLeagues({
    liveModels: ['vendor/keep-1'],
    shadowExtra: ['vendor/new-1'],
  });
  const options = { shadowDirectory: shadowDir, liveDirectory: liveDir, now: NOW, log: () => {} };
  await migrateTop40ToLive(options);
  const { migrated, rosterCounts } = await migrateTop40ToLive(options);
  assert.deepEqual(migrated, []);
  assert.deepEqual(rosterCounts, { L0: 2, L1: 2, L2: 2, L3: 2 });
});
