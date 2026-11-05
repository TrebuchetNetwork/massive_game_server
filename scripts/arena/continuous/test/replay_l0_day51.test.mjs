import { test } from 'node:test';
import assert from 'node:assert/strict';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import * as replay from '../replay_l0_day51.mjs';
import {
  atomicWriteJson,
  createState,
  statePathFor,
  writeState,
} from '../state.mjs';

const { buildBoundaryRoster, validateReplay } = replay;
const ROTATION_AT = '2026-11-04T10:23:13.994Z';
const PREVIOUS_AT = '2026-11-04T10:23:13.995Z';
const TAINTED_AT = '2026-11-05T12:00:13.516Z';

function currentEntry(modelId, overrides = {}) {
  return {
    model_id: modelId,
    slug: `current-${modelId}`,
    joined_at: '2026-11-01T00:00:00.000Z',
    submissions_used: 1,
    artifact: { version: 1, wasm_sha256: 'current-artifact' },
    rating: 91,
    wins: 80,
    losses: 18,
    draws: 2,
    matches: 100,
    days_in_league: 9,
    status: 'active',
    ...overrides,
  };
}

function previousEntry(modelId, overrides = {}) {
  return {
    model_id: modelId,
    slug: `previous-${modelId}`,
    rating: 55,
    wins: 20,
    losses: 10,
    draws: 2,
    matches: 32,
    ...overrides,
  };
}

test('rotation evidence baselines actual Hy4 and Crouton despite earlier joined_at', () => {
  const current = [
    currentEntry('vendor/existing'),
    currentEntry('tencent/hy4', {
      joined_at: ROTATION_AT,
      rating: 72,
      wins: 22,
      losses: 0,
      draws: 0,
      matches: 22,
    }),
    currentEntry('stealth/crouton-alpha', {
      joined_at: ROTATION_AT,
      rating: 68,
      wins: 18,
      losses: 0,
      draws: 0,
      matches: 18,
    }),
  ];
  const previous = [
    previousEntry('vendor/existing'),
    previousEntry('vendor/displaced'),
  ];
  const currentBefore = structuredClone(current);
  const announcements = [
    { type: 'fresh_challenger', model_id: 'tencent/hy4', at: ROTATION_AT },
    { type: 'fresh_challenger', model_id: 'stealth/crouton-alpha', at: ROTATION_AT },
    { type: 'entrant', model_id: 'vendor/other-entrant', at: '2026-11-04T11:00:00.000Z' },
    { type: 'entrant', model_id: 'vendor/previous-day', at: '2026-11-03T23:59:59.999Z' },
    { type: 'entrant', model_id: 'vendor/too-late', at: '2026-11-05T12:00:13.517Z' },
    { type: 'retirement', model_id: 'vendor/old-missing', at: ROTATION_AT },
  ];

  const postSnapshotEntrantIds = replay.rotationEntrantIds(
    announcements,
    PREVIOUS_AT,
    TAINTED_AT,
  );
  assert.ok(postSnapshotEntrantIds.has('tencent/hy4'));
  assert.ok(postSnapshotEntrantIds.has('stealth/crouton-alpha'));
  assert.ok(postSnapshotEntrantIds.has('vendor/other-entrant'));
  assert.equal(postSnapshotEntrantIds.has('vendor/previous-day'), false);
  assert.equal(postSnapshotEntrantIds.has('vendor/too-late'), false);
  assert.equal(postSnapshotEntrantIds.has('vendor/old-missing'), false);

  const boundary = buildBoundaryRoster(current, previous, postSnapshotEntrantIds);

  assert.deepEqual(boundary[0], {
    ...current[0],
    rating: previous[0].rating,
    wins: previous[0].wins,
    losses: previous[0].losses,
    draws: previous[0].draws,
    matches: previous[0].matches,
  });
  for (const [index, entrant] of [boundary[1], boundary[2]].entries()) {
    assert.deepEqual(entrant, {
      ...current[index + 1],
      rating: 50,
      wins: 0,
      losses: 0,
      draws: 0,
      matches: 0,
    });
  }
  assert.deepEqual(current, currentBefore);
  assert.throws(
    () => buildBoundaryRoster([
      currentEntry('vendor/old-missing'),
    ], [], postSnapshotEntrantIds),
    /unexpected membership.*vendor\/old-missing.*rotation evidence/i,
  );
});

test('replay validation is order-insensitive and enforces IDs plus exactly 288 matches', () => {
  const boundary = [
    { model_id: 'vendor/existing', matches: 32, rating: 55 },
    { model_id: 'tencent/hy4', matches: 0, rating: 50 },
  ];
  const replayed = [
    { model_id: 'tencent/hy4', matches: 288 },
    { model_id: 'vendor/existing', matches: 320, rating: 1 },
  ];

  assert.equal(validateReplay(boundary, replayed, 51), true);
  assert.throws(
    () => validateReplay(boundary, [
      replayed[0],
      { ...replayed[1], matches: 319 },
    ], 51),
    /day 51.*vendor\/existing.*matches.*320.*319/i,
  );
  assert.throws(
    () => validateReplay(boundary, [
      replayed[0],
      { model_id: 'vendor/intruder', matches: 320 },
    ], 51),
    /day 51.*model-id sets/i,
  );
});

