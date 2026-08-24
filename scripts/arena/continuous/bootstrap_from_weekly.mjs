#!/usr/bin/env node

// Continuous Model League — shadow bootstrap from the weekly season.
//
// Imports the CURRENT weekly roster into a continuous-league state in the
// shadow directory (artifacts/arena/continuous-shadow/) without any paid
// generation: the weekly season's compiled schema-v2 generation checkpoints
// and Rust sources are copied verbatim into EACH of the four tracks' fighter
// records (L0/L1/L2/L3 all begin from the SAME compiled v1 artifacts, then
// diverge by policy), and every track's roster is built with fresh league
// bookkeeping (submissions_used=1, artifact v1, rating 50, zeroed record).
//
// The weekly checkpoints pin the same contract fields the continuous
// league's materialization expects (generation.mjs); identity fields are
// re-pointed per evaluation day at materialize time, so the weekly arena
// model ids inside the checkpoints are harmless.
//
// Idempotent: refuses to overwrite a non-empty roster unless --force.

import { promises as fs } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { mascotFor } from '../mascots.mjs';
import { TRACKS } from './league.mjs';
import { writeFighterRecord } from './generation.mjs';
import {
  loadOrCreateState,
  validateState,
  writeState,
} from './state.mjs';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT_DIR = path.resolve(SCRIPT_DIR, '../../..');
const DEFAULT_WEEKLY_STATE_DIR = path.join(ROOT_DIR, 'artifacts/arena/weekly-supervisor');
const DEFAULT_SEASONS_DIR = path.join(ROOT_DIR, 'artifacts/arena/seasons');
const DEFAULT_SHADOW_DIR = path.join(ROOT_DIR, 'artifacts/arena/continuous-shadow');
const MAX_ERROR_CHARS = 2_000;

const readJson = async (target) => JSON.parse(await fs.readFile(target, 'utf8'));

async function latestWeekDirectory(weeklyStateDir) {
  const weeks = (await fs.readdir(weeklyStateDir))
    .filter((name) => /^\d{4}-W(?:0[1-9]|[1-4]\d|5[0-3])$/.test(name))
    .sort();
  if (weeks.length === 0) {
    throw new Error(`no weekly supervisor state found in ${weeklyStateDir}`);
  }
  return path.join(weeklyStateDir, weeks.at(-1));
}

/**
 * Import the latest (or given) weekly week roster into the shadow league.
 * Returns { state, imported } after validating and persisting the state.
 */
export async function bootstrapFromWeekly({
  weeklyStateDir = DEFAULT_WEEKLY_STATE_DIR,
  seasonsDirectory = DEFAULT_SEASONS_DIR,
  shadowDirectory = DEFAULT_SHADOW_DIR,
  weekDirectory = null,
  force = false,
  now = new Date(),
  log = (line) => process.stdout.write(`[arena-continuous-bootstrap] ${line}\n`),
} = {}) {
  const weekDir = weekDirectory ?? await latestWeekDirectory(weeklyStateDir);
  const weekly = await readJson(path.join(weekDir, 'state.json'));
  if (weekly?.generation?.completed !== true || !Array.isArray(weekly.artifact_bindings)) {
    throw new Error(`weekly week ${path.basename(weekDir)} has no completed generation to import`);
  }
  const seasonId = weekly.season_id;
  const seasonDirectory = path.join(seasonsDirectory, seasonId);
  const ranking = await readJson(path.join(seasonDirectory, 'ranking.json'));
  const rankingById = new Map(
    (Array.isArray(ranking?.models) ? ranking.models : []).map((model) => [model.id, model]),
  );

  const { state } = await loadOrCreateState(shadowDirectory, { now }).catch(async (error) => {
    throw new Error(`shadow league state is unusable: ${error?.message || error}`);
  });
  const occupied = TRACKS.filter((trackId) => state.tracks[trackId].roster.length > 0);
  if (occupied.length > 0 && !force) {
    throw new Error(
      `shadow league roster is not empty (tracks ${occupied.join(', ')}); use --force to overwrite`,
    );
  }

  const joinedAt = (now instanceof Date ? now : new Date(now)).toISOString();
  const imported = [];
  for (const binding of weekly.artifact_bindings) {
    const arenaId = String(binding?.model_id || '');
    const checkpoint = await readJson(
      path.join(seasonDirectory, 'generations', `${arenaId}.json`),
    );
    if (checkpoint?.schema_version !== 2 || checkpoint?.stage !== 'compiled'
        || checkpoint?.compiled !== true) {
      throw new Error(`weekly generation checkpoint is not a compiled v2 artifact for ${arenaId}`);
    }
    if (checkpoint.wasm_sha256 !== binding.wasm_sha256) {
      // Expected after the weekly revision epoch recompiles fighters: the
      // binding froze the gen-1 digest, the checkpoint carries the live one.
      log(`${checkpoint.provider_model}: checkpoint wasm differs from the frozen binding (revised artifact); using the checkpoint`);
    }
    const source = await fs.readFile(
      path.join(seasonDirectory, 'sources', `${arenaId}.rs`),
      'utf8',
    );
    const providerModel = checkpoint.provider_model;
    const ranked = rankingById.get(providerModel);
    const meta = {
      model_id: providerModel,
      slug: checkpoint.canonical_slug,
      model_name: checkpoint.model_name,
      reasoning_policy: checkpoint.reasoning_policy,
      pricing: ranked?.pricing ?? null,
      context_length: ranked?.context_length ?? null,
      created: ranked?.created ?? null,
    };
    imported.push({
      model_id: providerModel,
      slug: checkpoint.canonical_slug || providerModel,
      mascot: mascotFor(providerModel),
      joined_at: joinedAt,
      submissions_used: 1,
      artifact: {
        wasm_sha256: checkpoint.wasm_sha256,
        source_sha256: checkpoint.source_sha256,
        prompt_sha256: checkpoint.prompt_sha256,
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

    // Every track starts from the SAME compiled v1 artifact (a copy per
    // track); the tracks then diverge by policy.
    for (const trackId of TRACKS) {
      await writeFighterRecord(
        path.join(shadowDirectory, 'tracks', trackId),
        providerModel,
        { checkpoint, source, meta },
      );
    }
  }

  const next = {
    ...state,
    tracks: Object.fromEntries(TRACKS.map((trackId) => [
      trackId,
      { ...state.tracks[trackId], roster: imported.map((entry) => ({ ...entry })) },
    ])),
    updated_at: joinedAt,
  };
  validateState(next);
  await writeState(shadowDirectory, next);
  log(
    `imported ${imported.length} models from ${path.basename(weekDir)} (${seasonId}) `
    + `into ${shadowDirectory} (tracks ${TRACKS.join(', ')}, identical v1 artifacts)`,
  );
  for (const entry of imported) {
    log(`  ${entry.model_id} — v1, rating 50, wasm ${entry.artifact.wasm_sha256.slice(0, 12)}…`);
  }
  return { state: next, imported };
}

async function main() {
  const force = process.argv.slice(2).includes('--force');
  await bootstrapFromWeekly({ force });
}

const invokedAsScript = process.argv[1]
  && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedAsScript) {
  main().then(() => {
    process.exitCode = 0;
  }).catch((error) => {
    process.stderr.write(
      `[arena-continuous-bootstrap] ${String(error?.message || error).slice(0, MAX_ERROR_CHARS)}\n`,
    );
    process.exitCode = 1;
  });
}
