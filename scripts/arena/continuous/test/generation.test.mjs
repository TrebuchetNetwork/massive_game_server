import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { validateGenerationCheckpoint } from '../../run_top10_season.mjs';
import {
  compileFighterSource,
  entrantFromChallenger,
  generateFighter,
  materializeSeasonFighter,
  rankingModelFromMeta,
  readFighterRecord,
  reviseFighter,
  writeFighterRecord,
} from '../generation.mjs';

const sha256 = (value) => createHash('sha256').update(value).digest('hex');
const sha = (ch) => ch.repeat(64);

const REASONING_POLICY = Object.freeze({
  version: 'capability_minimum_v1',
  mode: 'disabled',
  effort: null,
  exclude: true,
});

const PROMPT_TEXT = 'arena fighter prompt v1 (test fixture)';
const PROMPT_SHA256 = sha256(PROMPT_TEXT);
const SOURCE = 'fn bot_tick_v2() { /* test fighter */ }\n';

const CODE_STATUS_PAYLOAD = {
  provider_configured: true,
  collaboration_abi_version: 'bot_tick_v2/1',
  prompt_version: 'arena-v1',
  prompt_sha256: PROMPT_SHA256,
  source_limit_bytes: 50 * 1024,
  max_tokens: 4096,
  provider_sort_policy: 'throughput',
  temperature_policy: 'provider_default',
  reasoning_policy_version: 'capability_minimum_v1',
  provider_require_parameters: true,
  reasoning_exclude: true,
  response_transport_policy: 'sse_v1',
  simulator_rules_version: 'sim-2026-08',
  provider_timeout_secs: 120,
};

function challenger() {
  return {
    provider_rank: 1,
    id: 'vendor/model-x',
    canonical_slug: 'vendor/model-x-20260801',
    name: 'Vendor: Model X',
    reasoning_policy: { ...REASONING_POLICY },
  };
}

function entrant() {
  return entrantFromChallenger(challenger(), 'continuous-cml-test-day0', '2026-08-23T00:00:00.000Z');
}

function generationResponse(providerModel) {
  return {
    simulated: false,
    source_code: SOURCE,
    prompt_version: 'arena-v1',
    prompt_sha256: PROMPT_SHA256,
    prompt_text: PROMPT_TEXT,
    model: providerModel,
    max_completion_tokens: 4096,
    provider_sort_policy: 'throughput',
    temperature_policy: 'provider_default',
    reasoning_policy_version: 'capability_minimum_v1',
    provider_require_parameters: true,
    reasoning_mode: 'disabled',
    reasoning_effort: null,
    reasoning_exclude: true,
    response_transport_policy: 'sse_v1',
    finish_reason: 'stop',
    resolved_model: 'vendor/model-x-20260801',
    provider_name: 'test-provider',
    provider_response_id: 'gen-test-1',
    usage: { prompt_tokens: 100, completion_tokens: 200, total_tokens: 300, cost: 0.001 },
  };
}