const readJson = async (target) => JSON.parse(await fs.readFile(target, 'utf8'));

async function recoveryFixture(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cml-l0-recovery-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const backupDirectory = path.join(root, 'backup');
  const stagedRootDirectory = path.join(backupDirectory, 'staging');
  const continuousDirectory = path.join(root, 'live', 'continuous');
  const trackDirectory = path.join(continuousDirectory, 'tracks', 'L0');
  const seasonsDirectory = path.join(root, 'live', 'seasons');
  const stagedContinuousDirectory = path.join(
    stagedRootDirectory,
    'artifacts/arena/continuous',
  );
  const stagedTrackDirectory = path.join(stagedContinuousDirectory, 'tracks', 'L0');
  const stagedSeasonsDirectory = path.join(stagedRootDirectory, 'artifacts/arena/seasons');
  const seasonIds = ['season-a', 'season-b', 'season-c', 'season-d'];
  const historyName = '2026-11-05.json';

  const originalState = createState({ now: '2026-11-01T00:00:00.000Z', leagueId: 'cml-test' });
  const liveState = createState({ now: '2026-11-06T00:00:00.000Z', leagueId: 'cml-test' });
  const stagedState = createState({ now: '2026-11-05T12:00:13.516Z', leagueId: 'cml-test' });
  const originalHistory = [{ day_index: 51, marker: 'original-history' }];
  const liveHistory = [{ day_index: 51, marker: 'live-history' }];
  const stagedHistory = [{ day_index: 51, marker: 'staged-history' }];
  const originalSeasons = Object.fromEntries(seasonIds.map((id) => [
    id,
    { season_id: id, marker: `original-${id}` },
  ]));
  const originalRankings = Object.fromEntries(seasonIds.map((id) => [
    id,
    { season_id: id, marker: `original-ranking-${id}` },
  ]));

  await writeState(continuousDirectory, liveState);
  await writeState(stagedContinuousDirectory, stagedState);
  await atomicWriteJson(path.join(backupDirectory, 'state.json'), originalState);
  await atomicWriteJson(path.join(backupDirectory, historyName), originalHistory);
  await atomicWriteJson(path.join(trackDirectory, 'history', historyName), liveHistory);
  await atomicWriteJson(path.join(stagedTrackDirectory, 'history', historyName), stagedHistory);
  for (const seasonId of seasonIds) {
    await atomicWriteJson(
      path.join(backupDirectory, 'seasons', seasonId, 'season.json'),
      originalSeasons[seasonId],
    );
    await atomicWriteJson(
      path.join(seasonsDirectory, seasonId, 'season.json'),
      { season_id: seasonId, marker: `live-${seasonId}` },
    );
    await atomicWriteJson(
      path.join(stagedSeasonsDirectory, seasonId, 'season.json'),
      { season_id: seasonId, marker: `staged-${seasonId}` },
    );
    await atomicWriteJson(
      path.join(backupDirectory, 'rankings', `${seasonId}.json`),
      originalRankings[seasonId],
    );
    await atomicWriteJson(
      path.join(trackDirectory, 'rankings', `${seasonId}.json`),
      { season_id: seasonId, marker: `live-ranking-${seasonId}` },
    );
    await atomicWriteJson(
      path.join(stagedTrackDirectory, 'rankings', `${seasonId}.json`),
      { season_id: seasonId, marker: `staged-ranking-${seasonId}` },
    );
  }
  await atomicWriteJson(path.join(backupDirectory, 'journal.json'), {
    version: 1,
    phase: 'prepared',
    season_ids: seasonIds,
  });

  return {
    layout: {
      backupDirectory,
      stagedRootDirectory,
      continuousDirectory,
      trackDirectory,
      seasonsDirectory,
      seasonIds,
    },
    historyName,
    originalState,
    stagedState,
    originalHistory,
    originalSeasons,
    originalRankings,
  };
}

async function assertLiveRestored(fixture) {
  const { layout } = fixture;
  assert.deepEqual(
    await readJson(statePathFor(layout.continuousDirectory)),
    fixture.originalState,
  );
  assert.deepEqual(
    await readJson(path.join(layout.trackDirectory, 'history', fixture.historyName)),
    fixture.originalHistory,
  );
  for (const seasonId of layout.seasonIds) {
    assert.deepEqual(
      await readJson(path.join(layout.seasonsDirectory, seasonId, 'season.json')),
      fixture.originalSeasons[seasonId],
    );
    assert.deepEqual(
      await readJson(path.join(layout.trackDirectory, 'rankings', `${seasonId}.json`)),
      fixture.originalRankings[seasonId],
    );
  }
  assert.equal(
    (await readJson(path.join(layout.backupDirectory, 'journal.json'))).phase,
    'rolled_back',
  );
}

