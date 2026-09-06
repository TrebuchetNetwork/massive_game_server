import { test } from 'node:test';
import assert from 'node:assert/strict';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { mascotFor } from '../../mascots.mjs';
import { runCycle, runTrackCycle, stateDirectoryFromEnv } from '../../continuous_league.mjs';
import { trackPolicy } from '../league.mjs';
import { writeFighterRecord } from '../generation.mjs';
import { createState, validateState, validateTrackSlice } from '../state.mjs';

const NOW = Date.parse('2026-08-23T00:00:00.000Z');
const DAY_MS = 24 * 60 * 60 * 1000;

const sha = (ch) => ch.repeat(64);
const REASONING_POLICY = Object.freeze({
  version: 'capability_minimum_v1',
  mode: 'disabled',
  effort: null,
  exclude: true,
});

function model(overrides = {}) {
  const modelId = overrides.model_id || 'vendor/model-a';
  return {
    model_id: modelId,
    slug: `${modelId}-20260801`,
    mascot: mascotFor(modelId),
    joined_at: new Date(NOW - 5 * DAY_MS).toISOString(),
    submissions_used: 1,
    artifact: {
      wasm_sha256: sha('a'),
      source_sha256: sha('b'),
      prompt_sha256: sha('c'),
      version: 1,
      parent_version: null,
    },
    rating: 50,
    wins: 5,
    losses: 5,
    draws: 0,
    matches: 10,
    days_in_league: 5,
    status: 'active',
    ...overrides,
  };
}

// Track-slice fixture (schema v2): the per-track league state runTrackCycle
// operates on. L2 policy by default (the pre-multitrack behavior).
function policySlice(trackId) {
  const policy = trackPolicy(trackId);
  return {
    max_submissions: policy.maxSubmissions,
    compile_attempts: policy.compileAttempts,
    feedback_interval_ms: policy.feedbackIntervalMs,
    max_revisions: policy.maxRevisions,
  };
}

function stateWith(overrides = {}) {
  return {
    day_index: 0,
    policy: policySlice('L2'),
    roster: [],
    retired: [],
    announcements: [],
    last_feedback_at: null,
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

async function makeFighter(stateDir, modelId) {
  await writeFighterRecord(stateDir, modelId, {
    checkpoint: {
      schema_version: 2,
      stage: 'compiled',
      provider_model: modelId,
      wasm_sha256: sha('a'),
      source_sha256: sha('b'),
      prompt_sha256: sha('c'),
    },
    source: 'fn bot_tick_v2() {}\n',
    meta: {
      model_id: modelId,
      slug: `${modelId}-20260801`,
      model_name: `Vendor: ${modelId}`,
      reasoning_policy: { ...REASONING_POLICY },
      pricing: null,
      context_length: 1000000,
      created: 1780000000,
    },
  });
}

async function tempDirs() {
  const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), 'cml-state-'));
  const rootDir = await fs.mkdtemp(path.join(os.tmpdir(), 'cml-root-'));
  return { stateDir, rootDir };
}

/** Fake season runner: writes a season.json with the given per-model results. */
function fakeEvaluateRunner(rootDir, resultsByModel, calls) {
  return async (args, options) => {
    calls.push({ args, env: options?.env });
    const seasonId = args[args.indexOf('--season-id') + 1];
    const seasonDir = path.join(rootDir, 'artifacts/arena/seasons', seasonId);
    await fs.mkdir(seasonDir, { recursive: true });
    await fs.writeFile(path.join(seasonDir, 'season.json'), JSON.stringify({
      schema_version: 1,
      season_id: seasonId,
      roster: Object.entries(resultsByModel).map(([modelId, result]) => ({
        model_id: `arena-${modelId}`,
        provider_model: modelId,
        ...result,
      })),
    }));
    return { stdout: '', stderr: '' };
  };
}

test('evaluate folds season results into ratings and appends a history snapshot', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const roster = Array.from({ length: 10 }, (_, index) => model({
    model_id: `vendor/model-${index}`,
  }));
  for (const entry of roster) await makeFighter(stateDir, entry.model_id);
  // Feedback recently ran: this test exercises evaluate, not revisions.
  const state = stateWith({ roster, last_feedback_at: new Date(NOW).toISOString() });

  const runnerCalls = [];
  const published = [];
  const logs = [];
  const results = Object.fromEntries(roster.map((entry) => [
    entry.model_id,
    { wins: 3, losses: 1, draws: 0, matches_played: 4 },
  ]));
  const next = await runTrackCycle({
    track: state,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
      runRunner: fakeEvaluateRunner(rootDir, results, runnerCalls),
      publishFighter: async ({ entrant }) => {
        published.push(entrant.model_id);
      },
      adminToken: 'test-token',
      apiBase: 'http://127.0.0.1:9',
    },
  });

  assert.equal(next.day_index, 1);
  for (const entry of next.roster) {
    assert.equal(entry.wins, 8);
    assert.equal(entry.losses, 6);
    assert.equal(entry.matches, 14);
    assert.equal(entry.rating, Math.round((100 * 8 / 14) * 100) / 100);
  }
  assert.equal(published.length, 10);
  assert.equal(runnerCalls.length, 1);
  const { args, env } = runnerCalls[0];
  assert.ok(args.includes('--evaluate-only'));
  assert.ok(args.includes('--no-publish'));
  assert.ok(args.includes('continuous-cml-test-L2-day0-premier'));
  assert.match(env.ARENA_SEEDS, /^\d+,\d+,\d+,\d+$/);
  assert.equal(env.ARENA_TOP_MODELS, '10');

  const history = JSON.parse(await fs.readFile(
    path.join(stateDir, 'history', '2026-08-23.json'),
    'utf8',
  ));
  assert.equal(history.length, 1);
  assert.equal(history[0].season_id, 'continuous-cml-test-L2-day0-premier');
  assert.equal(history[0].day_index, 0);
  assert.equal(history[0].roster.length, 10);
  assert.equal(history[0].roster[0].rating, next.roster[0].rating);

  const seasonDir = path.join(rootDir, 'artifacts/arena/seasons', 'continuous-cml-test-L2-day0-premier');
  assert.equal((await fs.readdir(path.join(seasonDir, 'generations'))).length, 10);
  assert.equal((await fs.readdir(path.join(seasonDir, 'sources'))).length, 10);

  const ranking = JSON.parse(await fs.readFile(
    path.join(stateDir, 'rankings', 'continuous-cml-test-L2-day0-premier.json'),
    'utf8',
  ));
  assert.equal(ranking.models.length, 10);
  assert.equal(ranking.models[0].id, 'vendor/model-0');
  assert.deepEqual(ranking.models[0].reasoning_policy, REASONING_POLICY);

  validateTrackSlice(next, 'L2');
});

test('retires an exhausted model below the bar with an announcement', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const failing = model({
    model_id: 'vendor/failing',
    submissions_used: 3,
    rating: 20,
    wins: 1,
    losses: 9,
    matches: 10,
    joined_at: new Date(NOW - 4 * DAY_MS).toISOString(),
  });
  const healthy = model({
    model_id: 'vendor/healthy',
    submissions_used: 3,
    rating: 80,
    wins: 8,
    losses: 2,
    matches: 10,
  });
  await makeFighter(stateDir, failing.model_id);
  await makeFighter(stateDir, healthy.model_id);
  const state = stateWith({ roster: [failing, healthy] });

  const logs = [];
  const next = await runTrackCycle({
    track: state,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
      fetchRanking: async () => ({
        // Only the roster/retired models are ranked: nothing to recruit.
        models: ['vendor/failing', 'vendor/healthy']
          .map((id, index) => rankingEntry(id, index + 1)),
      }),
      runRunner: async (args, options) => {
        return fakeEvaluateRunner(rootDir, {
          'vendor/failing': { wins: 0, losses: 4, draws: 0, matches_played: 4 },
          'vendor/healthy': { wins: 4, losses: 0, draws: 0, matches_played: 4 },
        }, [])(args, options);
      },
      publishFighter: async () => {},
      adminToken: 'test-token',
      apiBase: 'http://127.0.0.1:9',
    },
  });

  assert.deepEqual(next.roster.map((entry) => entry.model_id), ['vendor/healthy']);
  assert.equal(next.retired.length, 1);
  const retired = next.retired[0];
  assert.equal(retired.model_id, 'vendor/failing');
  assert.equal(retired.retired_at, new Date(NOW).toISOString());
  assert.match(retired.reason, /rating .* < 35/);
  assert.equal(retired.days_in_league, 4);
  assert.equal(retired.matches, 14);

  assert.equal(next.announcements.length, 1);
  const announcement = next.announcements[0];
  assert.equal(announcement.type, 'retirement');
  assert.equal(announcement.model_id, 'vendor/failing');
  assert.equal(announcement.reason, retired.reason);
  assert.equal(announcement.stats.rating, retired.rating);
  assert.equal(announcement.stats.submissions_used, 3);
  assert.ok(logs.some((line) => line.includes('retire: vendor/failing')));
  validateTrackSlice(next, 'L2');
});