test('generateFighter follows the runner contract and builds a valid compiled checkpoint', async () => {
  const dayEntrant = entrant();
  const requests = [];
  const apiClient = async (request) => {
    requests.push(request);
    if (request.route === '/api/arena/code/status') return CODE_STATUS_PAYLOAD;
    if (request.route === '/api/arena/code/generate') {
      return generationResponse(request.body.model);
    }
    if (request.route === '/api/arena/models/register') return { registered: true };
    if (request.route === '/api/arena/code/compile') {
      return {
        model_id: request.body.model_id,
        compiled: true,
        bytes_written: 1234,
        wasm_sha256: sha('9'),
      };
    }
    throw new Error(`unexpected route ${request.route}`);
  };

  const { checkpoint, source, codeStatus } = await generateFighter({
    apiBase: 'http://127.0.0.1:9',
    adminToken: 'test-token',
    entrant: dayEntrant,
    apiClient,
    now: () => '2026-08-23T00:00:00.000Z',
  });

  assert.deepEqual(requests.map((request) => `${request.method || 'GET'} ${request.route}`), [
    'GET /api/arena/code/status',
    'POST /api/arena/code/generate',
    'POST /api/arena/models/register',
    'POST /api/arena/code/compile',
  ]);
  assert.deepEqual(requests[1].body, {
    model: 'vendor/model-x',
    reasoning_mode: 'disabled',
    reasoning_effort: null,
  });
  assert.equal(requests[2].body.model_id, dayEntrant.model_id);
  assert.equal(requests[3].body.source_code, SOURCE);
  assert.equal(requests[3].body.overwrite, true);
  assert.equal(source, SOURCE);

  // The checkpoint must satisfy the runner's own strict validation, i.e. the
  // evaluate-only rehydration path accepts it as-is.
  validateGenerationCheckpoint(checkpoint, dayEntrant, source, codeStatus);
  assert.equal(checkpoint.stage, 'compiled');
  assert.equal(checkpoint.wasm_sha256, sha('9'));
  assert.equal(checkpoint.source_sha256, sha256(SOURCE));
  assert.equal(checkpoint.prompt_sha256, PROMPT_SHA256);
});

test('materializeSeasonFighter re-points identity fields at the day entrant', async () => {
  const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), 'cml-gen-state-'));
  const seasonDir = await fs.mkdtemp(path.join(os.tmpdir(), 'cml-gen-season-'));
  const { checkpoint, source } = await generateFighter({
    apiBase: 'http://127.0.0.1:9',
    adminToken: 'test-token',
    entrant: entrant(),
    apiClient: async (request) => {
      if (request.route === '/api/arena/code/status') return CODE_STATUS_PAYLOAD;
      if (request.route === '/api/arena/code/generate') {
        return generationResponse(request.body.model);
      }
      if (request.route === '/api/arena/models/register') return { registered: true };
      if (request.route === '/api/arena/code/compile') {
        return {
          model_id: request.body.model_id,
          compiled: true,
          bytes_written: 1234,
          wasm_sha256: sha('9'),
        };
      }
      throw new Error(`unexpected route ${request.route}`);
    },
    now: () => '2026-08-23T00:00:00.000Z',
  });
  const meta = {
    model_id: 'vendor/model-x',
    slug: 'vendor/model-x-20260801',
    model_name: 'Vendor: Model X',
    reasoning_policy: { ...REASONING_POLICY },
  };
  await writeFighterRecord(stateDir, 'vendor/model-x', { checkpoint, source, meta });
  const fighter = await readFighterRecord(stateDir, 'vendor/model-x');
  assert.equal(fighter.source, SOURCE);
  assert.deepEqual(fighter.meta, meta);

  // A later day derives a different arena model id from its own season id.
  const dayFiveEntrant = entrantFromChallenger(
    challenger(),
    'continuous-cml-test-day5',
    '2026-08-28T00:00:00.000Z',
  );
  assert.notEqual(dayFiveEntrant.model_id, entrant().model_id);
  await materializeSeasonFighter({
    seasonDirectory: seasonDir,
    entrant: dayFiveEntrant,
    fighter,
  });

  const materialized = JSON.parse(await fs.readFile(
    path.join(seasonDir, 'generations', `${dayFiveEntrant.model_id}.json`),
    'utf8',
  ));
  const materializedSource = await fs.readFile(
    path.join(seasonDir, 'sources', `${dayFiveEntrant.model_id}.rs`),
    'utf8',
  );
  assert.equal(materialized.model_id, dayFiveEntrant.model_id);
  assert.equal(materializedSource, SOURCE);
  // The generation proof is untouched, so rehydration validation still passes.
  const { normalizeCodeStatus } = await import('../../run_top10_season.mjs');
  validateGenerationCheckpoint(
    materialized,
    dayFiveEntrant,
    materializedSource,
    normalizeCodeStatus(CODE_STATUS_PAYLOAD),
  );
});

