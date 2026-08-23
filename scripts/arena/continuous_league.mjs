#!/usr/bin/env node

// Continuous Model League — supervisor (Tasks 1-2).
//
// Parses --once/--shadow, acquires the owned lock, loads-or-creates the
// league state at artifacts/arena/continuous/state.json (override with
// ARENA_CONTINUOUS_STATE_DIR), and runs the daily cycle:
//
//   1. evaluate — round-robin battles among the roster via
//      run_top10_season.mjs --evaluate-only (season id
//      continuous-<league_id>-day<day_index>, 4 deterministic seeds), then
//      applyBattleRatings, day_index/days_in_league bookkeeping, and a
//      compact snapshot appended to history/<YYYY-MM-DD>.json (idempotent
//      per day_index). Stale fighter checkpoints after a server contract
//      change trigger one recompile-only rebind + retry; if the contract
//      change is not reparable that way, the cycle fails closed with a
//      "manual rebind required" error.
//   2. retire   — shouldRetire per roster model; retired entries move to
//      retired[] with a reason and final stats, plus an announcement
//   3. feedback — every 48h, each active model with submissions left gets a
//      revision: an improvement brief from its own recent battles feeds the
//      server's /api/arena/code/revise contract; accepted revisions bump the
//      artifact version (parent-linked) and resync roster digests with the
//      fighter record, any failure still consumes the submission. Every
//      attempt is appended to submissions.jsonl and announced. Skipped in
//      --shadow mode (no codegen calls), like recruit.
//   4. recruit  — fill open slots from the live OpenRouter ranking; bots are
//      generated through the server admin API (the runner only generates
//      full rosters). Skipped entirely in --shadow mode.
//
// --shadow mode never touches the live state dir: it uses
// <stateDir>-shadow (or ARENA_CONTINUOUS_SHADOW_DIR). The resident 24h loop
// lands in Task 5.

import { promises as fs } from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { acquireOwnedLock, releaseOwnedLock } from './owned_lock.mjs';
import { arenaApiJson } from './arena_api_client.mjs';
import { mascotFor } from './mascots.mjs';
import { entrantsFromRanking } from './run_top10_season.mjs';
import { deterministicSeedPack, isoWeekId } from './weekly_supervisor.mjs';
import {
  MAX_ANNOUNCEMENTS,
  MAX_ROSTER_SIZE,
  atomicWriteJson,
  loadOrCreateState,
  validateState,
  writeState,
} from './continuous/state.mjs';
import {
  MAX_SUBMISSIONS,
  RETIRE_RATING,
  RETIRE_WINRATE,
  applyBattleRatings,
  daysInLeague,
  eligibleChallengers,
  feedbackDue,
  nextVersion,
  shouldRetire,
  winRate,
} from './continuous/league.mjs';
import { runSeasonRunner } from './continuous/runner.mjs';
import { buildBrief, sampleModelBattles } from './continuous/brief.mjs';
import {
  apiBaseFromEnv,
  compileFighterSource,
  compileRevision,
  entrantFromChallenger,
  fighterKeyFor,
  generateFighter,
  loadCodeStatus,
  materializeSeasonFighter,
  rankingModelFromMeta,
  readAdminToken,
  readFighterRecord,
  requestRevision,
  writeFighterRecord,
} from './continuous/generation.mjs';

const sha256 = (value) => createHash('sha256').update(value).digest('hex');

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT_DIR = path.resolve(SCRIPT_DIR, '../..');
const DEFAULT_STATE_DIR = path.join(ROOT_DIR, 'artifacts/arena/continuous');
const MAX_ERROR_CHARS = 2_000;
const SEASON_ID_PATTERN = /^[A-Za-z0-9_.:-]{1,128}$/;
const EVALUATION_SEED_COUNT = 4;

function parseArgs(argv) {
  const flags = { once: false, shadow: false };
  for (const arg of argv) {
    if (arg === '--once') flags.once = true;
    else if (arg === '--shadow') flags.shadow = true;
    else throw new Error(`unknown argument: ${arg}`);
  }
  return flags;
}

/**
 * State directory for this run. Shadow mode is hard-isolated from the live
 * league: it defaults to `<live dir>-shadow` and can be overridden with
 * ARENA_CONTINUOUS_SHADOW_DIR, so a shadow run can never load or mutate the
 * live league state. A shadow directory that resolves to the live directory
 * is a configuration error and refused here.
 */
