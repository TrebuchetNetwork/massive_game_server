import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  assertCodeStatusUnchanged,
  buildRevisionStatsDigest,
  generateEntrant,
  loadRanking,
  normalizeCodeStatus,
  parseArgs,
  reasoningPolicyFromModelMetadata,
  readArtifactBinding,
  rehydrateEntrant,
  rehydrateLegacyGeneration,
  validateGeneratedCheckpoint,
  validateGeneratedResponse,
  validateGeneration,
  validateGenerationCheckpoint,
  validateGenerationUsage,
  validateLegacyCompiledCheckpoint,
  validateReasoningPolicy,
  verifyPinnedArtifact,
} from './run_top10_season.mjs';

const sha256 = (value) => createHash('sha256').update(value).digest('hex');
const promptText = 'frozen arena prompt';
const source = '#[no_mangle]\npub extern "C" fn bot_tick_v2() {}';
const wasmDigest = 'd'.repeat(64);

function rawCodeStatus(overrides = {}) {
  return {
    provider_configured: true,
    prompt_version: 'arena-rust-v3.1.0',
    prompt_sha256: sha256(promptText),
    source_limit_bytes: 50 * 1024,
    max_tokens: 2_049,
    provider_sort_policy: 'throughput',
    provider_require_parameters: true,
    temperature_policy: 'provider_default',
    reasoning_policy_version: 'capability_minimum_v1',
    reasoning_exclude: true,
    response_transport_policy: 'sse_v1',
    provider_timeout_secs: 120,
    collaboration_abi_version: 'bot_tick_v2/1',
    simulator_rules_version: 'arena-sim-v2',
    ...overrides,
  };
}

const entrant = Object.freeze({
  provider_rank: 1,
  model_id: 'entrant-one',
  model_name: 'Entrant One',
  provider_model: 'provider/model-one',
  canonical_slug: 'provider/model-one',
  reasoning_policy: Object.freeze({
    version: 'capability_minimum_v1',
    mode: 'disabled',
    effort: null,
    exclude: true,
  }),
});

function generationResponse(overrides = {}) {
  const codeStatus = normalizeCodeStatus(rawCodeStatus());
  return {
    generated: {
      model: entrant.provider_model,
      source_code: source,
      simulated: false,
      prompt_version: codeStatus.prompt_version,
      prompt_sha256: codeStatus.prompt_sha256,
      prompt_text: promptText,
      max_completion_tokens: codeStatus.max_tokens,
      provider_sort_policy: codeStatus.provider_sort_policy,
      provider_require_parameters: codeStatus.provider_require_parameters,
      temperature_policy: codeStatus.temperature_policy,
      reasoning_policy_version: codeStatus.reasoning_policy_version,
      reasoning_mode: entrant.reasoning_policy.mode,
      reasoning_effort: entrant.reasoning_policy.effort,
      reasoning_exclude: codeStatus.reasoning_exclude,
      response_transport_policy: codeStatus.response_transport_policy,
      finish_reason: 'stop',
      resolved_model: 'provider/model-one-20260701',
      provider_name: 'Provider One',
      provider_response_id: 'generation-123',
      usage: {
        prompt_tokens: 100,
        completion_tokens: 25,
        total_tokens: 125,
        cost: 0.0125,
      },
      ...overrides,
    },
    compile: {
      model_id: entrant.model_id,
      compiled: true,
      bytes_written: 512,
      wasm_sha256: wasmDigest,
    },
  };
}

function compiledCheckpoint(overrides = {}) {
  const codeStatus = normalizeCodeStatus(rawCodeStatus());
  const response = generationResponse();
  const providerGenerated = { ...response.generated };
  delete providerGenerated.source_code;
  const providerResponse = { generated: providerGenerated };
  const sourceSha256 = sha256(source);
  return {
    schema_version: 2,
    stage: 'compiled',
    provider_rank: entrant.provider_rank,
    model_id: entrant.model_id,
    model_name: entrant.model_name,
    provider_model: entrant.provider_model,
    canonical_slug: entrant.canonical_slug,
    generated_at: '2026-07-24T00:00:00.000Z',
    prompt_version: codeStatus.prompt_version,
    prompt_sha256: codeStatus.prompt_sha256,
    max_completion_tokens: codeStatus.max_tokens,
    provider_sort_policy: codeStatus.provider_sort_policy,
    temperature_policy: codeStatus.temperature_policy,
    reasoning_policy: { ...entrant.reasoning_policy },
    reasoning_policy_version: codeStatus.reasoning_policy_version,
    provider_require_parameters: codeStatus.provider_require_parameters,
    reasoning_mode: entrant.reasoning_policy.mode,
    reasoning_effort: entrant.reasoning_policy.effort,
    reasoning_exclude: codeStatus.reasoning_exclude,
    response_transport_policy: codeStatus.response_transport_policy,
    collaboration_abi_version: codeStatus.collaboration_abi_version,
    simulator_rules_version: codeStatus.simulator_rules_version,
    source_limit_bytes: codeStatus.source_limit_bytes,
    compiled: true,
    simulated: false,
    finish_reason: providerGenerated.finish_reason,
    resolved_model: providerGenerated.resolved_model,
    provider_name: providerGenerated.provider_name,
    provider_response_id: providerGenerated.provider_response_id,
    source_bytes: Buffer.byteLength(source, 'utf8'),
    source_sha256: sourceSha256,
    wasm_bytes: 512,
    wasm_sha256: wasmDigest,
    generation_attempts: 1,
    compile_attempts: 1,
    compiled_at: '2026-07-24T00:00:01.000Z',
    last_compile_attempt_at: '2026-07-24T00:00:01.000Z',
    last_compile_error_sha256: null,
    usage: { ...providerGenerated.usage },
    generation_archive_sha256: sha256(JSON.stringify({
      provider_response: providerResponse,
      source_sha256: sourceSha256,
    })),
    provider_response: providerResponse,
    ...overrides,
  };
}

function legacyCompiledCheckpoint(overrides = {}) {
  const checkpoint = compiledCheckpoint(overrides);
  delete checkpoint.wasm_sha256;
  return checkpoint;
}