test('recruits eligible challengers into open slots', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
  });
  // Feedback recently ran: this test exercises recruit, not revisions.
  const state = stateWith({
    roster: [keeper],
    last_feedback_at: new Date(NOW).toISOString(),
  });
  const rankingModels = [
    rankingEntry('vendor/model-0', 1),
    ...Array.from({ length: 9 }, (_, index) => rankingEntry(`vendor/new-${index + 1}`, index + 2)),
  ];

  const generatedFor = [];
  const logs = [];
  const next = await runTrackCycle({
    track: state,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
      fetchRanking: async () => ({ models: rankingModels }),
      generateFighter: async ({ entrant }) => {
        generatedFor.push(entrant.provider_model);
        return {
          checkpoint: {
            wasm_sha256: sha('d'),
            source_sha256: sha('e'),
            prompt_sha256: sha('f'),
          },
          source: 'fn bot_tick_v2() {}\n',
        };
      },
      adminToken: 'test-token',
      apiBase: 'http://127.0.0.1:9',
    },
  });

  assert.equal(next.roster.length, 10);
  assert.deepEqual(generatedFor, rankingModels.slice(1).map((entry) => entry.id));
  const recruit = next.roster.find((entry) => entry.model_id === 'vendor/new-1');
  assert.equal(recruit.submissions_used, 1);
  assert.equal(recruit.rating, 50);
  assert.equal(recruit.matches, 0);
  assert.equal(recruit.days_in_league, 0);
  assert.equal(recruit.artifact.version, 1);
  assert.equal(recruit.artifact.parent_version, null);
  assert.equal(recruit.artifact.wasm_sha256, sha('d'));
  assert.equal(recruit.joined_at, new Date(NOW).toISOString());

  const entrants = next.announcements.filter((entry) => entry.type === 'entrant');
  assert.equal(entrants.length, 9);
  assert.equal(entrants[0].model_id, 'vendor/new-1');
  assert.ok(entrants[0].mascot.emoji);
  assert.ok(logs.some((line) => line.includes('evaluate: skipped')));

  const fighterDirs = await fs.readdir(path.join(stateDir, 'fighters'));
  assert.equal(fighterDirs.length, 9);
  validateTrackSlice(next, 'L2');
});

test('recruit failure leaves the slot open without consuming a submission', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
  });
  // Feedback recently ran: this test exercises recruit, not revisions.
  const state = stateWith({
    roster: [keeper],
    last_feedback_at: new Date(NOW).toISOString(),
  });
  const rankingModels = [
    rankingEntry('vendor/model-0', 1),
    rankingEntry('vendor/new-1', 2),
  ];

  const logs = [];
  const next = await runTrackCycle({
    track: state,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
      fetchRanking: async () => ({ models: rankingModels }),
      generateFighter: async () => {
        throw new Error('code generation unavailable');
      },
      adminToken: 'test-token',
      apiBase: 'http://127.0.0.1:9',
    },
  });

  assert.deepEqual(next.roster.map((entry) => entry.model_id), ['vendor/model-0']);
  assert.equal(next.announcements.length, 0);
  assert.ok(logs.some((line) => (
    line.includes('recruit: generation failed for vendor/new-1')
    && line.includes('slot stays open')
  )));
  // No fighter record persisted for the failed challenger.
  assert.equal(await fs.readdir(path.join(stateDir, 'fighters')).catch(() => null), null);
  validateTrackSlice(next, 'L2');
});

test('shadow mode skips recruit and resolves an isolated state directory', async () => {
  assert.equal(
    stateDirectoryFromEnv({ shadow: true }, { ARENA_CONTINUOUS_STATE_DIR: '/tmp/live' }),
    '/tmp/live-shadow',
  );
  assert.equal(
    stateDirectoryFromEnv({ shadow: true }, {
      ARENA_CONTINUOUS_STATE_DIR: '/tmp/live',
      ARENA_CONTINUOUS_SHADOW_DIR: '/tmp/shadow',
    }),
    '/tmp/shadow',
  );
  assert.equal(
    stateDirectoryFromEnv({}, { ARENA_CONTINUOUS_STATE_DIR: '/tmp/live' }),
    '/tmp/live',
  );

  const { stateDir, rootDir } = await tempDirs();
  const logs = [];
  const next = await runTrackCycle({
    track: stateWith({ roster: [] }),
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: true },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
      runRunner: async () => {
        throw new Error('runner must not be called for an empty shadow roster');
      },
      fetchRanking: async () => {
        throw new Error('ranking must not be fetched in shadow');
      },
    },
  });
  assert.equal(next.roster.length, 0);
  assert.ok(logs.some((line) => line.includes('evaluate: skipped, roster is empty')));
  assert.ok(logs.some((line) => line.includes('shadow: recruit skipped')));
  validateTrackSlice(next, 'L2');
});

test('recruit ranking fetch failure skips recruit without failing the cycle', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
  });
  const logs = [];
  const next = await runTrackCycle({
    track: stateWith({
      roster: [keeper],
      // Feedback recently ran: this test exercises recruit, not revisions.
      last_feedback_at: new Date(NOW).toISOString(),
    }),
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
      fetchRanking: async () => {
        throw new Error('OpenRouter ranking request failed with HTTP 500');
      },
    },
  });
  assert.deepEqual(next.roster.map((entry) => entry.model_id), ['vendor/model-0']);
  assert.equal(next.announcements.length, 0);
  assert.ok(logs.some((line) => (
    line.includes('recruit: skipped, live ranking unavailable')
    && line.includes('HTTP 500')
  )));
  validateTrackSlice(next, 'L2');
});

test('history append is idempotent per day_index on a retried cycle', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const roster = Array.from({ length: 2 }, (_, index) => model({
    model_id: `vendor/model-${index}`,
  }));
  for (const entry of roster) await makeFighter(stateDir, entry.model_id);
  // Feedback recently ran: this test exercises evaluate/history, not revisions.
  const state = stateWith({ roster, last_feedback_at: new Date(NOW).toISOString() });

  // Simulate a previous cycle that crashed after the history write but
  // before the state write: a snapshot for day_index 0 already exists.
  const historyDir = path.join(stateDir, 'history');
  await fs.mkdir(historyDir, { recursive: true });
  const seeded = {
    at: '2026-08-23T00:00:00.000Z',
    league_id: 'cml-test',
    day_index: 0,
    season_id: 'continuous-cml-test-L2-day0-premier',
    roster: [],
  };
  await fs.writeFile(path.join(historyDir, '2026-08-23.json'), JSON.stringify([seeded]));

  const runnerCalls = [];
  const results = Object.fromEntries(roster.map((entry) => [
    entry.model_id,
    { wins: 3, losses: 1, draws: 0, matches_played: 4 },
  ]));
  const next = await runTrackCycle({
    track: state,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: () => {},
      fetchRanking: async () => ({ models: [] }),
      runRunner: fakeEvaluateRunner(rootDir, results, runnerCalls),
      publishFighter: async () => {},
      adminToken: 'test-token',
      apiBase: 'http://127.0.0.1:9',
    },
  });

  assert.equal(next.day_index, 1);
  const history = JSON.parse(await fs.readFile(
    path.join(historyDir, '2026-08-23.json'),
    'utf8',
  ));
  assert.equal(history.length, 1);
  assert.deepEqual(history[0], seeded);
  validateTrackSlice(next, 'L2');
});

const REBOUND_CODE_STATUS = {
  prompt_version: 'arena-v2',
  prompt_sha256: sha('1'),
  max_tokens: 4096,
  provider_sort_policy: 'throughput',
  temperature_policy: 'provider_default',
  reasoning_policy_version: 'capability_minimum_v1',
  provider_require_parameters: true,
  reasoning_exclude: true,
  response_transport_policy: 'sse_v1',
  collaboration_abi_version: 'bot_tick_v2/1',
  simulator_rules_version: 'sim-2026-09',
  source_limit_bytes: 50 * 1024,
};

function staleCheckpointRunner(rootDir, resultsByModel, runnerCalls, { alwaysStale = false } = {}) {
  let evaluateAttempts = 0;
  return async (args, options) => {
    evaluateAttempts += 1;
    if (alwaysStale || evaluateAttempts === 1) {
      throw new Error(
        'season runner exited with code 1: [arena] Error: generation checkpoint '
        + 'is stale or unverified for vendor/model-0',
      );
    }
    return fakeEvaluateRunner(rootDir, resultsByModel, runnerCalls)(args, options);
  };
}

