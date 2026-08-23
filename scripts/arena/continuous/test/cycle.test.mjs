import { test } from 'node:test';
import assert from 'node:assert/strict';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { mascotFor } from '../../mascots.mjs';
import { runCycle, stateDirectoryFromEnv } from '../../continuous_league.mjs';
import { writeFighterRecord } from '../generation.mjs';
import { validateState } from '../state.mjs';

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

function stateWith(overrides = {}) {
  return {
    schema_version: 1,
    league_id: 'cml-test',
    day_index: 0,
    roster: [],
    retired: [],
    announcements: [],
    last_feedback_at: null,
    created_at: '2026-08-18T00:00:00.000Z',
    updated_at: '2026-08-23T00:00:00.000Z',
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
  const next = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
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
  assert.ok(args.includes('continuous-cml-test-day0'));
  assert.match(env.ARENA_SEEDS, /^\d+,\d+,\d+,\d+$/);
  assert.equal(env.ARENA_TOP_MODELS, '10');

  const history = JSON.parse(await fs.readFile(
    path.join(stateDir, 'history', '2026-08-23.json'),
    'utf8',
  ));
  assert.equal(history.length, 1);
  assert.equal(history[0].season_id, 'continuous-cml-test-day0');
  assert.equal(history[0].day_index, 0);
  assert.equal(history[0].roster.length, 10);
  assert.equal(history[0].roster[0].rating, next.roster[0].rating);

  const seasonDir = path.join(rootDir, 'artifacts/arena/seasons', 'continuous-cml-test-day0');
  assert.equal((await fs.readdir(path.join(seasonDir, 'generations'))).length, 10);
  assert.equal((await fs.readdir(path.join(seasonDir, 'sources'))).length, 10);

  const ranking = JSON.parse(await fs.readFile(
    path.join(stateDir, 'rankings', 'continuous-cml-test-day0.json'),
    'utf8',
  ));
  assert.equal(ranking.models.length, 10);
  assert.equal(ranking.models[0].id, 'vendor/model-0');
  assert.deepEqual(ranking.models[0].reasoning_policy, REASONING_POLICY);

  validateState(next);
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
  const next = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
      runRunner: async (args, options) => {
        if (args.includes('--dry-run')) {
          // Only the roster/retired models are ranked: nothing to recruit.
          return {
            stdout: JSON.stringify({
              ranking: {
                models: ['vendor/failing', 'vendor/healthy']
                  .map((id, index) => rankingEntry(id, index + 1)),
              },
            }),
            stderr: '',
          };
        }
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
  validateState(next);
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
  const next = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
      runRunner: async (args) => {
        assert.deepEqual(args, ['--dry-run']);
        return { stdout: JSON.stringify({ ranking: { models: rankingModels } }), stderr: '' };
      },
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
  validateState(next);
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
  const next = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
      runRunner: async () => ({
        stdout: JSON.stringify({ ranking: { models: rankingModels } }),
        stderr: '',
      }),
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
  validateState(next);
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
  const next = await runCycle({
    state: stateWith({ roster: [] }),
    flags: { shadow: true },
    stateDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
      runRunner: async () => {
        throw new Error('runner must not be called for an empty shadow roster');
      },
    },
  });
  assert.equal(next.roster.length, 0);
  assert.ok(logs.some((line) => line.includes('evaluate: skipped, roster is empty')));
  assert.ok(logs.some((line) => line.includes('shadow: recruit skipped')));
  validateState(next);
});

