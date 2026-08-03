#!/usr/bin/env node

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { acquireOwnedLock, releaseOwnedLock } from './owned_lock.mjs';
import {
  entrantsFromRanking,
  loadRanking,
  normalizeCodeStatus,
  readArtifactBinding,
  validateReasoningPolicy,
} from './run_top10_season.mjs';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT_DIR = path.resolve(SCRIPT_DIR, '../..');
const RUNNER_PATH = path.join(SCRIPT_DIR, 'run_top10_season.mjs');
const DEFAULT_STATE_DIR = path.join(ROOT_DIR, 'artifacts/arena/weekly-supervisor');
const DEFAULT_PUBLISH_PATH = path.join(ROOT_DIR, 'data/arena_ratings.json');
const DEFAULT_POINTS_BY_RANK = Object.freeze([1000, 700, 500, 360, 250, 180, 120, 80, 50, 30]);
const SECRET_ENV_NAMES = Object.freeze([
  'ARENA_ADMIN_BEARER_TOKEN',
  'OPENROUTER_API_KEY',
]);
const MAX_CAPTURE_BYTES = 8 * 1024 * 1024;
const MAX_ERROR_CHARS = 2_000;
const PROVIDER_SORT_POLICY = 'throughput';
const REASONING_POLICY_VERSION = 'capability_minimum_v1';
const PROVIDER_REQUIRE_PARAMETERS = true;
const REASONING_EXCLUDE = true;
const RESPONSE_TRANSPORT_POLICY = 'sse_v1';
const SOURCE_LIMIT_BYTES = 50 * 1024;
const MIN_COMPLETION_TOKENS = 2_049;
const MAX_COMPLETION_TOKENS = 16_384;
const COLLABORATION_ABI_VERSION = 'bot_tick_v2/1';
const ARTIFACT_BINDING_VERSION = 1;
const LEGACY_ARCHIVE_KIND = 'unbound_wasm_v1';

const sha256 = (value) => createHash('sha256').update(value).digest('hex');
const nowIso = () => new Date().toISOString();

/** Return an ISO-8601 week identifier using UTC, for example `2026-W01`. */
export function isoWeekId(input = new Date()) {
  const parsed = input instanceof Date ? new Date(input.getTime()) : new Date(input);
  if (!Number.isFinite(parsed.getTime())) throw new Error('isoWeekId requires a valid date');

  const date = new Date(Date.UTC(
    parsed.getUTCFullYear(),
    parsed.getUTCMonth(),
    parsed.getUTCDate(),
  ));
  const weekday = date.getUTCDay() || 7;
  date.setUTCDate(date.getUTCDate() + 4 - weekday);
  const weekYear = date.getUTCFullYear();
  const yearStart = new Date(Date.UTC(weekYear, 0, 1));
  const week = Math.ceil((((date - yearStart) / 86_400_000) + 1) / 7);
  return `${weekYear}-W${String(week).padStart(2, '0')}`;
}

/**
 * Build a deterministic, collision-free-in-practice 32-bit seed pack.
 * The odd Weyl increment is a permutation modulo 2^32, so packs do not repeat
 * until more than 2^32 total seeds have been requested for one week.
 */
export function deterministicSeedPack(weekId, epochIndex, packSize = 4) {
  if (!/^\d{4}-W(?:0[1-9]|[1-4]\d|5[0-3])$/.test(String(weekId))) {
    throw new Error('invalid ISO week ID');
  }
  if (!Number.isSafeInteger(epochIndex) || epochIndex < 0) {
    throw new Error('epoch index must be a non-negative safe integer');
  }
  if (!Number.isSafeInteger(packSize) || packSize < 1 || packSize > 64) {
    throw new Error('seed pack size must be between 1 and 64');
  }
  const base = BigInt(`0x${sha256(String(weekId)).slice(0, 8)}`);
  const first = BigInt(epochIndex) * BigInt(packSize);
  const increment = 2_654_435_761n;
  const mask = 0xffff_ffffn;
  return Array.from({ length: packSize }, (_, slot) => (
    Number((base + ((first + BigInt(slot)) * increment)) & mask)
  ));
}

function integerEnv(name, fallback, min, max) {
  const parsed = Number.parseInt(process.env[name] || '', 10);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.max(min, Math.min(max, parsed));
}

function resolveFromRoot(value, fallback) {
  const configured = String(value || '').trim();
  return configured ? path.resolve(ROOT_DIR, configured) : fallback;
}

async function atomicWriteJson(targetPath, value) {
  await fs.mkdir(path.dirname(targetPath), { recursive: true });
  const temporaryPath = `${targetPath}.tmp-${process.pid}-${Date.now()}`;
  try {
    await fs.writeFile(temporaryPath, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
    await fs.rename(temporaryPath, targetPath);
  } catch (error) {
    await fs.rm(temporaryPath, { force: true }).catch(() => {});
    throw error;
  }
}

async function atomicWriteBytes(targetPath, value) {
  await fs.mkdir(path.dirname(targetPath), { recursive: true });
  const temporaryPath = `${targetPath}.tmp-${process.pid}-${Date.now()}`;
  try {
    await fs.writeFile(temporaryPath, value, { mode: 0o600 });
    await fs.rename(temporaryPath, targetPath);
  } catch (error) {
    await fs.rm(temporaryPath, { force: true }).catch(() => {});
    throw error;
  }
}

async function readJson(targetPath) {
  return JSON.parse(await fs.readFile(targetPath, 'utf8'));
}

async function fileExists(targetPath) {
  try {
    await fs.access(targetPath);
    return true;
  } catch {
    return false;
  }
}

function isIsoTimestamp(value) {
  if (typeof value !== 'string') return false;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) && new Date(parsed).toISOString() === value;
}

function relativeArtifactPath(targetPath) {
  return path.relative(ROOT_DIR, targetPath).split(path.sep).join('/');
}

function roundRating(value) {
  return Math.round(Math.max(0, Math.min(100, Number(value) || 0)) * 100) / 100;
}

function validateReasoningPolicyProvenance(entries, expectedModelIds) {
  if (!Array.isArray(entries) || entries.length !== expectedModelIds.length) {
    throw new Error('weekly arena reasoning policy provenance is incomplete');
  }
  const normalized = entries.map((entry) => {
    const modelId = String(entry?.model_id || '').trim();
    const providerModel = String(entry?.provider_model || '').trim();
    if (!modelId || !providerModel) {
      throw new Error('weekly arena reasoning policy provenance has an invalid model');
    }
    return {
      model_id: modelId,
      provider_model: providerModel,
      reasoning_policy: validateReasoningPolicy(entry.reasoning_policy, providerModel),
    };
  });
  if (new Set(normalized.map((entry) => entry.model_id)).size !== normalized.length
      || new Set(normalized.map((entry) => entry.provider_model)).size !== normalized.length
      || normalized.map((entry) => entry.model_id).sort().join('\n')
        !== [...expectedModelIds].sort().join('\n')) {
    throw new Error('weekly arena reasoning policy provenance differs from the entrant roster');
  }
  return normalized;
}

function validateArtifactBindings(entries, expectedModelIds) {
  if (!Array.isArray(entries) || entries.length !== expectedModelIds.length) {
    throw new Error('weekly arena artifact bindings are incomplete');
  }
  const normalized = entries.map((entry) => {
    const modelId = String(entry?.model_id || '').trim();
    if (!modelId
        || !Number.isSafeInteger(entry?.wasm_bytes)
        || entry.wasm_bytes < 1
        || entry.wasm_bytes > 2 * 1024 * 1024
        || !/^[a-f0-9]{64}$/.test(String(entry?.wasm_sha256 || ''))) {
      throw new Error('weekly arena artifact binding is invalid');
    }
    return {
      model_id: modelId,
      wasm_bytes: entry.wasm_bytes,
      wasm_sha256: entry.wasm_sha256,
    };
  });
  if (new Set(normalized.map((entry) => entry.model_id)).size !== normalized.length
      || normalized.map((entry) => entry.model_id).sort().join('\n')
        !== [...expectedModelIds].sort().join('\n')) {
    throw new Error('weekly arena artifact bindings differ from the entrant roster');
  }
  return normalized;
}

function validateArenaContract(contract, errorMessage) {
  if (!/^[a-f0-9]{64}$/.test(String(contract?.prompt_sha256 || ''))
      || !/^[A-Za-z0-9_.:-]{1,128}$/.test(String(contract?.prompt_version || ''))
      || !Number.isSafeInteger(contract?.max_completion_tokens)
      || contract.max_completion_tokens < MIN_COMPLETION_TOKENS
      || contract.max_completion_tokens > MAX_COMPLETION_TOKENS
      || contract.provider_sort_policy !== PROVIDER_SORT_POLICY
      || contract.temperature_policy !== 'provider_default'
      || contract.reasoning_policy_version !== REASONING_POLICY_VERSION
      || contract.provider_require_parameters !== PROVIDER_REQUIRE_PARAMETERS
      || contract.reasoning_exclude !== REASONING_EXCLUDE
      || contract.response_transport_policy !== RESPONSE_TRANSPORT_POLICY
      || contract.source_limit_bytes !== SOURCE_LIMIT_BYTES
      || contract.collaboration_abi_version !== COLLABORATION_ABI_VERSION
      || typeof contract.simulator_rules_version !== 'string'
      || !contract.simulator_rules_version.trim()) {
    throw new Error(errorMessage);
  }
  return contract;
}

function arenaContractFromServerStatus(serverStatus) {
  return validateArenaContract({
    prompt_sha256: serverStatus.prompt_sha256,
    prompt_version: serverStatus.prompt_version,
    max_completion_tokens: serverStatus.max_tokens,
    provider_sort_policy: serverStatus.provider_sort_policy,
    temperature_policy: serverStatus.temperature_policy,
    reasoning_policy_version: serverStatus.reasoning_policy_version,
    provider_require_parameters: serverStatus.provider_require_parameters,
    reasoning_exclude: serverStatus.reasoning_exclude,
    response_transport_policy: serverStatus.response_transport_policy,
    source_limit_bytes: serverStatus.source_limit_bytes,
    collaboration_abi_version: serverStatus.collaboration_abi_version,
    simulator_rules_version: serverStatus.simulator_rules_version,
  }, 'generation did not checkpoint a valid competition contract');
}

