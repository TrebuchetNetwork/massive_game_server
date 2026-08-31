import { test } from 'node:test';
import assert from 'node:assert/strict';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { mascotFor } from '../../mascots.mjs';
import { bootstrapTop40 } from '../bootstrap_top40.mjs';
import { readFighterRecord } from '../generation.mjs';
import { TRACKS } from '../league.mjs';
import { createState, loadOrCreateState, validateState, writeState } from '../state.mjs';

const sha = (ch) => ch.repeat(64);
const NOW = new Date('2026-08-24T00:00:00.000Z');

const REASONING_POLICY = {
  version: 'capability_minimum_v1',
  mode: 'disabled',
  effort: null,
  exclude: true,
};

function rosterEntry(modelId, overrides = {}) {
  return {
    model_id: modelId,
    slug: `${modelId}-20260801`,
    mascot: mascotFor(modelId),
    joined_at: '2026-08-17T00:00:00.000Z',
    submissions_used: 1,
    artifact: {
      wasm_sha256: sha('a'),
      source_sha256: sha('b'),
      prompt_sha256: sha('c'),
      version: 1,
      parent_version: null,
    },
    rating: 50,
    wins: 0,
    losses: 0,
    draws: 0,
    matches: 0,
    days_in_league: 7,
    status: 'active',
    ...overrides,
  };
}

function rankingEntry(modelId, providerRank) {
  return {
    provider_rank: providerRank,
    id: modelId,
    canonical_slug: `${modelId}-20260801`,
    name: `Vendor: ${modelId}`,
    pricing: null,
    context_length: 1000000,
    created: 1780000000,
    reasoning_policy: { ...REASONING_POLICY },
  };
}

async function seededShadow(existing = 10) {
  const shadowDir = await fs.mkdtemp(path.join(os.tmpdir(), 'cml-top40-'));
  const state = createState({ now: NOW, leagueId: 'cml-top40-test' });
  for (const trackId of TRACKS) {
    for (let index = 0; index < existing; index += 1) {
      state.tracks[trackId].roster.push(rosterEntry(`vendor/keep-${index}`, {
        rating: 90 - index,
      }));
    }
  }
  await writeState(shadowDir, state);
  return { shadowDir, state };
}

function fakeRanking(size) {
  return {
    models: [
      ...Array.from({ length: 10 }, (_, index) => rankingEntry(`vendor/keep-${index}`, index + 1)),
      ...Array.from({ length: size - 10 }, (_, index) => rankingEntry(`vendor/new-${index}`, index + 11)),
    ],
  };
}

function fakeGenerator(calls, { failFor = new Set() } = {}) {
  return async ({ entrant }) => {
    calls.push(entrant.provider_model);
    if (failFor.has(entrant.provider_model)) {
      throw new Error('fighter compilation failed: error[E0308]');
    }
    return {
      checkpoint: {
        wasm_sha256: sha('d'),
        source_sha256: sha('e'),
        prompt_sha256: sha('f'),
      },
      source: 'fn bot_tick_v2() {}\n',
    };
  };
}

test('bootstrapTop40 keeps existing entries and seeds 30 new models once into all tracks', async () => {
  const { shadowDir } = await seededShadow(10);
  const calls = [];
  const { state, results } = await bootstrapTop40({
    shadowDirectory: shadowDir,
    now: NOW,
    fetchRanking: async () => fakeRanking(45),
    generateFighterFn: fakeGenerator(calls),
    adminToken: 'test-token',
    apiBase: 'http://127.0.0.1:9',
    log: () => {},
  });

  // One codegen per model, never four.
  assert.equal(calls.length, 30);
  assert.equal(results.imported.length, 30);
  assert.equal(results.failed.length, 0);
  assert.equal(results.kept, 10);

  for (const trackId of TRACKS) {
    const roster = state.tracks[trackId].roster;
    assert.equal(roster.length, 40, trackId);
    // The existing 10 kept their entries and ratings.
    assert.equal(roster[0].model_id, 'vendor/keep-0');
    assert.equal(roster[0].rating, 90);
    assert.equal(roster[0].days_in_league, 7);
    // New models: submissions_used=1, artifact v1, rating 50, joined now.
    const recruit = roster.find((entry) => entry.model_id === 'vendor/new-0');
    assert.equal(recruit.submissions_used, 1);
    assert.equal(recruit.artifact.version, 1);
    assert.equal(recruit.artifact.parent_version, null);
    assert.equal(recruit.rating, 50);
    assert.equal(recruit.joined_at, NOW.toISOString());
    // The v1 artifact was copied to this track's fighter directory.
    const fighter = await readFighterRecord(
      path.join(shadowDir, 'tracks', trackId),
      'vendor/new-0',
    );
    assert.equal(fighter.checkpoint.wasm_sha256, sha('d'));
    assert.equal(fighter.meta.model_id, 'vendor/new-0');
  }
  validateState(state);

  // Persisted and reloads clean.
  const reloaded = await loadOrCreateState(shadowDir);
  assert.equal(reloaded.state.tracks.L3.roster.length, 40);
});

test('bootstrapTop40 refuses a roster of 30+ unless forced, then fills only missing slots', async () => {
  const { shadowDir } = await seededShadow(10);
  const options = {
    shadowDirectory: shadowDir,
    now: NOW,
    fetchRanking: async () => fakeRanking(45),
    generateFighterFn: fakeGenerator([]),
    adminToken: 'test-token',
    apiBase: 'http://127.0.0.1:9',
    log: () => {},
  };
  await bootstrapTop40(options);

  await assert.rejects(
    bootstrapTop40({ ...options, generateFighterFn: fakeGenerator([]) }),
    /roster already has 40 models per track; use --force/,
  );

  // --force fills only the still-missing slots (nothing to do at 40/40).
  const calls = [];
  const { results } = await bootstrapTop40({
    ...options,
    force: true,
    generateFighterFn: fakeGenerator(calls),
  });
  assert.equal(calls.length, 0);
  assert.equal(results.imported.length, 0);
});

test('bootstrapTop40 reports per-model failures and leaves those slots open', async () => {
  const { shadowDir } = await seededShadow(10);
  const calls = [];
  const { state, results } = await bootstrapTop40({
    shadowDirectory: shadowDir,
    now: NOW,
    fetchRanking: async () => fakeRanking(45),
    generateFighterFn: fakeGenerator(calls, {
      failFor: new Set(['vendor/new-3', 'vendor/new-7']),
    }),
    adminToken: 'test-token',
    apiBase: 'http://127.0.0.1:9',
    log: () => {},
  });

  assert.equal(results.imported.length, 28);
  assert.deepEqual(
    results.failed.map((failure) => failure.model_id),
    ['vendor/new-3', 'vendor/new-7'],
  );
  for (const trackId of TRACKS) {
    const roster = state.tracks[trackId].roster;
    assert.equal(roster.length, 38, trackId);
    assert.ok(!roster.some((entry) => entry.model_id === 'vendor/new-3'));
    assert.ok(!roster.some((entry) => entry.model_id === 'vendor/new-7'));
  }
  validateState(state);
});