test('stale checkpoints trigger a recompile-only rebind and one evaluate retry', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const roster = Array.from({ length: 2 }, (_, index) => model({
    model_id: `vendor/model-${index}`,
  }));
  for (const entry of roster) await makeFighter(stateDir, entry.model_id);
  // Feedback recently ran: this test exercises the evaluate rebind path.
  const state = stateWith({ roster, last_feedback_at: new Date(NOW).toISOString() });

  const runnerCalls = [];
  const recompiled = [];
  const logs = [];
  const results = Object.fromEntries(roster.map((entry) => [
    entry.model_id,
    { wins: 3, losses: 1, draws: 0, matches_played: 4 },
  ]));
  const next = await runTrackCycle({
    track: state,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
      fetchRanking: async () => ({ models: [] }),
      runRunner: staleCheckpointRunner(rootDir, results, runnerCalls),
      publishFighter: async () => {},
      recompileFighter: async ({ entrant }) => {
        recompiled.push(entrant.model_id);
        return { wasmBytes: 4321, wasmSha256: sha('9') };
      },
      codeStatus: REBOUND_CODE_STATUS,
      adminToken: 'test-token',
      apiBase: 'http://127.0.0.1:9',
    },
  });

  // Evaluate succeeded on the retry; no submissions consumed, no roster churn.
  assert.equal(next.day_index, 1);
  assert.equal(next.roster.length, 2);
  assert.ok(next.roster.every((entry) => entry.submissions_used === 1));
  assert.ok(next.roster.every((entry) => entry.artifact.version === 1));
  assert.equal(runnerCalls.length, 1);
  assert.equal(recompiled.length, 2);
  assert.ok(logs.some((line) => line.includes('rebinding via recompile')));
  assert.ok(logs.some((line) => line.includes('rebound 2 fighter(s)')));

  // The fighter record checkpoints now carry the rebound contract fields.
  for (const entry of next.roster) {
    const checkpoint = JSON.parse(await fs.readFile(
      path.join(stateDir, 'fighters', entry.model_id.replace(/[^A-Za-z0-9._-]+/g, '__'), 'checkpoint.json'),
      'utf8',
    ));
    assert.equal(checkpoint.simulator_rules_version, 'sim-2026-09');
    assert.equal(checkpoint.prompt_sha256, sha('1'));
    assert.equal(checkpoint.wasm_sha256, sha('9'));
    assert.equal(checkpoint.wasm_bytes, 4321);
  }
  validateTrackSlice(next, 'L2');
});

test('an unfixable contract change fails closed with a manual rebind error', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const roster = Array.from({ length: 2 }, (_, index) => model({
    model_id: `vendor/model-${index}`,
  }));
  for (const entry of roster) await makeFighter(stateDir, entry.model_id);
  const state = stateWith({ roster });

  const recompiled = [];
  await assert.rejects(
    runTrackCycle({
      track: state,
      leagueId: 'cml-test',
      trackId: 'L2',
      flags: { shadow: false },
      stateDirectory: stateDir,
      trackDirectory: stateDir,
      rootDirectory: rootDir,
      deps: {
        nowMs: NOW,
        log: () => {},
        fetchRanking: async () => ({ models: [] }),
        runRunner: staleCheckpointRunner(rootDir, {}, [], { alwaysStale: true }),
        publishFighter: async () => {},
        recompileFighter: async ({ entrant }) => {
          recompiled.push(entrant.model_id);
          return { wasmBytes: 4321, wasmSha256: sha('9') };
        },
        codeStatus: REBOUND_CODE_STATUS,
        adminToken: 'test-token',
        apiBase: 'http://127.0.0.1:9',
      },
    }),
    /server contract changed, manual rebind required/,
  );
  // The rebind was attempted exactly once before failing closed.
  assert.equal(recompiled.length, 2);
});

test('a shadow directory equal to the live directory is refused', async () => {
  assert.throws(
    () => stateDirectoryFromEnv({ shadow: true }, {
      ARENA_CONTINUOUS_STATE_DIR: '/tmp/live',
      ARENA_CONTINUOUS_SHADOW_DIR: '/tmp/live',
    }),
    /ARENA_CONTINUOUS_SHADOW_DIR must differ from the live state directory/,
  );
  assert.throws(
    () => stateDirectoryFromEnv({ shadow: true }, {
      ARENA_CONTINUOUS_STATE_DIR: '/tmp/live',
      ARENA_CONTINUOUS_SHADOW_DIR: '/tmp/live/',
    }),
    /ARENA_CONTINUOUS_SHADOW_DIR must differ from the live state directory/,
  );
});

// --- Task 3: feedback / revision rounds -------------------------------------

const DAY_ONLY_RANKING = { models: [rankingEntry('vendor/model-0', 1)] };

function feedbackDeps(overrides = {}) {
  return {
    nowMs: NOW,
    log: () => {},
    fetchRanking: async () => DAY_ONLY_RANKING,
    sampleBattles: async () => [],
    adminToken: 'test-token',
    apiBase: 'http://127.0.0.1:9',
    ...overrides,
  };
}

async function readSubmissions(stateDir) {
  const raw = await fs.readFile(path.join(stateDir, 'submissions.jsonl'), 'utf8');
  return raw.trim().split('\n').map((line) => JSON.parse(line));
}

test('feedback due: accepted revision bumps version, links parent, resyncs digests', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
    submissions_used: 1,
  });
  await makeFighter(stateDir, keeper.model_id);
  const state = stateWith({ roster: [keeper], last_feedback_at: null });
  const logs = [];
  const next = await runTrackCycle({
    track: state,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      log: (line) => logs.push(line),
      requestRevision: async ({ brief }) => {
        assert.ok(brief.length > 0);
        assert.ok(Buffer.byteLength(brief, 'utf8') <= 2048);
        return {
          response: { simulated: false },
          verified: { source: 'fn bot_tick_v2() { /* v2 */ }\n' },
          codeStatus: null,
        };
      },
      compileRevision: async ({ previousCheckpoint }) => {
        assert.equal(previousCheckpoint.provider_model, 'vendor/model-0');
        return {
          checkpoint: {
            ...previousCheckpoint,
            prompt_sha256: sha('1'),
            source_sha256: sha('2'),
            wasm_sha256: sha('3'),
          },
          source: 'fn bot_tick_v2() { /* v2 */ }\n',
        };
      },
    }),
  });

  const entry = next.roster[0];
  assert.equal(entry.submissions_used, 2);
  assert.equal(entry.artifact.version, 2);
  assert.equal(entry.artifact.parent_version, 1);
  assert.equal(entry.artifact.wasm_sha256, sha('3'));
  assert.equal(entry.artifact.source_sha256, sha('2'));
  assert.equal(entry.artifact.prompt_sha256, sha('1'));
  assert.equal(next.last_feedback_at, new Date(NOW).toISOString());

  // Roster digests resynced with the fighter record on disk.
  const fighterSource = await fs.readFile(
    path.join(stateDir, 'fighters', 'vendor__model-0', 'source.rs'),
    'utf8',
  );
  assert.equal(fighterSource, 'fn bot_tick_v2() { /* v2 */ }\n');
  const fighterCheckpoint = JSON.parse(await fs.readFile(
    path.join(stateDir, 'fighters', 'vendor__model-0', 'checkpoint.json'),
    'utf8',
  ));
  assert.equal(fighterCheckpoint.wasm_sha256, entry.artifact.wasm_sha256);

  const submissions = await readSubmissions(stateDir);
  assert.equal(submissions.length, 1);
  assert.equal(submissions[0].model_id, 'vendor/model-0');
  assert.equal(submissions[0].version_attempted, 2);
  assert.equal(submissions[0].parent_version, 1);
  assert.equal(submissions[0].outcome, 'accepted');
  assert.equal(submissions[0].wasm_sha256, sha('3'));
  assert.match(submissions[0].brief_sha256, /^[a-f0-9]{64}$/);
  assert.equal(submissions[0].at, new Date(NOW).toISOString());

  const revision = next.announcements.find((a) => a.type === 'revision');
  assert.equal(revision.outcome, 'accepted');
  assert.equal(revision.version, 2);
  assert.ok(logs.some((line) => line.includes('revision accepted')));
  validateTrackSlice(next, 'L2');
});