async function secretRedactor() {
  const values = new Set();
  for (const name of SECRET_ENV_NAMES) {
    const direct = String(process.env[name] || '').trim();
    if (direct) values.add(direct);
    const secretPath = String(process.env[`${name}_FILE`] || '').trim();
    if (!secretPath) continue;
    values.add(secretPath);
    try {
      const fromFile = (await fs.readFile(secretPath, 'utf8')).trim();
      if (fromFile) values.add(fromFile);
    } catch {
      // The runner reports an unusable credential without its contents.
    }
  }
  const ordered = [...values].filter(Boolean).sort((left, right) => right.length - left.length);
  return (value) => {
    let sanitized = String(value ?? '');
    for (const secret of ordered) sanitized = sanitized.split(secret).join('[REDACTED]');
    return sanitized;
  };
}

let activeChild = null;
let stopRequested = false;
let stopTimer = null;

function requestStop(signal) {
  if (stopRequested) return;
  stopRequested = true;
  process.stdout.write(`[arena-weekly] ${signal}; stopping after the active checkpoint\n`);
  if (activeChild && activeChild.exitCode === null) {
    activeChild.kill('SIGTERM');
    stopTimer = setTimeout(() => {
      if (activeChild && activeChild.exitCode === null) activeChild.kill('SIGKILL');
    }, 10_000);
    stopTimer.unref();
  }
}

async function runRunner(args, { env = {}, capture = false, redact }) {
  if (stopRequested) throw new Error('supervisor is stopping');
  const child = spawn(process.execPath, [RUNNER_PATH, ...args], {
    cwd: ROOT_DIR,
    env: { ...process.env, ARENA_TOP_MODELS: '10', ...env },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  activeChild = child;
  let stdout = '';
  let stderr = '';
  let stdoutPending = '';
  let stderrPending = '';
  const collect = (current, chunk) => {
    const combined = current + chunk;
    return combined.length > MAX_CAPTURE_BYTES
      ? combined.slice(combined.length - MAX_CAPTURE_BYTES)
      : combined;
  };

  const writeCompleteLines = (destination, current, text) => {
    const pending = current + text;
    const boundary = pending.lastIndexOf('\n');
    if (boundary < 0) return pending;
    destination.write(redact(pending.slice(0, boundary + 1)));
    return pending.slice(boundary + 1);
  };

  child.stdout.on('data', (chunk) => {
    const text = chunk.toString('utf8');
    if (capture) stdout = collect(stdout, text);
    else stdoutPending = writeCompleteLines(process.stdout, stdoutPending, text);
  });
  child.stderr.on('data', (chunk) => {
    const text = chunk.toString('utf8');
    stderr = collect(stderr, text);
    if (!capture) stderrPending = writeCompleteLines(process.stderr, stderrPending, text);
  });

  const result = await new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => resolve({ code, signal }));
  }).finally(() => {
    if (!capture && stdoutPending) process.stdout.write(redact(stdoutPending));
    if (!capture && stderrPending) process.stderr.write(redact(stderrPending));
    activeChild = null;
    if (stopTimer) clearTimeout(stopTimer);
    stopTimer = null;
  });
  if (result.code !== 0) {
    const detail = redact(stderr).trim().slice(-MAX_ERROR_CHARS);
    throw new Error(
      `season runner exited ${result.signal ? `on ${result.signal}` : `with code ${result.code}`}`
      + (detail ? `: ${detail}` : ''),
    );
  }
  return { stdout, stderr: redact(stderr) };
}

function validatePlan(plan, weekId) {
  if (!plan || typeof plan !== 'object' || typeof plan.season_id !== 'string') {
    throw new Error('season dry-run did not return a valid plan');
  }
  if (!Array.isArray(plan.ranking?.models) || plan.ranking.models.length !== 10) {
    throw new Error('weekly arena requires exactly ten ranked models');
  }
  if (!Array.isArray(plan.entrants) || plan.entrants.length !== 10) {
    throw new Error('weekly arena plan requires exactly ten entrants');
  }
  if (new Set(plan.ranking.models.map((model) => model.id)).size !== 10) {
    throw new Error('weekly ranking has duplicate model IDs');
  }
  const reasoningPolicies = validateReasoningPolicyProvenance(
    plan.entrants.map((entrant, index) => {
      const rankedModel = plan.ranking.models[index];
      if (entrant.provider_model !== rankedModel?.id
          || JSON.stringify(validateReasoningPolicy(
            entrant.reasoning_policy,
            entrant.provider_model,
          )) !== JSON.stringify(validateReasoningPolicy(
            rankedModel?.reasoning_policy,
            rankedModel?.id,
          ))) {
        throw new Error('weekly entrant reasoning policy differs from its ranked model');
      }
      return {
        model_id: entrant.model_id,
        provider_model: entrant.provider_model,
        reasoning_policy: entrant.reasoning_policy,
      };
    }),
    plan.entrants.map((entrant) => entrant.model_id),
  );
  if (!/^weekly-[A-Za-z0-9_.:-]+$/.test(plan.season_id)) {
    throw new Error('weekly arena plan has an invalid season ID');
  }
  if (!Number.isSafeInteger(plan.team_size) || plan.team_size < 2 || plan.team_size > 20) {
    throw new Error('weekly arena plan has an invalid team size');
  }
  if (JSON.stringify(plan.modes) !== JSON.stringify(['arena', 'ctf', 'koth', 'tdm'])) {
    throw new Error('weekly arena plan must contain the four competition modes');
  }
  if (plan.strategy_weights?.duel !== 0.75
      || plan.strategy_weights?.world !== 0.25
      || plan.world_squad_size !== 3
      || plan.world_max_ticks !== 600) {
    throw new Error('weekly arena plan has an invalid world strategy contract');
  }
  return {
    schema_version: 1,
    week_id: weekId,
    status: 'candidate',
    season_id: plan.season_id,
    created_at: nowIso(),
    updated_at: nowIso(),
    ranking_retrieved_at: plan.ranking.retrieved_at,
    roster_sha256: sha256(plan.ranking.models.map((model) => model.id).join('\n')),
    candidate_ranking_sha256: null,
    entrant_model_ids: plan.entrants.map((entrant) => entrant.model_id),
    reasoning_policies: reasoningPolicies,
    team_size: plan.team_size,
    modes: plan.modes,
    rating_weights: plan.rating_weights,
    strategy_weights: plan.strategy_weights,
    world_squad_size: plan.world_squad_size,
    world_max_ticks: plan.world_max_ticks,
    generation: { completed: false, completed_at: null },
    seed_pack_size: null,
    points_by_rank: [...DEFAULT_POINTS_BY_RANK],
    epochs: [],
    consecutive_failures: 0,
    last_error: null,
    last_failure_at: null,
    next_retry_at: null,
  };
}