export function stateDirectoryFromEnv(flags = {}, env = process.env) {
  const live = String(env.ARENA_CONTINUOUS_STATE_DIR || '').trim();
  const liveDirectory = live ? path.resolve(live) : DEFAULT_STATE_DIR;
  if (flags.shadow) {
    const shadow = String(env.ARENA_CONTINUOUS_SHADOW_DIR || '').trim();
    const shadowDirectory = shadow ? path.resolve(shadow) : `${liveDirectory}-shadow`;
    if (shadowDirectory === liveDirectory) {
      throw new Error('ARENA_CONTINUOUS_SHADOW_DIR must differ from the live state directory');
    }
    return shadowDirectory;
  }
  return liveDirectory;
}

function seasonIdFor(state) {
  const seasonId = `continuous-${state.league_id}-day${state.day_index}`;
  if (!SEASON_ID_PATTERN.test(seasonId)) {
    throw new Error(`continuous season ID is invalid: ${seasonId.slice(0, 160)}`);
  }
  return seasonId;
}

async function readJson(targetPath) {
  return JSON.parse(await fs.readFile(targetPath, 'utf8'));
}

/**
 * Append a compact per-evaluation snapshot to the day's history file.
 * Idempotent per (league_id, day_index): if the cycle crashed after the
 * history write but before the state write, the retried cycle re-runs the
 * same day_index and must not duplicate the snapshot.
 */
async function appendHistorySnapshot(stateDirectory, snapshot) {
  const historyPath = path.join(
    stateDirectory,
    'history',
    `${snapshot.at.slice(0, 10)}.json`,
  );
  let entries = [];
  try {
    const parsed = await readJson(historyPath);
    if (Array.isArray(parsed)) entries = parsed;
  } catch {
    // First snapshot of the day.
  }
  const duplicate = entries.some((entry) => (
    entry?.league_id === snapshot.league_id && entry?.day_index === snapshot.day_index
  ));
  if (!duplicate) {
    entries.push(snapshot);
    await atomicWriteJson(historyPath, entries);
  }
  return historyPath;
}

/**
 * Step 1 — evaluate. Build a runner-shaped ranking file from the roster's
 * fighter records, materialize the season's generation layout, publish the
 * day's derived arena model ids (battles resolve <model_id>.wasm server
 * side), then run the season runner in --evaluate-only mode with four
 * deterministic seeds and fold the resulting season.json into the roster.
 */
async function evaluateRoster({ state, stateDirectory, rootDirectory, deps, log, nowMs }) {
  const runRunner = deps.runRunner ?? runSeasonRunner;
  const seasonId = seasonIdFor(state);
  const seasonDirectory = path.join(rootDirectory, 'artifacts/arena/seasons', seasonId);

  const fighters = [];
  const models = [];
  for (const [index, entry] of state.roster.entries()) {
    const fighter = deps.readFighter
      ? await deps.readFighter(stateDirectory, entry.model_id)
      : await readFighterRecord(stateDirectory, entry.model_id);
    fighters.push(fighter);
    models.push(rankingModelFromMeta(fighter.meta, index + 1));
  }
  const ranking = {
    schema_version: 1,
    retrieved_at: new Date(nowMs).toISOString(),
    source: 'continuous-league-roster',
    window: 'weekly',
    sort: 'top-weekly',
    models,
  };
  const rankingPath = path.join(stateDirectory, 'rankings', `${seasonId}.json`);
  await atomicWriteJson(rankingPath, ranking);
  const entrants = entrantsFromRanking(ranking, seasonId);

  const adminToken = deps.adminToken ?? await readAdminToken();
  const apiBase = deps.apiBase ?? apiBaseFromEnv();
  for (const [index, entrant] of entrants.entries()) {
    await materializeSeasonFighter({ seasonDirectory, entrant, fighter: fighters[index] });
    if (deps.publishFighter) {
      await deps.publishFighter({ apiBase, adminToken, entrant, source: fighters[index].source });
    } else {
      await compileFighterSource({
        apiBase,
        adminToken,
        entrant,
        source: fighters[index].source,
        apiClient: deps.apiClient ?? arenaApiJson,
      });
    }
  }

  const seeds = deterministicSeedPack(
    isoWeekId(new Date(state.created_at)),
    state.day_index,
    EVALUATION_SEED_COUNT,
  );
  await runRunner(
    ['--ranking-file', rankingPath, '--season-id', seasonId, '--evaluate-only', '--no-publish'],
    {
      env: {
        ARENA_SEEDS: seeds.join(','),
        ARENA_TOP_MODELS: String(models.length),
      },
    },
  );

  const seasonJson = await readJson(path.join(seasonDirectory, 'season.json'));
  state.roster = applyBattleRatings(state.roster, seasonJson);
  const completedDay = state.day_index;
  state.day_index += 1;
  const historyPath = await appendHistorySnapshot(stateDirectory, {
    at: new Date(nowMs).toISOString(),
    league_id: state.league_id,
    day_index: completedDay,
    season_id: seasonId,
    roster: state.roster.map((entry) => ({
      model_id: entry.model_id,
      slug: entry.slug,
      rating: entry.rating,
      wins: entry.wins,
      losses: entry.losses,
      draws: entry.draws,
      matches: entry.matches,
    })),
  });
  log(
    `evaluate: ${seasonId} complete (${models.length} models, seeds ${seeds.join('/')}); `
    + `snapshot appended to ${path.relative(rootDirectory, historyPath)}`,
  );
}