test('compile failure consumes the submission but keeps the old artifact', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
    submissions_used: 1,
  });
  await makeFighter(stateDir, keeper.model_id);
  const state = stateWith({ roster: [keeper], last_feedback_at: null });
  const next = await runTrackCycle({
    track: state,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => ({
        response: { simulated: false },
        verified: { source: 'fn bot_tick_v2() { /* broken */ }\n' },
        codeStatus: null,
      }),
      compileRevision: async () => {
        const error = new Error('fighter compilation failed: rustc E0308');
        error.phase = 'compile';
        throw error;
      },
    }),
  });

  const entry = next.roster[0];
  assert.equal(entry.submissions_used, 2);
  assert.equal(entry.artifact.version, 1);
  assert.equal(entry.artifact.parent_version, null);
  assert.equal(entry.artifact.wasm_sha256, sha('a'));

  const submissions = await readSubmissions(stateDir);
  assert.equal(submissions[0].outcome, 'compile_failed');
  assert.equal(submissions[0].compile_attempts, 1);
  assert.equal(submissions[0].source_sha256, null);
  assert.equal(submissions[0].wasm_sha256, null);

  const revision = next.announcements.find((a) => a.type === 'revision');
  assert.equal(revision.outcome, 'compile_failed');
  assert.equal(revision.version, 2); // the version that was attempted
  validateTrackSlice(next, 'L2');
});

test('codegen failure consumes the submission with outcome codegen_failed', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
    submissions_used: 1,
  });
  await makeFighter(stateDir, keeper.model_id);
  const state = stateWith({ roster: [keeper], last_feedback_at: null });
  const next = await runTrackCycle({
    track: state,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => {
        const error = new Error('POST /api/arena/code/revise failed with HTTP 500');
        error.phase = 'codegen';
        throw error;
      },
    }),
  });

  const entry = next.roster[0];
  assert.equal(entry.submissions_used, 2);
  assert.equal(entry.artifact.version, 1);
  const submissions = await readSubmissions(stateDir);
  assert.equal(submissions[0].outcome, 'codegen_failed');
  assert.equal(submissions[0].compile_attempts, 0);
  validateTrackSlice(next, 'L2');
});

test('a model with 3/3 submissions is never revised', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const exhausted = model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
    submissions_used: 3,
    rating: 60,
    wins: 6,
    losses: 4,
  });
  await makeFighter(stateDir, exhausted.model_id);
  const state = stateWith({ roster: [exhausted], last_feedback_at: null });
  let reviseCalled = false;
  const next = await runTrackCycle({
    track: state,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => {
        reviseCalled = true;
        throw new Error('must not be called');
      },
    }),
  });
  assert.equal(reviseCalled, false);
  assert.equal(next.roster[0].submissions_used, 3);
  assert.equal(next.roster[0].artifact.version, 1);
  assert.equal(next.last_feedback_at, new Date(NOW).toISOString());
  assert.equal(await fs.readFile(path.join(stateDir, 'submissions.jsonl'), 'utf8').catch(() => null), null);
  validateTrackSlice(next, 'L2');
});

test('feedback cadence gate: not due means no revisions', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
    submissions_used: 1,
  });
  await makeFighter(stateDir, keeper.model_id);
  const recent = new Date(NOW - 60 * 60 * 1000).toISOString();
  const state = stateWith({ roster: [keeper], last_feedback_at: recent });
  let reviseCalled = false;
  const next = await runTrackCycle({
    track: state,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => {
        reviseCalled = true;
        throw new Error('must not be called');
      },
    }),
  });
  assert.equal(reviseCalled, false);
  assert.equal(next.roster[0].submissions_used, 1);
  assert.equal(next.last_feedback_at, recent);
  assert.equal(next.announcements.filter((a) => a.type === 'revision').length, 0);
  validateTrackSlice(next, 'L2');
});

test('shadow mode skips feedback without touching the cadence clock', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
    submissions_used: 1,
  });
  const state = stateWith({ roster: [keeper], last_feedback_at: null });
  const logs = [];
  const next = await runTrackCycle({
    track: state,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: true },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      log: (line) => logs.push(line),
      runRunner: async () => {
        throw new Error('runner must not be called in shadow');
      },
      requestRevision: async () => {
        throw new Error('codegen must not be called in shadow');
      },
    }),
  });
  assert.equal(next.roster[0].submissions_used, 1);
  assert.equal(next.last_feedback_at, null);
  assert.ok(logs.some((line) => line.includes('shadow: feedback skipped')));
  validateTrackSlice(next, 'L2');
});

// --- Revision journal idempotency (crash-window double-spend) ----------------

// Journal filenames carry the stint discriminator: <modelKey>-<joinedAtMs>-v<N>.
const JOURNAL_KEY = `vendor__model-0-${NOW}-v2-s2`; // keeper submissions_used 1 -> revision is submission 2

async function writeJournal(stateDir, journal) {
  const dir = path.join(stateDir, 'revision-journal');
  await fs.mkdir(dir, { recursive: true });
  await fs.writeFile(path.join(dir, `${JOURNAL_KEY}.json`), JSON.stringify(journal));
}

function journalBase(overrides = {}) {
  return {
    schema_version: 1,
    league_id: 'cml-test',
    day_index: 0,
    model_id: 'vendor/model-0',
    version_attempted: 2,
    parent_version: 1,
    started_at: new Date(NOW).toISOString(),
    brief_sha256: sha('7'),
    brief: 'journaled brief',
    ...overrides,
  };
}

function keeperState() {
  return model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
    submissions_used: 1,
  });
}

test('a pending revision journal is consumed as interrupted without a provider call', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = keeperState();
  await makeFighter(stateDir, keeper.model_id);
  await writeJournal(stateDir, journalBase({ phase: 'pending' }));
  let codegenCalled = false;
  const next = await runTrackCycle({
    track: stateWith({ roster: [keeper], last_feedback_at: null }),
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => {
        codegenCalled = true;
        throw new Error('provider must not be called for a pending journal');
      },
    }),
  });

  assert.equal(codegenCalled, false);
  const entry = next.roster[0];
  assert.equal(entry.submissions_used, 2);
  assert.equal(entry.artifact.version, 1);
  const submissions = await readSubmissions(stateDir);
  assert.equal(submissions.length, 1);
  assert.equal(submissions[0].outcome, 'interrupted');
  assert.equal(submissions[0].compile_attempts, 0);
  assert.equal(submissions[0].version_attempted, 2);
  const journal = JSON.parse(await fs.readFile(
    path.join(stateDir, 'revision-journal', `${JOURNAL_KEY}.json`),
    'utf8',
  ));
  assert.equal(journal.outcome, 'interrupted');
  assert.equal(next.announcements.find((a) => a.type === 'revision').outcome, 'interrupted');
  validateTrackSlice(next, 'L2');
});

test('a revised journal resumes at compile without a second codegen call', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = keeperState();
  await makeFighter(stateDir, keeper.model_id);
  await writeJournal(stateDir, journalBase({
    phase: 'revised',
    request: {
      response: { simulated: false },
      verified: { source: 'fn bot_tick_v2() { /* v2 */ }\n' },
      codeStatus: null,
    },
  }));
  let codegenCalled = false;
  let compileCalled = 0;
  const next = await runTrackCycle({
    track: stateWith({ roster: [keeper], last_feedback_at: null }),
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => {
        codegenCalled = true;
        throw new Error('provider must not be re-called for a revised journal');
      },
      compileRevision: async ({ request, previousCheckpoint }) => {
        compileCalled += 1;
        assert.equal(request.brief, 'journaled brief');
        assert.equal(previousCheckpoint.provider_model, 'vendor/model-0');
        return {
          checkpoint: {
            ...previousCheckpoint,
            prompt_sha256: sha('1'),
            source_sha256: sha('2'),
            wasm_sha256: sha('3'),
          },
          source: 'fn bot_tick_v2() { /* v2 */ }\n',
        };
      },
    }),
  });

  assert.equal(codegenCalled, false);
  assert.equal(compileCalled, 1);
  const entry = next.roster[0];
  assert.equal(entry.submissions_used, 2);
  assert.equal(entry.artifact.version, 2);
  assert.equal(entry.artifact.parent_version, 1);
  assert.equal(entry.artifact.wasm_sha256, sha('3'));
  const submissions = await readSubmissions(stateDir);
  assert.equal(submissions.length, 1);
  assert.equal(submissions[0].outcome, 'accepted');
  validateTrackSlice(next, 'L2');
});

