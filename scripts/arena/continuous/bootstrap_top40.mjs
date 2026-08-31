#!/usr/bin/env node

// Continuous Model League — top-40 seeding from the live OpenRouter ranking.
//
// Expands the league roster to 40 models per track: the current roster
// models keep their entries and ratings; the missing slots are filled from
// the live top-weekly ranking (skipping recently-retired models). Each new
// model gets ONE codegen call — its compiled v1 artifact is then copied to
// all four tracks (L0/L1/L2/L3), which diverge by policy afterwards.
//
// Idempotent: refuses to run against a roster that already has 30+ models
// unless --force (which only fills still-missing slots — existing entries
// and artifacts are never regenerated). Per-model generation failures are
// reported and leave the slot open; the daily recruit step retries them.

import { promises as fs } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { arenaApiJson } from '../arena_api_client.mjs';
import { mascotFor } from '../mascots.mjs';
import { TRACKS, eligibleChallengers } from './league.mjs';
import {
  apiBaseFromEnv,
  entrantFromChallenger,
  fetchEligibleRanking,
  generateFighter,
  readAdminToken,
  writeFighterRecord,
} from './generation.mjs';
import {
  MAX_ROSTER_SIZE,
  loadOrCreateState,
  validateState,
  writeState,
} from './state.mjs';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT_DIR = path.resolve(SCRIPT_DIR, '../../..');
const DEFAULT_SHADOW_DIR = path.join(ROOT_DIR, 'artifacts/arena/continuous-shadow');
const MAX_ERROR_CHARS = 2_000;
const RANKING_FETCH_TOP_N = 60;
const EXISTING_ROSTER_GUARD = 30;

/**
 * Seed the league to `targetSize` models per track from the live ranking.
 * Returns { state, results: { imported, failed, kept } }.
 */
export async function bootstrapTop40({
  shadowDirectory = process.env.ARENA_CONTINUOUS_SHADOW_DIR
    ? path.resolve(process.env.ARENA_CONTINUOUS_SHADOW_DIR)
    : DEFAULT_SHADOW_DIR,
  targetSize = MAX_ROSTER_SIZE,
  force = false,
  now = new Date(),
  fetchRanking = () => fetchEligibleRanking({ topModels: RANKING_FETCH_TOP_N, log }),
  adminToken = null,
  apiBase = null,
  apiClient = arenaApiJson,
  generateFighterFn = null,
  log = (line) => process.stdout.write(`[arena-continuous-top40] ${line}\n`),
} = {}) {
  const stamp = now instanceof Date ? now : new Date(now);
  const at = stamp.toISOString();
  const { state } = await loadOrCreateState(shadowDirectory, { now: stamp });
  const current = state.tracks.L0.roster.length;
  if (current >= EXISTING_ROSTER_GUARD && !force) {
    throw new Error(
      `roster already has ${current} models per track; use --force to fill remaining slots`,
    );
  }
  const needed = targetSize - current;
  if (needed <= 0) {
    log(`roster already complete (${current}/${targetSize}); nothing to do`);
    return { state, results: { imported: [], failed: [], kept: current } };
  }

  const ranking = await fetchRanking();
  if (!Array.isArray(ranking?.models)) {
    throw new Error('live ranking did not return a model list');
  }
  const eligible = eligibleChallengers(ranking.models, state.tracks.L0, stamp.getTime())
    .slice(0, needed);
  log(
    `roster ${current}/${targetSize}; ${eligible.length} eligible challenger(s) in the live ranking`,
  );

  const token = adminToken ?? await readAdminToken();
  const base = apiBase ?? apiBaseFromEnv();
  const results = { imported: [], failed: [], kept: current };
  for (const challenger of eligible) {
    const providerId = String(challenger.id);
    const slug = String(challenger.canonical_slug || providerId);
    try {
      const entrant = entrantFromChallenger(
        challenger,
        `continuous-${state.league_id}-bootstrap`,
        at,
      );
      // ONE codegen per model; the compiled v1 artifact is copied to all
      // four tracks below (not four generations).
      const generated = generateFighterFn
        ? await generateFighterFn({ apiBase: base, adminToken: token, entrant })
        : await generateFighter({
          apiBase: base,
          adminToken: token,
          entrant,
          compileAttempts: 3,
          apiClient,
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
      const checkpoint = generated.checkpoint;
      const source = generated.source;
      for (const trackId of TRACKS) {
        const trackDirectory = path.join(shadowDirectory, 'tracks', trackId);
        await writeFighterRecord(trackDirectory, providerId, { checkpoint, source, meta });
        state.tracks[trackId].roster.push({
          model_id: providerId,
          slug,
          mascot: mascotFor(providerId),
          joined_at: at,
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
      }
      results.imported.push(providerId);
      log(`  imported ${providerId} (rank ${challenger.provider_rank ?? '?'}) into all 4 tracks`);
    } catch (error) {
      results.failed.push({
        model_id: providerId,
        error: String(error?.message || error).slice(0, 300),
      });
      log(`  FAILED ${providerId}: ${String(error?.message || error).slice(0, 200)}; slot stays open`);
    }
  }

  state.updated_at = at;
  validateState(state);
  await writeState(shadowDirectory, state);
  log(
    `done: ${results.imported.length} imported, ${results.failed.length} failed, `
    + `roster now ${state.tracks.L0.roster.length}/${targetSize} per track`,
  );
  return { state, results };
}

async function main() {
  const force = process.argv.slice(2).includes('--force');
  const { results } = await bootstrapTop40({ force });
  if (results.failed.length > 0) {
    process.stdout.write(
      `[arena-continuous-top40] ${results.failed.length} generation failure(s); `
      + 'open slots will be retried by the daily recruit step\n',
    );
  }
}

const invokedAsScript = process.argv[1]
  && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedAsScript) {
  main().then(() => {
    process.exitCode = 0;
  }).catch((error) => {
    process.stderr.write(
      `[arena-continuous-top40] ${String(error?.message || error).slice(0, MAX_ERROR_CHARS)}\n`,
    );
    process.exitCode = 1;
  });
}