// The runner's checkpoint validator rejects a materialized checkpoint whose
// frozen competition contract no longer matches the live server status with
// "generation checkpoint is stale or unverified" (see validateCheckpointAudit
// in run_top10_season.mjs). Only that specific failure is recoverable here.
const STALE_CHECKPOINT_PATTERN = /stale or unverified/i;

/**
 * Rebind every roster fighter to the server's current competition contract.
 *
 * Policy: this is a *recompile*, never a regeneration — the stored source is
 * recompiled through the same admin compile contract used at recruit time, so
 * no codegen runs, no submission is consumed, and every roster artifact keeps
 * its version/parent lineage. The fighter record's checkpoint contract fields
 * (prompt/rules versions, limits, policies) are re-pointed at the live
 * code/status and the wasm digests refreshed from the recompile.
 *
 * What rebind can repair: simulator rules version, source limit, max-token,
 * ABI, and policy-metadata bumps. What it cannot repair: a generation-prompt
 * change — the archived provider response is pinned to the original prompt
 * hash, so rehydration validation still fails after rebind. In that case
 * evaluateWithRebind fails closed with a "manual rebind required" error
 * instead of bricking the league with an opaque validation message.
 */
async function rebindFighters({ state, stateDirectory, deps, log }) {
  const adminToken = deps.adminToken ?? await readAdminToken();
  const apiBase = deps.apiBase ?? apiBaseFromEnv();
  const apiClient = deps.apiClient ?? arenaApiJson;
  const codeStatus = deps.codeStatus
    ?? await loadCodeStatus({ apiBase, adminToken, apiClient });
  const reboundAt = new Date().toISOString();
  for (const entry of state.roster) {
    const fighter = deps.readFighter
      ? await deps.readFighter(stateDirectory, entry.model_id)
      : await readFighterRecord(stateDirectory, entry.model_id);
    const entrant = {
      model_id: fighter.checkpoint.model_id,
      model_name: fighter.checkpoint.model_name,
      provider_model: fighter.checkpoint.provider_model,
      canonical_slug: fighter.checkpoint.canonical_slug,
    };
    const wasm = deps.recompileFighter
      ? await deps.recompileFighter({ apiBase, adminToken, entrant, source: fighter.source })
      : await compileFighterSource({ apiBase, adminToken, entrant, source: fighter.source, apiClient });
    const checkpoint = {
      ...fighter.checkpoint,
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
      wasm_bytes: wasm.wasmBytes,
      wasm_sha256: wasm.wasmSha256,
      compiled_at: reboundAt,
    };
    if (deps.writeFighter) {
      await deps.writeFighter(stateDirectory, entry.model_id, { ...fighter, checkpoint });
    } else {
      await writeFighterRecord(stateDirectory, entry.model_id, { ...fighter, checkpoint });
    }
  }
  log(`evaluate: rebound ${state.roster.length} fighter(s) to the current server contract`);
}