test('recruit dry-run failure skips recruit without failing the cycle', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = model({
    model_id: 'vendor/model-0',
    joined_at: new Date(NOW).toISOString(),
    days_in_league: 0,
  });
  const logs = [];
  const next = await runCycle({
    state: stateWith({
      roster: [keeper],
      // Feedback recently ran: this test exercises recruit, not revisions.
      last_feedback_at: new Date(NOW).toISOString(),
    }),
    flags: { shadow: false },
    stateDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
      runRunner: async () => {
        throw new Error('season runner exited with code 1: OpenRouter ranking request failed with HTTP 500');
      },
    },
  });
  assert.deepEqual(next.roster.map((entry) => entry.model_id), ['vendor/model-0']);
  assert.equal(next.announcements.length, 0);
  assert.equal(next.updated_at, new Date(NOW).toISOString());
  assert.ok(logs.some((line) => (
    line.includes('recruit: skipped, live ranking unavailable')
    && line.includes('HTTP 500')
  )));
  validateState(next);
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
    season_id: 'continuous-cml-test-day0',
    roster: [],
  };
  await fs.writeFile(path.join(historyDir, '2026-08-23.json'), JSON.stringify([seeded]));

  const runnerCalls = [];
  const results = Object.fromEntries(roster.map((entry) => [
    entry.model_id,
    { wins: 3, losses: 1, draws: 0, matches_played: 4 },
  ]));
  const next = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: () => {},
      runRunner: async (args, options) => {
        if (args.includes('--dry-run')) {
          return { stdout: JSON.stringify({ ranking: { models: [] } }), stderr: '' };
        }
        return fakeEvaluateRunner(rootDir, results, runnerCalls)(args, options);
      },
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
  validateState(next);
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
    if (args.includes('--dry-run')) {
      return { stdout: JSON.stringify({ ranking: { models: [] } }), stderr: '' };
    }
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
  const next = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
    rootDirectory: rootDir,
    deps: {
      nowMs: NOW,
      log: (line) => logs.push(line),
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
  validateState(next);
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
    runCycle({
      state,
      flags: { shadow: false },
      stateDirectory: stateDir,
      rootDirectory: rootDir,
      deps: {
        nowMs: NOW,
        log: () => {},
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

const DAY_ONLY_RANKING = {
  ranking: { models: [rankingEntry('vendor/model-0', 1)] },
};

function feedbackDeps(overrides = {}) {
  return {
    nowMs: NOW,
    log: () => {},
    runRunner: async () => ({ stdout: JSON.stringify(DAY_ONLY_RANKING), stderr: '' }),
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
  const next = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
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
  validateState(next);
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
  const next = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
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
  validateState(next);
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
  const next = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
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
  validateState(next);
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
  const next = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
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
  validateState(next);
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
  const next = await runCycle({
    state,
    flags: { shadow: false },
    stateDirectory: stateDir,
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
  validateState(next);
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
  const next = await runCycle({
    state,
    flags: { shadow: true },
    stateDirectory: stateDir,
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
  validateState(next);
});

// --- Revision journal idempotency (crash-window double-spend) ----------------

const JOURNAL_KEY = 'vendor__model-0-v2';

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
  const next = await runCycle({
    state: stateWith({ roster: [keeper], last_feedback_at: null }),
    flags: { shadow: false },
    stateDirectory: stateDir,
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
  validateState(next);
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
  const next = await runCycle({
    state: stateWith({ roster: [keeper], last_feedback_at: null }),
    flags: { shadow: false },
    stateDirectory: stateDir,
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
  validateState(next);
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
  const next = await runCycle({
    state: stateWith({ roster: [keeper], last_feedback_at: null }),
    flags: { shadow: false },
    stateDirectory: stateDir,
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
  validateState(next);
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
  const next = await runCycle({
    state: stateWith({ roster: [keeper], last_feedback_at: null }),
    flags: { shadow: false },
    stateDirectory: stateDir,
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
  validateState(next);
});

test('an untagged post-codegen failure records interrupted, not codegen_failed', async () => {
  const { stateDir, rootDir } = await tempDirs();
  const keeper = keeperState();
  await makeFighter(stateDir, keeper.model_id);
  const next = await runCycle({
    state: stateWith({ roster: [keeper], last_feedback_at: null }),
    flags: { shadow: false },
    stateDirectory: stateDir,
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
  validateState(next);
});
