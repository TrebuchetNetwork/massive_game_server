// Continuous Model League — fighter generation and artifact handling.
//
// run_top10_season.mjs only generates for a full ARENA_TOP_MODELS roster
// (loadRanking refuses anything but exactly N ranked models), so a partial
// roster — one challenger filling one open slot — cannot go through the
// runner's --generate-only. Recruit therefore calls the server admin API
// directly, reusing the runner's exact request contract:
//
//   GET  /api/arena/code/status            (frozen competition contract)
//   POST /api/arena/code/generate          (model + reasoning policy)
//   POST /api/arena/models/register        (arena model id)
//   POST /api/arena/code/compile           (source_code, overwrite)
//
// The compiled artifact is persisted as a schema-v2 generation checkpoint
// plus its Rust source in the league's fighters directory. Each day's
// evaluation materializes the runner's expected season layout from those
// records (arena model ids are derived from the day-specific season id) and
// re-registers/recompiles the day's ids, because the server resolves battle
// fighters as <wasm_dir>/<model_id>.wasm.

import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { arenaApiJson } from '../arena_api_client.mjs';
import {
  entrantsFromRanking,
  normalizeCodeStatus,
  reasoningPolicyFromModelMetadata,
  validateGeneratedResponse,
  validateGenerationCheckpoint,
  validateReasoningPolicy,
  validateRevisionResponse,
} from '../run_top10_season.mjs';
import { atomicWriteJson } from './state.mjs';

const sha256 = (value) => createHash('sha256').update(value).digest('hex');
const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

const CHECKPOINT_SCHEMA_VERSION = 2;
const CHECKPOINT_STAGE_COMPILED = 'compiled';
const MAX_PUBLISHED_WASM_BYTES = 2 * 1024 * 1024;
const DEFAULT_ADMIN_TOKEN_PATH = path.join(
  os.homedir(),
  '.config/massive-game-server/secrets/arena-admin-bearer-token',
);

/** Admin credential: env, then env-pointed file, then the default 0600 file. */
export async function readAdminToken({ env = process.env, readFile = fs.readFile } = {}) {
  const direct = String(env.ARENA_ADMIN_BEARER_TOKEN || '').trim();
  if (direct) return direct;
  const configured = String(env.ARENA_ADMIN_BEARER_TOKEN_FILE || '').trim();
  if (configured) {
    const value = (await readFile(configured, 'utf8')).trim();
    if (value) return value;
    throw new Error('ARENA_ADMIN_BEARER_TOKEN_FILE is empty');
  }
  try {
    const value = (await readFile(DEFAULT_ADMIN_TOKEN_PATH, 'utf8')).trim();
    if (value) return value;
  } catch {
    // Fall through to the uniform error below.
  }
  throw new Error('ARENA_ADMIN_BEARER_TOKEN or ARENA_ADMIN_BEARER_TOKEN_FILE is required');
}

export function apiBaseFromEnv(env = process.env) {
  return String(env.ARENA_API_BASE || 'http://127.0.0.1:8080').replace(/\/$/, '');
}

export async function loadCodeStatus({ apiBase, adminToken, apiClient = arenaApiJson }) {
  return normalizeCodeStatus(await apiClient({
    apiBase,
    adminToken,
    route: '/api/arena/code/status',
  }));
}

/**
 * Build the runner-shaped entrant for one ranking entry (a challenger from a
 * --dry-run plan) under the given season id. The arena model id is derived
 * exactly the way the runner derives it.
 */
export function entrantFromChallenger(challenger, seasonId, retrievedAt) {
  const ranking = {
    retrieved_at: retrievedAt,
    models: [{
      provider_rank: challenger.provider_rank,
      id: challenger.id,
      canonical_slug: challenger.canonical_slug ?? null,
      name: challenger.name || challenger.id,
      reasoning_policy: validateReasoningPolicy(challenger.reasoning_policy, challenger.id),
    }],
  };
  return entrantsFromRanking(ranking, seasonId)[0];
}

// Same acceptance checks as the runner's private validateCompileResponse.
function validateCompileResponse(compile, expectedModelId) {
  if (!compile || typeof compile !== 'object' || Array.isArray(compile)) {
    throw new Error('compile response is incomplete');
  }
  if (compile.model_id !== expectedModelId) {
    throw new Error('compile response model_id differs from the requested entrant');
  }
  if (compile.compiled !== true) {
    const compilerError = String(compile.compiler_stderr || '').trim().slice(0, 500);
    throw new Error(`fighter compilation failed${compilerError ? `: ${compilerError}` : ''}`);
  }
  const wasmBytes = Number(compile.bytes_written);
  if (!Number.isSafeInteger(wasmBytes) || wasmBytes <= 0 || wasmBytes > MAX_PUBLISHED_WASM_BYTES) {
    throw new Error('compiled WASM size is invalid');
  }
  const wasmSha256 = String(compile.wasm_sha256 || '');
  if (!/^[a-f0-9]{64}$/.test(wasmSha256)) {
    throw new Error('compiled WASM digest is invalid');
  }
  return { wasmBytes, wasmSha256 };
}