/**
 * Evaluate, with one rebind-and-retry on stale checkpoints. A server contract
 * change must never brick the league silently: the first stale-checkpoint
 * failure triggers a recompile-only rebind of every fighter and exactly one
 * retry; if that does not fix it (e.g. the generation prompt itself changed),
 * the cycle fails closed with an explicit manual-rebind error.
 */
async function evaluateWithRebind(context) {
  const { log } = context;
  try {
    await evaluateRoster(context);
  } catch (error) {
    if (!STALE_CHECKPOINT_PATTERN.test(String(error?.message || error))) throw error;
    log(
      'evaluate: fighter checkpoints are stale against the current server contract; '
      + 'rebinding via recompile (no codegen, no new submission, versions unchanged)',
    );
    try {
      await rebindFighters(context);
    } catch (rebindError) {
      throw new Error(
        'server contract changed, manual rebind required: '
        + String(rebindError?.message || rebindError).slice(0, 500),
      );
    }
    try {
      await evaluateRoster(context);
    } catch (retryError) {
      if (STALE_CHECKPOINT_PATTERN.test(String(retryError?.message || retryError))) {
        throw new Error(
          'server contract changed, manual rebind required: fighter checkpoints still '
          + 'fail validation after a recompile rebind (the generation prompt likely changed): '
          + String(retryError?.message || retryError).slice(0, 500),
        );
      }
      throw retryError;
    }
  }
}

/** Step 2 — retire models under the bar into the hall of fame. */
function retireExhausted({ state, log, nowMs }) {
  const at = new Date(nowMs).toISOString();
  const staying = [];
  for (const entry of state.roster) {
    if (!shouldRetire(entry, nowMs)) {
      staying.push(entry);
      continue;
    }
    const reasons = [];
    if (entry.rating < RETIRE_RATING) reasons.push(`rating ${entry.rating} < ${RETIRE_RATING}`);
    if (winRate(entry) < RETIRE_WINRATE) {
      reasons.push(`win rate ${(winRate(entry) * 100).toFixed(1)}% < ${RETIRE_WINRATE * 100}%`);
    }
    const reason = `${entry.days_in_league} days in league, `
      + `submissions ${entry.submissions_used}/${MAX_SUBMISSIONS}: ${reasons.join(' and ')}`;
    state.retired.push({ ...entry, retired_at: at, reason });
    state.announcements.push({
      type: 'retirement',
      model_id: entry.model_id,
      slug: entry.slug,
      mascot: entry.mascot,
      reason,
      stats: {
        rating: entry.rating,
        wins: entry.wins,
        losses: entry.losses,
        draws: entry.draws,
        matches: entry.matches,
        days_in_league: entry.days_in_league,
        submissions_used: entry.submissions_used,
      },
      at,
    });
    log(`retire: ${entry.model_id} (${reason})`);
  }
  state.roster = staying;
}

/**
 * Append one submission ledger record to submissions.jsonl (append-only).
 * Idempotent per (model_id, version_attempted): version numbers are
 * monotonically increasing per model, so the pair is a natural idempotency
 * key and a retried cycle can never duplicate a lineage record.
 */
async function appendSubmissionRecord(stateDirectory, record) {
  const target = path.join(stateDirectory, 'submissions.jsonl');
  await fs.mkdir(path.dirname(target), { recursive: true });
  let existing = [];
  try {
    existing = (await fs.readFile(target, 'utf8')).trim().split('\n')
      .filter(Boolean)
      .map((line) => JSON.parse(line));
  } catch {
    // First record of the league.
  }
  const duplicate = existing.some((entry) => (
    entry?.model_id === record.model_id
    && entry?.version_attempted === record.version_attempted
  ));
  if (duplicate) return;
  await fs.appendFile(target, `${JSON.stringify(record)}\n`, { mode: 0o600 });
}

/**
 * Per-attempt revision journal (one file per attempt key), the same
 * one-chance pattern as the runner's revision journal: the attempt is
 * journaled BEFORE the provider call, so a crash anywhere in the attempt can
 * never cause a duplicate codegen call for the same
 * {league_id, day_index, model_id, version_attempted}.
 */
function revisionJournalPath(stateDirectory, modelId, versionAttempted) {
  return path.join(
    stateDirectory,
    'revision-journal',
    `${fighterKeyFor(modelId)}-v${versionAttempted}.json`,
  );
}