test('code status keeps the exact ABI label and enforces the Rust token floor', () => {
  const frozen = normalizeCodeStatus(rawCodeStatus());
  assert.equal(frozen.collaboration_abi_version, 'bot_tick_v2/1');
  assert.equal(frozen.max_tokens, 2_049);

  assert.throws(
    () => normalizeCodeStatus(rawCodeStatus({ max_tokens: 2_048 })),
    /valid max_tokens/,
  );
  assert.throws(
    () => normalizeCodeStatus(rawCodeStatus({ collaboration_abi_version: 'bot_tick_v2/2' })),
    /must be bot_tick_v2\/1/,
  );
  assert.throws(
    () => normalizeCodeStatus(rawCodeStatus({ collaboration_abi_version: 2 })),
    /must be bot_tick_v2\/1/,
  );
  assert.throws(
    () => assertCodeStatusUnchanged(frozen, {
      ...frozen,
      collaboration_abi_version: 'bot_tick_v2/2',
    }),
    /collaboration_abi_version/,
  );
});

test('rehydrate-only requires frozen local inputs and cannot combine with generation or evaluation', () => {
  assert.equal(parseArgs([
    '--rehydrate-only',
    '--ranking-file', 'ranking.json',
    '--season-id', 'weekly-test',
  ]).rehydrateOnly, true);
  assert.throws(
    () => parseArgs(['--rehydrate-only', '--season-id', 'weekly-test']),
    /requires --ranking-file and --season-id/,
  );
  assert.throws(
    () => parseArgs([
      '--rehydrate-only',
      '--evaluate-only',
      '--ranking-file', 'ranking.json',
      '--season-id', 'weekly-test',
    ]),
    /cannot be combined/,
  );
});

test('revise-only parses and cannot combine with other runner modes', () => {
  const options = parseArgs([
    '--revise-only',
    '--ranking-file', 'ranking.json',
    '--season-id', 'weekly-test',
    '--stats-state', 'state.json',
  ]);
  assert.equal(options.reviseOnly, true);
  assert.equal(options.statsState, 'state.json');
  assert.throws(
    () => parseArgs([
      '--revise-only', '--evaluate-only',
      '--ranking-file', 'r', '--season-id', 's', '--stats-state', 'x',
    ]),
    /cannot be combined/,
  );
  assert.throws(
    () => parseArgs(['--revise-only', '--season-id', 's', '--stats-state', 'x']),
    /requires --ranking-file, --season-id and --stats-state/,
  );
  assert.throws(
    () => parseArgs(['--revise-only', '--ranking-file', 'r', '--season-id', 's']),
    /requires --ranking-file, --season-id and --stats-state/,
  );
});

test('revision checkpoint validates against the revision contract', () => {
  const revisionPromptText = 'SYSTEM\nrevision system\n\nUSER\nrevision prefix';
  const revisionPromptSha256 = sha256(revisionPromptText);
  const revisionStatus = normalizeCodeStatus(rawCodeStatus({
    revision_prompt_version: 'arena-rust-revision-v1.0.0',
    revision_prompt_sha256: revisionPromptSha256,
  }));
  const revised = compiledCheckpoint({
    prompt_version: 'arena-rust-revision-v1.0.0',
    prompt_sha256: revisionPromptSha256,
  });
  // a real revised checkpoint archives the revision-flavored provider response
  revised.provider_response.generated = {
    ...revised.provider_response.generated,
    prompt_version: 'arena-rust-revision-v1.0.0',
    prompt_sha256: revisionPromptSha256,
    prompt_text: revisionPromptText,
  };
  revised.generation_archive_sha256 = sha256(JSON.stringify({
    provider_response: revised.provider_response,
    source_sha256: revised.source_sha256,
  }));
  // revision pair accepted when the server advertises the revision contract
  validateGenerationCheckpoint(revised, entrant, source, revisionStatus);
  // generation pair still accepted alongside it
  validateGenerationCheckpoint(compiledCheckpoint(), entrant, source, revisionStatus);
  // unknown prompt contract rejected
  assert.throws(
    () => validateGenerationCheckpoint(
      compiledCheckpoint({ prompt_sha256: 'b'.repeat(64) }),
      entrant,
      source,
      revisionStatus,
    ),
    /stale or unverified/,
  );
  // revision pair rejected when the server never advertised a revision contract
  assert.throws(
    () => validateGenerationCheckpoint(
      revised,
      entrant,
      source,
      normalizeCodeStatus(rawCodeStatus()),
    ),
    /stale or unverified/,
  );
  // malformed revision contract in the status response is rejected
  assert.throws(
    () => normalizeCodeStatus(rawCodeStatus({ revision_prompt_sha256: 'nope' })),
    /invalid revision prompt contract/,
  );
  assert.throws(
    () => normalizeCodeStatus(rawCodeStatus({ revision_prompt_version: 'bad version!' })),
    /invalid revision prompt contract/,
  );
});

test('stats digest is bounded, deterministic and model-scoped', () => {
  const seasonSnapshot = {
    season_id: 'weekly-test',
    roster: [
      {
        model_id: 'a', model_name: 'A', personal_rating: 50, team_rating: 40,
        collaboration_rating: 30, world_rating: 20, strategy_rating: 44, rank: 2,
        wins: 3, losses: 5, draws: 1, matches_played: 9,
      },
      {
        model_id: 'b', model_name: 'B', personal_rating: 60, team_rating: 55,
        collaboration_rating: 50, world_rating: 45, strategy_rating: 61, rank: 1,
        wins: 7, losses: 2, draws: 0, matches_played: 9,
      },
    ],
  };
  const supervisorState = {
    epochs: Array.from({ length: 12 }, (_, index) => ({
      standings: [
        { model_id: 'a', epoch_rank: (index % 3) + 1 },
        { model_id: 'b', epoch_rank: ((index + 1) % 3) + 1 },
      ],
    })),
  };
  const digest = buildRevisionStatsDigest({ seasonSnapshot, supervisorState, modelId: 'a' });
  const parsed = JSON.parse(digest);
  assert.equal(parsed.model_id, 'a');
  assert.equal(parsed.season_id, 'weekly-test');
  assert.equal(parsed.epochs_completed, 12);
  assert.equal(parsed.current.strategy_rating, 44);
  assert.equal(parsed.current.rank, 2);
  assert.equal(parsed.last_epoch_ranks.length, 10, 'only the last 10 epochs');
  assert.deepEqual(parsed.last_epoch_ranks, [3, 1, 2, 3, 1, 2, 3, 1, 2, 3]);
  assert.equal(parsed.top_opponents[0].model_id, 'b');
  assert.equal(parsed.top_opponents[0].strategy_rating, 61);
  assert.ok(Buffer.byteLength(digest, 'utf8') <= 4096);
  assert.equal(
    digest,
    buildRevisionStatsDigest({ seasonSnapshot, supervisorState, modelId: 'a' }),
    'deterministic',
  );
  assert.throws(
    () => buildRevisionStatsDigest({ seasonSnapshot, supervisorState, modelId: 'missing' }),
    /no roster entry/,
  );
});