test('a finalized journal re-applies its outcome without IO or ledger writes', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = keeperState();
  await makeFighter(stateDir, keeper.model_id);
  await writeJournal(stateDir, journalBase({
    phase: 'compiled',
    checkpoint: { prompt_sha256: sha('1'), source_sha256: sha('2'), wasm_sha256: sha('3') },
    source: 'fn bot_tick_v2() { /* v2 */ }\n',
    outcome: 'accepted',
    completed_at: new Date(NOW).toISOString(),
  }));
  // The ledger record from the crashed cycle already exists.
  await fs.writeFile(path.join(stateDir, 'submissions.jsonl'), `${JSON.stringify({
    model_id: 'vendor/model-0',
    slug: keeper.slug,
    version_attempted: 2,
    parent_version: 1,
    prompt_sha256: sha('1'),
    brief_sha256: sha('7'),
    source_sha256: sha('2'),
    wasm_sha256: sha('3'),
    compile_attempts: 1,
    outcome: 'accepted',
    at: new Date(NOW).toISOString(),
  })}\n`);
  const next = await runTrackCycle({
    track: stateWith({ roster: [keeper], last_feedback_at: null }),
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => {
        throw new Error('provider must not be called for a finalized journal');
      },
      compileRevision: async () => {
        throw new Error('compile must not run for a finalized journal');
      },
    }),
  });

  const entry = next.roster[0];
  assert.equal(entry.submissions_used, 2);
  assert.equal(entry.artifact.version, 2);
  assert.equal(entry.artifact.parent_version, 1);
  assert.equal(entry.artifact.wasm_sha256, sha('3'));
  const submissions = await readSubmissions(stateDir);
  assert.equal(submissions.length, 1); // no duplicate lineage record
  assert.equal(next.announcements.find((a) => a.type === 'revision').outcome, 'accepted');
  validateTrackSlice(next, 'L2');
});

test('a compiled journal resumes the commit stage without duplicating the ledger', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = keeperState();
  await makeFighter(stateDir, keeper.model_id);
  await writeJournal(stateDir, journalBase({
    phase: 'compiled',
    checkpoint: {
      provider_model: 'vendor/model-0',
      prompt_sha256: sha('1'),
      source_sha256: sha('2'),
      wasm_sha256: sha('3'),
    },
    source: 'fn bot_tick_v2() { /* v2 */ }\n',
  }));
  // Crash window: the jsonl record was appended but the journal never
  // finalized — the rerun must dedup the ledger append.
  await fs.writeFile(path.join(stateDir, 'submissions.jsonl'), `${JSON.stringify({
    track: 'L2',
    model_id: 'vendor/model-0',
    slug: keeper.slug,
    stint: keeper.joined_at,
    submission: 2,
    version_attempted: 2,
    parent_version: 1,
    prompt_sha256: sha('1'),
    brief_sha256: sha('7'),
    source_sha256: sha('2'),
    wasm_sha256: sha('3'),
    compile_attempts: 1,
    outcome: 'accepted',
    at: new Date(NOW).toISOString(),
  })}\n`);
  const next = await runTrackCycle({
    track: stateWith({ roster: [keeper], last_feedback_at: null }),
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => {
        throw new Error('provider must not be called for a compiled journal');
      },
      compileRevision: async () => {
        throw new Error('compile must not run for a compiled journal');
      },
    }),
  });

  const entry = next.roster[0];
  assert.equal(entry.submissions_used, 2);
  assert.equal(entry.artifact.version, 2);
  const submissions = await readSubmissions(stateDir);
  assert.equal(submissions.length, 1);
  const journal = JSON.parse(await fs.readFile(
    path.join(stateDir, 'revision-journal', `${JOURNAL_KEY}.json`),
    'utf8',
  ));
  assert.equal(journal.outcome, 'accepted');
  // The fighter record was rewritten from the journaled checkpoint/source.
  assert.equal(
    await fs.readFile(path.join(stateDir, 'fighters', 'vendor__model-0', 'source.rs'), 'utf8'),
    'fn bot_tick_v2() { /* v2 */ }\n',
  );
  validateTrackSlice(next, 'L2');
});

test('an untagged post-codegen failure records interrupted, not codegen_failed', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = keeperState();
  await makeFighter(stateDir, keeper.model_id);
  const next = await runTrackCycle({
    track: stateWith({ roster: [keeper], last_feedback_at: null }),
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => ({
        response: { simulated: false },
        verified: { source: 'fn bot_tick_v2() {}\n' },
        codeStatus: null,
      }),
      compileRevision: async () => {
        // Untagged: not a compile-route failure, something local blew up.
        throw new Error('EROFS: read-only file system');
      },
    }),
  });

  const entry = next.roster[0];
  assert.equal(entry.submissions_used, 2);
  assert.equal(entry.artifact.version, 1);
  const submissions = await readSubmissions(stateDir);
  assert.equal(submissions[0].outcome, 'interrupted');
  assert.equal(submissions[0].compile_attempts, 0);
  validateTrackSlice(next, 'L2');
});

// --- Multi-track orchestration (schema v2) -----------------------------------

const ALL_TRACKS = ['L0', 'L1', 'L2', 'L3'];

test('--track runs only the selected track', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const state = createState({ now: new Date(NOW), leagueId: 'cml-test' });
  state.tracks.L2.roster.push(model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
  }));
  state.tracks.L2.last_feedback_at = new Date(NOW).toISOString();
  const logs = [];
  const next = await runCycle({
    state,
    flags: { shadow: false, track: 'L2' },
    stateDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
      fetchRanking: async () => ({ models: [rankingEntry('vendor/model-0', 1)] }),
      fetchCatalog: async () => ({ models: [] }),
      adminToken: 'test-token',
      apiBase: 'http://127.0.0.1:9',
    },
  });
  // L2 ran (recruit dry-run happened, no challengers), the rest untouched.
  assert.equal(next.tracks.L2.roster.length, 1);
  for (const trackId of ['L0', 'L1', 'L3']) {
    assert.equal(next.tracks[trackId].roster.length, 0, trackId);
    assert.equal(next.tracks[trackId].announcements.length, 0, trackId);
  }
  validateState(next);
});

test('a failing track never blocks the other tracks', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const state = createState({ now: new Date(NOW), leagueId: 'cml-test' });
  // L0 has two models but NO fighter records: its evaluate fails; the other
  // tracks must still run their (empty-roster, shadow) cycles.
  state.tracks.L0.roster.push(model({ model_id: 'vendor/a' }), model({ model_id: 'vendor/b' }));
  const errors = [];
  const next = await runCycle({
    state,
    flags: { shadow: true },
    stateDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: () => {},
      errorLog: (line) => errors.push(line),
    },
  });
  assert.equal(next.tracks.L0.day_index, 0); // evaluate failed before increment
  validateState(next);
});

test('retirement is isolated per track', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const state = createState({ now: new Date(NOW), leagueId: 'cml-test' });
  // The same model: exhausted + below the bar in L0 (max 1 submission),
  // healthy and still revisable in L2.
  state.tracks.L0.roster.push(model({
    model_id: 'vendor/model-x',
    submissions_used: 1,
    rating: 20,
    wins: 1,
    losses: 9,
    matches: 10,
    joined_at: new Date(NOW - 4 * DAY_MS).toISOString(),
  }));
  state.tracks.L2.roster.push(model({
    model_id: 'vendor/model-x',
    submissions_used: 1,
    rating: 80,
    wins: 8,
    losses: 2,
    matches: 10,
    joined_at: new Date(NOW - 4 * DAY_MS).toISOString(),
  }));
  const next = await runCycle({
    state,
    flags: { shadow: true },
    stateDirectory: stateDir,
    rootDirectory: rootDir,
    deps: { nowMs: NOW, log: () => {} },
  });
  assert.equal(next.tracks.L0.roster.length, 0);
  assert.equal(next.tracks.L0.retired.length, 1);
  assert.equal(next.tracks.L0.retired[0].model_id, 'vendor/model-x');
  assert.equal(next.tracks.L0.announcements[0].track, 'L0');
  assert.match(next.tracks.L0.retired[0].reason, /days in track L0/);
  assert.equal(next.tracks.L2.roster.length, 1);
  assert.equal(next.tracks.L2.retired.length, 0);
  validateState(next);
});

test('L0 recruit passes compileAttempts=1 and a failed compile means no entry', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const track = stateWith({
    policy: policySlice('L0'),
    roster: [],
  });
  let seenAttempts = null;
  const next = await runTrackCycle({
    track,
    leagueId: 'cml-test',
    trackId: 'L0',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: () => {},
      fetchRanking: async () => ({ models: [rankingEntry('vendor/new-1', 1)] }),
      generateFighter: async ({ compileAttempts }) => {
        seenAttempts = compileAttempts;
        throw new Error('fighter compilation failed: error[E0308]');
      },
      adminToken: 'test-token',
      apiBase: 'http://127.0.0.1:9',
    },
  });
  assert.equal(seenAttempts, 1);
  assert.equal(next.roster.length, 0);
  assert.equal(next.announcements.length, 0);
  validateTrackSlice(next, 'L0');
});

test('L1 recruit passes compileAttempts=3', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const track = stateWith({ policy: policySlice('L1'), roster: [] });
  let seenAttempts = null;
  await runTrackCycle({
    track,
    leagueId: 'cml-test',
    trackId: 'L1',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: () => {},
      fetchRanking: async () => ({ models: [rankingEntry('vendor/new-1', 1)] }),
      generateFighter: async ({ compileAttempts }) => {
        seenAttempts = compileAttempts;
        throw new Error('still broken');
      },
      adminToken: 'test-token',
      apiBase: 'http://127.0.0.1:9',
    },
  });
  assert.equal(seenAttempts, 3);
});