/**
 * Register the entrant's arena model id and compile its source on the server
 * (overwrite). Battles resolve fighters by model id, so every season day's
 * derived ids must be published before evaluation.
 *
 * `attempts` is the track's compile-attempt policy (L0: 1, L1+ : 3). Retries
 * recompile the SAME source — only transient compiler failures can benefit;
 * a deterministic rustc error fails every attempt. The raw compiler stderr
 * is surfaced in the thrown error, and the number of attempts used is tagged
 * as `error.compileAttempts` so the ledger reflects reality.
 */
export async function compileFighterSource({
  apiBase,
  adminToken,
  entrant,
  source,
  attempts = 1,
  apiClient = arenaApiJson,
}) {
  if (!Number.isSafeInteger(attempts) || attempts < 1 || attempts > 10) {
    throw new Error('compile attempts must be a safe integer in 1..10');
  }
  await apiClient({
    apiBase,
    adminToken,
    method: 'POST',
    route: '/api/arena/models/register',
    body: {
      model_id: entrant.model_id,
      model_name: entrant.model_name,
      provider: 'openrouter',
      version: entrant.canonical_slug || entrant.provider_model,
      active: true,
      initial_elo: 1000,
    },
  });
  let lastError;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      const compile = await apiClient({
        apiBase,
        adminToken,
        method: 'POST',
        route: '/api/arena/code/compile',
        timeoutMs: 180_000,
        body: {
          model_id: entrant.model_id,
          source_code: source,
          overwrite: true,
        },
      });
      return validateCompileResponse(compile, entrant.model_id);
    } catch (error) {
      lastError = error;
      if (attempt < attempts) await delay(Math.min(3_000, 500 * (2 ** (attempt - 1))));
    }
  }
  lastError.compileAttempts = attempts;
  throw lastError;
}

// Schema-v2 compiled checkpoint, same shape the runner's
// buildGeneratedCheckpoint produces after a successful compile.
function buildCompiledCheckpoint({ entrant, generated, verified, codeStatus, wasm, generatedAt }) {
  const archivedGenerated = { ...generated };
  delete archivedGenerated.source_code;
  const providerResponse = { generated: archivedGenerated };
  const sourceSha256 = sha256(verified.source);
  return {
    schema_version: CHECKPOINT_SCHEMA_VERSION,
    stage: CHECKPOINT_STAGE_COMPILED,
    provider_rank: entrant.provider_rank,
    model_id: entrant.model_id,
    model_name: entrant.model_name,
    provider_model: entrant.provider_model,
    canonical_slug: entrant.canonical_slug,
    generated_at: generatedAt,
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
    finish_reason: verified.finishReason,
    resolved_model: verified.resolvedModel,
    provider_name: verified.providerName,
    provider_response_id: verified.providerResponseId,
    source_bytes: verified.sourceBytes,
    source_sha256: sourceSha256,
    wasm_bytes: wasm.wasmBytes,
    wasm_sha256: wasm.wasmSha256,
    compiled_at: generatedAt,
    generation_attempts: 1,
    compile_attempts: 1,
    usage: verified.usage,
    generation_archive_sha256: sha256(JSON.stringify({
      provider_response: providerResponse,
      source_sha256: sourceSha256,
    })),
    provider_response: providerResponse,
  };
}

/**
 * Generate and compile one fighter bot via the server admin API, reusing the
 * runner's exact contract. `compileAttempts` is the track's compile policy
 * (L0: 1 — a failed compile means no entry; L1+: 3). Returns
 * { checkpoint, source, codeStatus }.
 */