/**
 * One feedback revision attempt for a single roster model, journaled in three
 * stages so a crash mid-attempt is resumable without re-paying codegen:
 *
 *   pending  — journaled before the provider call. A crash here leaves the
 *              call outcome ambiguous, so a rerun consumes the attempt as
 *              'interrupted' (one chance means one call) instead of
 *              re-calling the provider.
 *   revised  — provider response + verified fields journaled. A rerun
 *              resumes at the compile stage (compiles are idempotent, no new
 *              codegen).
 *   compiled — revised checkpoint journaled. A rerun resumes at the commit
 *              stage (fighter record write, jsonl ledger, journal finalize).
 *
 * A journal with a final outcome means every durable step completed and only
 * the in-memory state was lost (crash before end-of-cycle writeState); the
 * outcome is then re-applied to the roster without any IO beyond the state
 * write itself.
 *
 * Per spec, ANY failure — codegen, compile, or validation — still consumes
 * the submission (submissions_used += 1) while the previous artifact stays in
 * place; only an accepted revision bumps the artifact version and resyncs the
 * roster artifact digests with the fighter record. Untagged post-compile
 * errors record outcome 'interrupted' rather than a misclassified
 * 'codegen_failed'.
 */
async function reviseModel({ state, entry, stateDirectory, rootDirectory, deps, log, nowMs, at }) {
  const versionAttempted = entry.artifact.version + 1;
  const journalPath = revisionJournalPath(stateDirectory, entry.model_id, versionAttempted);
  let journal = await readJson(journalPath).catch(() => null);

  const announce = (outcome) => {
    state.announcements.push({
      type: 'revision',
      model_id: entry.model_id,
      slug: entry.slug,
      mascot: entry.mascot,
      version: versionAttempted,
      outcome,
      at,
    });
  };

  // Re-apply an already-final attempt after a pre-writeState crash.
  if (journal?.outcome) {
    entry.submissions_used += 1;
    if (journal.outcome === 'accepted') {
      entry.artifact = {
        ...nextVersion(entry),
        wasm_sha256: journal.checkpoint.wasm_sha256,
        source_sha256: journal.checkpoint.source_sha256,
        prompt_sha256: journal.checkpoint.prompt_sha256,
      };
    }
    announce(journal.outcome);
    log(`feedback: ${entry.model_id} revision ${journal.outcome} (re-applied journaled attempt)`);
    return;
  }

  const adminToken = deps.adminToken ?? await readAdminToken();
  const apiBase = deps.apiBase ?? apiBaseFromEnv();
  const readFighter = () => (deps.readFighter
    ? deps.readFighter(stateDirectory, entry.model_id)
    : readFighterRecord(stateDirectory, entry.model_id));
  const writeFighter = (record) => (deps.writeFighter
    ? deps.writeFighter(stateDirectory, entry.model_id, record)
    : writeFighterRecord(stateDirectory, entry.model_id, record));

  // Commit stage: durable writes first (fighter record, jsonl ledger with
  // idempotency dedup, journal finalize), in-memory state last. A failure
  // here propagates and is retried from the journaled stage on the next run.
  const finalize = async (outcome, { checkpoint = null, source = null, error = null } = {}) => {
    if (outcome === 'accepted') {
      const fighter = await readFighter();
      await writeFighter({ checkpoint, source, meta: fighter.meta });
    }
    await appendSubmissionRecord(stateDirectory, {
      model_id: entry.model_id,
      slug: entry.slug,
      version_attempted: versionAttempted,
      parent_version: entry.artifact.version,
      prompt_sha256: checkpoint?.prompt_sha256 ?? entry.artifact.prompt_sha256,
      brief_sha256: journal?.brief_sha256 ?? null,
      source_sha256: checkpoint?.source_sha256 ?? null,
      wasm_sha256: checkpoint?.wasm_sha256 ?? null,
      compile_attempts: outcome === 'codegen_failed' || outcome === 'interrupted' ? 0 : 1,
      outcome,
      at,
    });
    journal = {
      ...journal,
      outcome,
      completed_at: at,
      ...(error ? { error: String(error?.message || error).slice(0, 500) } : {}),
    };
    await atomicWriteJson(journalPath, journal);
    entry.submissions_used += 1;
    if (outcome === 'accepted') {
      entry.artifact = {
        ...nextVersion(entry),
        wasm_sha256: checkpoint.wasm_sha256,
        source_sha256: checkpoint.source_sha256,
        prompt_sha256: checkpoint.prompt_sha256,
      };
    }
    announce(outcome);
    log(
      `feedback: ${entry.model_id} revision ${outcome} `
      + `(submission ${entry.submissions_used}/${MAX_SUBMISSIONS})`,
    );
  };

  let finalizing = false;
  try {
    if (!journal) {
      // Fresh attempt: build the brief, journal BEFORE the provider call.
      const sample = deps.sampleBattles
        ? await deps.sampleBattles({
          seasonsDirectory: path.join(rootDirectory, 'artifacts/arena/seasons'),
          leagueId: state.league_id,
          dayIndex: state.day_index,
          modelId: entry.model_id,
        })
        : await sampleModelBattles({
          seasonsDirectory: path.join(rootDirectory, 'artifacts/arena/seasons'),
          leagueId: state.league_id,
          dayIndex: state.day_index,
          modelId: entry.model_id,
        });
      const brief = buildBrief({ model: entry, records: sample });
      journal = {
        schema_version: 1,
        league_id: state.league_id,
        day_index: state.day_index,
        model_id: entry.model_id,
        version_attempted: versionAttempted,
        parent_version: entry.artifact.version,
        started_at: at,
        phase: 'pending',
        brief_sha256: sha256(brief),
        brief,
      };
      await atomicWriteJson(journalPath, journal);
      const fighter = await readFighter();
      const entrant = {
        model_id: fighter.checkpoint.model_id,
        model_name: fighter.checkpoint.model_name,
        provider_model: fighter.checkpoint.provider_model,
        canonical_slug: fighter.checkpoint.canonical_slug,
        reasoning_policy: fighter.checkpoint.reasoning_policy,
      };
      const request = deps.requestRevision
        ? await deps.requestRevision({ apiBase, adminToken, entrant, source: fighter.source, brief })
        : await requestRevision({
          apiBase,
          adminToken,
          entrant,
          source: fighter.source,
          brief,
          apiClient: deps.apiClient ?? arenaApiJson,
        });
      journal = {
        ...journal,
        phase: 'revised',
        request: {
          response: request.response,
          verified: request.verified,
          codeStatus: request.codeStatus ?? null,
        },
      };
      await atomicWriteJson(journalPath, journal);
    }
    if (journal.phase === 'pending') {
      // The provider call is ambiguous: consume the attempt as interrupted.
      finalizing = true;
      await finalize('interrupted', {
        error: new Error('revision attempt interrupted before the provider response was journaled'),
      });
      return;
    }
    if (journal.phase === 'revised') {
      const fighter = await readFighter();
      const entrant = {
        model_id: fighter.checkpoint.model_id,
        model_name: fighter.checkpoint.model_name,
        provider_model: fighter.checkpoint.provider_model,
        canonical_slug: fighter.checkpoint.canonical_slug,
        reasoning_policy: fighter.checkpoint.reasoning_policy,
      };
      const request = {
        response: journal.request.response,
        verified: journal.request.verified,
        codeStatus: journal.request.codeStatus ?? deps.codeStatus ?? undefined,
        brief: journal.brief,
      };
      const compiled = deps.compileRevision
        ? await deps.compileRevision({
          apiBase,
          adminToken,
          entrant,
          request,
          previousCheckpoint: fighter.checkpoint,
        })
        : await compileRevision({
          apiBase,
          adminToken,
          entrant,
          request,
          previousCheckpoint: fighter.checkpoint,
          apiClient: deps.apiClient ?? arenaApiJson,
        });
      journal = {
        ...journal,
        phase: 'compiled',
        checkpoint: compiled.checkpoint,
        source: compiled.source,
      };
      await atomicWriteJson(journalPath, journal);
    }
    if (journal.phase === 'compiled') {
      finalizing = true;
      await finalize('accepted', {
        checkpoint: journal.checkpoint,
        source: journal.source,
      });
    }
  } catch (error) {
    if (finalizing) throw error; // commit-stage IO failure: rerun resumes from the journal
    const outcome = error?.phase === 'compile'
      ? 'compile_failed'
      : error?.phase === 'codegen'
        ? 'codegen_failed'
        : 'interrupted';
    await finalize(outcome, { error });
  }
}