test('generation validation requires frozen provenance and terminal provider metadata', () => {
  const codeStatus = normalizeCodeStatus(rawCodeStatus());
  assert.equal(
    validateGeneratedResponse(generationResponse().generated, codeStatus, entrant).source,
    source,
  );
  const verified = validateGeneration(generationResponse(), codeStatus, entrant);
  assert.equal(verified.finishReason, 'stop');
  assert.equal(verified.providerResponseId, 'generation-123');
  assert.equal(verified.wasmSha256, wasmDigest);

  assert.throws(
    () => validateGeneration(
      generationResponse({ prompt_version: 'arena-rust-future' }),
      codeStatus,
      entrant,
    ),
    /prompt version or hash differs/,
  );
  assert.throws(
    () => validateGeneration(
      generationResponse({ model: 'provider/other' }),
      codeStatus,
      entrant,
    ),
    /model differs/,
  );
  assert.throws(
    () => validateGeneration(
      generationResponse({ finish_reason: 'length' }),
      codeStatus,
      entrant,
    ),
    /finish with stop/,
  );
  assert.throws(
    () => validateGeneration({
      ...generationResponse(),
      compile: {
        ...generationResponse().compile,
        model_id: 'different-entrant',
      },
    }, codeStatus, entrant),
    /compile response model_id differs/,
  );
  assert.throws(
    () => validateGeneration({
      ...generationResponse(),
      compile: { ...generationResponse().compile, wasm_sha256: wasmDigest.toUpperCase() },
    }, codeStatus, entrant),
    /compiled WASM digest is invalid/,
  );
  for (const field of ['resolved_model', 'provider_name', 'provider_response_id']) {
    assert.throws(
      () => validateGeneration(generationResponse({ [field]: '  ' }), codeStatus, entrant),
      new RegExp(`missing ${field}`),
    );
  }
});

test('checkpoint identity exactly matches frozen rank, name, and nullable canonical slug', () => {
  const codeStatus = normalizeCodeStatus(rawCodeStatus());
  for (const [field, value] of [
    ['provider_rank', 2],
    ['model_name', 'Different name'],
    ['canonical_slug', 'provider/different'],
  ]) {
    assert.throws(
      () => validateGenerationCheckpoint(
        compiledCheckpoint({ [field]: value }),
        entrant,
        source,
        codeStatus,
      ),
      /stale or unverified/,
    );
  }

  const nullSlugEntrant = { ...entrant, canonical_slug: null };
  const nullSlugCheckpoint = compiledCheckpoint({ canonical_slug: null });
  validateGenerationCheckpoint(nullSlugCheckpoint, nullSlugEntrant, source, codeStatus);
  delete nullSlugCheckpoint.canonical_slug;
  assert.throws(
    () => validateGenerationCheckpoint(nullSlugCheckpoint, nullSlugEntrant, source, codeStatus),
    /stale or unverified/,
    'a missing property must not compare equal to an explicit null slug',
  );
});

test('ambiguous generation request failures are never retried automatically', async (t) => {
  const temporaryDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-ambiguous-'));
  t.after(() => fs.rm(temporaryDirectory, { recursive: true, force: true }));
  let generationRequests = 0;
  const context = {
    apiBase: 'http://arena.invalid',
    adminToken: 'test-token',
    codeStatus: normalizeCodeStatus(rawCodeStatus()),
    apiClient: async (request) => {
      assert.equal(request.route, '/api/arena/code/generate');
      generationRequests += 1;
      throw new Error('transport outcome unknown');
    },
  };
  await assert.rejects(
    generateEntrant(context, entrant, {
      generations: path.join(temporaryDirectory, 'generations'),
      sources: path.join(temporaryDirectory, 'sources'),
    }, 3, false),
    /ambiguous billing outcome; refusing automatic retry/,
  );
  assert.equal(generationRequests, 1);
});

test('validated generation is checkpointed before compile and resumes without a paid request', async (t) => {
  const temporaryDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-two-stage-'));
  t.after(() => fs.rm(temporaryDirectory, { recursive: true, force: true }));
  const directories = {
    generations: path.join(temporaryDirectory, 'generations'),
    sources: path.join(temporaryDirectory, 'sources'),
  };
  const codeStatus = normalizeCodeStatus(rawCodeStatus());
  const firstRoutes = [];
  const firstContext = {
    apiBase: 'http://arena.invalid',
    adminToken: 'test-token',
    codeStatus,
    apiClient: async (request) => {
      firstRoutes.push(request.route);
      if (request.route === '/api/arena/code/generate') {
        return generationResponse().generated;
      }
      if (request.route === '/api/arena/models/register') return {};
      if (request.route === '/api/arena/code/compile') {
        return {
          model_id: entrant.model_id,
          compiled: false,
          compiler_stderr: 'local compile failed',
          bytes_written: 0,
        };
      }
      throw new Error(`unexpected route ${request.route}`);
    },
  };

  await assert.rejects(
    generateEntrant(firstContext, entrant, directories, 3, false),
    /fighter compilation failed: local compile failed/,
  );
  assert.equal(
    firstRoutes.filter((route) => route === '/api/arena/code/generate').length,
    1,
  );

  const checkpointPath = path.join(directories.generations, `${entrant.model_id}.json`);
  const sourcePath = path.join(directories.sources, `${entrant.model_id}.rs`);
  const pendingCheckpoint = JSON.parse(await fs.readFile(checkpointPath, 'utf8'));
  const archivedSource = await fs.readFile(sourcePath, 'utf8');
  assert.equal(pendingCheckpoint.schema_version, 2);
  assert.equal(pendingCheckpoint.stage, 'generated');
  assert.equal(pendingCheckpoint.compiled, false);
  assert.equal(pendingCheckpoint.wasm_sha256, null);
  assert.equal(pendingCheckpoint.compile_attempts, 1);
  assert.equal(
    Object.prototype.hasOwnProperty.call(
      pendingCheckpoint.provider_response.generated,
      'source_code',
    ),
    false,
  );
  validateGeneratedCheckpoint(pendingCheckpoint, entrant, archivedSource, codeStatus);

  const resumeRoutes = [];
  const resumed = await generateEntrant({
    ...firstContext,
    apiClient: async (request) => {
      resumeRoutes.push(request.route);
      if (request.route === '/api/arena/models/register') return {};
      if (request.route === '/api/arena/code/compile') {
        return {
          model_id: entrant.model_id,
          compiled: true,
          bytes_written: 768,
          wasm_sha256: wasmDigest,
        };
      }
      throw new Error(`unexpected paid route ${request.route}`);
    },
  }, entrant, directories, 3, true);

  assert.deepEqual(resumeRoutes, [
    '/api/arena/models/register',
    '/api/arena/code/compile',
  ]);
  assert.equal(resumed.stage, 'compiled');
  assert.equal(resumed.compiled, true);
  assert.equal(resumed.wasm_bytes, 768);
  assert.equal(resumed.wasm_sha256, wasmDigest);
  assert.equal(resumed.compile_attempts, 2);
  validateGenerationCheckpoint(
    JSON.parse(await fs.readFile(checkpointPath, 'utf8')),
    entrant,
    archivedSource,
    codeStatus,
  );
});