export function validateState(state, weekId, seedPackSize) {
  if (state?.schema_version !== 1 || state.week_id !== weekId) {
    throw new Error(`invalid weekly supervisor state for ${weekId}`);
  }
  if (typeof state.season_id !== 'string' || !state.season_id) {
    throw new Error('weekly supervisor state is missing its season ID');
  }
  if (!/^[a-f0-9]{64}$/.test(String(state.candidate_ranking_sha256 || ''))) {
    throw new Error('weekly supervisor state is missing its candidate ranking digest');
  }
  if (!Array.isArray(state.epochs) || !Array.isArray(state.points_by_rank)) {
    throw new Error('weekly supervisor state has an invalid epoch ledger');
  }
  if (state.points_by_rank.length !== 10
      || state.points_by_rank.some((points) => !Number.isSafeInteger(points) || points < 0)) {
    throw new Error('weekly supervisor points table must contain ten ranks');
  }
  if (!Array.isArray(state.entrant_model_ids)
      || state.entrant_model_ids.length !== 10
      || new Set(state.entrant_model_ids).size !== 10) {
    throw new Error('weekly supervisor state has an invalid entrant roster');
  }
  const reasoningPolicies = validateReasoningPolicyProvenance(
    state.reasoning_policies,
    state.entrant_model_ids,
  );
  if (JSON.stringify(reasoningPolicies) !== JSON.stringify(state.reasoning_policies)) {
    throw new Error('weekly supervisor state has non-canonical reasoning policies');
  }
  if (!Number.isSafeInteger(state.team_size) || state.team_size < 2 || state.team_size > 20) {
    throw new Error('weekly supervisor state has an invalid team size');
  }
  if (JSON.stringify(state.modes) !== JSON.stringify(['arena', 'ctf', 'koth', 'tdm'])) {
    throw new Error('weekly supervisor state has invalid competition modes');
  }
  const weights = state.rating_weights;
  const ratingWeights = [weights?.personal, weights?.team, weights?.collaboration].map(Number);
  if (ratingWeights.some((weight) => !Number.isFinite(weight) || weight < 0 || weight > 1)
      || Math.abs(ratingWeights.reduce((sum, weight) => sum + weight, 0) - 1) > 0.000001) {
    throw new Error('weekly supervisor state has invalid rating weights');
  }
  const strategyWeights = [
    state.strategy_weights?.duel,
    state.strategy_weights?.world,
  ].map(Number);
  if (strategyWeights[0] !== 0.75
      || strategyWeights[1] !== 0.25
      || state.world_squad_size !== 3
      || state.world_max_ticks !== 600) {
    throw new Error('weekly supervisor state has an invalid world strategy contract');
  }
  const bindingMetadataFields = [
    'artifact_binding_version',
    'artifact_binding_started_at',
    'ledger_generation',
  ];
  const hasBindingMetadata = bindingMetadataFields.some((field) => (
    Object.prototype.hasOwnProperty.call(state, field)
  ));
  if (hasBindingMetadata
      && (bindingMetadataFields.some((field) => (
        !Object.prototype.hasOwnProperty.call(state, field)
      ))
        || state.artifact_binding_version !== ARTIFACT_BINDING_VERSION
        || !isIsoTimestamp(state.artifact_binding_started_at)
        || !Number.isSafeInteger(state.ledger_generation)
        || state.ledger_generation < 1)) {
    throw new Error('weekly supervisor state has invalid artifact binding metadata');
  }
  if (state.ledger_generation >= 2
      && (!/^[a-f0-9]{64}$/.test(String(state.artifact_binding_manifest_sha256 || ''))
        || !isNonEmptyRelativePath(state.artifact_binding_manifest_path))) {
    throw new Error('weekly supervisor state has invalid artifact binding manifest metadata');
  }
  if (state.legacy_history != null) {
    if (!Array.isArray(state.legacy_history) || state.legacy_history.length > 16) {
      throw new Error('weekly supervisor state has invalid legacy history');
    }
    for (const history of state.legacy_history) {
      if (history?.schema_version !== 1
          || history.kind !== LEGACY_ARCHIVE_KIND
          || history.season_id !== state.season_id
          || history.wasm_sha256_bound !== false
          || !Number.isSafeInteger(history.epoch_count)
          || history.epoch_count < 1
          || !/^[a-f0-9]{64}$/.test(String(history.ledger_sha256 || ''))
          || !isIsoTimestamp(history.archived_at)
          || !isNonEmptyRelativePath(history.state_path)
          || !isNonEmptyRelativePath(history.epochs_path)
          || (history.ratings_path != null && !isNonEmptyRelativePath(history.ratings_path))
          || (history.season_path != null && !isNonEmptyRelativePath(history.season_path))) {
        throw new Error('weekly supervisor state has invalid legacy history');
      }
    }
  }
  if (state.generation?.completed === true) {
    if (!/^[a-f0-9]{64}$/.test(String(state.ranking_sha256 || ''))
        || state.ranking_sha256 !== state.candidate_ranking_sha256) {
      throw new Error('weekly supervisor completed state has an invalid ranking digest');
    }
    validateArenaContract(
      state.arena_contract,
      'weekly supervisor state has an invalid arena contract',
    );
    const artifactBindings = validateArtifactBindings(
      state.artifact_bindings,
      state.entrant_model_ids,
    );
    if (JSON.stringify(artifactBindings) !== JSON.stringify(state.artifact_bindings)) {
      throw new Error('weekly supervisor state has non-canonical artifact bindings');
    }
  }
  if (state.revision != null) {
    const entries = Array.isArray(state.revision.entries) ? state.revision.entries : [];
    if (state.revision.completed !== true
        || !Number.isSafeInteger(state.revision.epoch_index)
        || state.revision.epoch_index < 1
        || !isIsoTimestamp(state.revision.completed_at)
        || entries.length !== 10
        || new Set(entries.map((entry) => entry?.model_id)).size !== 10
        || entries.some((entry) => (
          !state.entrant_model_ids.includes(entry?.model_id)
          || (entry.status !== 'improved' && entry.status !== 'kept_gen1')
          || (entry.status === 'improved' && (
            !Number.isSafeInteger(Number(entry.wasm_bytes_after))
            || !/^[a-f0-9]{64}$/.test(String(entry.wasm_sha256_after || ''))
          ))
        ))) {
      throw new Error('weekly supervisor state has an invalid revision record');
    }
  }
  if (state.seed_pack_size !== seedPackSize) {
    throw new Error(
      `seed pack size is frozen at ${state.seed_pack_size} for ${weekId}; configured ${seedPackSize}`,
    );
  }
  state.epochs.forEach((epoch, index) => {
    const expectedSeeds = deterministicSeedPack(weekId, index, seedPackSize);
    const standings = Array.isArray(epoch.standings) ? epoch.standings : [];
    const standingModels = standings.map((standing) => standing.model_id);
    const standingRanks = standings.map((standing) => standing.epoch_rank);
    if (epoch.index !== index
        || epoch.epoch_id !== `${weekId}-E${String(index + 1).padStart(6, '0')}`
        || !Array.isArray(epoch.seeds)
        || JSON.stringify(epoch.seeds) !== JSON.stringify(expectedSeeds)
        || epoch.battle_requests !== 45 * 4 * seedPackSize * 2
        || epoch.total_engagements
          !== (90 * seedPackSize) + (270 * seedPackSize * state.team_size)
        || epoch.world_requests !== seedPackSize
        || epoch.world_fighter_rounds
          !== seedPackSize * state.entrant_model_ids.length * state.world_squad_size
        || !/^[a-f0-9]{64}$/.test(String(epoch.artifact_sha256 || ''))
        || standings.length !== 10
        || new Set(standingModels).size !== 10
        || new Set(standingRanks).size !== 10
        || [...standingModels].sort().join('\n')
          !== [...state.entrant_model_ids].sort().join('\n')
        || standings.some((standing) => (
          !Number.isSafeInteger(standing.epoch_rank)
          || standing.epoch_rank < 1
          || standing.epoch_rank > 10
          || standing.points_awarded !== state.points_by_rank[standing.epoch_rank - 1]
          || !Number.isFinite(standing.overall_rating)
          || standing.overall_rating < 0
          || standing.overall_rating > 100
          || !Number.isFinite(standing.world_rating)
          || standing.world_rating < 0
          || standing.world_rating > 100
          || !Number.isFinite(standing.strategy_rating)
          || standing.strategy_rating < 0
          || standing.strategy_rating > 100
        ))) {
      throw new Error(`weekly epoch ledger is non-contiguous at index ${index}`);
    }
  });
  return state;
}

function isNonEmptyRelativePath(value) {
  return typeof value === 'string'
    && value.length > 0
    && !path.isAbsolute(value)
    && !value.split('/').includes('..');
}

/**
 * Validate the only pre-binding state shape eligible for the one-time
 * migration. Supplying placeholder digests lets the strict validator audit
 * every other field and the complete legacy epoch ledger without weakening
 * normal state validation.
 */
export function validateLegacyUnboundState(state, weekId, seedPackSize) {
  if (state?.generation?.completed !== true
      || state.status !== 'active'
      || Object.prototype.hasOwnProperty.call(state, 'artifact_bindings')
      || Object.prototype.hasOwnProperty.call(state, 'artifact_binding_version')
      || Object.prototype.hasOwnProperty.call(state, 'artifact_binding_started_at')
      || Object.prototype.hasOwnProperty.call(state, 'ledger_generation')
      || Object.prototype.hasOwnProperty.call(state, 'artifact_binding_manifest_sha256')
      || Object.prototype.hasOwnProperty.call(state, 'artifact_binding_manifest_path')
      || state.legacy_history != null
      || !Array.isArray(state.epochs)
      || state.epochs.length < 1) {
    throw new Error('weekly supervisor state is not an eligible legacy unbound season');
  }
  const placeholderBindings = state.entrant_model_ids?.map((modelId) => ({
    model_id: modelId,
    wasm_bytes: 1,
    wasm_sha256: '0'.repeat(64),
  }));
  validateState({ ...state, artifact_bindings: placeholderBindings }, weekId, seedPackSize);
  return state;
}

async function createWeekState({ weekId, weekDirectory, seedPackSize, redact }) {
  const { stdout } = await runRunner(['--dry-run'], { capture: true, redact });
  let plan;
  try {
    plan = JSON.parse(stdout);
  } catch {
    throw new Error('season dry-run returned malformed JSON');
  }
  const state = validatePlan(plan, weekId);
  state.seed_pack_size = seedPackSize;
  const candidateRankingPath = path.join(weekDirectory, 'candidate-ranking.json');
  await atomicWriteJson(candidateRankingPath, plan.ranking);
  state.candidate_ranking_sha256 = sha256(await fs.readFile(candidateRankingPath));
  validateState(state, weekId, seedPackSize);
  await atomicWriteJson(path.join(weekDirectory, 'candidate-plan.json'), plan);
  await atomicWriteJson(path.join(weekDirectory, 'state.json'), state);
  return state;
}

async function loadOrCreateWeekState(config, weekId, redact) {
  const weekDirectory = path.join(config.stateDirectory, weekId);
  const statePath = path.join(weekDirectory, 'state.json');
  await fs.mkdir(weekDirectory, { recursive: true });
  if (await fileExists(statePath)) {
    const persisted = await readJson(statePath);
    const state = persisted?.generation?.completed === true
      && !Object.prototype.hasOwnProperty.call(persisted, 'artifact_bindings')
      ? validateLegacyUnboundState(persisted, weekId, config.seedPackSize)
      : validateState(persisted, weekId, config.seedPackSize);
    return {
      weekDirectory,
      statePath,
      state,
    };
  }
  const state = await createWeekState({
    weekId,
    weekDirectory,
    seedPackSize: config.seedPackSize,
    redact,
  });
  return { weekDirectory, statePath, state };
}

export async function rankingPathFor(weekDirectory, state) {
  const frozen = path.join(weekDirectory, 'ranking.json');
  if (await fileExists(frozen)) {
    const expectedDigest = state?.generation?.completed === true
      ? state.ranking_sha256
      : state?.candidate_ranking_sha256;
    const digest = sha256(await fs.readFile(frozen));
    if (!expectedDigest || digest !== expectedDigest) {
      throw new Error('frozen weekly ranking hash mismatch');
    }
    return frozen;
  }
  if (state?.generation?.completed === true) throw new Error('frozen weekly ranking is missing');
  const candidate = path.join(weekDirectory, 'candidate-ranking.json');
  if (await fileExists(candidate)) {
    const digest = sha256(await fs.readFile(candidate));
    if (!state?.candidate_ranking_sha256 || digest !== state.candidate_ranking_sha256) {
      throw new Error('candidate weekly ranking hash mismatch');
    }
    return candidate;
  }
  throw new Error('weekly ranking checkpoint is missing');
}