test('rankingModelFromMeta produces the runner ranking shape', () => {
  const model = rankingModelFromMeta({
    model_id: 'vendor/model-x',
    slug: 'vendor/model-x-20260801',
    model_name: 'Vendor: Model X',
    reasoning_policy: { ...REASONING_POLICY },
    pricing: { prompt: '0.1' },
    context_length: 1000,
    created: 1780000000,
  }, 3);
  assert.deepEqual(model, {
    provider_rank: 3,
    id: 'vendor/model-x',
    canonical_slug: 'vendor/model-x-20260801',
    name: 'Vendor: Model X',
    pricing: { prompt: '0.1' },
    context_length: 1000,
    created: 1780000000,
    reasoning_policy: REASONING_POLICY,
  });
});

// --- reviseFighter: runner revision contract --------------------------------

const REVISION_PROMPT_TEXT = 'arena fighter revision prompt v1 (test fixture)';
const REVISION_PROMPT_SHA256 = sha256(REVISION_PROMPT_TEXT);
const REVISED_SOURCE = 'fn bot_tick_v2() { /* revised fighter */ }\n';

const CODE_STATUS_WITH_REVISION = {
  ...CODE_STATUS_PAYLOAD,
  revision_prompt_version: 'arena-rev-v1',
  revision_prompt_sha256: REVISION_PROMPT_SHA256,
};

function revisionResponse(providerModel, { promptSha256 = REVISION_PROMPT_SHA256 } = {}) {
  return {
    ...generationResponse(providerModel),
    source_code: REVISED_SOURCE,
    prompt_version: 'arena-rev-v1',
    prompt_sha256: promptSha256,
    prompt_text: REVISION_PROMPT_TEXT,
    provider_response_id: 'rev-test-1',
    usage: { prompt_tokens: 150, completion_tokens: 250, total_tokens: 400, cost: 0.002 },
  };
}

function reviseApiClient(requests, { failRevise = null, failCompile = null, revisePayload } = {}) {
  return async (request) => {
    requests.push(request);
    if (request.route === '/api/arena/code/status') return CODE_STATUS_WITH_REVISION;
    if (request.route === '/api/arena/code/generate') {
      return generationResponse(request.body.model);
    }
    if (request.route === '/api/arena/code/revise') {
      if (failRevise) throw failRevise;
      return revisePayload ?? revisionResponse(request.body.model);
    }
    if (request.route === '/api/arena/models/register') return { registered: true };
    if (request.route === '/api/arena/code/compile') {
      if (failCompile) return failCompile;
      return {
        model_id: request.body.model_id,
        compiled: true,
        bytes_written: 2345,
        wasm_sha256: sha('8'),
      };
    }
    throw new Error(`unexpected route ${request.route}`);
  };
}

async function generatePrevious(apiClient) {
  const dayEntrant = entrant();
  const { checkpoint, source, codeStatus } = await generateFighter({
    apiBase: 'http://127.0.0.1:9',
    adminToken: 'test-token',
    entrant: dayEntrant,
    apiClient,
    now: () => '2026-08-23T00:00:00.000Z',
  });
  return { entrant: dayEntrant, previousCheckpoint: checkpoint, previousSource: source, codeStatus };
}