test('legacy digest migration recompiles only the exact archived source through local routes', async (t) => {
  const temporaryDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-rehydrate-legacy-'));
  t.after(() => fs.rm(temporaryDirectory, { recursive: true, force: true }));
  const directories = {
    generations: path.join(temporaryDirectory, 'generations'),
    sources: path.join(temporaryDirectory, 'sources'),
  };
  await fs.mkdir(directories.generations, { recursive: true });
  await fs.mkdir(directories.sources, { recursive: true });
  const checkpointPath = path.join(directories.generations, `${entrant.model_id}.json`);
  await fs.writeFile(checkpointPath, JSON.stringify(legacyCompiledCheckpoint()));
  await fs.writeFile(path.join(directories.sources, `${entrant.model_id}.rs`), source);

  const routes = [];
  const codeStatus = normalizeCodeStatus(rawCodeStatus({ provider_configured: false }));
  const migrated = await rehydrateEntrant({
    apiBase: 'http://arena.invalid',
    adminToken: 'test-token',
    codeStatus,
    apiClient: async (request) => {
      routes.push(request.route);
      if (request.route === '/api/arena/models/register') return {};
      if (request.route === '/api/arena/code/compile') {
        assert.equal(request.body.source_code, source);
        assert.equal(request.body.verify_existing, true);
        assert.equal(request.body.overwrite, false);
        return {
          model_id: entrant.model_id,
          compiled: true,
          bytes_written: 512,
          wasm_sha256: wasmDigest,
        };
      }
      throw new Error(`provider/evaluation route must not run: ${request.route}`);
    },
  }, entrant, directories, { allowLegacyMissingDigest: true });

  assert.deepEqual(routes, [
    '/api/arena/code/compile',
  ]);
  assert.equal(migrated.wasm_sha256, wasmDigest);
  assert.equal(typeof migrated.rehydrated_at, 'string');
  assert.equal(migrated.compile_attempts, 2);
  assert.equal(migrated.last_compile_attempt_at, migrated.rehydrated_at);
  assert.equal(
    Object.prototype.hasOwnProperty.call(
      JSON.parse(await fs.readFile(checkpointPath, 'utf8')),
      'wasm_sha256',
    ),
    false,
    'legacy checkpoint remains immutable until the batch commit',
  );
});

test('legacy digest migration is explicit and rejects tampering before any local route', async (t) => {
  const temporaryDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-rehydrate-tamper-'));
  t.after(() => fs.rm(temporaryDirectory, { recursive: true, force: true }));
  const directories = {
    generations: path.join(temporaryDirectory, 'generations'),
    sources: path.join(temporaryDirectory, 'sources'),
  };
  await fs.mkdir(directories.generations, { recursive: true });
  await fs.mkdir(directories.sources, { recursive: true });
  const checkpointPath = path.join(directories.generations, `${entrant.model_id}.json`);
  const sourcePath = path.join(directories.sources, `${entrant.model_id}.rs`);
  const codeStatus = normalizeCodeStatus(rawCodeStatus());
  const routes = [];
  const context = {
    apiBase: 'http://arena.invalid',
    adminToken: 'test-token',
    codeStatus,
    apiClient: async (request) => {
      routes.push(request.route);
      throw new Error(`unexpected route ${request.route}`);
    },
  };

  await fs.writeFile(checkpointPath, JSON.stringify(legacyCompiledCheckpoint()));
  await fs.writeFile(sourcePath, source);
  await assert.rejects(
    rehydrateEntrant(context, entrant, directories),
    /compiled checkpoint stage is invalid/,
  );
  assert.deepEqual(routes, [], 'normal evaluation cannot silently enter migration mode');

  const tampered = legacyCompiledCheckpoint();
  tampered.provider_response.generated.provider_response_id = 'tampered';
  await fs.writeFile(checkpointPath, JSON.stringify(tampered));
  await assert.rejects(
    rehydrateEntrant(
      context,
      entrant,
      directories,
      { allowLegacyMissingDigest: true },
    ),
    /response archive.*differs/,
  );
  assert.deepEqual(routes, []);

  const nullDigest = legacyCompiledCheckpoint();
  nullDigest.wasm_sha256 = null;
  await fs.writeFile(checkpointPath, JSON.stringify(nullDigest));
  assert.throws(
    () => validateLegacyCompiledCheckpoint(nullDigest, entrant, source, codeStatus),
    /legacy compiled checkpoint is invalid/,
  );
  await assert.rejects(
    rehydrateEntrant(
      context,
      entrant,
      directories,
      { allowLegacyMissingDigest: true },
    ),
    /compiled checkpoint stage is invalid/,
  );
  assert.deepEqual(routes, []);
});