export async function readCompleteArtifactBindings(generationDirectory, modelIds) {
  const bindings = [];
  for (const modelId of modelIds) {
    const checkpoint = await readJson(path.join(generationDirectory, `${modelId}.json`));
    if (checkpoint?.schema_version !== 2
        || checkpoint.stage !== 'compiled'
        || checkpoint.model_id !== modelId
        || checkpoint.compiled !== true) {
      throw new Error(`generation artifact checkpoint is incomplete for ${modelId}`);
    }
    bindings.push({
      model_id: modelId,
      wasm_bytes: checkpoint.wasm_bytes,
      wasm_sha256: checkpoint.wasm_sha256,
    });
  }
  return validateArtifactBindings(bindings, modelIds);
}

function relativePathFrom(rootDirectory, targetPath) {
  return path.relative(rootDirectory, targetPath).split(path.sep).join('/');
}

async function verifyLegacyEpochArchives(
  state,
  boundState,
  weekDirectory,
  epochDirectory,
  rootDirectory,
) {
  const bindings = new Map(
    boundState.artifact_bindings.map((binding) => [binding.model_id, binding]),
  );
  const legacySnapshots = [];
  const validationSnapshots = [];
  for (const [index, epoch] of state.epochs.entries()) {
    const originalArchivePath = epochArchivePath(weekDirectory, index);
    const archivePath = path.join(
      epochDirectory,
      `epoch-${String(index).padStart(6, '0')}.json`,
    );
    if (epoch.artifact_path !== relativePathFrom(rootDirectory, originalArchivePath)) {
      throw new Error(`legacy epoch ${index + 1} path is not canonical`);
    }
    const raw = await fs.readFile(archivePath);
    if (sha256(raw) !== epoch.artifact_sha256) {
      throw new Error(`legacy epoch ${index + 1} archive hash mismatch`);
    }
    const snapshot = JSON.parse(raw.toString('utf8'));
    if (!Array.isArray(snapshot.roster)
        || snapshot.roster.some((entry) => (
          Object.prototype.hasOwnProperty.call(entry, 'wasm_sha256')
        ))) {
      throw new Error(`legacy epoch ${index + 1} is not an unbound WASM snapshot`);
    }
    const validationSnapshot = {
      ...snapshot,
      roster: snapshot.roster.map((entry) => ({
        ...entry,
        wasm_sha256: bindings.get(entry.model_id)?.wasm_sha256,
      })),
    };
    validateEpochSnapshot(validationSnapshot, boundState, epoch.seeds);
    legacySnapshots.push(snapshot);
    validationSnapshots.push(validationSnapshot);
  }
  return { legacySnapshots, validationSnapshots };
}

function stableJson(value) {
  if (Array.isArray(value)) return `[${value.map(stableJson).join(',')}]`;
  if (value && typeof value === 'object') {
    return `{${Object.keys(value).sort().map((key) => (
      `${JSON.stringify(key)}:${stableJson(value[key])}`
    )).join(',')}}`;
  }
  return JSON.stringify(value);
}

async function validateLegacySeasonPublication(sourcePath, legacySnapshots) {
  if (!(await fileExists(sourcePath))) return false;
  const published = await readJson(sourcePath);
  const expected = legacySnapshots.at(-1);
  if (!expected || stableJson(published) !== stableJson(expected)) {
    throw new Error('current season artifact does not match the final legacy epoch');
  }
  return true;
}

async function validateLegacyRatingsPublication(
  sourcePath,
  state,
  validationSnapshots,
  ledgerSha256,
) {
  if (!(await fileExists(sourcePath))) return false;
  const published = await readJson(sourcePath);
  const latest = validationSnapshots.at(-1);
  const expectedRoster = cumulativeRoster(validationSnapshots, state);
  if (published?.schema_version !== 1
      || published.active !== true
      || published.season_id !== state.season_id
      || published.league?.format !== 'weekly_continuous_v1'
      || published.league?.week_id !== state.week_id
      || (Object.prototype.hasOwnProperty.call(
        published.league || {},
        'ledger_generation',
      ) && published.league.ledger_generation !== 1)
      || published.league?.epochs_completed !== state.epochs.length
      || published.league?.ledger_sha256 !== ledgerSha256
      || !Array.isArray(published.roster)
      || published.roster.length !== expectedRoster.length
      || published.roster.some((entry) => (
        Object.prototype.hasOwnProperty.call(entry, 'wasm_sha256')
      ))) {
    throw new Error('current ratings artifact is not the legacy ledger publication');
  }
  const expectedIntegrity = {
    battle_requests: validationSnapshots.reduce(
      (sum, snapshot) => sum + snapshot.integrity.battle_requests,
      0,
    ),
    total_engagements: validationSnapshots.reduce(
      (sum, snapshot) => sum + snapshot.integrity.total_engagements,
      0,
    ),
    world_requests: validationSnapshots.reduce(
      (sum, snapshot) => sum + snapshot.integrity.world_requests,
      0,
    ),
    world_fighter_rounds: validationSnapshots.reduce(
      (sum, snapshot) => sum + snapshot.integrity.world_fighter_rounds,
      0,
    ),
  };
  if (Object.entries(expectedIntegrity).some(([field, value]) => (
    published.integrity?.[field] !== value
  )) || published.integrity?.epochs_completed !== validationSnapshots.length) {
    throw new Error('current ratings artifact totals differ from the legacy ledger');
  }
  for (const field of [
    'prompt_sha256',
    'prompt_version',
    'max_completion_tokens',
    'provider_sort_policy',
    'temperature_policy',
    'reasoning_policy_version',
    'provider_require_parameters',
    'reasoning_exclude',
    'reasoning_policies',
    'response_transport_policy',
    'source_limit_bytes',
    'collaboration_abi_version',
    'simulator_rules_version',
    'team_size',
    'modes',
    'personal_weight',
    'team_weight',
    'collaboration_weight',
    'duel_strategy_weight',
    'world_strategy_weight',
    'world_squad_size',
    'world_max_ticks',
  ]) {
    if (stableJson(published.methodology?.[field]) !== stableJson(latest.methodology?.[field])) {
      throw new Error('current ratings artifact contract differs from the legacy ledger');
    }
  }
  const publishedByModel = new Map(published.roster.map((entry) => [entry.model_id, entry]));
  for (const expected of expectedRoster) {
    const actual = publishedByModel.get(expected.model_id);
    if (!actual) throw new Error('current ratings artifact roster differs from the legacy ledger');
    for (const [field, value] of Object.entries(expected)) {
      if (field === 'wasm_sha256') continue;
      if (stableJson(actual[field]) !== stableJson(value)) {
        throw new Error(`current ratings artifact differs for ${expected.model_id}`);
      }
    }
  }
  return true;
}

async function archiveLegacyStateOnce(targetPath, state, ledgerSha256) {
  if (await fileExists(targetPath)) {
    const archived = await readJson(targetPath);
    if (archived?.season_id !== state.season_id
        || sha256(JSON.stringify(archived.epochs)) !== ledgerSha256) {
      throw new Error('legacy state archive conflicts with the season being migrated');
    }
    return;
  }
  await atomicWriteJson(targetPath, state);
}

async function archiveFileOnce(sourcePath, targetPath) {
  const sourceExists = await fileExists(sourcePath);
  const targetExists = await fileExists(targetPath);
  if (!sourceExists) {
    if (targetExists) {
      throw new Error(`legacy archive source is missing for existing '${targetPath}'`);
    }
    return false;
  }
  const sourceBytes = await fs.readFile(sourcePath);
  if (targetExists) {
    const targetBytes = await fs.readFile(targetPath);
    if (sha256(sourceBytes) !== sha256(targetBytes)) {
      throw new Error(`legacy archive conflicts with current source '${sourcePath}'`);
    }
    return true;
  }
  await atomicWriteBytes(targetPath, sourceBytes);
  return true;
}

export async function validateCommittedArtifactBinding({
  state,
  weekDirectory,
  rootDirectory = ROOT_DIR,
  artifactBindingReader = readArtifactBinding,
  rankingLoader = loadRanking,
  entrantBuilder = entrantsFromRanking,
  expectedBinding = null,
}) {
  const frozenRankingPath = await rankingPathFor(weekDirectory, state);
  const seasonDirectory = path.join(
    rootDirectory,
    'artifacts/arena/seasons',
    state.season_id,
  );
  const serverStatus = await readJson(path.join(seasonDirectory, 'server-status.json'));
  const codeStatus = normalizeCodeStatus(serverStatus);
  const serverContract = arenaContractFromServerStatus(serverStatus);
  if (Object.entries(serverContract).some(([field, value]) => (
    state.arena_contract?.[field] !== value
  ))) {
    throw new Error('artifact binding server contract differs from the frozen generation');
  }
  const rankingSha256 = sha256(await fs.readFile(frozenRankingPath));
  if (rankingSha256 !== state.ranking_sha256) {
    throw new Error('artifact binding ranking differs from the frozen weekly ranking');
  }
  const ranking = await rankingLoader({ rankingFile: frozenRankingPath, topModels: 10 });
  const entrants = entrantBuilder(ranking, state.season_id);
  if (JSON.stringify(entrants.map((entrant) => entrant.model_id))
      !== JSON.stringify(state.entrant_model_ids)) {
    throw new Error('artifact binding entrant IDs differ from the frozen weekly roster');
  }
  const binding = await artifactBindingReader({
    seasonDirectory,
    seasonId: state.season_id,
    rankingSha256,
    entrants,
    codeStatus,
    required: true,
  });
  const bindingPath = relativePathFrom(rootDirectory, binding.bindingPath);
  const expectedPath = expectedBinding?.bindingPath
    ?? state.artifact_binding_manifest_path;
  const expectedSha256 = expectedBinding?.manifestSha256
    ?? state.artifact_binding_manifest_sha256;
  const expectedBindings = expectedBinding?.bindings ?? state.artifact_bindings;
  if ((expectedPath != null && bindingPath !== expectedPath)
      || (expectedSha256 != null && binding.manifestSha256 !== expectedSha256)
      || (expectedBindings != null
        && JSON.stringify(binding.bindings) !== JSON.stringify(expectedBindings))) {
    throw new Error('committed artifact binding changed after validation');
  }
  return { ...binding, relativeBindingPath: bindingPath };
}