test('L2 caps revisions at 2 (submissions exhausted at 3)', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
    submissions_used: 2, // one revision already accepted
    artifact: {
      wasm_sha256: sha('a'),
      source_sha256: sha('b'),
      prompt_sha256: sha('c'),
      version: 2,
      parent_version: 1,
    },
  });
  await makeFighter(stateDir, keeper.model_id);
  const track = stateWith({ roster: [keeper], last_feedback_at: null });
  const next = await runTrackCycle({
    track,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => ({
        response: { simulated: false },
        verified: { source: 'fn bot_tick_v2() { /* v2 */ }\n' },
        codeStatus: null,
      }),
      compileRevision: async ({ previousCheckpoint }) => ({
        checkpoint: {
          ...previousCheckpoint,
          prompt_sha256: sha('1'),
          source_sha256: sha('2'),
          wasm_sha256: sha('3'),
        },
        source: 'fn bot_tick_v2() { /* v2 */ }\n',
      }),
    }),
  });
  // Second revision accepted: submissions 3/3, artifact v3 with parent v2.
  assert.equal(next.roster[0].submissions_used, 3);
  assert.equal(next.roster[0].artifact.version, 3);
  assert.equal(next.roster[0].artifact.parent_version, 2);

  // Next round: exhausted, never revised again.
  const again = await runTrackCycle({
    track: { ...next, roster: next.roster.map((entry) => ({ ...entry })), last_feedback_at: null },
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => {
        throw new Error('must not be called: L2 revisions are capped at 2');
      },
    }),
  });
  assert.equal(again.roster[0].submissions_used, 3);
  assert.equal(again.roster[0].artifact.version, 3);
  validateTrackSlice(again, 'L2');
});

test('L3 revises weekly: not due at 3 days, due at 8 days', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW - 10 * DAY_MS).toISOString(),
    days_in_league: 10,
    submissions_used: 4,
    artifact: {
      wasm_sha256: sha('a'),
      source_sha256: sha('b'),
      prompt_sha256: sha('c'),
      version: 4,
      parent_version: 3,
    },
  });
  await makeFighter(stateDir, keeper.model_id);

  const notDue = await runTrackCycle({
    track: stateWith({
      policy: policySlice('L3'),
      roster: [keeper],
      last_feedback_at: new Date(NOW - 3 * DAY_MS).toISOString(),
    }),
    leagueId: 'cml-test',
    trackId: 'L3',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => {
        throw new Error('must not be called: L3 revises weekly');
      },
    }),
  });
  assert.equal(notDue.roster[0].submissions_used, 4);

  const due = await runTrackCycle({
    track: stateWith({
      policy: policySlice('L3'),
      roster: [keeper],
      last_feedback_at: new Date(NOW - 8 * DAY_MS).toISOString(),
    }),
    leagueId: 'cml-test',
    trackId: 'L3',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => ({
        response: { simulated: false },
        verified: { source: 'fn bot_tick_v2() { /* v5 */ }\n' },
        codeStatus: null,
      }),
      compileRevision: async ({ previousCheckpoint }) => ({
        checkpoint: {
          ...previousCheckpoint,
          prompt_sha256: sha('1'),
          source_sha256: sha('2'),
          wasm_sha256: sha('3'),
        },
        source: 'fn bot_tick_v2() { /* v5 */ }\n',
      }),
    }),
  });
  assert.equal(due.roster[0].submissions_used, 5);
  assert.equal(due.roster[0].artifact.version, 5);
  assert.equal(due.roster[0].artifact.parent_version, 4);
  const submissions = await readSubmissions(stateDir);
  assert.equal(submissions[0].track, 'L3');
  validateTrackSlice(due, 'L3');
});

test('retire then re-recruit starts a fresh stint with its own journal and ledger lineage', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const OLD_JOINED = new Date(NOW - 12 * DAY_MS).toISOString();
  const OLD_JOURNAL_KEY = `vendor__model-x-${NOW - 12 * DAY_MS}-v2-s2`;
  // Old stint: retired 8 days ago (past the 7-day re-recruit cooldown).
  const retiredEntry = {
    ...model({
      model_id: 'vendor/model-x',
      joined_at: OLD_JOINED,
      submissions_used: 3,
      rating: 20,
      wins: 1,
      losses: 9,
      matches: 10,
      days_in_league: 4,
    }),
    retired_at: new Date(NOW - 8 * DAY_MS).toISOString(),
    reason: 'old stint exhausted',
  };
  // Old stint's finalized journal + ledger record for v2.
  await fs.mkdir(path.join(stateDir, 'revision-journal'), { recursive: true });
  await fs.writeFile(
    path.join(stateDir, 'revision-journal', `${OLD_JOURNAL_KEY}.json`),
    JSON.stringify(journalBase({
      model_id: 'vendor/model-x',
      stint: OLD_JOINED,
      phase: 'compiled',
      checkpoint: { prompt_sha256: sha('4'), source_sha256: sha('5'), wasm_sha256: sha('6') },
      outcome: 'accepted',
      completed_at: new Date(NOW - 11 * DAY_MS).toISOString(),
    })),
  );
  await fs.writeFile(path.join(stateDir, 'submissions.jsonl'), `${JSON.stringify({
    track: 'L2',
    model_id: 'vendor/model-x',
    slug: retiredEntry.slug,
    stint: OLD_JOINED,
    submission: 2,
    version_attempted: 2,
    parent_version: 1,
    prompt_sha256: sha('4'),
    brief_sha256: sha('7'),
    source_sha256: sha('5'),
    wasm_sha256: sha('6'),
    compile_attempts: 1,
    outcome: 'accepted',
    at: new Date(NOW - 11 * DAY_MS).toISOString(),
  })}\n`);

  // Cycle 1: the cooldown has elapsed, so the model is re-recruited at v1.
  const track = stateWith({ retired: [retiredEntry] });
  const afterRecruit = await runTrackCycle({
    track,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      fetchRanking: async () => ({ models: [rankingEntry('vendor/model-x', 1)] }),
      generateFighter: async () => ({
        checkpoint: {
          provider_model: 'vendor/model-x',
          wasm_sha256: sha('d'),
          source_sha256: sha('e'),
          prompt_sha256: sha('f'),
        },
        source: 'fn bot_tick_v2() { /* new stint */ }\n',
      }),
    }),
  });
  assert.equal(afterRecruit.roster.length, 1);
  assert.equal(afterRecruit.roster[0].artifact.version, 1);
  assert.equal(afterRecruit.roster[0].joined_at, new Date(NOW).toISOString());

  // Cycle 2 (3 days later): feedback revises the NEW stint at v2.
  const afterRevision = await runTrackCycle({
    track: afterRecruit,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      nowMs: NOW + 3 * DAY_MS,
      requestRevision: async () => ({
        response: { simulated: false },
        verified: { source: 'fn bot_tick_v2() { /* new stint v2 */ }\n' },
        codeStatus: null,
      }),
      compileRevision: async ({ previousCheckpoint }) => ({
        checkpoint: {
          ...previousCheckpoint,
          prompt_sha256: sha('1'),
          source_sha256: sha('2'),
          wasm_sha256: sha('3'),
        },
        source: 'fn bot_tick_v2() { /* new stint v2 */ }\n',
      }),
    }),
  });

  const entry = afterRevision.roster[0];
  assert.equal(entry.submissions_used, 2);
  assert.equal(entry.artifact.version, 2);
  assert.equal(entry.artifact.parent_version, 1);

  // Fresh journal for the new stint; the old stint's journal is untouched.
  const journals = (await fs.readdir(path.join(stateDir, 'revision-journal'))).sort();
  assert.deepEqual(journals, [
    `${OLD_JOURNAL_KEY}.json`,
    `vendor__model-x-${NOW}-v2-s2.json`,
  ]);
  const oldJournal = JSON.parse(await fs.readFile(
    path.join(stateDir, 'revision-journal', `${OLD_JOURNAL_KEY}.json`),
    'utf8',
  ));
  assert.equal(oldJournal.stint, OLD_JOINED);
  assert.equal(oldJournal.outcome, 'accepted');

  // Fresh ledger record alongside the old stint's — no dedup collision.
  const submissions = await readSubmissions(stateDir);
  assert.equal(submissions.length, 2);
  assert.deepEqual(
    submissions.map((record) => [record.stint, record.version_attempted, record.outcome]),
    [[OLD_JOINED, 2, 'accepted'], [new Date(NOW).toISOString(), 2, 'accepted']],
  );
  validateTrackSlice(afterRevision, 'L2');
});

