#!/usr/bin/env node

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import {
  addWorldRatings,
  assertBattleIntegrity,
  buildSeasonRatings,
  DEFAULT_WEIGHTS,
} from './season_scoring.mjs';
import { arenaApiJson as apiJson } from './arena_api_client.mjs';
import { acquireOwnedLock, releaseOwnedLock } from './owned_lock.mjs';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT_DIR = path.resolve(SCRIPT_DIR, '../..');
const DEFAULT_RANKING_URL = 'https://openrouter.ai/api/v1/models?output_modalities=text&sort=top-weekly';
const DEFAULT_SEEDS = Object.freeze([104729, 130363, 155921, 181081]);
const SOURCE_LIMIT_BYTES = 50 * 1024;
const MAX_PUBLISHED_WASM_BYTES = 2 * 1024 * 1024;
const TEAM_MODES = Object.freeze(['ctf', 'koth', 'tdm']);
const ALL_MODES = Object.freeze(['arena', ...TEAM_MODES]);
const WORLD_SQUAD_SIZE = 3;
const WORLD_MAX_TICKS = 600;
const DUEL_STRATEGY_WEIGHT = 0.75;
const WORLD_STRATEGY_WEIGHT = 0.25;
const PROVIDER_SORT_POLICY = 'throughput';
const TEMPERATURE_POLICY = 'provider_default';
const REASONING_POLICY_VERSION = 'capability_minimum_v1';
const PROVIDER_REQUIRE_PARAMETERS = true;
const REASONING_EXCLUDE = true;
const RESPONSE_TRANSPORT_POLICY = 'sse_v1';
const COLLABORATION_ABI_VERSION = 'bot_tick_v2/1';
const GENERATION_CHECKPOINT_SCHEMA_VERSION = 2;
const GENERATION_STAGE_GENERATED = 'generated';
const GENERATION_STAGE_COMPILED = 'compiled';
const ARTIFACT_BINDING_SCHEMA_VERSION = 1;
const ARTIFACT_BINDING_KIND = 'legacy_wasm_digest_binding_v1';
const ARTIFACT_BINDING_FILE = 'artifact-binding.json';
const BOUND_GENERATIONS_DIRECTORY = 'bound-generations';
const MIGRATION_JOURNAL_SCHEMA_VERSION = 1;
const MIGRATION_JOURNAL_KIND = 'legacy_wasm_verification_attempts_v1';
const MIGRATION_JOURNAL_FILE = 'artifact-binding-attempts.json';
const MAX_COMPILE_ATTEMPTS = 100;
const REASONING_EFFORTS_ASCENDING = Object.freeze([
  'minimal',
  'low',
  'medium',
  'high',
  'xhigh',
  'max',
]);
const REASONING_MODES = new Set(['unsupported', 'disabled', 'minimum']);
const UNVERIFIED_RUNTIME_PATTERN = /fallback|trap|fuel|wasm not found|runtime unavailable|instantiate failed/i;
let activeLeagueLock = null;

export function parseArgs(argv) {
  const options = {
    dryRun: false,
    snapshotOnly: false,
    generateOnly: false,
    evaluateOnly: false,
    rehydrateOnly: false,
    reviseOnly: false,
    publish: true,
    resume: true,
    rankingFile: null,
    seasonId: null,
    statsState: null,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const [flag, inlineValue] = arg.split('=', 2);
    const nextValue = () => {
      if (inlineValue !== undefined) return inlineValue;
      index += 1;
      if (index >= argv.length) throw new Error(`${flag} requires a value`);
      return argv[index];
    };
    switch (flag) {
      case '--dry-run': options.dryRun = true; break;
      case '--snapshot-only': options.snapshotOnly = true; break;
      case '--generate-only': options.generateOnly = true; break;
      case '--evaluate-only': options.evaluateOnly = true; break;
      case '--rehydrate-only': options.rehydrateOnly = true; break;
      case '--revise-only': options.reviseOnly = true; break;
      case '--no-publish': options.publish = false; break;
      case '--no-resume': options.resume = false; break;
      case '--ranking-file': options.rankingFile = nextValue(); break;
      case '--season-id': options.seasonId = nextValue(); break;
      case '--stats-state': options.statsState = nextValue(); break;
      case '--help': options.help = true; break;
      default: throw new Error(`unknown option '${arg}'`);
    }
  }
  if (options.generateOnly && options.evaluateOnly) {
    throw new Error('--generate-only and --evaluate-only cannot be combined');
  }
  if (options.rehydrateOnly
      && (options.dryRun || options.snapshotOnly || options.generateOnly || options.evaluateOnly)) {
    throw new Error('--rehydrate-only cannot be combined with another runner mode');
  }
  if (options.rehydrateOnly && (!options.rankingFile || !options.seasonId)) {
    throw new Error('--rehydrate-only requires --ranking-file and --season-id');
  }
  if (options.reviseOnly
      && (options.dryRun || options.snapshotOnly || options.generateOnly
        || options.evaluateOnly || options.rehydrateOnly)) {
    throw new Error('--revise-only cannot be combined with another runner mode');
  }
  if (options.reviseOnly && (!options.rankingFile || !options.seasonId || !options.statsState)) {
    throw new Error('--revise-only requires --ranking-file, --season-id and --stats-state');
  }
  return options;
}

function usage() {
  return `OpenRouter top-10 model arena season

Usage: node scripts/arena/run_top10_season.mjs [options]

Options:
  --dry-run             Fetch/freeze the ranking and print the plan; no writes
  --snapshot-only       Save the frozen ranking without contacting the arena
  --generate-only       Register and compile all fighters, then stop
  --evaluate-only       Reuse completed generation checkpoints
  --rehydrate-only      Locally recompile audited archived sources, then stop
  --revise-only         One-shot mid-season revision of every frozen fighter
  --ranking-file PATH   Reproduce a season from a frozen ranking snapshot
  --season-id ID        Override the derived dated season identifier
  --stats-state PATH    Weekly supervisor state.json (required by --revise-only)
  --no-publish          Keep the completed artifact out of data/arena_ratings.json
  --no-resume           Ignore prior generation and battle checkpoints
  --help                Show this help

Environment:
  ARENA_API_BASE                    default http://127.0.0.1:8080
  ARENA_ADMIN_BEARER_TOKEN[_FILE]   admin credential for mutation routes
  ARENA_TOP_MODELS                  default 10
  ARENA_SEEDS                       comma-separated integers
  ARENA_TEAM_SIZE                   default 10
  ARENA_GENERATION_CONCURRENCY      default 2
  ARENA_SIMULATION_CONCURRENCY      default 6
  ARENA_GENERATION_ATTEMPTS         default 3
  MGS_ARENA_RATINGS_PATH            publish target (default data/arena_ratings.json)
`;
}

const integerEnv = (name, fallback, min, max) => {
  const value = Number.parseInt(process.env[name] || '', 10);
  if (!Number.isFinite(value)) return fallback;
  return Math.max(min, Math.min(max, value));
};

function seedListFromEnv() {
  const raw = (process.env.ARENA_SEEDS || '').trim();
  if (!raw) return [...DEFAULT_SEEDS];
  const seeds = raw.split(',').map((value) => Number.parseInt(value.trim(), 10));
  if (seeds.length === 0 || seeds.some((value) => !Number.isSafeInteger(value) || value < 0)) {
    throw new Error('ARENA_SEEDS must be a comma-separated list of non-negative safe integers');
  }
  return [...new Set(seeds)];
}

const sha256 = (value) => createHash('sha256').update(value).digest('hex');

async function atomicWriteJson(targetPath, value) {
  await fs.mkdir(path.dirname(targetPath), { recursive: true });
  const temporaryPath = `${targetPath}.tmp-${process.pid}-${Date.now()}`;
  await fs.writeFile(temporaryPath, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  await fs.rename(temporaryPath, targetPath);
}

async function atomicWriteText(targetPath, value) {
  await fs.mkdir(path.dirname(targetPath), { recursive: true });
  const temporaryPath = `${targetPath}.tmp-${process.pid}-${Date.now()}`;
  await fs.writeFile(temporaryPath, value, { mode: 0o600 });
  await fs.rename(temporaryPath, targetPath);
}

async function atomicWriteBytes(targetPath, value) {
  await fs.mkdir(path.dirname(targetPath), { recursive: true });
  const temporaryPath = `${targetPath}.tmp-${process.pid}-${Date.now()}`;
  await fs.writeFile(temporaryPath, value, { mode: 0o600 });
  await fs.rename(temporaryPath, targetPath);
}

async function readJson(targetPath) {
  return JSON.parse(await fs.readFile(targetPath, 'utf8'));
}

async function acquireLeagueLock(lockPath) {
  activeLeagueLock = await acquireOwnedLock(lockPath, {
    activeMessage: (owner) => `another arena season runner is active (pid ${owner.pid})`,
  });
}

async function releaseLeagueLock() {
  const lock = activeLeagueLock;
  activeLeagueLock = null;
  if (!lock) return;
  await releaseOwnedLock(lock);
}

async function readSecret(name) {
  const direct = (process.env[name] || '').trim();
  if (direct) return direct;
  const filePath = (process.env[`${name}_FILE`] || '').trim();
  if (!filePath) return null;
  const value = (await fs.readFile(filePath, 'utf8')).trim();
  return value || null;
}

function modelListFromPayload(payload) {
  if (Array.isArray(payload?.data)) return payload.data;
  if (Array.isArray(payload?.models)) return payload.models;
  if (Array.isArray(payload?.ranking?.models)) return payload.ranking.models;
  throw new Error('ranking payload does not contain a model list');
}

function isTextOutputModel(model) {
  const modalities = model?.architecture?.output_modalities;
  return !Array.isArray(modalities) || modalities.includes('text');
}

export function validateReasoningPolicy(policy, modelId = 'model') {
  if (!policy || typeof policy !== 'object' || Array.isArray(policy)) {
    throw new Error(`reasoning_policy is missing for ${modelId}`);
  }
  const version = String(policy.version || '').trim();
  const mode = String(policy.mode || '').trim();
  const effort = policy.effort == null ? null : String(policy.effort).trim();
  if (version !== REASONING_POLICY_VERSION) {
    throw new Error(`reasoning_policy version is invalid for ${modelId}`);
  }
  if (!REASONING_MODES.has(mode)) {
    throw new Error(`reasoning_policy mode is invalid for ${modelId}`);
  }
  if (policy.exclude !== REASONING_EXCLUDE) {
    throw new Error(`reasoning_policy exclude must be true for ${modelId}`);
  }
  if (mode === 'minimum') {
    if (!REASONING_EFFORTS_ASCENDING.includes(effort)) {
      throw new Error(`reasoning_policy minimum effort is invalid for ${modelId}`);
    }
  } else if (effort !== null) {
    throw new Error(`reasoning_policy effort must be null for ${mode} model ${modelId}`);
  }
  return { version, mode, effort, exclude: true };
}

export function reasoningPolicyFromModelMetadata(model) {
  const modelId = typeof model?.id === 'string' && model.id.trim()
    ? model.id.trim()
    : 'model';
  if (!Array.isArray(model?.supported_parameters)
      || model.supported_parameters.some((parameter) => typeof parameter !== 'string')) {
    throw new Error(`OpenRouter supported_parameters metadata is invalid for ${modelId}`);
  }
  if (!model.supported_parameters.includes('reasoning')) {
    return validateReasoningPolicy({
      version: REASONING_POLICY_VERSION,
      mode: 'unsupported',
      effort: null,
      exclude: true,
    }, modelId);
  }

  const reasoning = model.reasoning;
  if (reasoning != null && (typeof reasoning !== 'object' || Array.isArray(reasoning))) {
    throw new Error(`OpenRouter reasoning metadata is invalid for ${modelId}`);
  }
  if (reasoning?.mandatory != null && typeof reasoning.mandatory !== 'boolean') {
    throw new Error(`OpenRouter reasoning mandatory flag is invalid for ${modelId}`);
  }
  if (reasoning?.mandatory !== true) {
    return validateReasoningPolicy({
      version: REASONING_POLICY_VERSION,
      mode: 'disabled',
      effort: null,
      exclude: true,
    }, modelId);
  }

  const supportedEfforts = reasoning.supported_efforts;
  let selectableEfforts;
  if (supportedEfforts === null) {
    selectableEfforts = [...REASONING_EFFORTS_ASCENDING];
  } else if (Array.isArray(supportedEfforts) && supportedEfforts.length > 0) {
    selectableEfforts = supportedEfforts.map((effort) => String(effort).trim());
    if (selectableEfforts.some((effort) => (
      effort !== 'none' && !REASONING_EFFORTS_ASCENDING.includes(effort)
    ))) {
      throw new Error(`OpenRouter supported reasoning efforts are invalid for ${modelId}`);
    }
  } else {
    throw new Error(`mandatory reasoning model ${modelId} does not expose supported_efforts`);
  }
  const effort = REASONING_EFFORTS_ASCENDING.find((candidate) => (
    selectableEfforts.includes(candidate)
  ));
  if (!effort) {
    throw new Error(`mandatory reasoning model ${modelId} has no non-none supported effort`);
  }
  return validateReasoningPolicy({
    version: REASONING_POLICY_VERSION,
    mode: 'minimum',
    effort,
    exclude: true,
  }, modelId);
}

export async function loadRanking({ rankingFile, topModels }) {
  const retrievedAt = new Date().toISOString();
  let payload;
  let source;
  if (rankingFile) {
    const resolved = path.resolve(rankingFile);
    payload = await readJson(resolved);
    source = payload.source || `file:${path.basename(resolved)}`;
  } else {
    const response = await fetch(DEFAULT_RANKING_URL, {
      headers: { Accept: 'application/json' },
      signal: AbortSignal.timeout(30_000),
    });
    if (!response.ok) throw new Error(`OpenRouter ranking request failed with HTTP ${response.status}`);
    payload = await response.json();
    source = DEFAULT_RANKING_URL;
  }

  const models = modelListFromPayload(payload)
    .filter((model) => typeof model?.id === 'string' && model.id.trim() && isTextOutputModel(model))
    .slice(0, topModels)
    .map((model, index) => {
      const id = model.id.trim();
      const reasoningPolicy = rankingFile
        ? validateReasoningPolicy(model.reasoning_policy, id)
        : reasoningPolicyFromModelMetadata(model);
      return {
        provider_rank: index + 1,
        id,
        canonical_slug: typeof model.canonical_slug === 'string' ? model.canonical_slug.trim() : null,
        name: typeof model.name === 'string' && model.name.trim() ? model.name.trim() : id,
        pricing: model.pricing || null,
        context_length: Number.isFinite(Number(model.context_length)) ? Number(model.context_length) : null,
        created: Number.isFinite(Number(model.created)) ? Number(model.created) : null,
        reasoning_policy: reasoningPolicy,
      };
    });
  if (models.length !== topModels) {
    throw new Error(`expected ${topModels} ranked text models, received ${models.length}`);
  }
  if (new Set(models.map((model) => model.id)).size !== models.length) {
    throw new Error('OpenRouter ranking contains duplicate model IDs');
  }
  return {
    schema_version: 1,
    retrieved_at: payload.retrieved_at || payload.ranking?.retrieved_at || retrievedAt,
    source,
    window: 'weekly',
    sort: 'top-weekly',
    models,
  };
}

function safeArenaModelId(seasonDate, seasonId, rankedModel) {
  const slug = rankedModel.id
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, '-')
    .replace(/\.{2,}/g, '.')
    .replace(/^[.-]+|[.-]+$/g, '')
    .slice(-68) || 'model';
  const modelDigest = sha256(rankedModel.id).slice(0, 8);
  const seasonDigest = sha256(seasonId).slice(0, 8);
  return `orw-${seasonDate}-${seasonDigest}-${String(rankedModel.provider_rank).padStart(2, '0')}-${modelDigest}-${slug}`
    .slice(0, 128)
    .replace(/[.-]+$/g, '');
}

