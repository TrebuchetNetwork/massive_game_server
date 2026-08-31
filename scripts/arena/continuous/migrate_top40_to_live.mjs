#!/usr/bin/env node

// Continuous Model League — one-shot migration of the paid top-40 fighters
// from the shadow league into the LIVE league.
//
// bootstrap_top40.mjs generated ~19 new fighter artifacts in the shadow dir
// with real credits. The live league kept evolving at 10/track — this script
// copies ONLY the models present in shadow but absent from live into the
// live league: their per-track fighter records plus a fresh roster entry in
// every track (submissions_used=1, artifact v1, rating 50, zeroed record,
// joined_at=now). The live roster is authoritative: nothing existing is
// overwritten, and per-track differences (e.g. retirements that only
// happened live) are preserved.
//
// Idempotent: models already in a live track roster (and fighter records
// already in the live store) are skipped. The live state is validated with
// validateState before it is written, under the live supervisor lock.

import { promises as fs } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { mascotFor } from '../mascots.mjs';
import { acquireOwnedLock, releaseOwnedLock } from '../owned_lock.mjs';
import { divisionSlices } from '../continuous_league.mjs';
import { TRACKS } from './league.mjs';
import { fighterDirectoryFor, readFighterRecord } from './generation.mjs';
import {
  MAX_ROSTER_SIZE,
  statePathFor,
  validateState,
  writeState,
} from './state.mjs';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT_DIR = path.resolve(SCRIPT_DIR, '../../..');
const DEFAULT_SHADOW_DIR = path.join(ROOT_DIR, 'artifacts/arena/continuous-shadow');
const DEFAULT_LIVE_DIR = path.join(ROOT_DIR, 'artifacts/arena/continuous');
const MAX_ERROR_CHARS = 2_000;

const readJson = async (target) => JSON.parse(await fs.readFile(target, 'utf8'));

async function copyFighterRecordIfMissing({ shadowDirectory, liveDirectory, trackId, modelId, log }) {
  const sourceDir = fighterDirectoryFor(path.join(shadowDirectory, 'tracks', trackId), modelId);
  const targetDir = fighterDirectoryFor(path.join(liveDirectory, 'tracks', trackId), modelId);
  try {
    await fs.access(targetDir);
    return false; // already in the live store: leave it untouched
  } catch {
    // missing: copy below
  }
  await fs.mkdir(path.dirname(targetDir), { recursive: true });
  await fs.cp(sourceDir, targetDir, { recursive: true });
  log(`  ${trackId}: fighter record copied for ${modelId}`);
  return true;
}

/**
 * Migrate the shadow-imported models missing from the live roster into the
 * live league. Returns { migrated, skipped, rosterCounts }.
 */
export async function migrateTop40ToLive({
  shadowDirectory = DEFAULT_SHADOW_DIR,
  liveDirectory = DEFAULT_LIVE_DIR,
  now = new Date(),
  log = (line) => process.stdout.write(`[arena-continuous-migrate] ${line}\n`),
} = {}) {
  const stamp = now instanceof Date ? now : new Date(now);
  const at = stamp.toISOString();
  const shadow = validateState(await readJson(statePathFor(shadowDirectory)));
  const live = validateState(await readJson(statePathFor(liveDirectory)));

  // The migration set is defined by the L2 track (no track-specific
  // retirement has ever diverged it from the import set).
  const liveIds = new Set(live.tracks.L2.roster.map((entry) => entry.model_id));
  const candidates = shadow.tracks.L2.roster.filter((entry) => !liveIds.has(entry.model_id));
  log(`${candidates.length} model(s) present in shadow but absent from live`);

  const migrated = [];
  const skipped = [];
  for (const entry of candidates) {
    const modelId = entry.model_id;
    const { checkpoint } = await readFighterRecord(
      path.join(shadowDirectory, 'tracks', 'L2'),
      modelId,
    );
    for (const trackId of TRACKS) {
      await copyFighterRecordIfMissing({
        shadowDirectory,
        liveDirectory,
        trackId,
        modelId,
        log,
      });
      const track = live.tracks[trackId];
      if (track.roster.some((rosterEntry) => rosterEntry.model_id === modelId)) {
        skipped.push(`${trackId}:${modelId}`);
        continue;
      }
      if (track.roster.length >= MAX_ROSTER_SIZE) {
        throw new Error(`live track ${trackId} is full; cannot add ${modelId}`);
      }
      track.roster.push({
        model_id: modelId,
        slug: entry.slug,
        mascot: mascotFor(modelId),
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
    migrated.push(modelId);
    log(`  added ${modelId} to all 4 live tracks (v1, rating 50)`);
  }

  live.updated_at = at;
  validateState(live);

  const lock = await acquireOwnedLock(path.join(liveDirectory, 'supervisor.lock'), {
    activeMessage: (owner) => `live league supervisor is running as PID ${owner.pid}; refusing to migrate under it`,
  });
  try {
    await writeState(liveDirectory, live);
  } finally {
    await releaseOwnedLock(lock).catch(() => {});
  }

  const rosterCounts = Object.fromEntries(
    TRACKS.map((trackId) => [trackId, live.tracks[trackId].roster.length]),
  );
  log(`migrated ${migrated.length} model(s); live roster counts: ${JSON.stringify(rosterCounts)}`);
  for (const trackId of TRACKS) {
    for (const division of divisionSlices(live.tracks[trackId].roster)) {
      const top = division.models.slice(0, 5)
        .map((entry) => `${entry.model_id.split('/').pop()} ${entry.rating}`)
        .join(' | ');
      log(`  ${trackId} ${division.name}: ${top}`);
    }
  }
  return { migrated, skipped, rosterCounts, state: live };
}

const invokedAsScript = process.argv[1]
  && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedAsScript) {
  migrateTop40ToLive().then(() => {
    process.exitCode = 0;
  }).catch((error) => {
    process.stderr.write(
      `[arena-continuous-migrate] ${String(error?.message || error).slice(0, MAX_ERROR_CHARS)}\n`,
    );
    process.exitCode = 1;
  });
}