// --- 40-model divisions ------------------------------------------------------

test('divisionSlices partitions by rating with slug tie-break', async () => {
  const { divisionSlices } = await import('../../continuous_league.mjs');
  const roster = [
    model({ model_id: 'vendor/b', slug: 'b', rating: 50 }),
    model({ model_id: 'vendor/a', slug: 'a', rating: 50 }),
    model({ model_id: 'vendor/c', slug: 'c', rating: 90 }),
    model({ model_id: 'vendor/d', slug: 'd', rating: 10 }),
  ];
  const slices = divisionSlices(roster, 2);
  assert.deepEqual(slices.map((slice) => slice.name), ['premier', 'challenger']);
  // rating desc; the 50-50 tie breaks by slug ascending (a before b).
  assert.deepEqual(slices[0].models.map((entry) => entry.slug), ['c', 'a']);
  assert.deepEqual(slices[1].models.map((entry) => entry.slug), ['b', 'd']);

  // A full 40-model roster yields four divisions of ten.
  const forty = Array.from({ length: 40 }, (_, index) => model({
    model_id: `vendor/m${String(index).padStart(2, '0')}`,
    slug: `m${String(index).padStart(2, '0')}`,
    rating: 100 - index,
  }));
  const four = divisionSlices(forty);
  assert.deepEqual(four.map((slice) => slice.name), ['premier', 'challenger', 'contender', 'prospect']);
  assert.ok(four.every((slice) => slice.models.length === 10));
  assert.equal(four[0].models[0].rating, 100);
  assert.equal(four[3].models[9].rating, 61);
});

test('evaluate runs one season per division with per-division season ids', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const roster = Array.from({ length: 40 }, (_, index) => model({
    model_id: `vendor/m${String(index).padStart(2, '0')}`,
    slug: `m${String(index).padStart(2, '0')}`,
    rating: 100 - index,
    wins: 0,
    losses: 0,
    draws: 0,
    matches: 0,
  }));
  for (const entry of roster) await makeFighter(stateDir, entry.model_id);
  const state = stateWith({ roster, last_feedback_at: new Date(NOW).toISOString() });

  const runnerCalls = [];
  const results = Object.fromEntries(roster.map((entry) => [
    entry.model_id,
    { wins: 3, losses: 1, draws: 0, matches_played: 4 },
  ]));
  const next = await runTrackCycle({
    track: state,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: () => {},
      fetchRanking: async () => ({ models: roster.map((entry, index) => rankingEntry(entry.model_id, index + 1)) }),
      runRunner: fakeEvaluateRunner(rootDir, results, runnerCalls),
      publishFighter: async () => {},
      adminToken: 'test-token',
      apiBase: 'http://127.0.0.1:9',
    },
  });

  assert.equal(runnerCalls.length, 4);
  const seasonIds = runnerCalls.map((call) => call.args[call.args.indexOf('--season-id') + 1]);
  assert.deepEqual(seasonIds, [
    'continuous-cml-test-L2-day0-premier',
    'continuous-cml-test-L2-day0-challenger',
    'continuous-cml-test-L2-day0-contender',
    'continuous-cml-test-L2-day0-prospect',
  ]);
  // Ratings accumulated across the division seasons: every model played 4.
  assert.ok(next.roster.every((entry) => entry.matches === 4 && entry.wins === 3));
  assert.equal(next.day_index, 1);

  // History snapshot records the post-evaluate division of each entry.
  const history = JSON.parse(await fs.readFile(
    path.join(stateDir, 'history', '2026-08-23.json'),
    'utf8',
  ));
  assert.equal(history.length, 1);
  const divisions = history[0].roster.map((entry) => entry.division);
  assert.equal(divisions.filter((name) => name === 'premier').length, 10);
  assert.equal(divisions.filter((name) => name === 'challenger').length, 10);
  assert.equal(divisions.filter((name) => name === 'contender').length, 10);
  assert.equal(divisions.filter((name) => name === 'prospect').length, 10);
  validateTrackSlice(next, 'L2');
});

test('recruit fills only open slots, in ranking order beyond the roster', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
  });
  const rankingModels = [
    rankingEntry('vendor/model-0', 1),
    ...Array.from({ length: 4 }, (_, index) => rankingEntry(`vendor/new-${index + 1}`, index + 2)),
  ];
  const generatedFor = [];
  const next = await runTrackCycle({
    track: stateWith({
      roster: [keeper],
      last_feedback_at: new Date(NOW).toISOString(),
    }),
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: () => {},
      fetchRanking: async () => ({ models: rankingModels }),
      generateFighter: async ({ entrant }) => {
        generatedFor.push(entrant.provider_model);
        return {
          checkpoint: {
            wasm_sha256: sha('d'),
            source_sha256: sha('e'),
            prompt_sha256: sha('f'),
          },
          source: 'fn bot_tick_v2() {}\n',
        };
      },
      adminToken: 'test-token',
      apiBase: 'http://127.0.0.1:9',
    },
  });
  // The roster model is skipped; the four challengers enter in ranking order.
  assert.deepEqual(generatedFor, ['vendor/new-1', 'vendor/new-2', 'vendor/new-3', 'vendor/new-4']);
  assert.equal(next.roster.length, 5);
  validateTrackSlice(next, 'L2');
});

test('a recruit generation failure is recorded in the cooldown ledger and skips the model for 7 days', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const recruitFailures = {};
  const rankingModels = [rankingEntry('vendor/new-1', 1), rankingEntry('vendor/new-2', 2)];
  const deps = {
    nowMs: NOW,
    log: () => {},
    fetchRanking: async () => ({ models: rankingModels }),
    generateFighter: async ({ entrant }) => {
      if (entrant.provider_model === 'vendor/new-1') {
        throw new Error('fighter compilation failed: error[E0308]');
      }
      return {
        checkpoint: { wasm_sha256: sha('d'), source_sha256: sha('e'), prompt_sha256: sha('f') },
        source: 'fn bot_tick_v2() {}\n',
      };
    },
    adminToken: 'test-token',
    apiBase: 'http://127.0.0.1:9',
  };
  const track = stateWith({
    roster: [],
    last_feedback_at: new Date(NOW).toISOString(),
  });
  const first = await runTrackCycle({
    track,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    recruitFailures,
    rootDirectory: rootDir,
    deps,
  });
  // new-1 failed and was recorded; new-2 entered.
  assert.equal(recruitFailures['vendor/new-1'], new Date(NOW).toISOString());
  assert.equal(recruitFailures['vendor/new-2'], undefined);
  assert.deepEqual(first.roster.map((entry) => entry.model_id), ['vendor/new-2']);

  // Next day: new-1 is cooldown-skipped (not re-attempted).
  let attempted = [];
  const second = await runTrackCycle({
    track: first,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    recruitFailures,
    rootDirectory: rootDir,
    deps: {
      ...deps,
      nowMs: NOW + DAY_MS,
      generateFighter: async ({ entrant }) => {
        attempted.push(entrant.provider_model);
        throw new Error('must not be re-attempted within the cooldown');
      },
    },
  });
  assert.deepEqual(attempted, []);
  assert.deepEqual(second.roster.map((entry) => entry.model_id), ['vendor/new-2']);
  validateTrackSlice(second, 'L2');
});

test('a ranking fetch failure records nothing in the cooldown ledger', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const recruitFailures = {};
  await runTrackCycle({
    track: stateWith({
      roster: [],
      last_feedback_at: new Date(NOW).toISOString(),
    }),
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    recruitFailures,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: () => {},
      fetchRanking: async () => {
        throw new Error('OpenRouter ranking request failed with HTTP 500');
      },
    },
  });
  assert.deepEqual(recruitFailures, {});
});