/**
 * One-time migration for a season created before WASM SHA-256 publication.
 * The child runner is explicitly rehydrate-only: it can register and compile
 * locally, but cannot generate, evaluate, or publish. State is not committed
 * until every checkpoint and every legacy epoch has passed verification.
 */
export async function migrateLegacyUnboundState({
  state,
  statePath,
  weekDirectory,
  publishPath = DEFAULT_PUBLISH_PATH,
  rootDirectory = ROOT_DIR,
  redact = (value) => String(value ?? ''),
  runner = runRunner,
  timestamp = nowIso,
  artifactBindingReader = readArtifactBinding,
  rankingLoader = loadRanking,
  entrantBuilder = entrantsFromRanking,
}) {
  validateLegacyUnboundState(state, state.week_id, state.seed_pack_size);
  const frozenRankingPath = await rankingPathFor(weekDirectory, state);
  await runner([
    '--ranking-file', frozenRankingPath,
    '--season-id', state.season_id,
    '--rehydrate-only',
  ], {
    env: { ARENA_TEAM_SIZE: String(state.team_size) },
    redact,
  });

  const seasonDirectory = path.join(
    rootDirectory,
    'artifacts/arena/seasons',
    state.season_id,
  );
  const binding = await validateCommittedArtifactBinding({
    state,
    weekDirectory,
    rootDirectory,
    artifactBindingReader,
    rankingLoader,
    entrantBuilder,
  });
  const artifactBindings = binding.bindings;
  const bindingManifestPath = binding.relativeBindingPath;
  const bindingStartedAt = binding.manifest?.created_at || timestamp();
  const validationState = {
    ...state,
    artifact_bindings: artifactBindings,
    artifact_binding_version: ARTIFACT_BINDING_VERSION,
    artifact_binding_started_at: bindingStartedAt,
    artifact_binding_manifest_path: bindingManifestPath,
    artifact_binding_manifest_sha256: binding.manifestSha256,
    ledger_generation: 2,
  };
  validateState(validationState, state.week_id, state.seed_pack_size);
  const ledgerSha256 = sha256(JSON.stringify(state.epochs));
  const legacyDirectory = path.join(
    weekDirectory,
    'legacy',
    `${LEGACY_ARCHIVE_KIND}-${ledgerSha256.slice(0, 12)}`,
  );
  const legacyStatePath = path.join(legacyDirectory, 'state.json');
  const legacyEpochsPath = path.join(legacyDirectory, 'epochs');
  const currentEpochsPath = path.join(weekDirectory, 'epochs');
  const currentEpochsExist = await fileExists(currentEpochsPath);
  const legacyEpochsExist = await fileExists(legacyEpochsPath);
  if (currentEpochsExist && legacyEpochsExist) {
    throw new Error('both current and legacy epoch directories exist; refusing ambiguous migration');
  }
  if (!currentEpochsExist && !legacyEpochsExist) {
    throw new Error('legacy epoch directory is missing');
  }
  const verifiedHistory = await verifyLegacyEpochArchives(
    state,
    validationState,
    weekDirectory,
    currentEpochsExist ? currentEpochsPath : legacyEpochsPath,
    rootDirectory,
  );

  const archivedAt = timestamp();
  const seasonSourcePath = path.join(seasonDirectory, 'season.json');
  const ratingsPresent = await validateLegacyRatingsPublication(
    publishPath,
    state,
    verifiedHistory.validationSnapshots,
    ledgerSha256,
  );
  const seasonPresent = await validateLegacySeasonPublication(
    seasonSourcePath,
    verifiedHistory.legacySnapshots,
  );
  await fs.mkdir(legacyDirectory, { recursive: true });
  await archiveLegacyStateOnce(legacyStatePath, state, ledgerSha256);
  const ratingsArchived = ratingsPresent && await archiveFileOnce(
    publishPath, path.join(legacyDirectory, 'ratings.json'),
  );
  const seasonArchived = seasonPresent && await archiveFileOnce(
    seasonSourcePath, path.join(legacyDirectory, 'season.json'),
  );

  if (currentEpochsExist) {
    await fs.rename(currentEpochsPath, legacyEpochsPath);
  }

  const history = {
    schema_version: 1,
    kind: LEGACY_ARCHIVE_KIND,
    season_id: state.season_id,
    epoch_count: state.epochs.length,
    ledger_sha256: ledgerSha256,
    wasm_sha256_bound: false,
    archived_at: archivedAt,
    state_path: relativePathFrom(rootDirectory, legacyStatePath),
    epochs_path: relativePathFrom(rootDirectory, legacyEpochsPath),
    ...(ratingsArchived
      ? { ratings_path: relativePathFrom(rootDirectory, path.join(legacyDirectory, 'ratings.json')) }
      : {}),
    ...(seasonArchived
      ? { season_path: relativePathFrom(rootDirectory, path.join(legacyDirectory, 'season.json')) }
      : {}),
  };
  const migrated = {
    ...state,
    artifact_bindings: artifactBindings,
    artifact_binding_version: ARTIFACT_BINDING_VERSION,
    artifact_binding_started_at: bindingStartedAt,
    artifact_binding_manifest_path: bindingManifestPath,
    artifact_binding_manifest_sha256: binding.manifestSha256,
    ledger_generation: 2,
    epochs: [],
    legacy_history: [history],
    updated_at: archivedAt,
    consecutive_failures: 0,
    last_error: null,
    last_failure_at: null,
    next_retry_at: null,
  };
  validateState(migrated, state.week_id, state.seed_pack_size);
  await validateCommittedArtifactBinding({
    state,
    weekDirectory,
    rootDirectory,
    artifactBindingReader,
    rankingLoader,
    entrantBuilder,
    expectedBinding: {
      bindingPath: bindingManifestPath,
      manifestSha256: binding.manifestSha256,
      bindings: artifactBindings,
    },
  });
  await atomicWriteJson(statePath, migrated);
  process.stdout.write(
    `[arena-weekly] archived ${history.epoch_count} unbound epochs and started digest-bound ledger generation 2\n`,
  );
  return migrated;
}

async function ensureGeneration({
  state,
  statePath,
  weekDirectory,
  redact,
  publishPath = DEFAULT_PUBLISH_PATH,
  rootDirectory = ROOT_DIR,
  artifactBindingReader = readArtifactBinding,
  rankingLoader = loadRanking,
  entrantBuilder = entrantsFromRanking,
}) {
  if (state.generation?.completed === true) {
    if (!Object.prototype.hasOwnProperty.call(state, 'artifact_bindings')) {
      return migrateLegacyUnboundState({
        state,
        statePath,
        weekDirectory,
        publishPath,
        redact,
        rootDirectory,
        artifactBindingReader,
        rankingLoader,
        entrantBuilder,
      });
    }
    if (state.ledger_generation >= 2) {
      await validateCommittedArtifactBinding({
        state,
        weekDirectory,
        rootDirectory,
        artifactBindingReader,
        rankingLoader,
        entrantBuilder,
      });
    }
    return state;
  }
  const rankingPath = await rankingPathFor(weekDirectory, state);
  process.stdout.write(`[arena-weekly] generating frozen roster for ${state.week_id}\n`);
  await runRunner([
    '--ranking-file', rankingPath,
    '--season-id', state.season_id,
    '--generate-only',
  ], { env: { ARENA_TEAM_SIZE: String(state.team_size) }, redact });

  const frozenPath = path.join(weekDirectory, 'ranking.json');
  if (!(await fileExists(frozenPath))) {
    await fs.rename(rankingPath, frozenPath);
  }
  const frozenRankingBytes = await fs.readFile(frozenPath);
  const frozenRankingSha256 = sha256(frozenRankingBytes);
  if (frozenRankingSha256 !== state.candidate_ranking_sha256) {
    throw new Error('frozen weekly ranking hash differs from the pre-generation candidate');
  }
  const serverStatusPath = path.join(
    ROOT_DIR,
    'artifacts/arena/seasons',
    state.season_id,
    'server-status.json',
  );
  const serverStatus = await readJson(serverStatusPath);
  const arenaContract = arenaContractFromServerStatus(serverStatus);
  const generationDirectory = path.join(
    ROOT_DIR,
    'artifacts/arena/seasons',
    state.season_id,
    'generations',
  );
  const artifactBindings = await Promise.all(state.entrant_model_ids.map(async (modelId) => {
    const checkpoint = await readJson(path.join(generationDirectory, `${modelId}.json`));
    if (checkpoint?.model_id !== modelId || checkpoint?.compiled !== true) {
      throw new Error(`generation artifact checkpoint is incomplete for ${modelId}`);
    }
    return {
      model_id: modelId,
      wasm_bytes: checkpoint.wasm_bytes,
      wasm_sha256: checkpoint.wasm_sha256,
    };
  }));
  validateArtifactBindings(artifactBindings, state.entrant_model_ids);
  const frozenAt = nowIso();
  const nextState = {
    ...state,
    status: 'active',
    frozen_at: frozenAt,
    ranking_sha256: frozenRankingSha256,
    arena_contract: arenaContract,
    artifact_bindings: artifactBindings,
    artifact_binding_version: ARTIFACT_BINDING_VERSION,
    artifact_binding_started_at: frozenAt,
    ledger_generation: 1,
    generation: { completed: true, completed_at: nowIso() },
    updated_at: nowIso(),
    consecutive_failures: 0,
    last_error: null,
    next_retry_at: null,
  };
  validateState(nextState, state.week_id, state.seed_pack_size);
  await atomicWriteJson(statePath, nextState);
  process.stdout.write(`[arena-weekly] froze ${state.week_id} roster after successful generation\n`);
  return nextState;
}