test('legacy digest migration refuses a non-reproducible compile before checkpoint commit', async (t) => {
  const temporaryDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-rehydrate-drift-'));
  t.after(() => fs.rm(temporaryDirectory, { recursive: true, force: true }));
  const directories = {
    generations: path.join(temporaryDirectory, 'generations'),
    sources: path.join(temporaryDirectory, 'sources'),
  };
  await fs.mkdir(directories.generations, { recursive: true });
  await fs.mkdir(directories.sources, { recursive: true });
  const checkpointPath = path.join(directories.generations, `${entrant.model_id}.json`);
  await fs.writeFile(checkpointPath, JSON.stringify(legacyCompiledCheckpoint()));
  await fs.writeFile(path.join(directories.sources, `${entrant.model_id}.rs`), source);
  const routes = [];

  await assert.rejects(
    rehydrateEntrant({
      apiBase: 'http://arena.invalid',
      adminToken: 'test-token',
      codeStatus: normalizeCodeStatus(rawCodeStatus()),
      apiClient: async (request) => {
        routes.push(request.route);
        if (request.route === '/api/arena/models/register') return {};
        if (request.route === '/api/arena/code/compile') {
          return {
            model_id: entrant.model_id,
            compiled: true,
            bytes_written: 513,
            wasm_sha256: 'f'.repeat(64),
          };
        }
        throw new Error(`unexpected route ${request.route}`);
      },
    }, entrant, directories, { allowLegacyMissingDigest: true }),
    /recompile size changed.*refusing migration/,
  );
  assert.deepEqual(routes, [
    '/api/arena/code/compile',
  ]);
  const persisted = JSON.parse(await fs.readFile(checkpointPath, 'utf8'));
  assert.equal(Object.prototype.hasOwnProperty.call(persisted, 'wasm_sha256'), false);
});

test('epoch evaluation without a binding trusts frozen compiled checkpoints without consuming compile attempts', async (t) => {
  const temporaryDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-epoch-trust-'));
  t.after(() => fs.rm(temporaryDirectory, { recursive: true, force: true }));
  const directories = {
    generations: path.join(temporaryDirectory, 'generations'),
    sources: path.join(temporaryDirectory, 'sources'),
  };
  await fs.mkdir(directories.generations, { recursive: true });
  await fs.mkdir(directories.sources, { recursive: true });
  const checkpointPath = path.join(directories.generations, `${entrant.model_id}.json`);
  // A fighter deep into a 7-day season: one more counted recompile would trip
  // the >100 metadata guard and stall every remaining epoch (the epoch-96 bug).
  // Server-side verify_existing cannot be used here: published v2 artifacts
  // embed a staging basename in the wasm name section, so a byte-identical
  // rebuild comparison can never match them.
  await fs.writeFile(
    checkpointPath,
    JSON.stringify(compiledCheckpoint({ compile_attempts: 100 })),
  );
  await fs.writeFile(path.join(directories.sources, `${entrant.model_id}.rs`), source);

  const routes = [];
  const context = {
    apiBase: 'http://arena.invalid',
    adminToken: 'test-token',
    codeStatus: normalizeCodeStatus(rawCodeStatus()),
    apiClient: async (request) => {
      routes.push(request.route);
      throw new Error(`no server round-trip is allowed: ${request.route}`);
    },
  };

  for (let epoch = 0; epoch < 3; epoch += 1) {
    const verified = await rehydrateEntrant(
      context,
      entrant,
      directories,
      { trustArchivedArtifact: true },
    );
    assert.equal(verified.compile_attempts, 100, 'trusting the archive is free');
    assert.equal(verified.wasm_sha256, wasmDigest);
    assert.equal(verified.stage, 'compiled');
  }
  assert.deepEqual(routes, [], 'frozen checkpoints need no per-epoch server call');
  const persisted = JSON.parse(await fs.readFile(checkpointPath, 'utf8'));
  assert.equal(persisted.compile_attempts, 100, 'checkpoint file stays untouched');
  assert.equal(Object.prototype.hasOwnProperty.call(persisted, 'rehydrated_at'), false);
});

test('epoch verification of an uncompiled fighter still registers and counts the first compile', async (t) => {
  const temporaryDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-epoch-first-compile-'));
  t.after(() => fs.rm(temporaryDirectory, { recursive: true, force: true }));
  const directories = {
    generations: path.join(temporaryDirectory, 'generations'),
    sources: path.join(temporaryDirectory, 'sources'),
  };
  await fs.mkdir(directories.generations, { recursive: true });
  await fs.mkdir(directories.sources, { recursive: true });
  const checkpointPath = path.join(directories.generations, `${entrant.model_id}.json`);
  await fs.writeFile(checkpointPath, JSON.stringify(compiledCheckpoint({
    stage: 'generated',
    compiled: false,
    wasm_bytes: null,
    wasm_sha256: null,
    compiled_at: null,
    compile_attempts: 1,
  })));
  await fs.writeFile(path.join(directories.sources, `${entrant.model_id}.rs`), source);

  const routes = [];
  const completed = await rehydrateEntrant({
    apiBase: 'http://arena.invalid',
    adminToken: 'test-token',
    codeStatus: normalizeCodeStatus(rawCodeStatus()),
    apiClient: async (request) => {
      routes.push(request.route);
      if (request.route === '/api/arena/models/register') return {};
      if (request.route === '/api/arena/code/compile') {
        assert.equal(request.body.overwrite, true);
        assert.equal(Object.prototype.hasOwnProperty.call(request.body, 'verify_existing'), false);
        return {
          model_id: entrant.model_id,
          compiled: true,
          bytes_written: 512,
          wasm_sha256: wasmDigest,
        };
      }
      throw new Error(`unexpected route ${request.route}`);
    },
  }, entrant, directories, { trustArchivedArtifact: true });

  assert.deepEqual(routes, [
    '/api/arena/models/register',
    '/api/arena/code/compile',
  ]);
  assert.equal(completed.stage, 'compiled');
  assert.equal(completed.compile_attempts, 2, 'a genuine first compile is still accounted');
  const persisted = JSON.parse(await fs.readFile(checkpointPath, 'utf8'));
  assert.equal(persisted.stage, 'compiled');
  assert.equal(persisted.compile_attempts, 2);
});