/**
 * Step 3 — feedback. Every 48h (feedbackDue), each active model with
 * submissions left gets one revision attempt built from its own recent
 * battles. last_feedback_at advances whenever the round runs, even with no
 * eligible models, so the cadence gate stays honest.
 */
async function feedbackRevisions({ state, stateDirectory, rootDirectory, deps, log, nowMs }) {
  if (!feedbackDue(state, nowMs)) return;
  const at = new Date(nowMs).toISOString();
  const candidates = state.roster.filter((entry) => entry.submissions_used < MAX_SUBMISSIONS);
  for (const entry of candidates) {
    await reviseModel({ state, entry, stateDirectory, rootDirectory, deps, log, nowMs, at });
  }
  state.last_feedback_at = at;
  if (candidates.length > 0) {
    log(`feedback: ${candidates.length} revision attempt(s) completed`);
  }
}

/**
 * Step 4 — recruit. Take the next eligible challengers from a fresh
 * --dry-run ranking and generate a bot for each through the server admin
 * API. A generation failure consumes no submission: the model simply never
 * enters the roster and is retried on the next cycle. A dry-run/ranking
 * failure (OpenRouter outage, malformed plan) skips the whole recruit step
 * per spec — "skip cycle step, never block the loop" — so the evaluate
 * results already folded into the state are still persisted.
 */