test('reviseFighter follows the runner revision contract with lineage', async () => {
  const requests = [];
  const apiClient = reviseApiClient(requests);
  const { entrant: dayEntrant, previousCheckpoint, previousSource, codeStatus } = (
    await generatePrevious(apiClient)
  );
  requests.length = 0;

  const brief = 'Arena fighter improvement brief — test digest';
  const { checkpoint, source } = await reviseFighter({
    apiBase: 'http://127.0.0.1:9',
    adminToken: 'test-token',
    entrant: dayEntrant,
    source: previousSource,
    brief,
    previousCheckpoint,
    apiClient,
    now: () => '2026-08-24T00:00:00.000Z',
  });

  // Request body matches reviseEntrant's contract exactly.
  assert.deepEqual(requests.map((request) => `${request.method || 'GET'} ${request.route}`), [
    'GET /api/arena/code/status',
    'POST /api/arena/code/revise',
    'POST /api/arena/models/register',
    'POST /api/arena/code/compile',
  ]);
  assert.deepEqual(requests[1].body, {
    model: 'vendor/model-x',
    previous_source: previousSource,
    stats_digest: brief,
    reasoning_mode: 'disabled',
    reasoning_effort: null,
  });
  assert.equal(requests[3].body.source_code, REVISED_SOURCE);

  // Lineage fields on the built checkpoint.
  assert.equal(source, REVISED_SOURCE);
  assert.equal(checkpoint.revision_of, previousCheckpoint.source_sha256);
  assert.equal(checkpoint.stats_digest_sha256, sha256(brief));
  assert.equal(checkpoint.prompt_version, 'arena-rev-v1');
  assert.equal(checkpoint.prompt_sha256, REVISION_PROMPT_SHA256);
  assert.equal(checkpoint.source_sha256, sha256(REVISED_SOURCE));
  assert.equal(checkpoint.wasm_sha256, sha('8'));

  // The runner's own strict self-check accepts the revised checkpoint.
  const { normalizeCodeStatus } = await import('../../run_top10_season.mjs');
  validateGenerationCheckpoint(
    checkpoint,
    dayEntrant,
    source,
    normalizeCodeStatus(CODE_STATUS_WITH_REVISION),
  );
  assert.ok(codeStatus.revision_prompt_version);
});

test('reviseFighter tags codegen-phase failures (transport and validation)', async () => {
  // Transport failure on the revise route.
  const transport = await reviseFighter({
    apiBase: 'http://127.0.0.1:9',
    adminToken: 'test-token',
    entrant: entrant(),
    source: SOURCE,
    brief: 'brief',
    previousCheckpoint: (await generatePrevious(reviseApiClient([]))).previousCheckpoint,
    apiClient: reviseApiClient([], { failRevise: new Error('HTTP 500') }),
  }).catch((error) => error);
  assert.equal(transport.phase, 'codegen');

  // Response that fails validateRevisionResponse (wrong prompt hash).
  const requests = [];
  const previous = await generatePrevious(reviseApiClient([]));
  const invalid = await reviseFighter({
    apiBase: 'http://127.0.0.1:9',
    adminToken: 'test-token',
    entrant: previous.entrant,
    source: previous.previousSource,
    brief: 'brief',
    previousCheckpoint: previous.previousCheckpoint,
    apiClient: reviseApiClient(requests, {
      revisePayload: revisionResponse('vendor/model-x', { promptSha256: sha('0') }),
    }),
  }).catch((error) => error);
  assert.equal(invalid.phase, 'codegen');
  // The compile route was never reached.
  assert.ok(!requests.some((request) => request.route === '/api/arena/code/compile'));
});

test('reviseFighter tags compile-phase failures', async () => {
  const requests = [];
  const previous = await generatePrevious(reviseApiClient([]));
  const failure = await reviseFighter({
    apiBase: 'http://127.0.0.1:9',
    adminToken: 'test-token',
    entrant: previous.entrant,
    source: previous.previousSource,
    brief: 'brief',
    previousCheckpoint: previous.previousCheckpoint,
    apiClient: reviseApiClient(requests, {
      failCompile: {
        model_id: previous.entrant.model_id,
        compiled: false,
        compiler_stderr: 'error[E0308]: mismatched types',
      },
    }),
  }).catch((error) => error);
  assert.equal(failure.phase, 'compile');
  assert.match(failure.message, /fighter compilation failed/);
});

// --- Track compile-attempt policies ------------------------------------------