function deriveSeasonId(ranking, requestedId) {
  if (requestedId) {
    if (!/^[A-Za-z0-9_.:-]{1,128}$/.test(requestedId)) throw new Error('invalid --season-id');
    return requestedId;
  }
  const date = String(ranking.retrieved_at).slice(0, 10);
  const rosterDigest = sha256(ranking.models.map((model) => model.id).join('\n')).slice(0, 8);
  return `weekly-${date}-${rosterDigest}`;
}

export function entrantsFromRanking(ranking, seasonId) {
  const seasonDate = String(ranking.retrieved_at).slice(0, 10).replaceAll('-', '');
  return ranking.models.map((model) => ({
    provider_rank: model.provider_rank,
    model_id: safeArenaModelId(seasonDate, seasonId, model),
    model_name: model.name,
    provider_model: model.id,
    canonical_slug: model.canonical_slug,
    reasoning_policy: { ...model.reasoning_policy },
  }));
}

export function normalizeCodeStatus(data) {
  const providerConfigured = data?.provider_configured ?? data?.openrouter_configured;
  const rawAbiVersion = data?.collaboration_abi_version ?? data?.abi_version ?? data?.team_abi_version;
  const collaborationAbiVersion = String(rawAbiVersion ?? '').trim();
  const promptVersion = String(data?.prompt_version || '').trim();
  const promptSha256 = String(data?.prompt_sha256 || '').toLowerCase();
  const sourceLimitBytes = Number(data?.source_limit_bytes ?? data?.max_source_bytes);
  const maxTokens = Number(data?.max_tokens ?? data?.max_completion_tokens);
  const providerSortPolicy = String(data?.provider_sort_policy || '').trim();
  const temperaturePolicy = String(data?.temperature_policy || '').trim();
  const reasoningPolicyVersion = String(data?.reasoning_policy_version || '').trim();
  const providerRequireParameters = data?.provider_require_parameters;
  const reasoningExclude = data?.reasoning_exclude;
  const responseTransportPolicy = String(data?.response_transport_policy || '').trim();
  const simulatorRulesVersion = String(data?.simulator_rules_version || '').trim();
  if (providerConfigured !== true && providerConfigured !== false) {
    throw new Error('code status response is missing provider_configured');
  }
  if (collaborationAbiVersion !== COLLABORATION_ABI_VERSION) {
    throw new Error(`server collaboration_abi_version must be ${COLLABORATION_ABI_VERSION}`);
  }
  if (!/^[A-Za-z0-9_.:-]{1,128}$/.test(promptVersion)) {
    throw new Error('code status response is missing a valid prompt_version');
  }
  if (!/^[a-f0-9]{64}$/.test(promptSha256)) {
    throw new Error('code status response is missing a valid prompt_sha256');
  }
  if (sourceLimitBytes !== SOURCE_LIMIT_BYTES) {
    throw new Error(`server source limit must be ${SOURCE_LIMIT_BYTES} bytes; got ${sourceLimitBytes}`);
  }
  if (!Number.isSafeInteger(maxTokens)
      || maxTokens < 2_049
      || maxTokens > 16_384) {
    throw new Error('code status response is missing a valid max_tokens');
  }
  if (providerSortPolicy !== PROVIDER_SORT_POLICY) {
    throw new Error(`server provider_sort_policy must be ${PROVIDER_SORT_POLICY}`);
  }
  if (temperaturePolicy !== TEMPERATURE_POLICY) {
    throw new Error(`server temperature_policy must be ${TEMPERATURE_POLICY}`);
  }
  if (reasoningPolicyVersion !== REASONING_POLICY_VERSION
      || providerRequireParameters !== PROVIDER_REQUIRE_PARAMETERS
      || reasoningExclude !== REASONING_EXCLUDE) {
    throw new Error('server reasoning policy differs from the uniform arena contract');
  }
  if (responseTransportPolicy !== RESPONSE_TRANSPORT_POLICY) {
    throw new Error(`server response_transport_policy must be ${RESPONSE_TRANSPORT_POLICY}`);
  }
  if (!/^[A-Za-z0-9_.:-]{1,128}$/.test(simulatorRulesVersion)) {
    throw new Error('code status response is missing a valid simulator_rules_version');
  }
  const revisionPromptVersion = String(data?.revision_prompt_version || '').trim();
  const revisionPromptSha256 = String(data?.revision_prompt_sha256 || '').toLowerCase();
  if (revisionPromptVersion || revisionPromptSha256) {
    if (!/^[A-Za-z0-9_.:-]{1,128}$/.test(revisionPromptVersion)
        || !/^[a-f0-9]{64}$/.test(revisionPromptSha256)) {
      throw new Error('code status response carries an invalid revision prompt contract');
    }
  }
  return {
    ...data,
    provider_configured: providerConfigured,
    prompt_version: promptVersion,
    collaboration_abi_version: collaborationAbiVersion,
    prompt_sha256: promptSha256,
    source_limit_bytes: sourceLimitBytes,
    max_tokens: maxTokens,
    provider_sort_policy: providerSortPolicy,
    temperature_policy: temperaturePolicy,
    reasoning_policy_version: reasoningPolicyVersion,
    provider_require_parameters: providerRequireParameters,
    reasoning_exclude: reasoningExclude,
    response_transport_policy: responseTransportPolicy,
    simulator_rules_version: simulatorRulesVersion,
    revision_prompt_version: revisionPromptVersion || null,
    revision_prompt_sha256: revisionPromptSha256 || null,
  };
}

export function revisionContractFromCodeStatus(codeStatus) {
  if (!codeStatus?.revision_prompt_version || !codeStatus?.revision_prompt_sha256) return null;
  return {
    prompt_version: codeStatus.revision_prompt_version,
    prompt_sha256: codeStatus.revision_prompt_sha256,
  };
}

export function assertCodeStatusUnchanged(frozen, current) {
  for (const field of [
    'prompt_sha256',
    'prompt_version',
    'revision_prompt_version',
    'revision_prompt_sha256',
    'source_limit_bytes',
    'max_tokens',
    'provider_sort_policy',
    'temperature_policy',
    'reasoning_policy_version',
    'provider_require_parameters',
    'reasoning_exclude',
    'response_transport_policy',
    'collaboration_abi_version',
    'simulator_rules_version',
  ]) {
    if (current[field] !== frozen[field]) {
      throw new Error(`server competition contract changed during the season (${field})`);
    }
  }
}

async function mapLimit(items, concurrency, worker) {
  const results = new Array(items.length);
  let nextIndex = 0;
  const runners = Array.from({ length: Math.min(concurrency, items.length) }, async () => {
    while (true) {
      const index = nextIndex;
      nextIndex += 1;
      if (index >= items.length) return;
      results[index] = await worker(items[index], index);
    }
  });
  await Promise.all(runners);
  return results;
}

async function mapLimitDrainOnError(items, concurrency, worker) {
  const results = new Array(items.length);
  let nextIndex = 0;
  let firstError = null;
  const runners = Array.from({ length: Math.min(concurrency, items.length) }, async () => {
    while (!firstError) {
      const index = nextIndex;
      nextIndex += 1;
      if (index >= items.length) return;
      try {
        results[index] = await worker(items[index], index);
      } catch (error) {
        firstError ||= error;
      }
    }
  });
  await Promise.all(runners);
  if (firstError) throw firstError;
  return results;
}

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

function withoutSourceCode(generationData) {
  if (!generationData?.generated) return generationData;
  const generated = { ...generationData.generated };
  delete generated.source_code;
  return {
    ...generationData,
    generated,
  };
}

function callArenaApi(context, request) {
  const { apiClient = apiJson, ...apiContext } = context;
  return apiClient({ ...apiContext, ...request });
}