async function recruitChallengers({ state, stateDirectory, deps, log, nowMs }) {
  const openSlots = MAX_ROSTER_SIZE - state.roster.length;
  if (openSlots < 1) return;
  const runRunner = deps.runRunner ?? runSeasonRunner;
  let rankingModels;
  try {
    const { stdout } = await runRunner(['--dry-run'], {});
    const plan = JSON.parse(stdout);
    if (!Array.isArray(plan?.ranking?.models)) {
      throw new Error('season dry-run did not return a ranking');
    }
    rankingModels = plan.ranking.models;
  } catch (error) {
    log(
      `recruit: skipped, live ranking unavailable: `
      + `${String(error?.message || error).slice(0, 300)}`,
    );
    return;
  }
  const challengers = eligibleChallengers(rankingModels, state, nowMs).slice(0, openSlots);
  if (challengers.length === 0) {
    log('recruit: no eligible challengers in the live ranking');
    return;
  }
  const adminToken = deps.adminToken ?? await readAdminToken();
  const apiBase = deps.apiBase ?? apiBaseFromEnv();
  const at = new Date(nowMs).toISOString();
  for (const challenger of challengers) {
    const providerId = String(challenger.id);
    const slug = String(challenger.canonical_slug || providerId);
    try {
      const entrant = entrantFromChallenger(challenger, seasonIdFor(state), at);
      const generated = deps.generateFighter
        ? await deps.generateFighter({ apiBase, adminToken, entrant })
        : await generateFighter({
          apiBase,
          adminToken,
          entrant,
          apiClient: deps.apiClient ?? arenaApiJson,
        });
      const meta = {
        model_id: providerId,
        slug,
        model_name: challenger.name || providerId,
        reasoning_policy: { ...entrant.reasoning_policy },
        pricing: challenger.pricing ?? null,
        context_length: challenger.context_length ?? null,
        created: challenger.created ?? null,
      };
      if (deps.writeFighter) {
        await deps.writeFighter(stateDirectory, providerId, {
          checkpoint: generated.checkpoint,
          source: generated.source,
          meta,
        });
      } else {
        await writeFighterRecord(stateDirectory, providerId, {
          checkpoint: generated.checkpoint,
          source: generated.source,
          meta,
        });
      }
      const mascot = mascotFor(providerId);
      state.roster.push({
        model_id: providerId,
        slug,
        mascot,
        joined_at: at,
        submissions_used: 1,
        artifact: {
          wasm_sha256: generated.checkpoint.wasm_sha256,
          source_sha256: generated.checkpoint.source_sha256,
          prompt_sha256: generated.checkpoint.prompt_sha256,
          version: 1,
          parent_version: null,
        },
        rating: 50,
        wins: 0,
        losses: 0,
        draws: 0,
        matches: 0,
        days_in_league: 0,
        status: 'active',
      });
      state.announcements.push({
        type: 'entrant',
        model_id: providerId,
        slug,
        mascot,
        provider_rank: challenger.provider_rank ?? null,
        at,
      });
      log(`recruit: ${providerId} enters the league (submission 1/${MAX_SUBMISSIONS})`);
    } catch (error) {
      log(
        `recruit: generation failed for ${providerId}: `
        + `${String(error?.message || error).slice(0, 300)}; slot stays open`,
      );
    }
  }
}