test('legacy batch binding is all-or-nothing on Nth failure and retry is idempotent', async (t) => {
  const seasonDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-rehydrate-batch-'));
  t.after(() => fs.rm(seasonDirectory, { recursive: true, force: true }));
  const directories = {
    root: seasonDirectory,
    generations: path.join(seasonDirectory, 'generations'),
    sources: path.join(seasonDirectory, 'sources'),
  };
  await fs.mkdir(directories.generations, { recursive: true });
  await fs.mkdir(directories.sources, { recursive: true });
  const entrants = Array.from({ length: 10 }, (_, index) => ({
    ...entrant,
    provider_rank: index + 1,
    model_id: `entrant-${index}`,
    model_name: `Entrant ${index}`,
    provider_model: `provider/model-${index}`,
    canonical_slug: index === 9 ? null : `provider/model-${index}`,
  }));
  const originalCheckpoints = new Map();
  const originalSources = new Map();
  const liveArtifacts = new Map();
  for (const candidate of entrants) {
    const checkpoint = legacyCompiledCheckpoint({
      provider_rank: candidate.provider_rank,
      model_id: candidate.model_id,
      model_name: candidate.model_name,
      provider_model: candidate.provider_model,
      canonical_slug: candidate.canonical_slug,
    });
    checkpoint.provider_response.generated.model = candidate.provider_model;
    checkpoint.generation_archive_sha256 = sha256(JSON.stringify({
      provider_response: checkpoint.provider_response,
      source_sha256: checkpoint.source_sha256,
    }));
    const checkpointBytes = `${JSON.stringify(checkpoint, null, 2)}\n`;
    const sourceBytes = Buffer.from(source);
    await fs.writeFile(
      path.join(directories.generations, `${candidate.model_id}.json`),
      checkpointBytes,
    );
    await fs.writeFile(path.join(directories.sources, `${candidate.model_id}.rs`), sourceBytes);
    originalCheckpoints.set(candidate.model_id, checkpointBytes);
    originalSources.set(candidate.model_id, sourceBytes);
    liveArtifacts.set(candidate.model_id, Buffer.from(`live-wasm-${candidate.model_id}`));
  }

  const codeStatus = normalizeCodeStatus(rawCodeStatus({ provider_configured: false }));
  const rankingSha256 = '9'.repeat(64);
  let verifyCalls = 0;
  const failingContext = {
    apiBase: 'http://arena.invalid',
    adminToken: 'test-token',
    codeStatus,
    apiClient: async (request) => {
      assert.equal(request.route, '/api/arena/code/compile');
      assert.equal(request.body.verify_existing, true);
      assert.equal(request.body.source_code, source);
      assert.equal(request.body.overwrite, false);
      verifyCalls += 1;
      if (verifyCalls === 5) {
        return {
          model_id: request.body.model_id,
          compiled: false,
          bytes_written: 0,
          compiler_stderr: 'staged bytes differ from live artifact',
        };
      }
      return {
        model_id: request.body.model_id,
        compiled: true,
        bytes_written: 512,
        wasm_sha256: sha256(liveArtifacts.get(request.body.model_id)),
      };
    },
  };
  await assert.rejects(
    rehydrateLegacyGeneration({
      context: failingContext,
      entrants,
      directories,
      seasonId: 'weekly-batch',
      rankingSha256,
      concurrency: 1,
      timestamp: () => '2026-07-25T12:00:00.000Z',
    }),
    /fighter compilation failed/,
  );
  assert.equal(verifyCalls, 5);
  await assert.rejects(fs.access(path.join(seasonDirectory, 'artifact-binding.json')));
  await assert.rejects(fs.access(path.join(seasonDirectory, 'bound-generations')));
  const journalPath = path.join(seasonDirectory, 'artifact-binding-attempts.json');
  const failedJournal = JSON.parse(await fs.readFile(journalPath, 'utf8'));
  assert.deepEqual(
    failedJournal.entrants.map((entry) => entry.attempts.length),
    [1, 1, 1, 1, 1, 0, 0, 0, 0, 0],
  );
  assert.deepEqual(
    failedJournal.entrants.slice(0, 5).map((entry) => entry.attempts[0].status),
    ['succeeded', 'succeeded', 'succeeded', 'succeeded', 'failed'],
  );
  const failedJournalBytes = await fs.readFile(journalPath);
  const identityTamper = structuredClone(failedJournal);
  identityTamper.entrants[0].model_name = 'Tampered entrant';
  await fs.writeFile(journalPath, `${JSON.stringify(identityTamper, null, 2)}\n`);
  await assert.rejects(
    rehydrateLegacyGeneration({
      context: {
        ...failingContext,
        apiClient: async () => { throw new Error('tampered journal must fail before API use'); },
      },
      entrants,
      directories,
      seasonId: 'weekly-batch',
      rankingSha256,
      concurrency: 1,
      timestamp: () => '2026-07-25T12:00:30.000Z',
    }),
    /attempt journal differs/,
  );
  await fs.writeFile(journalPath, failedJournalBytes);

  // Model a process crash after the API attempt began but before its outcome
  // could be durably recorded. The reservation remains an actual attempt.
  const crashedJournal = JSON.parse(failedJournalBytes.toString('utf8'));
  const crashedAttempt = crashedJournal.entrants[4].attempts[0];
  crashedAttempt.status = 'started';
  crashedAttempt.completed_at = null;
  crashedAttempt.error_sha256 = null;
  await fs.writeFile(journalPath, `${JSON.stringify(crashedJournal, null, 2)}\n`);
  for (const candidate of entrants) {
    assert.equal(
      await fs.readFile(
        path.join(directories.generations, `${candidate.model_id}.json`),
        'utf8',
      ),
      originalCheckpoints.get(candidate.model_id),
    );
    assert.deepEqual(
      await fs.readFile(path.join(directories.sources, `${candidate.model_id}.rs`)),
      originalSources.get(candidate.model_id),
    );
    assert.deepEqual(
      liveArtifacts.get(candidate.model_id),
      Buffer.from(`live-wasm-${candidate.model_id}`),
    );
  }

  verifyCalls = 0;
  const successfulContext = {
    ...failingContext,
    apiClient: async (request) => {
      assert.equal(request.route, '/api/arena/code/compile');
      assert.equal(request.body.verify_existing, true);
      assert.equal(request.body.source_code, source);
      verifyCalls += 1;
      return {
        model_id: request.body.model_id,
        compiled: true,
        bytes_written: 512,
        wasm_sha256: sha256(liveArtifacts.get(request.body.model_id)),
      };
    },
  };
  const bound = await rehydrateLegacyGeneration({
    context: successfulContext,
    entrants,
    directories,
    seasonId: 'weekly-batch',
    rankingSha256,
    concurrency: 2,
    timestamp: () => '2026-07-25T12:01:00.000Z',
  });
  assert.equal(verifyCalls, 10);
  assert.equal(bound.checkpoints.length, 10);
  for (const [index, checkpoint] of bound.checkpoints.entries()) {
    assert.equal(checkpoint.compile_attempts, index < 5 ? 3 : 2);
    assert.equal(checkpoint.last_compile_attempt_at, '2026-07-25T12:01:00.000Z');
    assert.equal(checkpoint.rehydrated_at, '2026-07-25T12:01:00.000Z');
  }
  const recoveredJournal = JSON.parse(await fs.readFile(journalPath, 'utf8'));
  assert.deepEqual(
    recoveredJournal.entrants[4].attempts.map((attempt) => attempt.status),
    ['interrupted', 'succeeded'],
  );
  const persisted = await readArtifactBinding({
    seasonDirectory,
    seasonId: 'weekly-batch',
    rankingSha256,
    entrants,
    codeStatus,
    required: true,
  });
  assert.equal(persisted.manifestSha256, bound.manifestSha256);

  const retried = await rehydrateLegacyGeneration({
    context: {
      ...successfulContext,
      apiClient: async () => { throw new Error('committed retry must not call the API'); },
    },
    entrants,
    directories,
    seasonId: 'weekly-batch',
    rankingSha256,
    concurrency: 2,
    timestamp: () => '2026-07-25T12:02:00.000Z',
  });
  assert.equal(retried.manifestSha256, bound.manifestSha256);
  assert.deepEqual(
    retried.checkpoints.map((checkpoint) => checkpoint.compile_attempts),
    [3, 3, 3, 3, 3, 2, 2, 2, 2, 2],
  );

  const journalBytes = await fs.readFile(journalPath);
  const tamperedJournal = JSON.parse(journalBytes.toString('utf8'));
  tamperedJournal.migration_key_sha256 = 'a'.repeat(64);
  await fs.writeFile(journalPath, JSON.stringify(tamperedJournal));
  await assert.rejects(
    readArtifactBinding({
      seasonDirectory,
      seasonId: 'weekly-batch',
      rankingSha256,
      entrants,
      codeStatus,
      required: true,
    }),
    /attempt journal hash differs/,
  );
  await fs.writeFile(journalPath, journalBytes);

  const pinned = bound.verifiedEntrants[0];
  await fs.writeFile(
    path.join(bound.generationDirectory, `${entrants[0].model_id}.json`),
    '{"tampered":true}\n',
  );
  await fs.writeFile(
    path.join(directories.sources, `${entrants[0].model_id}.rs`),
    'tampered source',
  );
  const pinnedResult = await verifyPinnedArtifact(successfulContext, pinned);
  assert.equal(pinnedResult.model_id, entrants[0].model_id);
  assert.equal(pinnedResult.wasm_sha256, sha256(liveArtifacts.get(entrants[0].model_id)));
});