/**
 * The artifact one model is expected to carry in a given epoch. Frozen
 * bindings govern every epoch before the recorded mid-season revision; from
 * the revision boundary onward, models marked `improved` are expected to
 * carry their revised artifact. Anything else is tampering.
 */
function expectedArtifactForEpoch(state, modelId, epochIndex) {
  const frozen = (state.artifact_bindings || []).find((binding) => binding.model_id === modelId);
  const revision = state.revision;
  if (revision?.completed === true
      && Number.isSafeInteger(revision.epoch_index)
      && Number.isSafeInteger(epochIndex)
      && epochIndex >= revision.epoch_index) {
    const entry = (revision.entries || []).find((candidate) => candidate.model_id === modelId);
    if (entry?.status === 'improved'
        && Number.isSafeInteger(Number(entry.wasm_bytes_after))
        && /^[a-f0-9]{64}$/.test(String(entry.wasm_sha256_after || ''))) {
      return { wasm_bytes: Number(entry.wasm_bytes_after), wasm_sha256: entry.wasm_sha256_after };
    }
  }
  return frozen;
}

export function validateEpochSnapshot(snapshot, state, seeds, epochIndex = null) {
  if (snapshot?.schema_version !== 1 || snapshot.active !== true) {
    throw new Error('epoch artifact is not an active schema-v1 rating snapshot');
  }
  if (snapshot.season_id !== state.season_id) throw new Error('epoch season ID mismatch');
  if (!Array.isArray(snapshot.roster) || snapshot.roster.length !== 10) {
    throw new Error('epoch roster must contain ten models');
  }
  const snapshotSeeds = snapshot.methodology?.seed_sets;
  if (!Array.isArray(snapshotSeeds) || JSON.stringify(snapshotSeeds) !== JSON.stringify(seeds)) {
    throw new Error('epoch artifact seed pack mismatch');
  }
  if (snapshot.methodology?.side_swapped !== true || snapshot.integrity?.verified !== true) {
    throw new Error('epoch artifact did not verify balanced side swaps');
  }
  const contract = state.arena_contract;
  validateArenaContract(contract, 'epoch frozen generation contract is invalid');
  if (snapshot.methodology?.prompt_sha256 !== contract.prompt_sha256
      || snapshot.methodology?.prompt_version !== contract.prompt_version
      || snapshot.methodology?.max_completion_tokens !== contract.max_completion_tokens
      || snapshot.methodology?.provider_sort_policy !== contract.provider_sort_policy
      || snapshot.methodology?.temperature_policy !== contract.temperature_policy
      || snapshot.methodology?.reasoning_policy_version !== contract.reasoning_policy_version
      || snapshot.methodology?.provider_require_parameters
        !== contract.provider_require_parameters
      || snapshot.methodology?.reasoning_exclude !== contract.reasoning_exclude
      || JSON.stringify(snapshot.methodology?.reasoning_policies)
        !== JSON.stringify(state.reasoning_policies)
      || snapshot.methodology?.response_transport_policy !== contract.response_transport_policy
      || snapshot.methodology?.source_limit_bytes !== contract.source_limit_bytes
      || snapshot.methodology?.collaboration_abi_version
        !== contract.collaboration_abi_version
      || snapshot.methodology?.simulator_rules_version !== contract.simulator_rules_version
      || snapshot.integrity?.simulator_rules_version !== contract.simulator_rules_version) {
    throw new Error('epoch arena prompt/source contract differs from the frozen generation');
  }
  if (snapshot.methodology?.team_size !== state.team_size
      || JSON.stringify(snapshot.methodology?.modes) !== JSON.stringify(state.modes)) {
    throw new Error('epoch team size or competition modes differ from the frozen season');
  }
  const weights = state.rating_weights;
  if (!weights
      || snapshot.methodology?.personal_weight !== weights.personal
      || snapshot.methodology?.team_weight !== weights.team
      || snapshot.methodology?.collaboration_weight !== weights.collaboration) {
    throw new Error('epoch rating weights differ from the frozen season');
  }
  if (snapshot.methodology?.duel_strategy_weight !== state.strategy_weights.duel
      || snapshot.methodology?.world_strategy_weight !== state.strategy_weights.world
      || snapshot.methodology?.world_squad_size !== state.world_squad_size
      || snapshot.methodology?.world_max_ticks !== state.world_max_ticks) {
    throw new Error('epoch world strategy contract differs from the frozen season');
  }
  const expectedBattleRequests = 45 * 4 * seeds.length * 2;
  if (snapshot.integrity?.battle_requests !== expectedBattleRequests) {
    throw new Error(
      `incomplete epoch: expected ${expectedBattleRequests} battles, got ${snapshot.integrity?.battle_requests}`,
    );
  }
  const expectedEngagements = (90 * seeds.length) + (270 * seeds.length * state.team_size);
  if (snapshot.integrity?.total_engagements !== expectedEngagements) {
    throw new Error(
      `incomplete epoch: expected ${expectedEngagements} engagements, got ${snapshot.integrity?.total_engagements}`,
    );
  }
  if (snapshot.integrity?.world_requests !== seeds.length
      || snapshot.integrity?.world_fighter_rounds
        !== seeds.length * state.entrant_model_ids.length * state.world_squad_size) {
    throw new Error('epoch world evaluation is incomplete');
  }
  const modelIds = snapshot.roster.map((entry) => entry.model_id);
  if (new Set(modelIds).size !== 10) throw new Error('epoch roster contains duplicate model IDs');
  if ([...modelIds].sort().join('\n') !== [...state.entrant_model_ids].sort().join('\n')) {
    throw new Error('epoch fighter roster differs from the frozen season');
  }
  const rankedProviderModels = snapshot.ranking?.models?.map((model) => model.id);
  if (!rankedProviderModels
      || sha256(rankedProviderModels.join('\n')) !== state.roster_sha256) {
    throw new Error('epoch provider ranking differs from the frozen season');
  }
  const ranks = snapshot.roster.map((entry) => entry.rank).sort((left, right) => left - right);
  if (ranks.some((rank, index) => rank !== index + 1)) {
    throw new Error('epoch roster ranks must be contiguous from one through ten');
  }
  // Canonicalize the frozen bindings even though per-epoch expectations now
  // come from expectedArtifactForEpoch: malformed bindings must still fail.
  validateArtifactBindings(state.artifact_bindings, state.entrant_model_ids);
  for (const entry of snapshot.roster) {
    const ratings = [
      entry.personal_rating,
      entry.team_rating,
      entry.collaboration_rating,
      entry.overall_rating,
      entry.world_rating,
      entry.strategy_rating,
    ];
    const rawScores = [
      entry.personal_score_for,
      entry.personal_score_against,
      entry.team_objective_for,
      entry.team_objective_against,
      entry.collaboration_score_for,
      entry.collaboration_score_against,
      entry.world_points,
      entry.world_round_wins,
      entry.world_eliminations,
      entry.world_deaths,
      entry.world_collaboration_score,
    ];
    const matchRecord = [entry.wins, entry.losses, entry.draws];
    const expectedOverall = roundRating(
      (entry.personal_rating * weights.personal)
        + (entry.team_rating * weights.team)
        + (entry.collaboration_rating * weights.collaboration),
    );
    const expectedArtifact = expectedArtifactForEpoch(state, entry.model_id, epochIndex);
    if (entry.compiled !== true || entry.simulated !== false
        || ratings.some((rating) => !Number.isFinite(rating) || rating < 0 || rating > 100)
        || Math.abs(entry.overall_rating - expectedOverall) > 0.01
        || Math.abs(
          entry.strategy_rating
            - roundRating((entry.overall_rating * 0.75) + (entry.world_rating * 0.25)),
        ) > 0.01
        || rawScores.some((score) => !Number.isSafeInteger(score) || score < 0)
        || matchRecord.some((value) => !Number.isSafeInteger(value) || value < 0)
        || !Number.isSafeInteger(entry.matches_played)
        || entry.matches_played < 1
        || matchRecord.reduce((sum, value) => sum + value, 0) !== entry.matches_played
        || !Number.isSafeInteger(entry.evaluation_engagements)
        || entry.evaluation_engagements < entry.matches_played
        || !Number.isSafeInteger(entry.source_bytes)
        || entry.source_bytes < 1
        || entry.source_bytes > contract.source_limit_bytes
        || entry.source_limit_bytes !== contract.source_limit_bytes
        || !/^[a-f0-9]{64}$/.test(String(entry.source_sha256 || ''))
        || !Number.isSafeInteger(entry.wasm_bytes)
        || entry.wasm_bytes < 1
        || entry.wasm_bytes > 2 * 1024 * 1024
        || !/^[a-f0-9]{64}$/.test(String(entry.wasm_sha256 || ''))
        || expectedArtifact?.wasm_bytes !== entry.wasm_bytes
        || expectedArtifact?.wasm_sha256 !== entry.wasm_sha256
        || !Number.isSafeInteger(entry.compile_attempts)
        || entry.compile_attempts < 1
        || entry.compile_attempts > 100
        || entry.integrity_status !== 'verified_wasm') {
      throw new Error(`epoch roster integrity failed for ${entry.model_id || 'unknown model'}`);
    }
  }
  return snapshot;
}

function epochArchivePath(weekDirectory, epochIndex) {
  return path.join(weekDirectory, 'epochs', `epoch-${String(epochIndex).padStart(6, '0')}.json`);
}