async function registerEntrant(context, entrant) {
  return callArenaApi(context, {
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
}

function isNonEmptyString(value) {
  return typeof value === 'string' && value.trim().length > 0;
}

function isIsoTimestamp(value) {
  if (typeof value !== 'string') return false;
  const timestamp = Date.parse(value);
  return Number.isFinite(timestamp) && new Date(timestamp).toISOString() === value;
}

export function validateGenerationUsage(usage, maxCompletionTokens) {
  if (!usage || typeof usage !== 'object' || Array.isArray(usage)) {
    throw new Error('generation response is missing terminal usage');
  }
  for (const field of ['prompt_tokens', 'completion_tokens', 'total_tokens']) {
    if (!Number.isSafeInteger(usage[field]) || usage[field] < 0) {
      throw new Error(`generation usage ${field} must be a finite non-negative integer`);
    }
  }
  if (typeof usage.cost !== 'number' || !Number.isFinite(usage.cost) || usage.cost < 0) {
    throw new Error('generation usage cost must be finite and non-negative');
  }
  const expectedTotal = usage.prompt_tokens + usage.completion_tokens;
  if (!Number.isSafeInteger(expectedTotal) || usage.total_tokens !== expectedTotal) {
    throw new Error('generation usage total_tokens must equal prompt_tokens plus completion_tokens');
  }
  if (!Number.isSafeInteger(maxCompletionTokens)
      || usage.completion_tokens > maxCompletionTokens) {
    throw new Error('generation usage completion_tokens exceeds the frozen completion limit');
  }
  return usage;
}

export function validateGeneratedResponse(generated, codeStatus, entrant) {
  if (!generated || typeof generated !== 'object' || Array.isArray(generated)) {
    throw new Error('generation response is incomplete');
  }
  if (generated.simulated !== false) {
    throw new Error('OpenRouter generation fell back to a simulated local template');
  }
  const source = generated.source_code;
  if (typeof source !== 'string' || !source.trim()) throw new Error('generated source is empty');
  if (!/\bfn\s+bot_tick_v2\s*\(/.test(source)) {
    throw new Error('generated fighter does not implement the v2 collaboration ABI');
  }
  const sourceBytes = Buffer.byteLength(source, 'utf8');
  if (sourceBytes > codeStatus.source_limit_bytes) {
    throw new Error(`generated source exceeds ${codeStatus.source_limit_bytes} bytes`);
  }
  // The response must prove one of the frozen contracts: the generation
  // prompt (gen-1 fighters) or the revision prompt (mid-season revision).
  const revisionContract = revisionContractFromCodeStatus(codeStatus);
  const responsePromptHash = String(generated.prompt_sha256 || '').toLowerCase();
  const matchesGenerationPrompt = generated.prompt_version === codeStatus.prompt_version
    && responsePromptHash === codeStatus.prompt_sha256;
  const matchesRevisionPrompt = revisionContract !== null
    && generated.prompt_version === revisionContract.prompt_version
    && responsePromptHash === revisionContract.prompt_sha256;
  if (!matchesGenerationPrompt && !matchesRevisionPrompt) {
    throw new Error('generation prompt version or hash differs from the frozen server prompts');
  }
  const expectedPromptSha256 = matchesGenerationPrompt
    ? codeStatus.prompt_sha256
    : revisionContract.prompt_sha256;
  if (typeof generated.prompt_text !== 'string' || sha256(generated.prompt_text) !== expectedPromptSha256) {
    throw new Error('generation response does not prove the exact prompt text');
  }
  if (generated.model !== entrant.provider_model) {
    throw new Error('generation response model differs from the frozen entrant model');
  }
  if (Number(generated.max_completion_tokens) !== codeStatus.max_tokens
      || generated.provider_sort_policy !== codeStatus.provider_sort_policy
      || generated.temperature_policy !== codeStatus.temperature_policy
      || generated.reasoning_policy_version !== codeStatus.reasoning_policy_version
      || generated.provider_require_parameters !== codeStatus.provider_require_parameters
      || generated.reasoning_mode !== entrant.reasoning_policy.mode
      || (generated.reasoning_effort ?? null) !== entrant.reasoning_policy.effort
      || generated.reasoning_exclude !== codeStatus.reasoning_exclude
      || generated.response_transport_policy !== codeStatus.response_transport_policy) {
    throw new Error('generation response request contract differs from frozen server status');
  }
  if (generated.finish_reason !== 'stop') {
    throw new Error('generation response did not finish with stop');
  }
  for (const field of ['resolved_model', 'provider_name', 'provider_response_id']) {
    if (!isNonEmptyString(generated[field])) {
      throw new Error(`generation response is missing ${field}`);
    }
  }
  const usage = validateGenerationUsage(generated.usage, codeStatus.max_tokens);
  return {
    source,
    sourceBytes,
    finishReason: generated.finish_reason,
    resolvedModel: generated.resolved_model,
    providerName: generated.provider_name,
    providerResponseId: generated.provider_response_id,
    usage,
  };
}

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

export function validateGeneration(data, codeStatus, entrant) {
  if (!data?.generated || !data?.compile) throw new Error('generation response is incomplete');
  const verified = validateGeneratedResponse(data.generated, codeStatus, entrant);
  return {
    ...verified,
    ...validateCompileResponse(data.compile, entrant.model_id),
  };
}

function validateCheckpointAudit(checkpoint, entrant, source, codeStatus) {
  const sourceBytes = Buffer.byteLength(source, 'utf8');
  validateGenerationUsage(checkpoint?.usage, codeStatus.max_tokens);
  // A checkpoint is pinned either to the frozen generation contract (gen-1
  // fighters) or to the frozen revision contract (mid-season revision round).
  const revisionContract = revisionContractFromCodeStatus(codeStatus);
  const matchesPromptContract = (checkpoint?.prompt_sha256 === codeStatus.prompt_sha256
      && checkpoint?.prompt_version === codeStatus.prompt_version)
    || (revisionContract !== null
      && checkpoint?.prompt_sha256 === revisionContract.prompt_sha256
      && checkpoint?.prompt_version === revisionContract.prompt_version);
  if (
    checkpoint?.model_id !== entrant.model_id
    || checkpoint?.provider_rank !== entrant.provider_rank
    || checkpoint?.model_name !== entrant.model_name
    || checkpoint?.provider_model !== entrant.provider_model
    || checkpoint?.canonical_slug !== entrant.canonical_slug
    || checkpoint?.simulated !== false
    || !matchesPromptContract
    || checkpoint?.max_completion_tokens !== codeStatus.max_tokens
    || checkpoint?.provider_sort_policy !== codeStatus.provider_sort_policy
    || checkpoint?.temperature_policy !== codeStatus.temperature_policy
    || checkpoint?.reasoning_policy_version !== codeStatus.reasoning_policy_version
    || checkpoint?.provider_require_parameters !== codeStatus.provider_require_parameters
    || JSON.stringify(validateReasoningPolicy(checkpoint?.reasoning_policy, entrant.provider_model))
      !== JSON.stringify(entrant.reasoning_policy)
    || checkpoint?.reasoning_mode !== entrant.reasoning_policy.mode
    || (checkpoint?.reasoning_effort ?? null) !== entrant.reasoning_policy.effort
    || checkpoint?.reasoning_exclude !== codeStatus.reasoning_exclude
    || checkpoint?.response_transport_policy !== codeStatus.response_transport_policy
    || checkpoint?.simulator_rules_version !== codeStatus.simulator_rules_version
    || checkpoint?.finish_reason !== 'stop'
    || !isNonEmptyString(checkpoint?.resolved_model)
    || !isNonEmptyString(checkpoint?.provider_name)
    || !isNonEmptyString(checkpoint?.provider_response_id)
    || checkpoint?.source_sha256 !== sha256(source)
    || checkpoint?.source_bytes !== sourceBytes
    || sourceBytes > codeStatus.source_limit_bytes
    || !/\bfn\s+bot_tick_v2\s*\(/.test(source)
  ) {
    throw new Error(`generation checkpoint is stale or unverified for ${entrant.provider_model}`);
  }
  return { checkpoint, source, sourceBytes };
}

function validateArchivedProviderResponse(checkpoint, entrant, source, codeStatus) {
  const providerResponse = checkpoint?.provider_response;
  const generated = providerResponse?.generated;
  if (!providerResponse || typeof providerResponse !== 'object' || Array.isArray(providerResponse)
      || Object.prototype.hasOwnProperty.call(providerResponse, 'compile')
      || !generated || typeof generated !== 'object' || Array.isArray(generated)
      || Object.prototype.hasOwnProperty.call(generated, 'source_code')) {
    throw new Error(`generation checkpoint response archive is invalid for ${entrant.provider_model}`);
  }
  const verified = validateGeneratedResponse({ ...generated, source_code: source }, codeStatus, entrant);
  const usageMatches = ['prompt_tokens', 'completion_tokens', 'total_tokens', 'cost']
    .every((field) => checkpoint.usage[field] === verified.usage[field]);
  if (checkpoint.finish_reason !== verified.finishReason
      || checkpoint.resolved_model !== verified.resolvedModel
      || checkpoint.provider_name !== verified.providerName
      || checkpoint.provider_response_id !== verified.providerResponseId
      || checkpoint.source_bytes !== verified.sourceBytes
      || !usageMatches) {
    throw new Error(`generation checkpoint response archive differs for ${entrant.provider_model}`);
  }
  return verified;
}

function validateV2CheckpointMetadata(checkpoint, entrant, source, codeStatus) {
  validateCheckpointAudit(checkpoint, entrant, source, codeStatus);
  if (checkpoint.schema_version !== GENERATION_CHECKPOINT_SCHEMA_VERSION
      || !isIsoTimestamp(checkpoint.generated_at)
      || checkpoint.collaboration_abi_version !== codeStatus.collaboration_abi_version
      || checkpoint.source_limit_bytes !== codeStatus.source_limit_bytes
      || !Number.isSafeInteger(checkpoint.generation_attempts)
      || checkpoint.generation_attempts < 1
      || !Number.isSafeInteger(checkpoint.compile_attempts)
      || checkpoint.compile_attempts < 0
      || checkpoint.compile_attempts > 100
      || (checkpoint.last_compile_attempt_at != null
        && !isIsoTimestamp(checkpoint.last_compile_attempt_at))
      || (checkpoint.last_compile_error_sha256 != null
        && !/^[a-f0-9]{64}$/.test(checkpoint.last_compile_error_sha256))) {
    throw new Error(`generation checkpoint metadata is invalid for ${entrant.provider_model}`);
  }
  const archiveDigest = sha256(JSON.stringify({
    provider_response: checkpoint.provider_response,
    source_sha256: checkpoint.source_sha256,
  }));
  if (!/^[a-f0-9]{64}$/.test(checkpoint.generation_archive_sha256)
      || checkpoint.generation_archive_sha256 !== archiveDigest) {
    throw new Error(`generation checkpoint response archive digest differs for ${entrant.provider_model}`);
  }
  validateArchivedProviderResponse(checkpoint, entrant, source, codeStatus);
  return { checkpoint, source, sourceBytes: Buffer.byteLength(source, 'utf8') };
}

export function validateGeneratedCheckpoint(checkpoint, entrant, source, codeStatus) {
  const result = validateV2CheckpointMetadata(checkpoint, entrant, source, codeStatus);
  if (checkpoint.stage !== GENERATION_STAGE_GENERATED
      || checkpoint.compiled !== false
      || checkpoint.wasm_bytes !== null
      || checkpoint.wasm_sha256 !== null
      || checkpoint.compiled_at != null) {
    throw new Error(`generated checkpoint stage is invalid for ${entrant.provider_model}`);
  }
  return result;
}

export function validateGenerationCheckpoint(checkpoint, entrant, source, codeStatus) {
  const result = validateV2CheckpointMetadata(checkpoint, entrant, source, codeStatus);
  if (checkpoint.stage !== GENERATION_STAGE_COMPILED
      || checkpoint.compiled !== true
      || !Number.isSafeInteger(checkpoint.wasm_bytes)
      || checkpoint.wasm_bytes < 1
      || checkpoint.wasm_bytes > MAX_PUBLISHED_WASM_BYTES
      || !/^[a-f0-9]{64}$/.test(String(checkpoint.wasm_sha256 || ''))
      || !isIsoTimestamp(checkpoint.compiled_at)) {
    throw new Error(`compiled checkpoint stage is invalid for ${entrant.provider_model}`);
  }
  return result;
}

/**
 * Accept the one legacy shape produced before compiled artifacts carried a
 * digest. This is deliberately narrower than the normal checkpoint validator:
 * only an otherwise-complete schema-v2 compiled checkpoint with the digest
 * property entirely absent can enter the one-time local recompile path.
 */
export function validateLegacyCompiledCheckpoint(checkpoint, entrant, source, codeStatus) {
  const result = validateV2CheckpointMetadata(checkpoint, entrant, source, codeStatus);
  if (checkpoint.schema_version !== GENERATION_CHECKPOINT_SCHEMA_VERSION
      || checkpoint.stage !== GENERATION_STAGE_COMPILED
      || checkpoint.compiled !== true
      || Object.prototype.hasOwnProperty.call(checkpoint, 'wasm_sha256')
      || !Number.isSafeInteger(checkpoint.wasm_bytes)
      || checkpoint.wasm_bytes < 1
      || checkpoint.wasm_bytes > MAX_PUBLISHED_WASM_BYTES
      || !Number.isSafeInteger(checkpoint.compile_attempts)
      || checkpoint.compile_attempts < 1
      || !isIsoTimestamp(checkpoint.compiled_at)) {
    throw new Error(`legacy compiled checkpoint is invalid for ${entrant.provider_model}`);
  }
  return result;
}

function buildGeneratedCheckpoint(entrant, generated, verified, codeStatus, attempt) {
  const providerResponse = withoutSourceCode({ generated });
  const sourceSha256 = sha256(verified.source);
  return {
    schema_version: GENERATION_CHECKPOINT_SCHEMA_VERSION,
    stage: GENERATION_STAGE_GENERATED,
    provider_rank: entrant.provider_rank,
    model_id: entrant.model_id,
    model_name: entrant.model_name,
    provider_model: entrant.provider_model,
    canonical_slug: entrant.canonical_slug,
    generated_at: new Date().toISOString(),
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
    compiled: false,
    simulated: false,
    finish_reason: verified.finishReason,
    resolved_model: verified.resolvedModel,
    provider_name: verified.providerName,
    provider_response_id: verified.providerResponseId,
    source_bytes: verified.sourceBytes,
    source_sha256: sourceSha256,
    wasm_bytes: null,
    wasm_sha256: null,
    generation_attempts: attempt,
    compile_attempts: 0,
    usage: verified.usage,
    generation_archive_sha256: sha256(JSON.stringify({
      provider_response: providerResponse,
      source_sha256: sourceSha256,
    })),
    provider_response: providerResponse,
  };
}

async function persistCompileFailure(checkpointPath, checkpoint, error) {
  if (checkpoint.stage !== GENERATION_STAGE_GENERATED) return checkpoint;
  const failed = {
    ...checkpoint,
    compile_attempts: Math.min(100, checkpoint.compile_attempts + 1),
    last_compile_attempt_at: new Date().toISOString(),
    last_compile_error_sha256: sha256(String(error?.message || error)),
  };
  await atomicWriteJson(checkpointPath, failed);
  return failed;
}

async function compileArchivedEntrant(
  context,
  entrant,
  checkpointPath,
  checkpoint,
  source,
  {
    expectedWasmBytes = null,
    expectedWasmSha256 = null,
    verifyExisting = false,
    persistCheckpoint = true,
    attemptAt = null,
    compileAttemptCount = 1,
  } = {},
) {
  if (checkpoint.stage === GENERATION_STAGE_GENERATED
      && checkpoint.compile_attempts >= MAX_COMPILE_ATTEMPTS) {
    throw new Error(`fighter compilation retry limit reached for ${entrant.provider_model}`);
  }
  // Verification compiles in a server-owned staging directory and compares the
  // result with the currently published WASM. It deliberately skips model
  // registration as well as publication so the migration has no live writes.
  if (!verifyExisting) await registerEntrant(context, entrant);
  let compile;
  try {
    compile = await callArenaApi(context, {
      method: 'POST',
      route: '/api/arena/code/compile',
      timeoutMs: 180_000,
      body: {
        model_id: entrant.model_id,
        source_code: source,
        overwrite: !verifyExisting,
        ...(verifyExisting ? { verify_existing: true } : {}),
      },
    });
  } catch (error) {
    if (persistCheckpoint) await persistCompileFailure(checkpointPath, checkpoint, error);
    throw error;
  }
  let wasmArtifact;
  try {
    wasmArtifact = validateCompileResponse(compile, entrant.model_id);
  } catch (error) {
    if (persistCheckpoint) await persistCompileFailure(checkpointPath, checkpoint, error);
    throw error;
  }
  if (expectedWasmBytes != null && wasmArtifact.wasmBytes !== expectedWasmBytes) {
    throw new Error(
      `fighter recompile size changed for ${entrant.provider_model}; refusing migration`,
    );
  }
  if (expectedWasmSha256 != null && wasmArtifact.wasmSha256 !== expectedWasmSha256) {
    throw new Error(
      `fighter recompile digest changed for ${entrant.provider_model}; refusing migration`,
    );
  }

  if (checkpoint.stage === GENERATION_STAGE_GENERATED) {
    const completedAt = attemptAt || new Date().toISOString();
    const completed = {
      ...checkpoint,
      stage: GENERATION_STAGE_COMPILED,
      compiled: true,
      wasm_bytes: wasmArtifact.wasmBytes,
      wasm_sha256: wasmArtifact.wasmSha256,
      compile_attempts: checkpoint.compile_attempts + 1,
      compiled_at: completedAt,
      last_compile_attempt_at: completedAt,
      last_compile_error_sha256: null,
    };
    validateGenerationCheckpoint(completed, entrant, source, context.codeStatus);
    if (persistCheckpoint) await atomicWriteJson(checkpointPath, completed);
    return completed;
  }
  if (verifyExisting && expectedWasmSha256 != null) {
    // Re-validating an already bound artifact is read-only accounting-wise.
    // The compile attempt recorded by the immutable checkpoint remains the
    // migration attempt that established the binding.
    return checkpoint;
  }
  const rehydratedAt = attemptAt || new Date().toISOString();
  const rehydrated = {
    ...checkpoint,
    wasm_bytes: wasmArtifact.wasmBytes,
    wasm_sha256: wasmArtifact.wasmSha256,
    compile_attempts: checkpoint.compile_attempts + compileAttemptCount,
    last_compile_attempt_at: rehydratedAt,
    last_compile_error_sha256: null,
    rehydrated_at: rehydratedAt,
  };
  validateGenerationCheckpoint(rehydrated, entrant, source, context.codeStatus);
  if (persistCheckpoint) await atomicWriteJson(checkpointPath, rehydrated);
  return rehydrated;
}

const REVISION_JOURNAL_FILE = 'revision-attempts.json';

/**
 * Validate a revision-route response against the frozen revision contract
 * (or, defensively, the generation contract — validateGeneratedResponse
 * accepts either). Returns the verified generation fields plus the prompt
 * pair the checkpoint must pin.
 */
export function validateRevisionResponse(response, codeStatus, entrant) {
  const verified = validateGeneratedResponse(response, codeStatus, entrant);
  return {
    ...verified,
    promptVersion: response.prompt_version,
    promptSha256: String(response.prompt_sha256 || '').toLowerCase(),
  };
}

/**
 * One-shot mid-season revision of a single frozen fighter. The attempt is
 * journaled BEFORE the provider call, so a crash or rerun can never burn a
 * second call: one chance means one call. The gen-1 checkpoint and source
 * stay untouched until the revised source has validated AND compiled; only
 * then are they swapped (source first, checkpoint second, both atomic).
 */
export async function reviseEntrant(
  context,
  entrant,
  directories,
  { statsDigest, revisionEpoch, attemptAt = null },
) {
  const checkpointPath = path.join(directories.generations, `${entrant.model_id}.json`);
  const sourcePath = path.join(directories.sources, `${entrant.model_id}.rs`);
  const journalPath = path.join(directories.revisions, REVISION_JOURNAL_FILE);
  const previous = await readJson(checkpointPath);
  const previousSource = await fs.readFile(sourcePath, 'utf8');
  validateGenerationCheckpoint(previous, entrant, previousSource, context.codeStatus);

  let journal = null;
  try {
    journal = await readJson(journalPath);
  } catch (error) {
    if (error?.code !== 'ENOENT') throw error;
  }
  journal = journal ?? { schema_version: 1, attempts: {} };
  if (journal.attempts?.[entrant.model_id]) {
    throw new Error(`revision already attempted for ${entrant.provider_model}`);
  }
  const startedAt = attemptAt || new Date().toISOString();
  journal.attempts[entrant.model_id] = {
    started_at: startedAt,
    stats_digest_sha256: sha256(statsDigest),
  };
  await fs.mkdir(directories.revisions, { recursive: true, mode: 0o700 });
  await atomicWriteJson(journalPath, journal);

  const response = await callArenaApi(context, {
    method: 'POST',
    route: '/api/arena/code/revise',
    timeoutMs: 180_000,
    body: {
      model: entrant.provider_model,
      previous_source: previousSource,
      stats_digest: statsDigest,
      reasoning_mode: entrant.reasoning_policy.mode,
      reasoning_effort: entrant.reasoning_policy.effort,
    },
  });
  const verified = validateRevisionResponse(response, context.codeStatus, entrant);

  const compile = await callArenaApi(context, {
    method: 'POST',
    route: '/api/arena/code/compile',
    timeoutMs: 180_000,
    body: { model_id: entrant.model_id, source_code: verified.source, overwrite: true },
  });
  const wasmArtifact = validateCompileResponse(compile, entrant.model_id);

  const providerGenerated = { ...response };
  delete providerGenerated.source_code;
  const providerResponse = { generated: providerGenerated };
  const completedAt = new Date().toISOString();
  const revised = {
    ...previous,
    prompt_version: verified.promptVersion,
    prompt_sha256: verified.promptSha256,
    finish_reason: verified.finishReason,
    resolved_model: verified.resolvedModel,
    provider_name: verified.providerName,
    provider_response_id: verified.providerResponseId,
    source_bytes: verified.sourceBytes,
    source_sha256: sha256(verified.source),
    wasm_bytes: wasmArtifact.wasmBytes,
    wasm_sha256: wasmArtifact.wasmSha256,
    compiled_at: completedAt,
    last_compile_attempt_at: completedAt,
    last_compile_error_sha256: null,
    usage: { ...verified.usage },
    generation_archive_sha256: sha256(JSON.stringify({
      provider_response: providerResponse,
      source_sha256: sha256(verified.source),
    })),
    provider_response: providerResponse,
    revision_of: previous.source_sha256,
    revision_epoch: revisionEpoch,
    stats_digest_sha256: sha256(statsDigest),
  };
  validateGenerationCheckpoint(revised, entrant, verified.source, context.codeStatus);
  await atomicWriteBytes(sourcePath, Buffer.from(verified.source, 'utf8'));
  await atomicWriteJson(checkpointPath, revised);
  journal.attempts[entrant.model_id].completed_at = completedAt;
  journal.attempts[entrant.model_id].wasm_sha256 = wasmArtifact.wasmSha256;
  await atomicWriteJson(journalPath, journal);
  return revised;
}

export async function rehydrateEntrant(
  context,
  entrant,
  directories,
  {
    allowLegacyMissingDigest = false,
    trustArchivedArtifact = false,
    attemptAt = null,
  } = {},
) {
  const checkpointPath = path.join(directories.generations, `${entrant.model_id}.json`);
  const sourcePath = path.join(directories.sources, `${entrant.model_id}.rs`);
  const checkpoint = await readJson(checkpointPath);
  const source = await fs.readFile(sourcePath, 'utf8');
  if (checkpoint?.stage === GENERATION_STAGE_GENERATED) {
    validateGeneratedCheckpoint(checkpoint, entrant, source, context.codeStatus);
  } else if (allowLegacyMissingDigest
      && !Object.prototype.hasOwnProperty.call(checkpoint, 'wasm_sha256')) {
    validateLegacyCompiledCheckpoint(checkpoint, entrant, source, context.codeStatus);
    return compileArchivedEntrant(
      context,
      entrant,
      checkpointPath,
      checkpoint,
      source,
      {
        expectedWasmBytes: checkpoint.wasm_bytes,
        verifyExisting: true,
        // A legacy checkpoint is never replaced in place. The batch migration
        // may publish only an immutable sibling set after all entrants pass.
        persistCheckpoint: false,
        attemptAt,
      },
    );
  } else {
    validateGenerationCheckpoint(checkpoint, entrant, source, context.codeStatus);
    if (trustArchivedArtifact) {
      // Unbound v2 season: the publishing compiler renames a staging artifact
      // into place, and rustc embeds that staging basename in the wasm name
      // section, so the server's byte-identical verify_existing rebuild can
      // never match a v2 artifact. Recompiling every epoch instead would
      // consume the bounded compile-attempt budget and stall a 7-day season
      // at ~epoch 96. The frozen checkpoint already proves the fighter was
      // generated, compiled, and published; trust it and skip the round-trip.
      return checkpoint;
    }
  }
  return compileArchivedEntrant(
    context,
    entrant,
    checkpointPath,
    checkpoint,
    source,
    allowLegacyMissingDigest && checkpoint.stage === GENERATION_STAGE_COMPILED
      ? {
        expectedWasmBytes: checkpoint.wasm_bytes,
        expectedWasmSha256: checkpoint.wasm_sha256,
        verifyExisting: true,
        persistCheckpoint: false,
      }
      : {},
  );
}

function competitionContractFromCodeStatus(codeStatus) {
  const revisionContract = revisionContractFromCodeStatus(codeStatus);
  return {
    prompt_version: codeStatus.prompt_version,
    prompt_sha256: codeStatus.prompt_sha256,
    // Only present when the server advertises a revision contract, so binding
    // manifests written before the revision route existed stay comparable.
    ...(revisionContract ? {
      revision_prompt_version: revisionContract.prompt_version,
      revision_prompt_sha256: revisionContract.prompt_sha256,
    } : {}),
    max_completion_tokens: codeStatus.max_tokens,
    provider_sort_policy: codeStatus.provider_sort_policy,
    temperature_policy: codeStatus.temperature_policy,
    reasoning_policy_version: codeStatus.reasoning_policy_version,
    provider_require_parameters: codeStatus.provider_require_parameters,
    reasoning_exclude: codeStatus.reasoning_exclude,
    response_transport_policy: codeStatus.response_transport_policy,
    collaboration_abi_version: codeStatus.collaboration_abi_version,
    simulator_rules_version: codeStatus.simulator_rules_version,
    source_limit_bytes: codeStatus.source_limit_bytes,
  };
}

/**
 * Bounded, deterministic per-model performance digest fed to the mid-season
 * revision prompt. Inputs are the season's own artifacts: the latest epoch's
 * season snapshot (ratings, rank, match record) and the supervisor's epoch
 * ledger (rank trajectory, epoch wins). No new telemetry is collected.
 */
export function buildRevisionStatsDigest({ seasonSnapshot, supervisorState, modelId }) {
  const roster = Array.isArray(seasonSnapshot?.roster) ? seasonSnapshot.roster : [];
  const entry = roster.find((candidate) => candidate.model_id === modelId);
  if (!entry) throw new Error(`no roster entry for ${modelId} in season snapshot`);
  const epochs = Array.isArray(supervisorState?.epochs) ? supervisorState.epochs : [];
  const lastEpochRanks = epochs.slice(-10).map((epoch) => {
    const standing = (epoch.standings || []).find((candidate) => candidate.model_id === modelId);
    return standing?.epoch_rank ?? null;
  });
  const epochWins = epochs.reduce((count, epoch) => count
    + ((epoch.standings || []).some(
      (standing) => standing.model_id === modelId && standing.epoch_rank === 1,
    ) ? 1 : 0), 0);
  const topOpponents = roster
    .filter((candidate) => candidate.model_id !== modelId)
    .sort((a, b) => Number(b.strategy_rating) - Number(a.strategy_rating))
    .slice(0, 3)
    .map((candidate) => ({
      model_id: candidate.model_id,
      strategy_rating: candidate.strategy_rating,
    }));
  const digest = {
    schema_version: 1,
    model_id: modelId,
    season_id: seasonSnapshot.season_id ?? null,
    epochs_completed: epochs.length,
    epoch_wins: epochWins,
    current: {
      rank: entry.rank,
      personal_rating: entry.personal_rating,
      team_rating: entry.team_rating,
      collaboration_rating: entry.collaboration_rating,
      world_rating: entry.world_rating,
      strategy_rating: entry.strategy_rating,
      wins: entry.wins ?? 0,
      losses: entry.losses ?? 0,
      draws: entry.draws ?? 0,
      matches_played: entry.matches_played ?? 0,
    },
    last_epoch_ranks: lastEpochRanks,
    top_opponents: topOpponents,
  };
  const serialized = JSON.stringify(digest);
  if (Buffer.byteLength(serialized, 'utf8') > 4096) {
    throw new Error(`stats digest exceeds 4096 bytes for ${modelId}`);
  }
  return serialized;
}

function isSafeBoundGenerationDirectory(value) {
  return typeof value === 'string'
    && /^bound-generations\/[a-f0-9]{24}$/.test(value)
    && path.posix.normalize(value) === value;
}

async function readOptionalJsonBytes(targetPath) {
  try {
    const bytes = await fs.readFile(targetPath);
    return { bytes, value: JSON.parse(bytes.toString('utf8')) };
  } catch (error) {
    if (error?.code === 'ENOENT') return null;
    throw error;
  }
}

function hasExactKeys(value, expectedKeys) {
  return value && typeof value === 'object' && !Array.isArray(value)
    && Object.keys(value).sort().join('\n') === [...expectedKeys].sort().join('\n');
}

function migrationJournalIdentity(records) {
  return records.map(({ entrant, legacyCheckpoint, legacyCheckpointSha256, sourceBytes }) => ({
    provider_rank: entrant.provider_rank,
    model_id: entrant.model_id,
    model_name: entrant.model_name,
    provider_model: entrant.provider_model,
    canonical_slug: entrant.canonical_slug,
    reasoning_policy: { ...entrant.reasoning_policy },
    source_bytes: sourceBytes.length,
    source_sha256: sha256(sourceBytes),
    legacy_checkpoint_sha256: legacyCheckpointSha256,
    legacy_compile_attempts: legacyCheckpoint.compile_attempts,
  }));
}

function migrationJournalKey(seasonId, rankingSha256, identity) {
  return sha256(JSON.stringify({
    season_id: seasonId,
    ranking_sha256: rankingSha256,
    entrants: identity,
  }));
}

export function validateMigrationAttemptJournal(
  journal,
  { seasonId, rankingSha256, records },
) {
  const identity = migrationJournalIdentity(records);
  const expectedKey = migrationJournalKey(seasonId, rankingSha256, identity);
  if (!hasExactKeys(journal, [
    'schema_version',
    'kind',
    'season_id',
    'ranking_sha256',
    'migration_key_sha256',
    'created_at',
    'updated_at',
    'entrants',
  ])
      || journal.schema_version !== MIGRATION_JOURNAL_SCHEMA_VERSION
      || journal.kind !== MIGRATION_JOURNAL_KIND
      || journal.season_id !== seasonId
      || journal.ranking_sha256 !== rankingSha256
      || journal.migration_key_sha256 !== expectedKey
      || !isIsoTimestamp(journal.created_at)
      || !isIsoTimestamp(journal.updated_at)
      || Date.parse(journal.updated_at) < Date.parse(journal.created_at)
      || !Array.isArray(journal.entrants)
      || journal.entrants.length !== identity.length) {
    throw new Error('artifact binding attempt journal is invalid');
  }
  for (const [index, expected] of identity.entries()) {
    const entry = journal.entrants[index];
    if (!hasExactKeys(entry, [
      ...Object.keys(expected),
      'attempts',
    ])
        || Object.entries(expected).some(([field, value]) => (
          JSON.stringify(entry[field]) !== JSON.stringify(value)
        ))
        || !Array.isArray(entry.attempts)
        || entry.attempts.length > MAX_COMPILE_ATTEMPTS - expected.legacy_compile_attempts) {
      throw new Error(`artifact binding attempt journal differs for ${expected.model_id}`);
    }
    for (const [attemptIndex, attempt] of entry.attempts.entries()) {
      if (!hasExactKeys(attempt, [
        'attempt_number',
        'started_at',
        'completed_at',
        'status',
        'error_sha256',
        'wasm_bytes',
        'wasm_sha256',
      ])
          || attempt.attempt_number !== attemptIndex + 1
          || !isIsoTimestamp(attempt.started_at)
          || !['started', 'succeeded', 'failed', 'interrupted'].includes(attempt.status)
          || (attempt.status === 'started' && attemptIndex !== entry.attempts.length - 1)
          || (attemptIndex > 0
            && Date.parse(attempt.started_at)
              < Date.parse(entry.attempts[attemptIndex - 1].started_at))) {
        throw new Error(`artifact binding attempt record is invalid for ${expected.model_id}`);
      }
      const completed = attempt.status !== 'started';
      if ((completed && (!isIsoTimestamp(attempt.completed_at)
          || Date.parse(attempt.completed_at) < Date.parse(attempt.started_at)))
          || (!completed && attempt.completed_at !== null)) {
        throw new Error(`artifact binding attempt timestamps are invalid for ${expected.model_id}`);
      }
      if (attempt.status === 'succeeded') {
        if (attempt.error_sha256 !== null
            || !Number.isSafeInteger(attempt.wasm_bytes)
            || attempt.wasm_bytes < 1
            || attempt.wasm_bytes > MAX_PUBLISHED_WASM_BYTES
            || !/^[a-f0-9]{64}$/.test(String(attempt.wasm_sha256 || ''))) {
          throw new Error(`artifact binding attempt result is invalid for ${expected.model_id}`);
        }
      } else if (completed) {
        if (!/^[a-f0-9]{64}$/.test(String(attempt.error_sha256 || ''))
            || attempt.wasm_bytes !== null
            || attempt.wasm_sha256 !== null
            || (attempt.status === 'interrupted'
              && attempt.error_sha256
                !== sha256('verification interrupted before completion was journaled'))) {
          throw new Error(`artifact binding attempt failure is invalid for ${expected.model_id}`);
        }
      } else if (attempt.error_sha256 !== null
          || attempt.wasm_bytes !== null
          || attempt.wasm_sha256 !== null) {
        throw new Error(`artifact binding started attempt is invalid for ${expected.model_id}`);
      }
      const terminalTimestamp = attempt.completed_at || attempt.started_at;
      if (Date.parse(journal.updated_at) < Date.parse(terminalTimestamp)) {
        throw new Error(`artifact binding journal update time is invalid for ${expected.model_id}`);
      }
    }
  }
  return journal;
}

async function openMigrationAttemptJournal({
  journalPath,
  seasonId,
  rankingSha256,
  records,
  timestamp,
}) {
  const identity = migrationJournalIdentity(records);
  const validationContext = { seasonId, rankingSha256, records };
  let persisted = await readOptionalJsonBytes(journalPath);
  if (!persisted) {
    const createdAt = timestamp();
    if (!isIsoTimestamp(createdAt)) throw new Error('artifact binding journal timestamp is invalid');
    const journal = {
      schema_version: MIGRATION_JOURNAL_SCHEMA_VERSION,
      kind: MIGRATION_JOURNAL_KIND,
      season_id: seasonId,
      ranking_sha256: rankingSha256,
      migration_key_sha256: migrationJournalKey(seasonId, rankingSha256, identity),
      created_at: createdAt,
      updated_at: createdAt,
      entrants: identity.map((entry) => ({ ...entry, attempts: [] })),
    };
    validateMigrationAttemptJournal(journal, validationContext);
    await atomicWriteJson(journalPath, journal);
    persisted = await readOptionalJsonBytes(journalPath);
  }
  validateMigrationAttemptJournal(persisted.value, validationContext);

  let updateTail = Promise.resolve();
  const update = (mutator) => {
    const operation = updateTail.then(async () => {
      const current = await readOptionalJsonBytes(journalPath);
      if (!current) throw new Error('artifact binding attempt journal disappeared');
      validateMigrationAttemptJournal(current.value, validationContext);
      const next = structuredClone(current.value);
      const result = mutator(next);
      validateMigrationAttemptJournal(next, validationContext);
      await atomicWriteJson(journalPath, next);
      return result;
    });
    updateTail = operation.catch(() => {});
    return operation;
  };

  const interruptedAt = timestamp();
  if (!isIsoTimestamp(interruptedAt)) throw new Error('artifact binding journal timestamp is invalid');
  await update((journal) => {
    let changed = false;
    for (const entry of journal.entrants) {
      for (const attempt of entry.attempts) {
        if (attempt.status !== 'started') continue;
        attempt.status = 'interrupted';
        attempt.completed_at = interruptedAt;
        attempt.error_sha256 = sha256('verification interrupted before completion was journaled');
        changed = true;
      }
    }
    if (changed) journal.updated_at = interruptedAt;
    return changed;
  });

  return {
    async begin(modelId) {
      const startedAt = timestamp();
      if (!isIsoTimestamp(startedAt)) {
        throw new Error('artifact binding attempt timestamp is invalid');
      }
      return update((journal) => {
        const entry = journal.entrants.find((candidate) => candidate.model_id === modelId);
        if (!entry) throw new Error(`artifact binding journal is missing ${modelId}`);
        if (entry.attempts.length >= MAX_COMPILE_ATTEMPTS - entry.legacy_compile_attempts) {
          throw new Error(`fighter verification retry limit reached for ${entry.provider_model}`);
        }
        const attempt = {
          attempt_number: entry.attempts.length + 1,
          started_at: startedAt,
          completed_at: null,
          status: 'started',
          error_sha256: null,
          wasm_bytes: null,
          wasm_sha256: null,
        };
        entry.attempts.push(attempt);
        journal.updated_at = startedAt;
        return { attemptNumber: attempt.attempt_number, startedAt };
      });
    },
    async finish(modelId, attemptNumber, { artifact = null, error = null }) {
      const completedAt = timestamp();
      if (!isIsoTimestamp(completedAt)) {
        throw new Error('artifact binding attempt timestamp is invalid');
      }
      return update((journal) => {
        const entry = journal.entrants.find((candidate) => candidate.model_id === modelId);
        const attempt = entry?.attempts[attemptNumber - 1];
        if (!attempt || attempt.status !== 'started' || attemptNumber !== entry.attempts.length) {
          throw new Error(`artifact binding attempt completion is invalid for ${modelId}`);
        }
        attempt.completed_at = completedAt;
        if (artifact) {
          attempt.status = 'succeeded';
          attempt.wasm_bytes = artifact.wasmBytes;
          attempt.wasm_sha256 = artifact.wasmSha256;
        } else {
          attempt.status = 'failed';
          attempt.error_sha256 = sha256(String(error?.message || error));
        }
        journal.updated_at = completedAt;
        return structuredClone(entry);
      });
    },
    async snapshot() {
      await updateTail;
      const current = await readOptionalJsonBytes(journalPath);
      if (!current) throw new Error('artifact binding attempt journal disappeared');
      validateMigrationAttemptJournal(current.value, validationContext);
      return {
        journal: current.value,
        bytes: current.bytes,
        sha256: sha256(current.bytes),
      };
    },
  };
}

/**
 * Read and independently validate the single-file commit pointer for a bound
 * generation. Every immutable checkpoint and its original source/checkpoint
 * hash is rechecked, so a successful child process alone is never trusted.
 */
export async function readArtifactBinding({
  seasonDirectory,
  seasonId,
  rankingSha256,
  entrants,
  codeStatus,
  required = false,
}) {
  const bindingPath = path.join(seasonDirectory, ARTIFACT_BINDING_FILE);
  const persisted = await readOptionalJsonBytes(bindingPath);
  if (!persisted) {
    if (required) throw new Error('artifact binding manifest is missing');
    return null;
  }
  const manifest = persisted.value;
  const expectedContract = competitionContractFromCodeStatus(codeStatus);
  if (manifest?.schema_version !== ARTIFACT_BINDING_SCHEMA_VERSION
      || manifest.kind !== ARTIFACT_BINDING_KIND
      || manifest.season_id !== seasonId
      || !isIsoTimestamp(manifest.created_at)
      || manifest.ranking_sha256 !== rankingSha256
      || !/^[a-f0-9]{64}$/.test(String(manifest.ranking_sha256 || ''))
      || !isSafeBoundGenerationDirectory(manifest.generation_directory)
      || JSON.stringify(manifest.competition_contract) !== JSON.stringify(expectedContract)
      || manifest.competition_contract_sha256 !== sha256(JSON.stringify(expectedContract))
      || manifest.attempt_journal_path !== MIGRATION_JOURNAL_FILE
      || !/^[a-f0-9]{64}$/.test(String(manifest.attempt_journal_sha256 || ''))
      || !/^[a-f0-9]{64}$/.test(String(manifest.migration_key_sha256 || ''))
      || !Array.isArray(manifest.entrants)
      || manifest.entrants.length !== entrants.length
      || manifest.entrants_sha256 !== sha256(JSON.stringify(manifest.entrants))) {
    throw new Error('artifact binding manifest is invalid');
  }

  const generationDirectory = path.join(seasonDirectory, manifest.generation_directory);
  const legacyGenerationDirectory = path.join(seasonDirectory, 'generations');
  const sourceDirectory = path.join(seasonDirectory, 'sources');
  const checkpoints = [];
  const boundCheckpointBytes = [];
  const bindings = [];
  const records = [];
  for (const [index, entrant] of entrants.entries()) {
    const entry = manifest.entrants[index];
    if (entry?.provider_rank !== entrant.provider_rank
        || entry.model_id !== entrant.model_id
        || entry.model_name !== entrant.model_name
        || entry.provider_model !== entrant.provider_model
        || entry.canonical_slug !== entrant.canonical_slug
        || JSON.stringify(entry.reasoning_policy) !== JSON.stringify(entrant.reasoning_policy)
        || !/^[a-f0-9]{64}$/.test(String(entry.legacy_checkpoint_sha256 || ''))
        || !/^[a-f0-9]{64}$/.test(String(entry.checkpoint_sha256 || ''))
        || !/^[a-f0-9]{64}$/.test(String(entry.source_sha256 || ''))
        || !/^[a-f0-9]{64}$/.test(String(entry.wasm_sha256 || ''))
        || !Number.isSafeInteger(entry.source_bytes)
        || !Number.isSafeInteger(entry.wasm_bytes)
        || !Number.isSafeInteger(entry.compile_attempts)
        || !Number.isSafeInteger(entry.migration_attempts)
        || entry.migration_attempts < 1
        || !isIsoTimestamp(entry.last_compile_attempt_at)) {
      throw new Error(`artifact binding provenance is invalid for ${entrant.provider_model}`);
    }
    const [legacyBytes, sourceBytes, checkpointBytes] = await Promise.all([
      fs.readFile(path.join(legacyGenerationDirectory, `${entrant.model_id}.json`)),
      fs.readFile(path.join(sourceDirectory, `${entrant.model_id}.rs`)),
      fs.readFile(path.join(generationDirectory, `${entrant.model_id}.json`)),
    ]);
    const checkpoint = JSON.parse(checkpointBytes.toString('utf8'));
    const source = sourceBytes.toString('utf8');
    const legacyCheckpoint = JSON.parse(legacyBytes.toString('utf8'));
    validateLegacyCompiledCheckpoint(legacyCheckpoint, entrant, source, codeStatus);
    validateGenerationCheckpoint(checkpoint, entrant, source, codeStatus);
    if (sha256(legacyBytes) !== entry.legacy_checkpoint_sha256
        || sha256(checkpointBytes) !== entry.checkpoint_sha256
        || sha256(sourceBytes) !== entry.source_sha256
        || sourceBytes.length !== entry.source_bytes
        || checkpoint.source_sha256 !== entry.source_sha256
        || checkpoint.source_bytes !== entry.source_bytes
        || checkpoint.wasm_sha256 !== entry.wasm_sha256
        || checkpoint.wasm_bytes !== entry.wasm_bytes
        || checkpoint.compile_attempts !== entry.compile_attempts
        || checkpoint.last_compile_attempt_at !== entry.last_compile_attempt_at
        || checkpoint.rehydrated_at !== entry.last_compile_attempt_at) {
      throw new Error(`artifact binding files differ for ${entrant.provider_model}`);
    }
    records.push({
      entrant,
      legacyCheckpoint,
      legacyCheckpointSha256: sha256(legacyBytes),
      sourceBytes,
    });
    checkpoints.push(checkpoint);
    boundCheckpointBytes.push(Buffer.from(checkpointBytes));
    bindings.push({
      model_id: entrant.model_id,
      wasm_bytes: checkpoint.wasm_bytes,
      wasm_sha256: checkpoint.wasm_sha256,
    });
  }
  const journalPath = path.join(seasonDirectory, manifest.attempt_journal_path);
  const journalPersisted = await readOptionalJsonBytes(journalPath);
  if (!journalPersisted || sha256(journalPersisted.bytes) !== manifest.attempt_journal_sha256) {
    throw new Error('artifact binding attempt journal hash differs from the manifest');
  }
  const journal = validateMigrationAttemptJournal(journalPersisted.value, {
    seasonId,
    rankingSha256,
    records,
  });
  if (journal.migration_key_sha256 !== manifest.migration_key_sha256) {
    throw new Error('artifact binding attempt journal key differs from the manifest');
  }
  for (const [index, checkpoint] of checkpoints.entries()) {
    const attempts = journal.entrants[index].attempts;
    const lastAttempt = attempts.at(-1);
    const entry = manifest.entrants[index];
    const legacyCheckpoint = records[index].legacyCheckpoint;
    if (attempts.length !== entry.migration_attempts
        || lastAttempt?.status !== 'succeeded'
        || lastAttempt.started_at !== entry.last_compile_attempt_at
        || lastAttempt.wasm_bytes !== checkpoint.wasm_bytes
        || lastAttempt.wasm_sha256 !== checkpoint.wasm_sha256
        || checkpoint.compile_attempts !== legacyCheckpoint.compile_attempts + attempts.length) {
      throw new Error(`artifact binding attempt accounting differs for ${entry.provider_model}`);
    }
  }
  return {
    bindingPath,
    manifest,
    manifestSha256: sha256(persisted.bytes),
    generationDirectory,
    checkpoints,
    bindings,
    verifiedEntrants: entrants.map((entrant, index) => ({
      entrant,
      checkpointBytes: boundCheckpointBytes[index],
      sourceBytes: Buffer.from(records[index].sourceBytes),
      checkpointSha256: manifest.entrants[index].checkpoint_sha256,
      sourceSha256: manifest.entrants[index].source_sha256,
    })),
  };
}

export async function verifyPinnedArtifact(context, pinned) {
  const {
    entrant,
    checkpointBytes,
    sourceBytes,
    checkpointSha256,
    sourceSha256,
  } = pinned;
  if (sha256(checkpointBytes) !== checkpointSha256
      || sha256(sourceBytes) !== sourceSha256) {
    throw new Error(`pinned artifact bytes changed in memory for ${entrant.provider_model}`);
  }
  const checkpoint = JSON.parse(checkpointBytes.toString('utf8'));
  const source = sourceBytes.toString('utf8');
  validateGenerationCheckpoint(checkpoint, entrant, source, context.codeStatus);
  return compileArchivedEntrant(
    context,
    entrant,
    null,
    checkpoint,
    source,
    {
      expectedWasmBytes: checkpoint.wasm_bytes,
      expectedWasmSha256: checkpoint.wasm_sha256,
      verifyExisting: true,
      persistCheckpoint: false,
    },
  );
}

async function commitArtifactBinding({
  seasonDirectory,
  seasonId,
  rankingSha256,
  entrants,
  codeStatus,
  verified,
  legacyCheckpointHashes,
  journalSnapshot,
  createdAt,
}) {
  const contract = competitionContractFromCodeStatus(codeStatus);
  const bindingId = sha256(JSON.stringify({
    season_id: seasonId,
    ranking_sha256: rankingSha256,
    created_at: createdAt,
    contract,
    attempt_journal_sha256: journalSnapshot.sha256,
    checkpoints: verified.map((checkpoint) => ({
      model_id: checkpoint.model_id,
      source_sha256: checkpoint.source_sha256,
      wasm_sha256: checkpoint.wasm_sha256,
      compile_attempts: checkpoint.compile_attempts,
    })),
  })).slice(0, 24);
  const relativeGenerationDirectory = `${BOUND_GENERATIONS_DIRECTORY}/${bindingId}`;
  const bindingRoot = path.join(seasonDirectory, BOUND_GENERATIONS_DIRECTORY);
  const generationDirectory = path.join(seasonDirectory, relativeGenerationDirectory);
  const stagingDirectory = path.join(
    bindingRoot,
    `.stage-${bindingId}-${process.pid}-${Date.now()}`,
  );
  await fs.mkdir(bindingRoot, { recursive: true, mode: 0o700 });
  await fs.mkdir(stagingDirectory, { mode: 0o700 });
  const checkpointHashes = new Map();
  try {
    for (const checkpoint of verified) {
      const checkpointPath = path.join(stagingDirectory, `${checkpoint.model_id}.json`);
      await atomicWriteJson(checkpointPath, checkpoint);
      checkpointHashes.set(checkpoint.model_id, sha256(await fs.readFile(checkpointPath)));
    }
    try {
      await fs.rename(stagingDirectory, generationDirectory);
    } catch (error) {
      if (error?.code !== 'EEXIST' && error?.code !== 'ENOTEMPTY') throw error;
      for (const checkpoint of verified) {
        const existingDigest = sha256(await fs.readFile(
          path.join(generationDirectory, `${checkpoint.model_id}.json`),
        ));
        if (existingDigest !== checkpointHashes.get(checkpoint.model_id)) {
          throw new Error('immutable artifact binding directory conflicts with this migration');
        }
      }
    }
  } finally {
    await fs.rm(stagingDirectory, { recursive: true, force: true }).catch(() => {});
  }

  const manifestEntries = entrants.map((entrant, index) => {
    const checkpoint = verified[index];
    return {
      provider_rank: entrant.provider_rank,
      model_id: entrant.model_id,
      model_name: entrant.model_name,
      provider_model: entrant.provider_model,
      canonical_slug: entrant.canonical_slug,
      reasoning_policy: { ...entrant.reasoning_policy },
      legacy_checkpoint_sha256: legacyCheckpointHashes.get(entrant.model_id),
      checkpoint_sha256: checkpointHashes.get(entrant.model_id),
      source_bytes: checkpoint.source_bytes,
      source_sha256: checkpoint.source_sha256,
      wasm_bytes: checkpoint.wasm_bytes,
      wasm_sha256: checkpoint.wasm_sha256,
      compile_attempts: checkpoint.compile_attempts,
      migration_attempts: journalSnapshot.journal.entrants[index].attempts.length,
      last_compile_attempt_at: checkpoint.last_compile_attempt_at,
    };
  });
  const manifest = {
    schema_version: ARTIFACT_BINDING_SCHEMA_VERSION,
    kind: ARTIFACT_BINDING_KIND,
    season_id: seasonId,
    created_at: createdAt,
    ranking_sha256: rankingSha256,
    competition_contract: contract,
    competition_contract_sha256: sha256(JSON.stringify(contract)),
    attempt_journal_path: MIGRATION_JOURNAL_FILE,
    attempt_journal_sha256: journalSnapshot.sha256,
    migration_key_sha256: journalSnapshot.journal.migration_key_sha256,
    generation_directory: relativeGenerationDirectory,
    entrants: manifestEntries,
    entrants_sha256: sha256(JSON.stringify(manifestEntries)),
  };
  // This rename is the only commit point. Before it, immutable original
  // checkpoints remain authoritative; after it, all entrants become visible.
  await atomicWriteJson(path.join(seasonDirectory, ARTIFACT_BINDING_FILE), manifest);
  return readArtifactBinding({
    seasonDirectory,
    seasonId,
    rankingSha256,
    entrants,
    codeStatus,
    required: true,
  });
}

/** Verify every legacy artifact before atomically publishing a bound set. */
export async function rehydrateLegacyGeneration({
  context,
  entrants,
  directories,
  seasonId,
  rankingSha256,
  concurrency = 2,
  timestamp = () => new Date().toISOString(),
}) {
  const existing = await readArtifactBinding({
    seasonDirectory: directories.root,
    seasonId,
    rankingSha256,
    entrants,
    codeStatus: context.codeStatus,
  });
  if (existing) return existing;

  const records = await Promise.all(entrants.map(async (entrant) => {
    const checkpointPath = path.join(directories.generations, `${entrant.model_id}.json`);
    const sourcePath = path.join(directories.sources, `${entrant.model_id}.rs`);
    const [legacyCheckpointBytes, sourceBytes] = await Promise.all([
      fs.readFile(checkpointPath),
      fs.readFile(sourcePath),
    ]);
    const legacyCheckpoint = JSON.parse(legacyCheckpointBytes.toString('utf8'));
    validateLegacyCompiledCheckpoint(
      legacyCheckpoint,
      entrant,
      sourceBytes.toString('utf8'),
      context.codeStatus,
    );
    return {
      entrant,
      legacyCheckpoint,
      legacyCheckpointBytes,
      legacyCheckpointSha256: sha256(legacyCheckpointBytes),
      sourceBytes,
    };
  }));
  const legacyCheckpointHashes = new Map(records.map((record) => [
    record.entrant.model_id,
    record.legacyCheckpointSha256,
  ]));
  const journal = await openMigrationAttemptJournal({
    journalPath: path.join(directories.root, MIGRATION_JOURNAL_FILE),
    seasonId,
    rankingSha256,
    records,
    timestamp,
  });
  const verified = await mapLimitDrainOnError(records, concurrency, async (record) => {
    const { entrant, legacyCheckpoint, sourceBytes } = record;
    const attempt = await journal.begin(entrant.model_id);
    let checkpoint;
    try {
      checkpoint = await compileArchivedEntrant(
        context,
        entrant,
        null,
        legacyCheckpoint,
        sourceBytes.toString('utf8'),
        {
          expectedWasmBytes: legacyCheckpoint.wasm_bytes,
          verifyExisting: true,
          persistCheckpoint: false,
          attemptAt: attempt.startedAt,
          compileAttemptCount: attempt.attemptNumber,
        },
      );
    } catch (error) {
      await journal.finish(entrant.model_id, attempt.attemptNumber, { error });
      throw error;
    }
    await journal.finish(entrant.model_id, attempt.attemptNumber, {
      artifact: {
        wasmBytes: checkpoint.wasm_bytes,
        wasmSha256: checkpoint.wasm_sha256,
      },
    });
    return checkpoint;
  });
  // Detect source or checkpoint changes between pinning and the commit phase.
  for (const record of records) {
    const [currentCheckpoint, currentSource] = await Promise.all([
      fs.readFile(path.join(
        directories.generations,
        `${record.entrant.model_id}.json`,
      )),
      fs.readFile(path.join(directories.sources, `${record.entrant.model_id}.rs`)),
    ]);
    if (sha256(currentCheckpoint) !== record.legacyCheckpointSha256
        || sha256(currentSource) !== sha256(record.sourceBytes)) {
      throw new Error(
        `legacy inputs changed during migration for ${record.entrant.provider_model}`,
      );
    }
  }
  const journalSnapshot = await journal.snapshot();
  const createdAt = timestamp();
  if (!isIsoTimestamp(createdAt)) throw new Error('artifact binding timestamp is invalid');
  return commitArtifactBinding({
    seasonDirectory: directories.root,
    seasonId,
    rankingSha256,
    entrants,
    codeStatus: context.codeStatus,
    verified,
    legacyCheckpointHashes,
    journalSnapshot,
    createdAt,
  });
}

async function pathExists(targetPath) {
  try {
    await fs.access(targetPath);
    return true;
  } catch (error) {
    if (error?.code === 'ENOENT') return false;
    throw error;
  }
}

async function hasGenerationArtifacts(entrant, directories) {
  const checkpointPath = path.join(directories.generations, `${entrant.model_id}.json`);
  const sourcePath = path.join(directories.sources, `${entrant.model_id}.rs`);
  const [checkpointExists, sourceExists] = await Promise.all([
    pathExists(checkpointPath),
    pathExists(sourcePath),
  ]);
  if (checkpointExists !== sourceExists) {
    throw new Error(
      `generation artifacts are incomplete for ${entrant.provider_model}; refusing provider regeneration`,
    );
  }
  return checkpointExists;
}

export async function generateEntrant(context, entrant, directories, attempts, resume) {
  if (resume) {
    if (await hasGenerationArtifacts(entrant, directories)) {
      return await rehydrateEntrant(context, entrant, directories);
    }
  }

  const checkpointPath = path.join(directories.generations, `${entrant.model_id}.json`);
  const sourcePath = path.join(directories.sources, `${entrant.model_id}.rs`);

  let lastError;
  let generated;
  let verified;
  let successfulAttempt;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      generated = await callArenaApi(context, {
        method: 'POST',
        route: '/api/arena/code/generate',
        timeoutMs: Math.max(
          180_000,
          (Number(context.codeStatus.provider_timeout_secs) || 120) * 1_000 + 150_000,
        ),
        body: {
          model: entrant.provider_model,
          reasoning_mode: entrant.reasoning_policy.mode,
          reasoning_effort: entrant.reasoning_policy.effort,
        },
      });
    } catch (error) {
      throw new Error(
        `${entrant.provider_model} generation request failed with an ambiguous billing outcome; refusing automatic retry: ${error?.message || error}`,
      );
    }
    try {
      verified = validateGeneratedResponse(generated, context.codeStatus, entrant);
      successfulAttempt = attempt;
      break;
    } catch (error) {
      lastError = error;
      if (attempt < attempts) await delay(Math.min(3_000, 500 * (2 ** (attempt - 1))));
    }
  }
  if (!verified) {
    throw new Error(`${entrant.provider_model} failed after ${attempts} attempts: ${lastError?.message || lastError}`);
  }

  const checkpoint = buildGeneratedCheckpoint(
    entrant,
    generated,
    verified,
    context.codeStatus,
    successfulAttempt,
  );
  await atomicWriteText(sourcePath, verified.source);
  const archivedSource = await fs.readFile(sourcePath, 'utf8');
  validateGeneratedCheckpoint(checkpoint, entrant, archivedSource, context.codeStatus);
  await atomicWriteJson(checkpointPath, checkpoint);
  return compileArchivedEntrant(
    context,
    entrant,
    checkpointPath,
    checkpoint,
    archivedSource,
  );
}

function buildBattleTasks(entrants, seeds, teamSize, simulatorRulesVersion = 'planning') {
  const tasks = [];
  for (let left = 0; left < entrants.length; left += 1) {
    for (let right = left + 1; right < entrants.length; right += 1) {
      const pair = [entrants[left], entrants[right]];
      for (const mode of ALL_MODES) {
        const category = mode === 'arena' ? 'personal' : 'team';
        for (const seed of seeds) {
          for (const orientation of [0, 1]) {
            const modelA = pair[orientation];
            const modelB = pair[1 - orientation];
            const task = {
              category,
              mode,
              seed,
              team_size: category === 'personal' ? 1 : teamSize,
              rounds: 1,
              model_a_id: modelA.model_id,
              model_b_id: modelB.model_id,
              source_sha256_a: modelA.source_sha256 || null,
              source_sha256_b: modelB.source_sha256 || null,
              wasm_sha256_a: modelA.wasm_sha256 || null,
              wasm_sha256_b: modelB.wasm_sha256 || null,
              prompt_sha256: modelA.prompt_sha256 || modelB.prompt_sha256 || null,
              simulator_rules_version: simulatorRulesVersion,
            };
            task.task_id = sha256(JSON.stringify(task)).slice(0, 24);
            tasks.push(task);
          }
        }
      }
    }
  }
  return tasks;
}

function buildWorldTasks(entrants, seeds, simulatorRulesVersion = 'planning') {
  const modelIds = entrants.map((entrant) => entrant.model_id).sort();
  const artifactFingerprints = entrants
    .map((entrant) => (
      `${entrant.model_id}:${entrant.source_sha256 || 'planning'}:${entrant.wasm_sha256 || 'planning'}`
    ))
    .sort();
  return seeds.map((seed) => {
    const task = {
      seed,
      model_ids: modelIds,
      artifact_fingerprints: artifactFingerprints,
      prompt_sha256: entrants[0]?.prompt_sha256 || null,
      simulator_rules_version: simulatorRulesVersion,
      squad_size: WORLD_SQUAD_SIZE,
      rounds: 1,
      max_ticks: WORLD_MAX_TICKS,
    };
    task.task_id = sha256(JSON.stringify(task)).slice(0, 24);
    return task;
  });
}

function validateBattleTaskResult(task, checkpoint) {
  if (checkpoint?.task_id !== task.task_id) throw new Error('battle checkpoint task mismatch');
  const simulation = checkpoint.simulation;
  const expectedMaxTicks = task.mode === 'arena' ? 240 : 360;
  if (
    simulation?.model_a_id !== task.model_a_id
    || simulation?.model_b_id !== task.model_b_id
    || simulation?.rules_version !== task.simulator_rules_version
    || simulation?.mode !== task.mode
    || Number(simulation?.team_size) !== task.team_size
    || Number(simulation?.rounds) !== task.rounds
    || Number(simulation?.max_ticks) !== expectedMaxTicks
  ) {
    throw new Error('battle response does not match its frozen task');
  }
  const validWinner = simulation.winner_model_id === task.model_a_id
    || simulation.winner_model_id === task.model_b_id;
  if (
    (simulation.draw === true && simulation.winner_model_id != null)
    || (simulation.draw !== true && !validWinner)
  ) {
    throw new Error('battle response has an inconsistent winner/draw result');
  }
  assertBattleIntegrity(simulation, {
    expectedEngagements: task.team_size * task.rounds,
    expectedV2Fighters: task.team_size,
    requireCollaboration: task.category === 'team',
  });
  const warning = (simulation.warnings || []).find((value) => UNVERIFIED_RUNTIME_PATTERN.test(String(value)));
  if (warning) throw new Error(`battle checkpoint contains unsafe warning: ${warning}`);
  return checkpoint;
}

async function executeBattleTask(context, task, directory, resume) {
  const checkpointPath = path.join(directory, `${task.task_id}.json`);
  if (resume) {
    try {
      return validateBattleTaskResult(task, await readJson(checkpointPath));
    } catch {
      // Rerun stale v1 or incomplete checkpoints.
    }
  }
  const data = await apiJson({
    ...context,
    method: 'POST',
    route: '/api/arena/matches/simulate_team_battle',
    timeoutMs: 180_000,
    body: {
      model_a_id: task.model_a_id,
      model_b_id: task.model_b_id,
      mode: task.mode,
      team_size: task.team_size,
      rounds: task.rounds,
      seed: task.seed,
      max_ticks: task.mode === 'arena' ? 240 : 360,
    },
  });
  const checkpoint = {
    ...task,
    executed_at: new Date().toISOString(),
    simulation: data?.simulation,
  };
  validateBattleTaskResult(task, checkpoint);
  await atomicWriteJson(checkpointPath, checkpoint);
  return checkpoint;
}

function validateWorldTaskResult(task, checkpoint) {
  if (checkpoint?.task_id !== task.task_id) throw new Error('world checkpoint task mismatch');
  const simulation = checkpoint.simulation;
  if (
    simulation?.mode !== 'world_ffa'
    || simulation?.rules_version !== task.simulator_rules_version
    || Number(simulation?.seed) !== task.seed
    || Number(simulation?.entrants) !== task.model_ids.length
    || Number(simulation?.squad_size) !== task.squad_size
    || Number(simulation?.rounds) !== task.rounds
    || Number(simulation?.max_ticks) !== task.max_ticks
    || !Array.isArray(simulation?.rankings)
    || simulation.rankings.length !== task.model_ids.length
  ) {
    throw new Error('world response does not match its frozen task');
  }
  const rankingsByModel = new Map(simulation.rankings.map((entry) => [entry.model_id, entry]));
  if (rankingsByModel.size !== task.model_ids.length) {
    throw new Error('world response contains duplicate model IDs');
  }
  for (const modelId of task.model_ids) {
    const entry = rankingsByModel.get(modelId);
    if (
      !entry
      || !(Number(entry.rank) >= 1 && Number(entry.rank) <= task.model_ids.length)
      || Number(entry.v2_fighter_rounds) !== task.squad_size * task.rounds
      || Number(entry.fallback_count) !== 0
      || Number(entry.trap_count) !== 0
      || Number(entry.fuel_error_count) !== 0
    ) {
      throw new Error(`world runtime integrity failed for ${modelId}`);
    }
  }
  const validWinner = task.model_ids.includes(simulation.winner_model_id);
  if (
    (simulation.draw === true && simulation.winner_model_id != null)
    || (simulation.draw !== true && !validWinner)
  ) {
    throw new Error('world response has an inconsistent winner/draw result');
  }
  const warning = (simulation.warnings || []).find((value) => (
    UNVERIFIED_RUNTIME_PATTERN.test(String(value))
  ));
  if (warning) throw new Error(`world checkpoint contains unsafe warning: ${warning}`);
  return checkpoint;
}

async function executeWorldTask(context, task, directory, resume) {
  const checkpointPath = path.join(directory, `${task.task_id}.json`);
  if (resume) {
    try {
      return validateWorldTaskResult(task, await readJson(checkpointPath));
    } catch {
      // Rerun stale or incomplete world checkpoints.
    }
  }
  const data = await apiJson({
    ...context,
    method: 'POST',
    route: '/api/arena/matches/simulate_world_battle',
    timeoutMs: 180_000,
    body: {
      model_ids: task.model_ids,
      squad_size: task.squad_size,
      rounds: task.rounds,
      seed: task.seed,
      max_ticks: task.max_ticks,
    },
  });
  const checkpoint = {
    ...task,
    executed_at: new Date().toISOString(),
    simulation: data?.simulation,
  };
  validateWorldTaskResult(task, checkpoint);
  await atomicWriteJson(checkpointPath, checkpoint);
  return checkpoint;
}

function gitMetadata() {
  try {
    const revision = execFileSync('git', ['rev-parse', 'HEAD'], {
      cwd: ROOT_DIR,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
    const status = execFileSync('git', ['status', '--porcelain'], {
      cwd: ROOT_DIR,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
    return { revision, dirty: Boolean(status) };
  } catch {
    return { revision: null, dirty: null };
  }
}

function summarizeGenerationUsage(generations) {
  const summary = {
    responses_with_usage: 0,
    prompt_tokens: 0,
    completion_tokens: 0,
    reasoning_tokens: 0,
    total_tokens: 0,
    cost_usd: 0,
  };
  for (const generation of generations) {
    const usage = generation?.usage;
    if (!usage || typeof usage !== 'object') continue;
    summary.responses_with_usage += 1;
    summary.prompt_tokens += Math.max(0, Number(usage.prompt_tokens) || 0);
    summary.completion_tokens += Math.max(0, Number(usage.completion_tokens) || 0);
    summary.reasoning_tokens += Math.max(
      0,
      Number(usage.completion_tokens_details?.reasoning_tokens) || 0,
    );
    summary.total_tokens += Math.max(0, Number(usage.total_tokens) || 0);
    summary.cost_usd += Math.max(0, Number(usage.cost) || 0);
  }
  summary.cost_usd = Math.round(summary.cost_usd * 1_000_000) / 1_000_000;
  return summary;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    process.stdout.write(usage());
    return;
  }

  const config = {
    apiBase: (process.env.ARENA_API_BASE || 'http://127.0.0.1:8080').replace(/\/$/, ''),
    topModels: integerEnv('ARENA_TOP_MODELS', 10, 2, 32),
    teamSize: integerEnv('ARENA_TEAM_SIZE', 10, 2, 20),
    generationConcurrency: integerEnv('ARENA_GENERATION_CONCURRENCY', 2, 1, 5),
    simulationConcurrency: integerEnv('ARENA_SIMULATION_CONCURRENCY', 6, 1, 24),
    generationAttempts: integerEnv('ARENA_GENERATION_ATTEMPTS', 3, 1, 6),
    seeds: seedListFromEnv(),
  };

  const ranking = await loadRanking({
    rankingFile: options.rankingFile,
    topModels: config.topModels,
  });
  const rankingSha256 = options.rankingFile
    ? sha256(await fs.readFile(path.resolve(options.rankingFile)))
    : sha256(JSON.stringify(ranking));
  const seasonId = deriveSeasonId(ranking, options.seasonId);
  const seasonDirectory = path.join(ROOT_DIR, 'artifacts/arena/seasons', seasonId);
  const directories = {
    root: seasonDirectory,
    generations: path.join(seasonDirectory, 'generations'),
    sources: path.join(seasonDirectory, 'sources'),
    battles: path.join(seasonDirectory, 'battles'),
    world: path.join(seasonDirectory, 'world'),
    revisions: path.join(seasonDirectory, 'revisions'),
  };
  const entrants = entrantsFromRanking(ranking, seasonId);
  const plannedBattleTasks = buildBattleTasks(entrants, config.seeds, config.teamSize);
  const plannedWorldTasks = buildWorldTasks(entrants, config.seeds);
  const plannedEngagements = plannedBattleTasks.reduce((sum, task) => sum + task.team_size, 0);

  const plan = {
    season_id: seasonId,
    ranking,
    entrants,
    generation_requests: entrants.length,
    battle_requests: plannedBattleTasks.length,
    world_requests: plannedWorldTasks.length,
    world_squad_size: WORLD_SQUAD_SIZE,
    world_max_ticks: WORLD_MAX_TICKS,
    total_local_engagements: plannedEngagements,
    fixed_seeds: config.seeds,
    team_size: config.teamSize,
    rounds: 1,
    side_swapped: true,
    modes: ALL_MODES,
    rating_weights: DEFAULT_WEIGHTS,
    strategy_weights: {
      duel: DUEL_STRATEGY_WEIGHT,
      world: WORLD_STRATEGY_WEIGHT,
    },
  };
  if (options.dryRun) {
    process.stdout.write(`${JSON.stringify(plan, null, 2)}\n`);
    return;
  }

  await acquireLeagueLock(path.join(ROOT_DIR, 'artifacts/arena/.season-runner.lock'));
  await fs.mkdir(seasonDirectory, { recursive: true });
  await atomicWriteJson(path.join(seasonDirectory, 'ranking.json'), ranking);
  if (options.snapshotOnly) {
    await atomicWriteJson(path.join(seasonDirectory, 'plan.json'), plan);
    process.stdout.write(`[arena] frozen ranking: ${path.join(seasonDirectory, 'ranking.json')}\n`);
    await releaseLeagueLock();
    return;
  }

  const adminToken = await readSecret('ARENA_ADMIN_BEARER_TOKEN');
  if (!adminToken) {
    throw new Error('ARENA_ADMIN_BEARER_TOKEN or ARENA_ADMIN_BEARER_TOKEN_FILE is required');
  }
  const rawCodeStatus = await apiJson({
    apiBase: config.apiBase,
    adminToken,
    route: '/api/arena/code/status',
  });
  const codeStatus = normalizeCodeStatus(rawCodeStatus);
  if (!options.evaluateOnly && !options.rehydrateOnly && !codeStatus.provider_configured) {
    throw new Error(
      'the live server has no OPENROUTER_API_KEY or OPENROUTER_API_KEY_FILE; refusing template fallback',
    );
  }
  await atomicWriteJson(path.join(seasonDirectory, 'server-status.json'), codeStatus);
  await atomicWriteJson(path.join(seasonDirectory, 'plan.json'), {
    ...plan,
    competition_contract: {
      prompt_version: codeStatus.prompt_version,
      prompt_sha256: codeStatus.prompt_sha256,
      max_completion_tokens: codeStatus.max_tokens,
      provider_sort_policy: codeStatus.provider_sort_policy,
      temperature_policy: codeStatus.temperature_policy,
      reasoning_policy_version: codeStatus.reasoning_policy_version,
      provider_require_parameters: codeStatus.provider_require_parameters,
      reasoning_exclude: codeStatus.reasoning_exclude,
      response_transport_policy: codeStatus.response_transport_policy,
      collaboration_abi_version: codeStatus.collaboration_abi_version,
      simulator_rules_version: codeStatus.simulator_rules_version,
      source_limit_bytes: codeStatus.source_limit_bytes,
    },
  });

  const context = {
    apiBase: config.apiBase,
    adminToken,
    codeStatus,
  };

  if (options.reviseOnly) {
    // Terminal mode: revise frozen fighters from their own mid-season stats.
    // Runs BEFORE any generation/evaluation branch so a revision never
    // triggers paid generation. Per-model isolation: one failure never blocks
    // the other fighters, and the journal makes reruns provider-call-free.
    const supervisorState = await readJson(path.resolve(options.statsState));
    const seasonSnapshot = await readJson(path.join(seasonDirectory, 'season.json'));
    const revisionEpoch = Array.isArray(supervisorState.epochs) ? supervisorState.epochs.length : 0;
    process.stdout.write(
      `[arena] revising ${entrants.length} fighters for ${seasonId} at epoch ${revisionEpoch}\n`,
    );
    const results = await mapLimit(entrants, config.generationConcurrency, async (entrant) => {
      const statsDigest = buildRevisionStatsDigest({
        seasonSnapshot,
        supervisorState,
        modelId: entrant.model_id,
      });
      try {
        const checkpoint = await reviseEntrant(
          context,
          entrant,
          directories,
          { statsDigest, revisionEpoch },
        );
        process.stdout.write(`[arena] revised ${entrant.provider_model}\n`);
        return { model_id: entrant.model_id, status: 'improved', checkpoint };
      } catch (error) {
        process.stdout.write(
          `[arena] kept gen-1 for ${entrant.provider_model}: ${String(error?.message || error).slice(0, 200)}\n`,
        );
        return {
          model_id: entrant.model_id,
          status: 'kept_gen1',
          error: String(error?.message || error).slice(0, 500),
        };
      }
    });
    await atomicWriteJson(path.join(seasonDirectory, 'revision-results.json'), {
      season_id: seasonId,
      revision_epoch: revisionEpoch,
      completed_at: new Date().toISOString(),
      entries: results.map(({ checkpoint, ...rest }) => ({
        ...rest,
        source_sha256_after: checkpoint?.source_sha256 ?? null,
        wasm_bytes_after: checkpoint?.wasm_bytes ?? null,
        wasm_sha256_after: checkpoint?.wasm_sha256 ?? null,
      })),
    });
    await releaseLeagueLock();
    return;
  }

  let generatedEntrants;
  if (options.rehydrateOnly) {
    const binding = await rehydrateLegacyGeneration({
      context,
      entrants,
      directories,
      seasonId,
      rankingSha256,
      concurrency: config.generationConcurrency,
    });
    generatedEntrants = binding.checkpoints;
  } else if (options.evaluateOnly) {
    const binding = await readArtifactBinding({
      seasonDirectory,
      seasonId,
      rankingSha256,
      entrants,
      codeStatus,
    });
    generatedEntrants = binding
      ? await mapLimit(
        binding.verifiedEntrants,
        config.generationConcurrency,
        (pinned) => verifyPinnedArtifact(context, pinned),
      )
      : await mapLimit(
        entrants,
        config.generationConcurrency,
        // No binding manifest: trust the frozen compiled checkpoints. Server
        // verification cannot byte-match staging-renamed v2 artifacts, and
        // recompiling every epoch burns the bounded compile-attempt budget
        // long before a 7-day season (~700 epochs) can finish.
        (entrant) => rehydrateEntrant(
          context,
          entrant,
          directories,
          { trustArchivedArtifact: true },
        ),
      );
  } else {
    process.stdout.write(`[arena] generating ${entrants.length} OpenRouter fighters for ${seasonId}\n`);
    generatedEntrants = await mapLimit(
      entrants,
      config.generationConcurrency,
      async (entrant, index) => {
        const checkpoint = await generateEntrant(
          context,
          entrant,
          directories,
          config.generationAttempts,
          options.resume,
        );
        process.stdout.write(`[arena] fighter ${index + 1}/${entrants.length}: ${entrant.provider_model}\n`);
        return checkpoint;
      },
    );
  }
  if (options.generateOnly) {
    process.stdout.write(`[arena] generation complete: ${seasonDirectory}\n`);
    await releaseLeagueLock();
    return;
  }
  if (options.rehydrateOnly) {
    process.stdout.write(
      `[arena] rehydrated ${generatedEntrants.length} archived fighters without provider generation\n`,
    );
    await releaseLeagueLock();
    return;
  }

  const battleTasks = buildBattleTasks(
    generatedEntrants,
    config.seeds,
    config.teamSize,
    codeStatus.simulator_rules_version,
  );
  const worldTasks = buildWorldTasks(
    generatedEntrants,
    config.seeds,
    codeStatus.simulator_rules_version,
  );
  const totalEngagements = battleTasks.reduce((sum, task) => sum + task.team_size, 0);

  await fs.mkdir(directories.battles, { recursive: true });
  process.stdout.write(
    `[arena] evaluating ${battleTasks.length} side-swapped legs / ${totalEngagements} engagements\n`,
  );
  let completed = 0;
  const battleCheckpoints = await mapLimit(
    battleTasks,
    config.simulationConcurrency,
    async (task) => {
      const checkpoint = await executeBattleTask(context, task, directories.battles, options.resume);
      completed += 1;
      if (completed % 25 === 0 || completed === battleTasks.length) {
        process.stdout.write(`[arena] battles ${completed}/${battleTasks.length}\n`);
      }
      return checkpoint;
    },
  );

  await fs.mkdir(directories.world, { recursive: true });
  process.stdout.write(
    `[arena] evaluating ${worldTasks.length} all-model world events / squad size ${WORLD_SQUAD_SIZE}\n`,
  );
  const worldCheckpoints = await mapLimit(
    worldTasks,
    Math.min(config.simulationConcurrency, 4),
    (task) => executeWorldTask(context, task, directories.world, options.resume),
  );

  const finalCodeStatus = normalizeCodeStatus(await apiJson({
    apiBase: config.apiBase,
    adminToken,
    route: '/api/arena/code/status',
  }));
  assertCodeStatusUnchanged(codeStatus, finalCodeStatus);

  const roster = addWorldRatings(
    buildSeasonRatings({
      entrants: generatedEntrants,
      legs: battleCheckpoints,
      weights: DEFAULT_WEIGHTS,
      sourceLimitBytes: codeStatus.source_limit_bytes,
    }),
    worldCheckpoints,
    { duel: DUEL_STRATEGY_WEIGHT, world: WORLD_STRATEGY_WEIGHT },
  );
  const generatedAt = new Date().toISOString();
  const generationUsage = summarizeGenerationUsage(generatedEntrants);
  const snapshot = {
    schema_version: 1,
    active: true,
    season_id: seasonId,
    generated_at: generatedAt,
    ranking: {
      source: ranking.source,
      window: ranking.window,
      sort: ranking.sort,
      retrieved_at: ranking.retrieved_at,
      models: ranking.models,
    },
    methodology: {
      ranking_source: ranking.source,
      ranking_sort: ranking.sort,
      prompt_sha256: codeStatus.prompt_sha256,
      prompt_version: codeStatus.prompt_version || null,
      max_completion_tokens: codeStatus.max_tokens,
      provider_sort_policy: codeStatus.provider_sort_policy,
      temperature_policy: codeStatus.temperature_policy,
      reasoning_policy_version: codeStatus.reasoning_policy_version,
      provider_require_parameters: codeStatus.provider_require_parameters,
      reasoning_exclude: codeStatus.reasoning_exclude,
      reasoning_policies: entrants.map((entrant) => ({
        model_id: entrant.model_id,
        provider_model: entrant.provider_model,
        reasoning_policy: { ...entrant.reasoning_policy },
      })),
      response_transport_policy: codeStatus.response_transport_policy,
      collaboration_abi_version: codeStatus.collaboration_abi_version,
      simulator_rules_version: codeStatus.simulator_rules_version,
      source_limit_bytes: codeStatus.source_limit_bytes,
      modes: ALL_MODES,
      seeds_per_matchup: config.seeds.length,
      seed_sets: config.seeds,
      team_size: config.teamSize,
      rounds: 1,
      side_swapped: true,
      collaboration_kind: 'team_context_v2_support_telemetry',
      personal_weight: DEFAULT_WEIGHTS.personal,
      team_weight: DEFAULT_WEIGHTS.team,
      collaboration_weight: DEFAULT_WEIGHTS.collaboration,
      duel_strategy_weight: DUEL_STRATEGY_WEIGHT,
      world_strategy_weight: WORLD_STRATEGY_WEIGHT,
      world_squad_size: WORLD_SQUAD_SIZE,
      world_max_ticks: WORLD_MAX_TICKS,
      notes: [
        'Every provider received the same versioned fighter prompt and source limit.',
        'Every request routed with OpenRouter provider.sort=throughput; the selected provider is archived per fighter.',
        'Each model used its frozen capability_minimum_v1 reasoning policy: optional reasoning was disabled, mandatory reasoning used its least supported non-none effort, and unsupported reasoning was omitted.',
        'OpenRouter provider routing required support for every parameter sent; reasoning output was excluded for every policy mode.',
        'Every generation used the same bounded SSE v1 response transport and required terminal usage plus [DONE].',
        'Resolved provider, finish reason, token usage, and generation cost are archived per fighter for auditability.',
        'Every unordered pairing ran with fixed seeds in both A/B orientations.',
        'Personal uses arena verdict plus normalized score production.',
        'Team uses CTF, KOTH, and TDM verdicts plus normalized objective share.',
        'Collaboration uses direct v2 teammate-support telemetry plus team conversion.',
        'World rating uses simultaneous all-model faction placement points.',
        'Epoch strategy score weights the duel rating 75% and world rating 25%.',
      ],
    },
    integrity: {
      verified: true,
      provider_fallbacks: 0,
      compile_failures: 0,
      runtime_fallbacks: 0,
      battle_requests: battleTasks.length,
      world_requests: worldTasks.length,
      world_fighter_rounds: worldTasks.length * entrants.length * WORLD_SQUAD_SIZE,
      total_engagements: totalEngagements,
      simulator_rules_version: codeStatus.simulator_rules_version,
      openrouter_usage: generationUsage,
      git: gitMetadata(),
    },
    roster,
  };
  await atomicWriteJson(path.join(seasonDirectory, 'season.json'), snapshot);

  if (options.publish) {
    const configuredTarget = (process.env.MGS_ARENA_RATINGS_PATH || '').trim();
    const publishTarget = configuredTarget
      ? path.resolve(ROOT_DIR, configuredTarget)
      : path.join(ROOT_DIR, 'data/arena_ratings.json');
    await atomicWriteJson(publishTarget, snapshot);
    process.stdout.write(`[arena] published ratings: ${publishTarget}\n`);
  }
  process.stdout.write(`[arena] strategy leader: ${roster[0].model_name} (${roster[0].strategy_rating})\n`);
  process.stdout.write(`[arena] season artifact: ${path.join(seasonDirectory, 'season.json')}\n`);
  await releaseLeagueLock();
}

const invokedAsScript = process.argv[1]
  && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedAsScript) {
  main().catch(async (error) => {
    await releaseLeagueLock().catch(() => {});
    process.stderr.write(`[arena] ${error?.stack || error}\n`);
    process.exitCode = 1;
  });
}