export async function runCycle({
  state,
  flags,
  stateDirectory,
  rootDirectory = ROOT_DIR,
  deps = {},
}) {
  const nowMs = deps.nowMs ?? Date.now();
  const log = deps.log ?? ((line) => process.stdout.write(`[arena-continuous] ${line}\n`));

  // Tenure advances even on days when evaluation cannot run.
  state.roster = state.roster.map((entry) => ({
    ...entry,
    days_in_league: daysInLeague(entry, nowMs),
  }));

  // 1. evaluate — the runner battles at least two fighters.
  if (state.roster.length >= 2) {
    await evaluateWithRebind({ state, stateDirectory, rootDirectory, deps, log, nowMs });
  } else if (state.roster.length > 0) {
    log(`evaluate: skipped, roster has ${state.roster.length} model(s), need at least 2`);
  } else {
    log('evaluate: skipped, roster is empty');
  }

  // 2. retire
  retireExhausted({ state, log, nowMs });

  // 3. feedback — 48h revision rounds with lineage-linked artifacts.
  //    Shadow mode never calls the codegen/admin APIs, so it is skipped
  //    there just like recruit.
  if (flags.shadow) {
    if (feedbackDue(state, nowMs)) log('shadow: feedback skipped');
  } else {
    await feedbackRevisions({ state, stateDirectory, rootDirectory, deps, log, nowMs });
  }

  // 4. recruit — skipped in shadow mode, which never performs recruit
  //    side-effects (codegen, registration, or admin API calls).
  if (flags.shadow) {
    if (state.roster.length < MAX_ROSTER_SIZE) log('shadow: recruit skipped');
  } else {
    await recruitChallengers({ state, stateDirectory, deps, log, nowMs });
  }

  state.announcements = state.announcements.slice(-MAX_ANNOUNCEMENTS);
  state.updated_at = new Date(nowMs).toISOString();
  return state;
}

async function main() {
  const flags = parseArgs(process.argv.slice(2));
  const stateDirectory = stateDirectoryFromEnv(flags);
  const mode = flags.shadow ? 'shadow' : 'live';

  const supervisorLock = await acquireOwnedLock(
    path.join(stateDirectory, 'supervisor.lock'),
    {
      activeMessage: (owner) => `continuous league supervisor is already running as PID ${owner.pid}`,
    },
  );
  try {
    const { state, created } = await loadOrCreateState(stateDirectory);
    validateState(state);

    const nowMs = Date.now();
    process.stdout.write(
      `[arena-continuous] state ${created ? 'created' : 'loaded'}: `
      + `league=${state.league_id} day=${state.day_index} mode=${mode}`
      + ` dir=${stateDirectory}`
      + ` roster=${state.roster.length}/${MAX_ROSTER_SIZE} retired=${state.retired.length}`
      + ` announcements=${state.announcements.length}`
      + ` feedback_due=${feedbackDue(state, nowMs)}\n`,
    );

    const next = await runCycle({ state, flags, stateDirectory });
    await writeState(stateDirectory, next);
    if (!flags.once) {
      process.stdout.write(
        '[arena-continuous] single pass complete; the resident loop is scheduled for Task 5\n',
      );
    }
    process.stdout.write('[arena-continuous] cycle complete\n');
  } finally {
    await releaseOwnedLock(supervisorLock).catch(() => {});
  }
}

const invokedAsScript = process.argv[1]
  && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedAsScript) {
  main().then(() => {
    process.exitCode = 0;
  }).catch((error) => {
    process.stderr.write(`[arena-continuous] ${String(error?.message || error).slice(0, MAX_ERROR_CHARS)}\n`);
    process.exitCode = 1;
  });
}