export async function generateFighter({
  apiBase,
  adminToken,
  entrant,
  compileAttempts = 1,
  codeStatus = null,
  apiClient = arenaApiJson,
  now = () => new Date().toISOString(),
}) {
  const status = codeStatus || await loadCodeStatus({ apiBase, adminToken, apiClient });
  if (status.provider_configured !== true) {
    throw new Error(
      'the live server has no OPENROUTER_API_KEY or OPENROUTER_API_KEY_FILE; refusing template fallback',
    );
  }
  const generated = await apiClient({
    apiBase,
    adminToken,
    method: 'POST',
    route: '/api/arena/code/generate',
    timeoutMs: Math.max(
      180_000,
      (Number(status.provider_timeout_secs) || 120) * 1_000 + 150_000,
    ),
    body: {
      model: entrant.provider_model,
      reasoning_mode: entrant.reasoning_policy.mode,
      reasoning_effort: entrant.reasoning_policy.effort,
    },
  });
  const verified = validateGeneratedResponse(generated, status, entrant);
  const wasm = await compileFighterSource({
    apiBase,
    adminToken,
    entrant,
    source: verified.source,
    attempts: compileAttempts,
    apiClient,
  });
  const checkpoint = buildCompiledCheckpoint({
    entrant,
    generated,
    verified,
    codeStatus: status,
    wasm,
    generatedAt: now(),
  });
  return { checkpoint, source: verified.source, codeStatus: status };
}

/**
 * Codegen half of a revision: POST /api/arena/code/revise with the exact
 * request body of the runner's epoch-336 revision flow (reviseEntrant) and
 * validate the response against the frozen revision contract. The brief
 * plays the stats_digest role. All failures are tagged `phase: 'codegen'`
 * so the caller can record the right submission outcome.
 */
export async function requestRevision({
  apiBase,
  adminToken,
  entrant,
  source,
  brief,
  codeStatus = null,
  apiClient = arenaApiJson,
}) {
  const status = codeStatus || await loadCodeStatus({ apiBase, adminToken, apiClient });
  if (status.provider_configured !== true) {
    const error = new Error(
      'the live server has no OPENROUTER_API_KEY or OPENROUTER_API_KEY_FILE; refusing template fallback',
    );
    error.phase = 'codegen';
    throw error;
  }
  let response;
  try {
    response = await apiClient({
      apiBase,
      adminToken,
      method: 'POST',
      route: '/api/arena/code/revise',
      timeoutMs: Math.max(
        180_000,
        (Number(status.provider_timeout_secs) || 120) * 1_000 + 150_000,
      ),
      body: {
        model: entrant.provider_model,
        previous_source: source,
        stats_digest: brief,
        reasoning_mode: entrant.reasoning_policy.mode,
        reasoning_effort: entrant.reasoning_policy.effort,
      },
    });
  } catch (error) {
    error.phase = 'codegen';
    throw error;
  }
  try {
    const verified = validateRevisionResponse(response, status, entrant);
    return { response, verified, codeStatus: status };
  } catch (error) {
    error.phase = 'codegen';
    throw error;
  }
}

/**
 * Build the revised schema-v2 checkpoint from the previous one (same lineage
 * fields), pinned to the revision prompt pair the server reported — the same
 * construction as the runner's reviseEntrant. Pure; exported for tests and
 * for the journaled resume path in the league supervisor.
 */
export function buildRevisedCheckpoint({ previousCheckpoint, response, verified, brief, wasm, now }) {
  const providerGenerated = { ...response };
  delete providerGenerated.source_code;
  const providerResponse = { generated: providerGenerated };
  const sourceSha256 = sha256(verified.source);
  const completedAt = now();
  return {
    ...previousCheckpoint,
    prompt_version: verified.promptVersion,
    prompt_sha256: verified.promptSha256,
    finish_reason: verified.finishReason,
    resolved_model: verified.resolvedModel,
    provider_name: verified.providerName,
    provider_response_id: verified.providerResponseId,
    source_bytes: verified.sourceBytes,
    source_sha256: sourceSha256,
    wasm_bytes: wasm.wasmBytes,
    wasm_sha256: wasm.wasmSha256,
    compiled_at: completedAt,
    usage: { ...verified.usage },
    generation_archive_sha256: sha256(JSON.stringify({
      provider_response: providerResponse,
      source_sha256: sourceSha256,
    })),
    provider_response: providerResponse,
    revision_of: previousCheckpoint.source_sha256,
    stats_digest_sha256: sha256(brief),
  };
}

/**
 * Compile half of a revision: register + compile the revised source, build
 * the revised checkpoint, and run the runner's own strict
 * validateGenerationCheckpoint self-check (matching reviseEntrant, which
 * validates before persisting). All failures are tagged `phase: 'compile'`.
 * `request` is the result of requestRevision plus the original brief.
 */