test('resume fails closed on source or archived provenance tampering', async (t) => {
  const temporaryDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-tamper-'));
  t.after(() => fs.rm(temporaryDirectory, { recursive: true, force: true }));
  const directories = {
    generations: path.join(temporaryDirectory, 'generations'),
    sources: path.join(temporaryDirectory, 'sources'),
  };
  const codeStatus = normalizeCodeStatus(rawCodeStatus());
  const generationContext = {
    apiBase: 'http://arena.invalid',
    adminToken: 'test-token',
    codeStatus,
    apiClient: async (request) => {
      if (request.route === '/api/arena/code/generate') {
        return generationResponse().generated;
      }
      if (request.route === '/api/arena/models/register') return {};
      if (request.route === '/api/arena/code/compile') {
        return {
          model_id: entrant.model_id,
          compiled: false,
          compiler_stderr: 'retry later',
          bytes_written: 0,
        };
      }
      throw new Error(`unexpected route ${request.route}`);
    },
  };
  await assert.rejects(
    generateEntrant(generationContext, entrant, directories, 1, false),
    /fighter compilation failed/,
  );

  const checkpointPath = path.join(directories.generations, `${entrant.model_id}.json`);
  const sourcePath = path.join(directories.sources, `${entrant.model_id}.rs`);
  const checkpoint = JSON.parse(await fs.readFile(checkpointPath, 'utf8'));

  const retryLimitCheckpoint = { ...checkpoint, compile_attempts: 100 };
  await fs.writeFile(checkpointPath, `${JSON.stringify(retryLimitCheckpoint, null, 2)}\n`);
  const resumedRoutes = [];
  const resumeContext = {
    ...generationContext,
    apiClient: async (request) => {
      resumedRoutes.push(request.route);
      throw new Error('no API route should run for a rejected checkpoint');
    },
  };
  await assert.rejects(
    generateEntrant(resumeContext, entrant, directories, 3, true),
    /compilation retry limit reached/,
  );
  assert.deepEqual(resumedRoutes, []);

  checkpoint.provider_response.generated.provider_response_id = 'tampered-response';
  await fs.writeFile(checkpointPath, `${JSON.stringify(checkpoint, null, 2)}\n`);

  await assert.rejects(
    generateEntrant(resumeContext, entrant, directories, 3, true),
    /response archive.*differs/,
  );
  assert.deepEqual(resumedRoutes, []);

  checkpoint.provider_response.generated.provider_response_id = checkpoint.provider_response_id;
  await fs.writeFile(checkpointPath, `${JSON.stringify(checkpoint, null, 2)}\n`);
  await fs.writeFile(sourcePath, `${source}\n// tampered`);
  await assert.rejects(
    generateEntrant(resumeContext, entrant, directories, 3, true),
    /stale or unverified/,
  );
  assert.deepEqual(resumedRoutes, []);
});

test('generation usage must be complete, numeric, non-negative, and internally consistent', () => {
  const valid = {
    prompt_tokens: 100,
    completion_tokens: 25,
    total_tokens: 125,
    cost: 0,
  };
  assert.equal(validateGenerationUsage(valid, 2_049), valid);

  for (const [field, value] of [
    ['prompt_tokens', -1],
    ['completion_tokens', Number.POSITIVE_INFINITY],
    ['total_tokens', 125.5],
    ['cost', Number.NaN],
  ]) {
    assert.throws(
      () => validateGenerationUsage({ ...valid, [field]: value }, 2_049),
      new RegExp(field === 'cost' ? 'usage cost' : `usage ${field}`),
    );
  }
  assert.throws(
    () => validateGenerationUsage({ ...valid, total_tokens: 126 }, 2_049),
    /must equal prompt_tokens plus completion_tokens/,
  );
  assert.throws(
    () => validateGenerationUsage({
      ...valid,
      completion_tokens: 2_050,
      total_tokens: 2_150,
    }, 2_049),
    /exceeds the frozen completion limit/,
  );
});

