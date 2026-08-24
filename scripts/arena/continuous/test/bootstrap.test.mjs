import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { bootstrapFromWeekly } from '../bootstrap_from_weekly.mjs';
import { readFighterRecord } from '../generation.mjs';
import { loadOrCreateState, validateState } from '../state.mjs';

const sha256 = (value) => createHash('sha256').update(value).digest('hex');
const sha = (ch) => ch.repeat(64);
const NOW = new Date('2026-08-23T00:00:00.000Z');

const REASONING_POLICY = {
  version: 'capability_minimum_v1',
  mode: 'disabled',
  effort: null,
  exclude: true,
};

function weeklyCheckpoint(arenaId, providerModel, index) {
  const source = `fn bot_tick_v2() { /* ${providerModel} */ }\n`;
  return {
    checkpoint: {
      schema_version: 2,
      stage: 'compiled',
      compiled: true,
      provider_rank: index + 1,
      model_id: arenaId,
      model_name: `Vendor: ${providerModel}`,
      provider_model: providerModel,
      canonical_slug: `${providerModel}-20260801`,
      reasoning_policy: { ...REASONING_POLICY },
      prompt_sha256: sha('c'),
      source_sha256: sha256(source),
      wasm_sha256: sha(index === 0 ? 'a' : 'b'),
      wasm_bytes: 1000 + index,
    },
    source,
  };
}

async function fixtureWeekly(root) {
  const providers = ['vendor/model-a', 'vendor/model-b'];
  const seasonId = 'weekly-2026-08-17-deadbeef';
  const weekDir = path.join(root, 'weekly-supervisor', '2026-W34');
  const seasonDir = path.join(root, 'seasons', seasonId);
  await fs.mkdir(path.join(seasonDir, 'generations'), { recursive: true });
  await fs.mkdir(path.join(seasonDir, 'sources'), { recursive: true });
  await fs.mkdir(weekDir, { recursive: true });

  const bindings = [];
  const rankingModels = [];
  for (const [index, provider] of providers.entries()) {
    const arenaId = `orw-20260817-deadbeef-0${index + 1}-aaaa000${index}-vendor-${provider.split('/')[1]}`;
    const { checkpoint, source } = weeklyCheckpoint(arenaId, provider, index);
    await fs.writeFile(
      path.join(seasonDir, 'generations', `${arenaId}.json`),
      JSON.stringify(checkpoint),
    );
    await fs.writeFile(path.join(seasonDir, 'sources', `${arenaId}.rs`), source);
    bindings.push({
      model_id: arenaId,
      wasm_bytes: checkpoint.wasm_bytes,
      wasm_sha256: checkpoint.wasm_sha256,
    });
    rankingModels.push({
      provider_rank: index + 1,
      id: provider,
      canonical_slug: `${provider}-20260801`,
      name: `Vendor: ${provider}`,
      pricing: { prompt: '0.1' },
      context_length: 1000 + index,
      created: 1780000000 + index,
    });
  }
  await fs.writeFile(path.join(seasonDir, 'ranking.json'), JSON.stringify({
    schema_version: 1,
    retrieved_at: '2026-08-17T00:00:00.000Z',
    models: rankingModels,
  }));
  await fs.writeFile(path.join(weekDir, 'state.json'), JSON.stringify({
    week_id: '2026-W34',
    season_id: seasonId,
    generation: { completed: true, completed_at: '2026-08-17T00:16:55.000Z' },
    artifact_bindings: bindings,
  }));
  return { weekDir, seasonDir, providers };
}