function epochLedgerEntry(
  snapshot,
  weekId,
  epochIndex,
  seeds,
  archivePath,
  artifactSha256,
  pointsByRank,
) {
  return {
    index: epochIndex,
    epoch_id: `${weekId}-E${String(epochIndex + 1).padStart(6, '0')}`,
    completed_at: snapshot.generated_at,
    seeds,
    battle_requests: snapshot.integrity.battle_requests,
    total_engagements: snapshot.integrity.total_engagements,
    world_requests: snapshot.integrity.world_requests,
    world_fighter_rounds: snapshot.integrity.world_fighter_rounds,
    artifact_path: relativeArtifactPath(archivePath),
    artifact_sha256: artifactSha256,
    standings: snapshot.roster.map((entry) => ({
      model_id: entry.model_id,
      epoch_rank: entry.rank,
      overall_rating: entry.overall_rating,
      world_rating: entry.world_rating,
      strategy_rating: entry.strategy_rating,
      points_awarded: pointsByRank[entry.rank - 1],
    })),
  };
}

async function archiveOrRunEpoch({ config, state, weekDirectory, redact }) {
  const epochIndex = state.epochs.length;
  const seeds = deterministicSeedPack(state.week_id, epochIndex, state.seed_pack_size);
  const archivePath = epochArchivePath(weekDirectory, epochIndex);
  let snapshot;
  let archiveBytes;
  if (await fileExists(archivePath)) {
    archiveBytes = await fs.readFile(archivePath);
    snapshot = validateEpochSnapshot(
      JSON.parse(archiveBytes.toString('utf8')),
      state,
      seeds,
      epochIndex,
    );
    process.stdout.write(`[arena-weekly] recovering completed epoch ${epochIndex + 1}\n`);
  } else {
    process.stdout.write(
      `[arena-weekly] starting balanced epoch ${epochIndex + 1} for ${state.week_id}\n`,
    );
    const rankingPath = await rankingPathFor(weekDirectory, state);
    await runRunner([
      '--ranking-file', rankingPath,
      '--season-id', state.season_id,
      '--evaluate-only',
      '--no-publish',
    ], {
      env: {
        ARENA_SEEDS: seeds.join(','),
        ARENA_TEAM_SIZE: String(state.team_size),
      },
      redact,
    });
    const runnerArtifact = path.join(
      ROOT_DIR,
      'artifacts/arena/seasons',
      state.season_id,
      'season.json',
    );
    const serverStatus = await readJson(path.join(
      ROOT_DIR,
      'artifacts/arena/seasons',
      state.season_id,
      'server-status.json',
    ));
    const currentContract = arenaContractFromServerStatus(serverStatus);
    if (Object.entries(currentContract).some(([field, value]) => (
      state.arena_contract?.[field] !== value
    ))) {
      throw new Error('epoch server competition contract differs from the frozen generation');
    }
    snapshot = validateEpochSnapshot(await readJson(runnerArtifact), state, seeds, epochIndex);
    await atomicWriteJson(archivePath, snapshot);
    archiveBytes = await fs.readFile(archivePath);
  }
  return {
    snapshot,
    ledgerEntry: epochLedgerEntry(
      snapshot,
      state.week_id,
      epochIndex,
      seeds,
      archivePath,
      sha256(archiveBytes),
      state.points_by_rank,
    ),
  };
}

async function loadCommittedSnapshots(state, weekDirectory) {
  return Promise.all(state.epochs.map(async (epoch, index) => {
    const archivePath = epochArchivePath(weekDirectory, index);
    const raw = await fs.readFile(archivePath);
    const snapshot = JSON.parse(raw.toString('utf8'));
    if (sha256(raw) !== epoch.artifact_sha256) {
      throw new Error(`epoch ${index + 1} archive hash mismatch`);
    }
    return validateEpochSnapshot(snapshot, state, epoch.seeds, index);
  }));
}

export function cumulativeRoster(snapshots, state) {
  const byModel = new Map();
  for (const [epochIndex, snapshot] of snapshots.entries()) {
    for (const entry of snapshot.roster) {
      let aggregate = byModel.get(entry.model_id);
      if (!aggregate) {
        aggregate = {
          template: entry,
          personal: 0,
          team: 0,
          collaboration: 0,
          wins: 0,
          losses: 0,
          draws: 0,
          matches: 0,
          engagements: 0,
          personalScoreFor: 0,
          personalScoreAgainst: 0,
          teamObjectiveFor: 0,
          teamObjectiveAgainst: 0,
          collaborationScoreFor: 0,
          collaborationScoreAgainst: 0,
          worldRating: 0,
          strategyRating: 0,
          worldPoints: 0,
          worldRoundWins: 0,
          worldEliminations: 0,
          worldDeaths: 0,
          worldCollaborationScore: 0,
          points: 0,
          epochWins: 0,
          bestRank: 10,
          lastRank: null,
        };
        byModel.set(entry.model_id, aggregate);
      } else if (aggregate.template.wasm_bytes !== entry.wasm_bytes
          || aggregate.template.wasm_sha256 !== entry.wasm_sha256) {
        // Exactly one artifact change per model is legal: the recorded
        // mid-season revision. Anything else is still fatal.
        const expected = expectedArtifactForEpoch(state, entry.model_id, epochIndex);
        if (expected?.wasm_bytes !== entry.wasm_bytes
            || expected?.wasm_sha256 !== entry.wasm_sha256) {
          throw new Error(`compiled artifact changed across epochs for ${entry.model_id}`);
        }
        aggregate.template = entry;
      }
      aggregate.personal += Number(entry.personal_rating);
      aggregate.team += Number(entry.team_rating);
      aggregate.collaboration += Number(entry.collaboration_rating);
      aggregate.wins += Number(entry.wins) || 0;
      aggregate.losses += Number(entry.losses) || 0;
      aggregate.draws += Number(entry.draws) || 0;
      aggregate.matches += Number(entry.matches_played) || 0;
      aggregate.engagements += Number(entry.evaluation_engagements) || 0;
      aggregate.personalScoreFor += Number(entry.personal_score_for) || 0;
      aggregate.personalScoreAgainst += Number(entry.personal_score_against) || 0;
      aggregate.teamObjectiveFor += Number(entry.team_objective_for) || 0;
      aggregate.teamObjectiveAgainst += Number(entry.team_objective_against) || 0;
      aggregate.collaborationScoreFor += Number(entry.collaboration_score_for) || 0;
      aggregate.collaborationScoreAgainst += Number(entry.collaboration_score_against) || 0;
      aggregate.worldRating += Number(entry.world_rating) || 0;
      aggregate.strategyRating += Number(entry.strategy_rating) || 0;
      aggregate.worldPoints += Number(entry.world_points) || 0;
      aggregate.worldRoundWins += Number(entry.world_round_wins) || 0;
      aggregate.worldEliminations += Number(entry.world_eliminations) || 0;
      aggregate.worldDeaths += Number(entry.world_deaths) || 0;
      aggregate.worldCollaborationScore += Number(entry.world_collaboration_score) || 0;
      aggregate.points += state.points_by_rank[entry.rank - 1];
      aggregate.epochWins += entry.rank === 1 ? 1 : 0;
      aggregate.bestRank = Math.min(aggregate.bestRank, entry.rank);
      aggregate.lastRank = entry.rank;
      aggregate.lastEpoch = epochIndex;
    }
  }

  const count = snapshots.length;
  const weights = snapshots.at(-1).methodology;
  const roster = [...byModel.values()].map((aggregate) => {
    const personal = roundRating(aggregate.personal / count);
    const team = roundRating(aggregate.team / count);
    const collaboration = roundRating(aggregate.collaboration / count);
    const overall = roundRating(
      personal * weights.personal_weight
      + team * weights.team_weight
      + collaboration * weights.collaboration_weight,
    );
    const world = roundRating(aggregate.worldRating / count);
    const strategy = roundRating(aggregate.strategyRating / count);
    return {
      ...aggregate.template,
      personal_rating: personal,
      team_rating: team,
      collaboration_rating: collaboration,
      overall_rating: overall,
      world_rating: world,
      strategy_rating: strategy,
      wins: aggregate.wins,
      losses: aggregate.losses,
      draws: aggregate.draws,
      matches_played: aggregate.matches,
      evaluation_engagements: aggregate.engagements,
      personal_score_for: aggregate.personalScoreFor,
      personal_score_against: aggregate.personalScoreAgainst,
      team_objective_for: aggregate.teamObjectiveFor,
      team_objective_against: aggregate.teamObjectiveAgainst,
      collaboration_score_for: aggregate.collaborationScoreFor,
      collaboration_score_against: aggregate.collaborationScoreAgainst,
      world_points: aggregate.worldPoints,
      world_round_wins: aggregate.worldRoundWins,
      world_eliminations: aggregate.worldEliminations,
      world_deaths: aggregate.worldDeaths,
      world_collaboration_score: aggregate.worldCollaborationScore,
      season_points: aggregate.points,
      epochs_played: count,
      epoch_wins: aggregate.epochWins,
      best_epoch_rank: aggregate.bestRank,
      last_epoch_rank: aggregate.lastRank,
    };
  });
  roster.sort((left, right) => (
    right.season_points - left.season_points
    || right.epoch_wins - left.epoch_wins
    || right.strategy_rating - left.strategy_rating
    || right.overall_rating - left.overall_rating
    || right.personal_rating - left.personal_rating
    || left.provider_rank - right.provider_rank
  ));
  roster.forEach((entry, index) => { entry.rank = index + 1; });
  return roster;
}