export async function compileRevision({
  apiBase,
  adminToken,
  entrant,
  request,
  previousCheckpoint,
  compileAttempts = 1,
  apiClient = arenaApiJson,
  now = () => new Date().toISOString(),
}) {
  let wasm;
  try {
    wasm = await compileFighterSource({
      apiBase,
      adminToken,
      entrant,
      source: request.verified.source,
      attempts: compileAttempts,
      apiClient,
    });
  } catch (error) {
    if (!error.phase) error.phase = 'compile';
    throw error;
  }
  try {
    const checkpoint = buildRevisedCheckpoint({
      previousCheckpoint,
      response: request.response,
      verified: request.verified,
      brief: request.brief,
      wasm,
      now,
    });
    validateGenerationCheckpoint(checkpoint, entrant, request.verified.source, request.codeStatus);
    return { checkpoint, source: request.verified.source };
  } catch (error) {
    if (!error.phase) error.phase = 'compile';
    throw error;
  }
}

/**
 * Full revision round-trip (requestRevision + compileRevision). Errors carry
 * a `phase` tag ('codegen' | 'compile'); a failed attempt leaves the previous
 * artifact untouched. The league supervisor calls the two halves separately
 * so it can journal the provider response between them.
 */
export async function reviseFighter({
  apiBase,
  adminToken,
  entrant,
  source,
  brief,
  previousCheckpoint,
  compileAttempts = 1,
  codeStatus = null,
  apiClient = arenaApiJson,
  now = () => new Date().toISOString(),
}) {
  const request = await requestRevision({
    apiBase,
    adminToken,
    entrant,
    source,
    brief,
    codeStatus,
    apiClient,
  });
  const { checkpoint, source: revisedSource } = await compileRevision({
    apiBase,
    adminToken,
    entrant,
    request: { ...request, brief },
    previousCheckpoint,
    compileAttempts,
    apiClient,
    now,
  });
  return { checkpoint, source: revisedSource, codeStatus: request.codeStatus };
}

export const fighterKeyFor = (modelId) => String(modelId).replace(/[^A-Za-z0-9._-]+/g, '__');

/**
 * Rebuild the runner-shaped entrant from a stored fighter checkpoint for the
 * revision/rebind flows. Every identity field the checkpoint audit compares
 * (validateCheckpointAudit in run_top10_season.mjs) must round-trip —
 * including provider_rank: it is pinned in the checkpoint at generation time
 * and the audit requires checkpoint.provider_rank === entrant.provider_rank.
 * (An earlier reconstruction dropped provider_rank, which made every
 * revision self-check fail with "generation checkpoint is stale or
 * unverified". The daily ranking position is league bookkeeping; evaluate
 * re-points it per day in materializeSeasonFighter, so carrying the pinned
 * value here is consistent and safe.)
 */
export function entrantFromCheckpoint(checkpoint) {
  return {
    provider_rank: checkpoint.provider_rank,
    model_id: checkpoint.model_id,
    model_name: checkpoint.model_name,
    provider_model: checkpoint.provider_model,
    canonical_slug: checkpoint.canonical_slug,
    reasoning_policy: checkpoint.reasoning_policy,
  };
}

const OPENROUTER_WEEKLY_RANKING_URL = 'https://openrouter.ai/api/v1/models?output_modalities=text&sort=top-weekly';

/**
 * Fetch the live OpenRouter top-weekly ranking for recruit/bootstrap,
 * tolerant of unusable entries: models whose reasoning metadata cannot
 * produce a valid capability_minimum_v1 policy (e.g. a mandatory-reasoning
 * model with broken supported_efforts) are skipped and logged rather than
 * failing the whole fetch — the league simply cannot field them. Returns
 * { models } in the runner's normalized ranking shape (provider_rank is the
 * rank among KEPT models).
 */
export async function fetchEligibleRanking({
  topModels = 60,
  fetchImpl = fetch,
  log = () => {},
} = {}) {
  if (!Number.isSafeInteger(topModels) || topModels < 1) {
    throw new Error('topModels must be a positive safe integer');
  }
  const response = await fetchImpl(OPENROUTER_WEEKLY_RANKING_URL, {
    headers: { Accept: 'application/json' },
    signal: AbortSignal.timeout(30_000),
  });
  if (!response.ok) {
    throw new Error(`OpenRouter ranking request failed with HTTP ${response.status}`);
  }
  const payload = await response.json();
  const raw = Array.isArray(payload?.data) ? payload.data : payload?.models;
  if (!Array.isArray(raw)) throw new Error('ranking payload does not contain a model list');
  const models = [];
  for (const candidate of raw) {
    if (models.length >= topModels) break;
    const id = typeof candidate?.id === 'string' ? candidate.id.trim() : '';
    if (!id) continue;
    const modalities = candidate?.architecture?.output_modalities;
    if (Array.isArray(modalities) && !modalities.includes('text')) continue;
    let reasoningPolicy;
    try {
      reasoningPolicy = reasoningPolicyFromModelMetadata(candidate);
    } catch (error) {
      log(`ranking: skipping ${id} (${String(error?.message || error).slice(0, 160)})`);
      continue;
    }
    models.push({
      provider_rank: models.length + 1,
      id,
      canonical_slug: typeof candidate.canonical_slug === 'string' ? candidate.canonical_slug.trim() : null,
      name: typeof candidate.name === 'string' && candidate.name.trim() ? candidate.name.trim() : id,
      pricing: candidate.pricing || null,
      context_length: Number.isFinite(Number(candidate.context_length)) ? Number(candidate.context_length) : null,
      created: Number.isFinite(Number(candidate.created)) ? Number(candidate.created) : null,
      reasoning_policy: reasoningPolicy,
    });
  }
  return { models };
}