test('compileFighterSource honors the L0 policy: one attempt, no recovery', async () => {
  let compileCalls = 0;
  const apiClient = async (request) => {
    if (request.route === '/api/arena/models/register') return { registered: true };
    if (request.route === '/api/arena/code/compile') {
      compileCalls += 1;
      return {
        model_id: request.body.model_id,
        compiled: false,
        compiler_stderr: 'error[E0308]: mismatched types',
      };
    }
    throw new Error(`unexpected route ${request.route}`);
  };
  const failure = await compileFighterSource({
    apiBase: 'http://127.0.0.1:9',
    adminToken: 'test-token',
    entrant: entrant(),
    source: SOURCE,
    attempts: 1,
    apiClient,
  }).catch((error) => error);
  assert.equal(compileCalls, 1);
  assert.match(failure.message, /fighter compilation failed: error\[E0308\]/);
  assert.equal(failure.compileAttempts, 1);
});

test('compileFighterSource honors the L1 policy: up to 3 attempts', async () => {
  let compileCalls = 0;
  const apiClient = async (request) => {
    if (request.route === '/api/arena/models/register') return { registered: true };
    if (request.route === '/api/arena/code/compile') {
      compileCalls += 1;
      if (compileCalls < 3) {
        return {
          model_id: request.body.model_id,
          compiled: false,
          compiler_stderr: 'error: linker transiently unavailable',
        };
      }
      return {
        model_id: request.body.model_id,
        compiled: true,
        bytes_written: 999,
        wasm_sha256: sha('7'),
      };
    }
    throw new Error(`unexpected route ${request.route}`);
  };
  const wasm = await compileFighterSource({
    apiBase: 'http://127.0.0.1:9',
    adminToken: 'test-token',
    entrant: entrant(),
    source: SOURCE,
    attempts: 3,
    apiClient,
  });
  assert.equal(compileCalls, 3);
  assert.equal(wasm.wasmSha256, sha('7'));

  // Deterministic failure exhausts all 3 attempts and reports the count.
  let failedCalls = 0;
  const alwaysFail = await compileFighterSource({
    apiBase: 'http://127.0.0.1:9',
    adminToken: 'test-token',
    entrant: entrant(),
    source: SOURCE,
    attempts: 3,
    apiClient: async (request) => {
      if (request.route === '/api/arena/models/register') return { registered: true };
      failedCalls += 1;
      return { model_id: request.body.model_id, compiled: false, compiler_stderr: 'error' };
    },
  }).catch((error) => error);
  assert.equal(failedCalls, 3);
  assert.equal(alwaysFail.compileAttempts, 3);
});

test('fetchEligibleRanking skips models with unusable reasoning metadata', async () => {
  const { fetchEligibleRanking } = await import('../generation.mjs');
  const payload = {
    data: [
      { id: 'vendor/good-1', canonical_slug: 'vendor/good-1-20260101', name: 'Good 1', supported_parameters: [] },
      {
        id: 'vendor/broken-reasoning',
        name: 'Broken',
        supported_parameters: ['reasoning'],
        reasoning: { mandatory: true }, // supported_efforts missing entirely
      },
      { id: 'vendor/good-2', name: 'Good 2', supported_parameters: ['reasoning'], reasoning: {} },
      { id: 'vendor/non-text', name: 'NoText', architecture: { output_modalities: ['image'] } },
    ],
  };
  const skipped = [];
  const ranking = await fetchEligibleRanking({
    topModels: 10,
    fetchImpl: async () => ({ ok: true, json: async () => payload }),
    log: (line) => skipped.push(line),
  });
  assert.deepEqual(ranking.models.map((model) => model.id), ['vendor/good-1', 'vendor/good-2']);
  assert.deepEqual(ranking.models.map((model) => model.provider_rank), [1, 2]);
  assert.equal(ranking.models[0].reasoning_policy.mode, 'unsupported');
  assert.equal(ranking.models[1].reasoning_policy.mode, 'disabled');
  assert.ok(skipped.some((line) => line.includes('vendor/broken-reasoning')));
  assert.ok(!skipped.some((line) => line.includes('vendor/non-text')));
});