test('a failed revision is retried under a fresh journal, not phantom re-consumed', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = keeperState();
  await makeFighter(stateDir, keeper.model_id);
  // Round 1: compile failure — submission consumed (2/3), artifact kept at v1.
  const failed = await runTrackCycle({
    track: stateWith({ roster: [keeper], last_feedback_at: null }),
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      requestRevision: async () => ({
        response: { simulated: false },
        verified: { source: 'fn bot_tick_v2() { /* broken */ }\n' },
        codeStatus: null,
      }),
      compileRevision: async () => {
        const error = new Error('fighter compilation failed');
        error.phase = 'compile';
        throw error;
      },
    }),
  });
  assert.equal(failed.roster[0].submissions_used, 2);
  assert.equal(failed.roster[0].artifact.version, 1);

  // Round 2 (next feedback window): the v2 attempt is RETRIED under a new
  // submission ordinal — accepted this time.
  const accepted = await runTrackCycle({
    track: failed,
    leagueId: 'cml-test',
    trackId: 'L2',
    flags: { shadow: false },
    stateDirectory: stateDir,
    trackDirectory: stateDir,
    rootDirectory: rootDir,
    deps: feedbackDeps({
      nowMs: NOW + 3 * DAY_MS,
      requestRevision: async () => ({
        response: { simulated: false },
        verified: { source: 'fn bot_tick_v2() { /* fixed */ }\n' },
        codeStatus: null,
      }),
      compileRevision: async ({ previousCheckpoint }) => ({
        checkpoint: {
          ...previousCheckpoint,
          prompt_sha256: sha('1'),
          source_sha256: sha('2'),
          wasm_sha256: sha('3'),
        },
        source: 'fn bot_tick_v2() { /* fixed */ }\n',
      }),
    }),
  });
  const entry = accepted.roster[0];
  // Exactly one more submission consumed (3/3) — not two.
  assert.equal(entry.submissions_used, 3);
  assert.equal(entry.artifact.version, 2);
  assert.equal(entry.artifact.parent_version, 1);

  // Two distinct journals (s2 failed, s3 accepted) and two ledger records.
  const journals = (await fs.readdir(path.join(stateDir, 'revision-journal'))).sort();
  assert.deepEqual(journals, [
    `vendor__model-0-${NOW}-v2-s2.json`,
    `vendor__model-0-${NOW}-v2-s3.json`,
  ]);
  const submissions = await readSubmissions(stateDir);
  assert.deepEqual(
    submissions.map((record) => [record.submission, record.version_attempted, record.outcome]),
    [[2, 2, 'compile_failed'], [3, 2, 'accepted']],
  );
  validateTrackSlice(accepted, 'L2');
});

// --- New-release fast lane (cycle wiring) ------------------------------------

function catalogModel(id, createdMs, slug = null) {
  return {
    provider_rank: 1,
    id,
    canonical_slug: slug ?? `${id}-20260831`,
    name: `Vendor: ${id}`,
    pricing: null,
    context_length: 1000000,
    created: Math.floor(createdMs / 1000),
    reasoning_policy: { ...REASONING_POLICY },
  };
}

function fastLaneDeps(catalog, overrides = {}) {
  return {
    nowMs: NOW,
    log: () => {},
    errorLog: () => {},
    fetchRanking: async () => ({ models: [] }),
    fetchCatalog: async () => ({ models: catalog }),
    generateFighter: async ({ entrant }) => ({
      checkpoint: {
        wasm_sha256: sha('d'),
        source_sha256: sha('e'),
        prompt_sha256: sha('f'),
      },
      source: 'fn bot_tick_v2() {}\n',
    }),
    adminToken: 'test-token',
    apiBase: 'http://127.0.0.1:9',
    ...overrides,
  };
}

function fastLaneState() {
  const state = createState({ now: new Date(NOW), leagueId: 'cml-test' });
  const keeper = model({
    model_id: 'vendor/keeper',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
  });
  for (const trackId of ALL_TRACKS) {
    state.tracks[trackId].roster.push({ ...keeper });
    state.tracks[trackId].last_feedback_at = new Date(NOW).toISOString();
  }
  return state;
}

test('fast lane recruits up to 2 new releases into all tracks with one artifact each', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const state = fastLaneState();
  const catalog = [
    catalogModel('vendor/newest', NOW - DAY_MS),
    catalogModel('vendor/middle', NOW - 3 * DAY_MS),
    catalogModel('vendor/too-old', NOW - 20 * DAY_MS),
    catalogModel('vendor/keeper', NOW - 2 * DAY_MS),
  ];
  const generatedFor = [];
  const next = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
    rootDirectory: rootDir,
    deps: fastLaneDeps(catalog, {
      generateFighter: async ({ entrant }) => {
        generatedFor.push(entrant.provider_model);
        return fastLaneDeps([]).generateFighter({ entrant });
      },
    }),
  });

  // ≤2/day, newest first, one codegen per model (not four).
  assert.deepEqual(generatedFor, ['vendor/newest', 'vendor/middle']);
  for (const trackId of ALL_TRACKS) {
    const roster = next.tracks[trackId].roster;
    assert.equal(roster.length, 3, trackId);
    for (const id of ['vendor/newest', 'vendor/middle']) {
      const entry = roster.find((candidate) => candidate.model_id === id);
      assert.equal(entry.submissions_used, 1);
      assert.equal(entry.artifact.version, 1);
      assert.equal(entry.rating, 50);
    }
    const announcements = next.tracks[trackId].announcements.filter((a) => a.type === 'fresh_challenger');
    assert.deepEqual(announcements.map((a) => a.model_id), ['vendor/newest', 'vendor/middle'], trackId);
    assert.equal(announcements[0].track, trackId);
    // Shared artifact copied into this track's fighter store.
    const fighter = JSON.parse(await fs.readFile(
      path.join(stateDir, 'tracks', trackId, 'fighters', 'vendor__newest', 'checkpoint.json'),
      'utf8',
    ));
    assert.equal(fighter.wasm_sha256, sha('d'));
  }
  assert.deepEqual(Object.keys(next.recruit_failures), []);
  validateState(next);
});

test('fast lane displaces the bottom-rated tenured model in full tracks only', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const state = fastLaneState();
  const bottom = model({
    model_id: 'vendor/bottom',
    rating: 40,
    wins: 5,
    losses: 5,
    matches: 10,
    joined_at: new Date(NOW - 5 * DAY_MS).toISOString(),
    days_in_league: 5,
  });
  state.tracks.L0.roster = [
    ...Array.from({ length: 39 }, (_, index) => model({
      model_id: `vendor/fill-${index}`,
      rating: 90 - index,
      joined_at: new Date(NOW - 5 * DAY_MS).toISOString(),
    })),
    bottom,
  ];
  // L1 is also full, but every model is sub-tenure: no displacement allowed.
  state.tracks.L1.roster = Array.from({ length: 40 }, (_, index) => model({
    model_id: `vendor/young-${index}`,
    rating: 90 - index,
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
  }));
  const catalog = [catalogModel('vendor/fresh', NOW - DAY_MS)];
  const next = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
    rootDirectory: rootDir,
    deps: fastLaneDeps(catalog),
  });

  // L0: bottom displaced through the normal retirement path, fresh added.
  const l0 = next.tracks.L0;
  assert.equal(l0.roster.length, 40);
  assert.ok(l0.roster.some((entry) => entry.model_id === 'vendor/fresh'));
  assert.ok(!l0.roster.some((entry) => entry.model_id === 'vendor/bottom'));
  assert.equal(l0.retired.length, 1);
  assert.equal(l0.retired[0].model_id, 'vendor/bottom');
  assert.equal(l0.retired[0].reason, 'displaced by fresh challenger vendor/fresh');
  const retirement = l0.announcements.find((a) => a.type === 'retirement');
  assert.equal(retirement.reason, 'displaced by fresh challenger vendor/fresh');
  assert.equal(retirement.stats.rating, 40);

  // L1: full with no tenured model — not joined, nobody displaced.
  const l1 = next.tracks.L1;
  assert.equal(l1.roster.length, 40);
  assert.ok(!l1.roster.some((entry) => entry.model_id === 'vendor/fresh'));
  assert.equal(l1.retired.length, 0);

  // Sub-full tracks just add.
  assert.ok(next.tracks.L2.roster.some((entry) => entry.model_id === 'vendor/fresh'));
  assert.ok(next.tracks.L3.roster.some((entry) => entry.model_id === 'vendor/fresh'));
  validateState(next);
});

test('fast lane cost guards: payment errors skip unrecorded, genuine failures record', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const state = fastLaneState();
  const catalog = [catalogModel('vendor/fresh', NOW - DAY_MS)];
  // 402 payment required: no roster change, nothing recorded.
  const payment = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
    rootDirectory: rootDir,
    deps: fastLaneDeps(catalog, {
      generateFighter: async () => {
        throw new Error('POST /api/arena/code/generate failed with HTTP 402');
      },
    }),
  });
  assert.equal(payment.tracks.L2.roster.length, 1);
  assert.deepEqual(Object.keys(payment.recruit_failures), []);
  validateState(payment);

  // Genuine compile failure: recorded in the cooldown ledger.
  const { stateDir: stateDir2, rootDir: rootDir2 } = await tempDirs();
  const genuine = await runCycle({
    state: fastLaneState(),
    flags: { shadow: false },
    stateDirectory: stateDir2,
    rootDirectory: rootDir2,
    deps: fastLaneDeps(catalog, {
      generateFighter: async () => {
        throw new Error('fighter compilation failed: error[E0308]');
      },
    }),
  });
  assert.equal(genuine.tracks.L2.roster.length, 1);
  assert.equal(genuine.recruit_failures['vendor/fresh'], new Date(NOW).toISOString());
  validateState(genuine);
});