test('bootstrap imports the weekly roster into a valid shadow state', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cml-boot-'));
  const { weekDir, providers } = await fixtureWeekly(root);
  const shadowDir = path.join(root, 'continuous-shadow');
  const logs = [];

  const { state, imported } = await bootstrapFromWeekly({
    weekDirectory: weekDir,
    seasonsDirectory: path.join(root, 'seasons'),
    shadowDirectory: shadowDir,
    now: NOW,
    log: (line) => logs.push(line),
  });

  assert.equal(imported.length, 2);
  for (const trackId of ['L0', 'L1', 'L2', 'L3']) {
    const roster = state.tracks[trackId].roster;
    assert.equal(roster.length, 2, trackId);
    for (const [index, entry] of roster.entries()) {
      assert.equal(entry.model_id, providers[index]);
      assert.equal(entry.slug, `${providers[index]}-20260801`);
      assert.equal(entry.submissions_used, 1);
      assert.equal(entry.artifact.version, 1);
      assert.equal(entry.artifact.parent_version, null);
      assert.equal(entry.rating, 50);
      assert.equal(entry.wins + entry.losses + entry.draws + entry.matches, 0);
      assert.equal(entry.days_in_league, 0);
      assert.equal(entry.joined_at, NOW.toISOString());
      assert.ok(entry.mascot.emoji);
    }
    assert.equal(roster[0].artifact.wasm_sha256, sha('a'));
    assert.equal(roster[1].artifact.wasm_sha256, sha('b'));
  }
  // All four tracks start from identical v1 artifacts.
  for (const trackId of ['L1', 'L2', 'L3']) {
    assert.deepEqual(
      state.tracks[trackId].roster.map((entry) => entry.artifact),
      state.tracks.L0.roster.map((entry) => entry.artifact),
      `${trackId} artifacts must equal L0's`,
    );
  }
  validateState(state);

  // Fighter records landed per track with checkpoint, source, and meta.
  for (const trackId of ['L0', 'L1', 'L2', 'L3']) {
    const fighter = await readFighterRecord(
      path.join(shadowDir, 'tracks', trackId),
      'vendor/model-a',
    );
    assert.equal(fighter.checkpoint.provider_model, 'vendor/model-a');
    assert.equal(fighter.source, 'fn bot_tick_v2() { /* vendor/model-a */ }\n');
    assert.equal(fighter.meta.slug, 'vendor/model-a-20260801');
    assert.equal(fighter.meta.context_length, 1000);
    assert.deepEqual(fighter.meta.reasoning_policy, REASONING_POLICY);
  }

  // The state persisted and reloads clean.
  const reloaded = await loadOrCreateState(shadowDir);
  assert.equal(reloaded.state.tracks.L2.roster.length, 2);
  assert.ok(logs.some((line) => line.includes('imported 2 models from 2026-W34')));
});

test('bootstrap refuses to overwrite a non-empty roster unless forced', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cml-boot-'));
  const { weekDir } = await fixtureWeekly(root);
  const shadowDir = path.join(root, 'continuous-shadow');
  const options = {
    weekDirectory: weekDir,
    seasonsDirectory: path.join(root, 'seasons'),
    shadowDirectory: shadowDir,
    now: NOW,
    log: () => {},
  };
  await bootstrapFromWeekly(options);
  await assert.rejects(
    bootstrapFromWeekly(options),
    /roster is not empty \(tracks L0, L1, L2, L3\); use --force/,
  );
  const { state } = await bootstrapFromWeekly({ ...options, force: true });
  assert.equal(state.tracks.L0.roster.length, 2);
  validateState(state);
});

test('bootstrap picks the latest week directory', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cml-boot-'));
  const { weekDir } = await fixtureWeekly(root);
  // An older, invalid week must not win over 2026-W34.
  const older = path.join(root, 'weekly-supervisor', '2026-W33');
  await fs.mkdir(older, { recursive: true });
  await fs.writeFile(path.join(older, 'state.json'), '{}');
  const shadowDir = path.join(root, 'continuous-shadow');
  const logs = [];
  await bootstrapFromWeekly({
    weeklyStateDir: path.join(root, 'weekly-supervisor'),
    seasonsDirectory: path.join(root, 'seasons'),
    shadowDirectory: shadowDir,
    now: NOW,
    log: (line) => logs.push(line),
  });
  assert.ok(logs.some((line) => line.includes('imported 2 models from 2026-W34')));
});