test('prepared journal restores all live artifacts before refusing startup', async (t) => {
  const fixture = await recoveryFixture(t);

  await assert.rejects(
    replay.recoverOnStartup(fixture.layout),
    /restored original live artifacts/i,
  );

  await assertLiveRestored(fixture);
  await assert.rejects(replay.recoverOnStartup(fixture.layout), /rolled_back/);
});

test('commit failure restores backup and marks journal rolled_back', async (t) => {
  const fixture = await recoveryFixture(t);
  const writeStateImpl = async (directory, state) => {
    if (path.resolve(directory) === path.resolve(fixture.layout.continuousDirectory)
        && state.updated_at === fixture.stagedState.updated_at) {
      throw new Error('injected commit failure');
    }
    return writeState(directory, state);
  };

  await assert.rejects(
    replay.commitStagedReplay({ ...fixture.layout, writeStateImpl }),
    /injected commit failure/,
  );

  await assertLiveRestored(fixture);
  assert.equal(
    (await readJson(path.join(
      fixture.layout.stagedRootDirectory,
      'artifacts/arena/seasons/season-a/season.json',
    ))).marker,
    'staged-season-a',
  );
});

test('isolated server environment covers every write path and shared store', () => {
  const isolatedDirectory = path.join(path.sep, 'recovery', 'isolated');
  const expectedPaths = {
    MGS_ACCESS_LOG_PATH: 'access.jsonl',
    MGS_LIVE_REPLAY_DISPUTE_STORE_PATH: 'live_replay/disputes.jsonl',
    MGS_LIVE_REPLAY_MATCH_STORE_DIR: 'live_replay/matches',
    MGS_BACKUP_DIR: 'backups',
    MGS_ARENA_EXHIBITION_ROSTER_PATH: 'exhibition_roster.json',
    MGS_PANIC_LOG_DIR: 'panic',
    MGS_ARENA_STORE_PATH: 'arena_store.json',
    MGS_ARENA_WASM_DIR: 'arena_bots',
    MGS_ARENA_RATINGS_PATH: 'arena_ratings.json',
    MGS_ARENA_SOURCE_DIR: 'arena_sources',
    MGS_AUTH_STORE_PATH: 'auth_store.json',
    MGS_FEATURE_FLAG_STORE_PATH: 'feature_flags.json',
    MGS_SHUTDOWN_STATE_PATH: 'shutdown_state.json',
  };
  const disabledRedis = [
    'MGS_ARENA_REDIS_URL',
    'MGS_REDIS_URL',
    'MGS_FEATURE_FLAGS_REDIS_URL',
    'MGS_BACKUP_REDIS_URL',
    'MGS_LIVE_REPLAY_DISPUTE_REDIS_URL',
    'MGS_LIVE_REPLAY_MATCH_REDIS_URL',
  ];
  const inherited = {
    ...Object.fromEntries(Object.keys(expectedPaths).map((key) => [key, `/production/${key}`])),
    ...Object.fromEntries(disabledRedis.map((key) => [key, 'redis://production'])),
    MGS_BACKUP_EXTRA_PATHS: '/production/state,/production/logs',
    MGS_PANIC_LOG_FILE: '/production/panic.log',
    MGS_CODEGEN_RUSTC_PATH: '/opt/rust/bin/rustc',
    MGS_MAP_PATH: '/production/map.json',
    MGS_QUIC_CERT_PATH: '/production/cert.pem',
  };
  const inheritedBefore = structuredClone(inherited);

  const env = replay.isolatedServerEnvironment('test-admin-token', {
    env: inherited,
    isolatedDirectory,
  });

  for (const [key, relativePath] of Object.entries(expectedPaths)) {
    assert.equal(env[key], path.join(isolatedDirectory, relativePath), key);
  }
  assert.equal(env.MGS_PANIC_LOG_FILE, 'server-panic.log');
  assert.equal(
    path.resolve(env.MGS_PANIC_LOG_DIR, env.MGS_PANIC_LOG_FILE),
    path.join(isolatedDirectory, 'panic/server-panic.log'),
  );
  assert.equal(env.MGS_BACKUP_EXTRA_PATHS, '');
  for (const key of disabledRedis) assert.equal(env[key], '', key);
  assert.equal(env.MGS_ADMIN_BEARER_TOKEN, 'test-admin-token');
  assert.equal(env.MGS_CODEGEN_RUSTC_PATH, inherited.MGS_CODEGEN_RUSTC_PATH);
  assert.equal(env.MGS_MAP_PATH, inherited.MGS_MAP_PATH);
  assert.equal(env.MGS_QUIC_CERT_PATH, inherited.MGS_QUIC_CERT_PATH);
  assert.deepEqual(inherited, inheritedBefore);
});