export function fighterDirectoryFor(stateDirectory, modelId) {
  return path.join(stateDirectory, 'fighters', fighterKeyFor(modelId));
}

async function writeTextAtomic(targetPath, contents) {
  await fs.mkdir(path.dirname(targetPath), { recursive: true });
  const temporaryPath = `${targetPath}.tmp-${process.pid}-${Date.now()}`;
  const handle = await fs.open(temporaryPath, 'w', 0o600);
  try {
    await handle.writeFile(contents);
    await handle.sync();
  } finally {
    await handle.close();
  }
  await fs.rename(temporaryPath, targetPath);
}

/**
 * Persist a fighter's compiled checkpoint, source, and ranking metadata so
 * every future evaluation day can rebuild the runner's season layout.
 * `meta` keeps the ranking-file fields: model_id (provider id), slug,
 * model_name, reasoning_policy, pricing, context_length, created.
 */
export async function writeFighterRecord(stateDirectory, modelId, { checkpoint, source, meta }) {
  const directory = fighterDirectoryFor(stateDirectory, modelId);
  await fs.mkdir(directory, { recursive: true });
  await atomicWriteJson(path.join(directory, 'checkpoint.json'), checkpoint);
  await writeTextAtomic(path.join(directory, 'source.rs'), source);
  await atomicWriteJson(path.join(directory, 'meta.json'), meta);
}

export async function readFighterRecord(stateDirectory, modelId) {
  const directory = fighterDirectoryFor(stateDirectory, modelId);
  const checkpoint = JSON.parse(await fs.readFile(path.join(directory, 'checkpoint.json'), 'utf8'));
  const source = await fs.readFile(path.join(directory, 'source.rs'), 'utf8');
  const meta = JSON.parse(await fs.readFile(path.join(directory, 'meta.json'), 'utf8'));
  return { checkpoint, source, meta };
}

/** Ranking-file model entry in the runner's frozen shape (see W33 plan). */
export function rankingModelFromMeta(meta, providerRank) {
  return {
    provider_rank: providerRank,
    id: meta.model_id,
    canonical_slug: meta.slug ?? null,
    name: meta.model_name || meta.model_id,
    pricing: meta.pricing ?? null,
    context_length: meta.context_length ?? null,
    created: meta.created ?? null,
    reasoning_policy: validateReasoningPolicy(meta.reasoning_policy, meta.model_id),
  };
}

/**
 * Write the runner's expected generation layout for one entrant into a season
 * directory: sources/<id>.rs plus generations/<id>.json. Only identity fields
 * (arena model id, rank, name, slug, reasoning policy) are re-pointed at the
 * day's entrant; the generation proof (provider response archive, digests,
 * contract fields) is carried over untouched.
 */
export async function materializeSeasonFighter({ seasonDirectory, entrant, fighter }) {
  const generations = path.join(seasonDirectory, 'generations');
  const sources = path.join(seasonDirectory, 'sources');
  await fs.mkdir(generations, { recursive: true });
  await fs.mkdir(sources, { recursive: true });
  await writeTextAtomic(path.join(sources, `${entrant.model_id}.rs`), fighter.source);
  const checkpoint = {
    ...fighter.checkpoint,
    provider_rank: entrant.provider_rank,
    model_id: entrant.model_id,
    model_name: entrant.model_name,
    canonical_slug: entrant.canonical_slug,
    reasoning_policy: { ...entrant.reasoning_policy },
    reasoning_mode: entrant.reasoning_policy.mode,
    reasoning_effort: entrant.reasoning_policy.effort,
  };
  await atomicWriteJson(path.join(generations, `${entrant.model_id}.json`), checkpoint);
}