async function buildCumulativeSnapshot(state, weekDirectory) {
  const snapshots = await loadCommittedSnapshots(state, weekDirectory);
  if (snapshots.length === 0) throw new Error('cannot publish a season without a completed epoch');
  const latest = snapshots.at(-1);
  const allSeeds = state.epochs.flatMap((epoch) => epoch.seeds);
  const exposedSeeds = allSeeds.slice(-10_000);
  const totalBattleRequests = snapshots.reduce(
    (sum, snapshot) => sum + Number(snapshot.integrity.battle_requests || 0),
    0,
  );
  const totalEngagements = snapshots.reduce(
    (sum, snapshot) => sum + Number(snapshot.integrity.total_engagements || 0),
    0,
  );
  const totalWorldRequests = snapshots.reduce(
    (sum, snapshot) => sum + Number(snapshot.integrity.world_requests || 0),
    0,
  );
  const totalWorldFighterRounds = snapshots.reduce(
    (sum, snapshot) => sum + Number(snapshot.integrity.world_fighter_rounds || 0),
    0,
  );
  const ledgerDigest = sha256(JSON.stringify(state.epochs));
  const notes = [
    ...(latest.methodology.notes || []).filter((note) => !String(note).startsWith('Weekly league:')),
    'Weekly league: ratings are cumulative equal-epoch means; match records are cumulative totals.',
    'Weekly league: standings use the frozen rank-points table, then epoch wins and strategy rating.',
  ].slice(0, 16);

  return {
    ...latest,
    generated_at: nowIso(),
    methodology: {
      ...latest.methodology,
      seeds_per_matchup: allSeeds.length,
      seed_sets: exposedSeeds,
      notes,
    },
    integrity: {
      ...latest.integrity,
      verified: true,
      battle_requests: totalBattleRequests,
      total_engagements: totalEngagements,
      world_requests: totalWorldRequests,
      world_fighter_rounds: totalWorldFighterRounds,
      epochs_completed: snapshots.length,
    },
    league: {
      format: 'weekly_continuous_v1',
      week_id: state.week_id,
      frozen_at: state.frozen_at,
      ledger_generation: state.ledger_generation || 1,
      artifact_binding_version: state.artifact_binding_version || ARTIFACT_BINDING_VERSION,
      artifact_binding_started_at: state.artifact_binding_started_at || state.frozen_at,
      epochs_completed: snapshots.length,
      total_seed_count: allSeeds.length,
      points_by_rank: state.points_by_rank,
      standings_order: ['season_points', 'epoch_wins', 'strategy_rating'],
      ledger_sha256: ledgerDigest,
      recent_epochs: state.epochs.slice(-64).map(({ artifact_path: _artifactPath, ...epoch }) => epoch),
      legacy_history: state.legacy_history || [],
    },
    roster: cumulativeRoster(snapshots, state),
  };
}

async function publicationIsCurrent(publishPath, state) {
  try {
    const published = await readJson(publishPath);
    const expectedBindings = new Map(
      validateArtifactBindings(state.artifact_bindings, state.entrant_model_ids)
        .map((binding) => [binding.model_id, binding]),
    );
    return published?.schema_version === 1
      && published?.active === true
      && published?.season_id === state.season_id
      && Array.isArray(published?.roster)
      && published.roster.length === 10
      && published?.league?.week_id === state.week_id
      && published?.league?.ledger_generation === (state.ledger_generation || 1)
      && published?.league?.epochs_completed === state.epochs.length
      && published?.league?.ledger_sha256 === sha256(JSON.stringify(state.epochs))
      && published.roster.every((entry) => {
        const binding = expectedBindings.get(entry.model_id);
        return binding?.wasm_bytes === entry.wasm_bytes
          && binding?.wasm_sha256 === entry.wasm_sha256;
      });
  } catch {
    return false;
  }
}

async function publishIfNeeded(config, state, weekDirectory) {
  if (state.epochs.length === 0 || await publicationIsCurrent(config.publishPath, state)) return;
  const cumulative = await buildCumulativeSnapshot(state, weekDirectory);
  await atomicWriteJson(config.publishPath, cumulative);
  process.stdout.write(
    `[arena-weekly] published ${state.week_id} cumulative standings after epoch ${state.epochs.length}\n`,
  );
}

async function recordEpoch({ config, state, statePath, weekDirectory, redact }) {
  const { ledgerEntry } = await archiveOrRunEpoch({ config, state, weekDirectory, redact });
  const nextState = {
    ...state,
    epochs: [...state.epochs, ledgerEntry],
    updated_at: nowIso(),
    consecutive_failures: 0,
    last_error: null,
    last_failure_at: null,
    next_retry_at: null,
  };
  await atomicWriteJson(statePath, nextState);
  try {
    await publishIfNeeded(config, nextState, weekDirectory);
  } catch (error) {
    error.committedState = nextState;
    throw error;
  }
  return nextState;
}

function sanitizedError(error, redact) {
  return redact(error?.message || error || 'unknown failure')
    .replace(/[\r\n]+/g, ' ')
    .slice(0, MAX_ERROR_CHARS);
}

async function recordFailure(statePath, state, error, redact, waitMs) {
  if (!statePath || !state) return;
  const nextState = {
    ...state,
    updated_at: nowIso(),
    consecutive_failures: (Number(state.consecutive_failures) || 0) + 1,
    last_failure_at: nowIso(),
    last_error: sanitizedError(error, redact),
    next_retry_at: new Date(Date.now() + waitMs).toISOString(),
  };
  await atomicWriteJson(statePath, nextState).catch(() => {});
}

function retryDelay(config, failures) {
  const exponent = Math.max(0, Math.min(16, failures - 1));
  const base = Math.min(config.retryMaxMs, config.retryMinMs * (2 ** exponent));
  const jitter = 0.85 + (Math.random() * 0.3);
  return Math.max(1_000, Math.round(base * jitter));
}

async function interruptibleDelay(milliseconds) {
  if (stopRequested) return;
  await new Promise((resolve) => {
    let finished = false;
    let poll;
    const finish = () => {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      clearInterval(poll);
      resolve();
    };
    const timer = setTimeout(finish, milliseconds);
    poll = setInterval(() => {
      if (!stopRequested) return;
      finish();
    }, Math.min(1_000, milliseconds));
  });
}

async function main() {
  if (process.argv.includes('--help')) {
    process.stdout.write(`Continuous weekly OpenRouter arena supervisor\n\nUsage:\n  node scripts/arena/weekly_supervisor.mjs\n\nThis process runs until SIGINT or SIGTERM. It is not installed or enabled automatically.\n`);
    return;
  }
  if (process.argv.length > 2) throw new Error(`unknown option '${process.argv[2]}'`);

  const config = {
    stateDirectory: resolveFromRoot(process.env.ARENA_WEEKLY_STATE_DIR, DEFAULT_STATE_DIR),
    publishPath: resolveFromRoot(process.env.MGS_ARENA_RATINGS_PATH, DEFAULT_PUBLISH_PATH),
    seedPackSize: integerEnv('ARENA_WEEKLY_SEEDS_PER_EPOCH', 4, 1, 64),
    epochIntervalMs: integerEnv('ARENA_WEEKLY_EPOCH_INTERVAL_MS', 60_000, 10_000, 86_400_000),
    retryMinMs: integerEnv('ARENA_WEEKLY_RETRY_MIN_MS', 30_000, 1_000, 3_600_000),
    retryMaxMs: integerEnv('ARENA_WEEKLY_RETRY_MAX_MS', 900_000, 1_000, 86_400_000),
  };
  config.retryMaxMs = Math.max(config.retryMinMs, config.retryMaxMs);

  const redact = await secretRedactor();
  const supervisorLock = await acquireOwnedLock(
    path.join(config.stateDirectory, 'supervisor.lock'),
    {
      activeMessage: (owner) => `weekly supervisor is already running as PID ${owner.pid}`,
    },
  );
  process.once('SIGINT', () => requestStop('SIGINT'));
  process.once('SIGTERM', () => requestStop('SIGTERM'));
  process.stdout.write('[arena-weekly] supervisor ready; ratings publish only after complete epochs\n');

  try {
    let transientFailures = 0;
    while (!stopRequested) {
      const weekId = isoWeekId();
      let state;
      let statePath;
      try {
        const loaded = await loadOrCreateWeekState(config, weekId, redact);
        ({ state, statePath } = loaded);
        const retryAt = Date.parse(state.next_retry_at || '');
        if (Number.isFinite(retryAt) && retryAt > Date.now()) {
          await interruptibleDelay(Math.min(config.retryMaxMs, retryAt - Date.now()));
          continue;
        }
        state = await ensureGeneration({
          ...loaded,
          state,
          redact,
          publishPath: config.publishPath,
        });
        await publishIfNeeded(config, state, loaded.weekDirectory);
        if (!stopRequested) {
          state = await recordEpoch({
            config,
            state,
            statePath,
            weekDirectory: loaded.weekDirectory,
            redact,
          });
        }
        transientFailures = 0;
        if (!stopRequested) await interruptibleDelay(config.epochIntervalMs);
      } catch (error) {
        if (stopRequested) break;
        if (error?.committedState) state = error.committedState;
        transientFailures = Math.max(
          transientFailures + 1,
          (Number(state?.consecutive_failures) || 0) + 1,
        );
        const failures = transientFailures;
        const waitMs = retryDelay(config, failures);
        await recordFailure(statePath, state, error, redact, waitMs);
        process.stderr.write(
          `[arena-weekly] ${sanitizedError(error, redact)}; retrying in ${Math.ceil(waitMs / 1000)}s\n`,
        );
        await interruptibleDelay(waitMs);
      }
    }
  } finally {
    if (activeChild && activeChild.exitCode === null) activeChild.kill('SIGTERM');
    await releaseOwnedLock(supervisorLock).catch(() => {});
    process.stdout.write('[arena-weekly] stopped\n');
  }
}

const invokedAsScript = process.argv[1]
  && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedAsScript) {
  main().catch((error) => {
    process.stderr.write(`[arena-weekly] ${String(error?.message || error).slice(0, MAX_ERROR_CHARS)}\n`);
    process.exitCode = 1;
  });
}