test('legacy checkpoints cannot bypass the audited schema through downgrade', () => {
  const codeStatus = normalizeCodeStatus(rawCodeStatus());
  const checkpoint = {
    model_id: entrant.model_id,
    provider_model: entrant.provider_model,
    compiled: true,
    simulated: false,
    prompt_sha256: codeStatus.prompt_sha256,
    prompt_version: codeStatus.prompt_version,
    max_completion_tokens: codeStatus.max_tokens,
    provider_sort_policy: codeStatus.provider_sort_policy,
    temperature_policy: codeStatus.temperature_policy,
    reasoning_policy_version: codeStatus.reasoning_policy_version,
    provider_require_parameters: codeStatus.provider_require_parameters,
    reasoning_policy: { ...entrant.reasoning_policy },
    reasoning_mode: entrant.reasoning_policy.mode,
    reasoning_effort: entrant.reasoning_policy.effort,
    reasoning_exclude: codeStatus.reasoning_exclude,
    response_transport_policy: codeStatus.response_transport_policy,
    simulator_rules_version: codeStatus.simulator_rules_version,
    finish_reason: 'stop',
    resolved_model: 'provider/model-one-20260701',
    provider_name: 'Provider One',
    provider_response_id: 'generation-123',
    usage: {
      prompt_tokens: 100,
      completion_tokens: 25,
      total_tokens: 125,
      cost: 0.0125,
    },
    source_sha256: sha256(source),
    source_bytes: Buffer.byteLength(source, 'utf8'),
  };
  assert.throws(
    () => validateGenerationCheckpoint(checkpoint, entrant, source, codeStatus),
    /stale or unverified|checkpoint metadata is invalid/,
  );
});

test('capability policy omits unsupported reasoning and disables optional reasoning', () => {
  assert.deepEqual(reasoningPolicyFromModelMetadata({
    id: 'provider/plain',
    supported_parameters: ['max_tokens'],
  }), {
    version: 'capability_minimum_v1',
    mode: 'unsupported',
    effort: null,
    exclude: true,
  });
  assert.deepEqual(reasoningPolicyFromModelMetadata({
    id: 'provider/optional',
    supported_parameters: ['max_tokens', 'reasoning'],
    reasoning: {
      mandatory: false,
      default_enabled: true,
      supported_efforts: ['xhigh', 'high'],
    },
  }), {
    version: 'capability_minimum_v1',
    mode: 'disabled',
    effort: null,
    exclude: true,
  });
});

test('mandatory reasoning uses the least advertised non-none effort', () => {
  assert.deepEqual(reasoningPolicyFromModelMetadata({
    id: 'provider/mandatory',
    supported_parameters: ['reasoning'],
    reasoning: {
      mandatory: true,
      supported_efforts: ['high', 'none', 'low', 'medium'],
    },
  }), {
    version: 'capability_minimum_v1',
    mode: 'minimum',
    effort: 'low',
    exclude: true,
  });
  assert.equal(reasoningPolicyFromModelMetadata({
    id: 'provider/all-efforts',
    supported_parameters: ['reasoning'],
    reasoning: { mandatory: true, supported_efforts: null },
  }).effort, 'minimal');
});

test('reasoning policy validation fails closed on ambiguous mandatory metadata', () => {
  assert.throws(() => reasoningPolicyFromModelMetadata({
    id: 'provider/ambiguous',
    supported_parameters: ['reasoning'],
    reasoning: { mandatory: true },
  }), /does not expose supported_efforts/);
  assert.throws(() => reasoningPolicyFromModelMetadata({
    id: 'provider/none-only',
    supported_parameters: ['reasoning'],
    reasoning: { mandatory: true, supported_efforts: ['none'] },
  }), /no non-none supported effort/);
  assert.throws(() => validateReasoningPolicy({
    version: 'capability_minimum_v1',
    mode: 'disabled',
    effort: 'low',
    exclude: true,
  }, 'provider/invalid'), /effort must be null/);
});

test('ranking files validate their frozen policy without re-deriving from metadata', async () => {
  const temporaryDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-ranking-policy-'));
  const rankingPath = path.join(temporaryDirectory, 'ranking.json');
  const policy = {
    version: 'capability_minimum_v1',
    mode: 'disabled',
    effort: null,
    exclude: true,
  };
  const payload = {
    schema_version: 1,
    retrieved_at: '2026-07-23T00:00:00.000Z',
    source: 'frozen-test',
    models: [
      {
        id: 'provider/one',
        reasoning_policy: policy,
        supported_parameters: ['reasoning'],
        reasoning: { mandatory: true, supported_efforts: ['high'] },
      },
      { id: 'provider/two', reasoning_policy: policy },
    ],
  };
  try {
    await fs.writeFile(rankingPath, JSON.stringify(payload));
    const ranking = await loadRanking({ rankingFile: rankingPath, topModels: 2 });
    assert.deepEqual(ranking.models.map((model) => model.reasoning_policy), [policy, policy]);

    payload.models[1].reasoning_policy = { ...policy, version: 'future-policy' };
    await fs.writeFile(rankingPath, JSON.stringify(payload));
    await assert.rejects(
      loadRanking({ rankingFile: rankingPath, topModels: 2 }),
      /reasoning_policy version is invalid/,
    );
  } finally {
    await fs.rm(temporaryDirectory, { recursive: true, force: true });
  }
});

test('checked-in ranking snapshot carries a valid policy for every model', async () => {
  const snapshotPath = path.join(
    path.dirname(fileURLToPath(import.meta.url)),
    'snapshots/openrouter_top_weekly_2026-07-23.json',
  );
  const ranking = await loadRanking({ rankingFile: snapshotPath, topModels: 10 });
  assert.equal(ranking.models.length, 10);
  assert.equal(ranking.models.at(-1).reasoning_policy.mode, 'minimum');
  assert.equal(ranking.models.at(-1).reasoning_policy.effort, 'low');
});
