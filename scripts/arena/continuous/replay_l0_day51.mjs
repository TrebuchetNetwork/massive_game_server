#!/usr/bin/env node

// Audited one-shot recovery for the L0 day-51 partial evaluation/double count.
// Evaluation runs entirely under the recovery directory against a short-lived
// isolated server. A prepared journal repairs partial live commits at startup;
// validated staged seasons/rankings/history are committed before state, last.
// The controller must quiesce all readers/publishers for that final live commit:
// stop media/generals/highlights jobs and the public game process during the
// short artifact swap to avoid mixed reads. No service control lives here.
// Isolated server registrations are NOT copied to production: future daily
// evaluations register their own new day IDs. Importing this module is inert.

import { spawn } from 'node:child_process';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

import { acquireOwnedLock, releaseOwnedLock } from '../owned_lock.mjs';
import { divisionSlices, runTrackCycle } from '../continuous_league.mjs';
import { readAdminToken } from './generation.mjs';
import {
  atomicWriteJson,
  statePathFor,
  validateState,
  writeState,
} from './state.mjs';

const TRACK = 'L0';
const TARGET_DAY = 51;
const PREVIOUS_DAY = 50;
const FINAL_DAY = 52;
const ROSTER_SIZE = 40;
const REPLAY_MATCHES = 288;
const PREVIOUS_HISTORY = '2026-11-04.json';
const CURRENT_HISTORY = '2026-11-05.json';
const SEASON_ID_PATTERN = /^[A-Za-z0-9_.:-]{1,128}$/;
const MAX_ERROR_CHARS = 2_000;
const MAX_CAPTURE_BYTES = 8 * 1024 * 1024;
const RUNNER_TIMEOUT_MS = 30 * 60 * 1000;
const KILL_GRACE_MS = 10_000;
const SERVER_READY_TIMEOUT_MS = 30_000;
const ISOLATED_API_BASE = 'http://127.0.0.1:19080';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT_DIR = path.resolve(SCRIPT_DIR, '../../..');
const CONTINUOUS_DIR = path.join(ROOT_DIR, 'artifacts/arena/continuous');
const TRACK_DIRECTORY = path.join(CONTINUOUS_DIR, 'tracks', TRACK);
const HISTORY_DIRECTORY = path.join(TRACK_DIRECTORY, 'history');
const PREVIOUS_HISTORY_PATH = path.join(HISTORY_DIRECTORY, PREVIOUS_HISTORY);
const CURRENT_HISTORY_PATH = path.join(HISTORY_DIRECTORY, CURRENT_HISTORY);
const SEASONS_DIRECTORY = path.join(ROOT_DIR, 'artifacts/arena/seasons');
const BACKUP_DIRECTORY = path.join(
  ROOT_DIR,
  'artifacts/arena/recovery/l0-day51-pre-replay',
);
const STAGING_ROOT = path.join(BACKUP_DIRECTORY, 'staging');
const STAGING_CONTINUOUS_DIRECTORY = path.join(
  STAGING_ROOT,
  'artifacts/arena/continuous',
);
const STAGING_TRACK_DIRECTORY = path.join(STAGING_CONTINUOUS_DIRECTORY, 'tracks', TRACK);
const STAGING_CURRENT_HISTORY_PATH = path.join(
  STAGING_TRACK_DIRECTORY,
  'history',
  CURRENT_HISTORY,
);
const STAGING_RUNNER_PATH = path.join(
  STAGING_ROOT,
  'scripts/arena/run_top10_season.mjs',
);
const ISOLATED_DIRECTORY = path.join(BACKUP_DIRECTORY, 'isolated');
const SERVER_BINARY = path.join(ROOT_DIR, 'target/release/massive_game_server_core');

const readJson = async (targetPath) => JSON.parse(await fs.readFile(targetPath, 'utf8'));

function requireSnapshot(history, dayIndex, sourcePath) {
  if (!Array.isArray(history)) throw new Error(`${sourcePath} must contain a JSON array`);
  const matches = history.filter((entry) => entry?.day_index === dayIndex);
  if (matches.length !== 1) {
    throw new Error(`${sourcePath} must contain exactly one day_index ${dayIndex} snapshot`);
  }
  return matches[0];
}

function seasonIdsFor(snapshot) {
  const seasonIds = String(snapshot?.season_id || '').split(',').map((id) => id.trim());
  if (seasonIds.length === 0 || seasonIds.some((id) => !SEASON_ID_PATTERN.test(id))) {
    throw new Error(`day_index ${snapshot?.day_index} snapshot has an invalid season_id list`);
  }
  return seasonIds;
}

async function createBackup(statePath, seasonIds) {
  await fs.mkdir(path.dirname(BACKUP_DIRECTORY), { recursive: true });
  try {
    await fs.mkdir(BACKUP_DIRECTORY, { recursive: false });
  } catch (error) {
    if (error?.code === 'EEXIST') {
      throw new Error(`recovery backup already exists: ${BACKUP_DIRECTORY}`);
    }
    throw error;
  }

  await fs.copyFile(statePath, path.join(BACKUP_DIRECTORY, 'state.json'));
  await fs.copyFile(CURRENT_HISTORY_PATH, path.join(BACKUP_DIRECTORY, CURRENT_HISTORY));
  for (const seasonId of seasonIds) {
    await fs.cp(
      path.join(SEASONS_DIRECTORY, seasonId),
      path.join(BACKUP_DIRECTORY, 'seasons', seasonId),
      { recursive: true, force: false, errorOnExist: true },
    );
    await copyFileInto(
      path.join(TRACK_DIRECTORY, 'rankings', `${seasonId}.json`),
      path.join(BACKUP_DIRECTORY, 'rankings', `${seasonId}.json`),
    );
  }
  // This durable marker precedes staging and every possible live commit write.
  await writeJournal(BACKUP_DIRECTORY, 'prepared', seasonIds);
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

function journalSeasonIds(journal) {
  const seasonIds = journal?.season_ids;
  if (!Array.isArray(seasonIds) || seasonIds.length !== 4
      || new Set(seasonIds).size !== 4
      || seasonIds.some((id) => typeof id !== 'string' || !SEASON_ID_PATTERN.test(id))) {
    throw new Error('recovery journal must name four distinct valid season IDs');
  }
  return seasonIds;
}

async function writeJournal(backupDirectory, phase, seasonIds) {
  await atomicWriteJson(path.join(backupDirectory, 'journal.json'), {
    version: 1,
    phase,
    season_ids: seasonIds,
    updated_at: new Date().toISOString(),
  });
}

async function copyFileInto(sourcePath, targetPath) {
  await fs.mkdir(path.dirname(targetPath), { recursive: true });
  await fs.copyFile(sourcePath, targetPath);
}

async function replaceDirectory(sourcePath, targetPath) {
  await fs.mkdir(path.dirname(targetPath), { recursive: true });
  await fs.rm(targetPath, { recursive: true, force: true });
  await fs.cp(sourcePath, targetPath, { recursive: true, force: false, errorOnExist: true });
}

async function restoreLiveArtifacts({
  backupDirectory,
  continuousDirectory,
  trackDirectory,
  seasonsDirectory,
  seasonIds,
  writeStateImpl = writeState,
}) {
  for (const seasonId of seasonIds) {
    await replaceDirectory(
      path.join(backupDirectory, 'seasons', seasonId),
      path.join(seasonsDirectory, seasonId),
    );
  }
  for (const seasonId of seasonIds) {
    await copyFileInto(
      path.join(backupDirectory, 'rankings', `${seasonId}.json`),
      path.join(trackDirectory, 'rankings', `${seasonId}.json`),
    );
  }
  const originalHistory = await readJson(path.join(backupDirectory, CURRENT_HISTORY));
  await atomicWriteJson(
    path.join(trackDirectory, 'history', CURRENT_HISTORY),
    originalHistory,
  );
  const originalState = await readJson(path.join(backupDirectory, 'state.json'));
  await writeStateImpl(continuousDirectory, originalState);
}

/** Recover a prepared commit at startup; any existing terminal journal refuses reuse. */
export async function recoverOnStartup({
  backupDirectory,
  continuousDirectory,
  trackDirectory,
  seasonsDirectory,
}) {
  const journalPath = path.join(backupDirectory, 'journal.json');
  let journal;
  try {
    journal = await readJson(journalPath);
  } catch (error) {
    if (error?.code !== 'ENOENT') throw error;
    if (await pathExists(backupDirectory)) {
      throw new Error(`recovery backup exists without a journal: ${backupDirectory}`);
    }
    return false;
  }

  if (journal.phase === 'prepared') {
    const seasonIds = journalSeasonIds(journal);
    await restoreLiveArtifacts({
      backupDirectory,
      continuousDirectory,
      trackDirectory,
      seasonsDirectory,
      seasonIds,
    });
    await writeJournal(backupDirectory, 'rolled_back', seasonIds);
    throw new Error(
      'interrupted L0 day 51 replay restored original live artifacts and was marked '
      + 'rolled_back; refusing further replay',
    );
  }
  if (journal.phase === 'committed') {
    throw new Error('L0 day 51 replay is already committed; refusing duplicate replay');
  }
  if (journal.phase === 'rolled_back') {
    throw new Error('recovery backup is rolled_back; refusing to overwrite it');
  }
  throw new Error(`recovery journal has an unknown phase: ${String(journal.phase)}`);
}

/** Copy a validated staged replay live, restoring every backed-up artifact on failure. */
export async function commitStagedReplay({
  backupDirectory,
  stagedRootDirectory,
  continuousDirectory,
  trackDirectory,
  seasonsDirectory,
  seasonIds,
  writeStateImpl = writeState,
}) {
  const journal = await readJson(path.join(backupDirectory, 'journal.json'));
  if (journal.phase !== 'prepared') {
    throw new Error(`staged commit requires a prepared journal, got ${String(journal.phase)}`);
  }
  const journaledIds = journalSeasonIds(journal);
  if (seasonIds.length !== 4
      || [...seasonIds].sort().join('\0') !== [...journaledIds].sort().join('\0')) {
    throw new Error('staged commit season IDs do not match the recovery journal');
  }

  const stagedContinuousDirectory = path.join(
    stagedRootDirectory,
    'artifacts/arena/continuous',
  );
  const stagedTrackDirectory = path.join(stagedContinuousDirectory, 'tracks', TRACK);
  const stagedSeasonsDirectory = path.join(stagedRootDirectory, 'artifacts/arena/seasons');
  try {
    const stagedState = await readJson(statePathFor(stagedContinuousDirectory));
    validateState(stagedState);
    const stagedHistory = await readJson(
      path.join(stagedTrackDirectory, 'history', CURRENT_HISTORY),
    );
    for (const seasonId of seasonIds) {
      await fs.access(path.join(stagedSeasonsDirectory, seasonId, 'season.json'));
      await fs.access(path.join(stagedTrackDirectory, 'rankings', `${seasonId}.json`));
    }

    for (const seasonId of seasonIds) {
      await replaceDirectory(
        path.join(stagedSeasonsDirectory, seasonId),
        path.join(seasonsDirectory, seasonId),
      );
    }
    for (const seasonId of seasonIds) {
      await copyFileInto(
        path.join(stagedTrackDirectory, 'rankings', `${seasonId}.json`),
        path.join(trackDirectory, 'rankings', `${seasonId}.json`),
      );
    }
    await atomicWriteJson(
      path.join(trackDirectory, 'history', CURRENT_HISTORY),
      stagedHistory,
    );
    // State is the final live artifact; the journal is committed only after it.
    await writeStateImpl(continuousDirectory, stagedState);
    await writeJournal(backupDirectory, 'committed', seasonIds);
  } catch (error) {
    const rollbackErrors = [];
    try {
      await restoreLiveArtifacts({
        backupDirectory,
        continuousDirectory,
        trackDirectory,
        seasonsDirectory,
        seasonIds,
        writeStateImpl,
      });
    } catch (rollbackError) {
      rollbackErrors.push(rollbackError);
    }
    if (rollbackErrors.length === 0) {
      try {
        await writeJournal(backupDirectory, 'rolled_back', seasonIds);
      } catch (rollbackError) {
        rollbackErrors.push(rollbackError);
      }
    }
    if (rollbackErrors.length > 0) {
      throw new AggregateError(
        [error, ...rollbackErrors],
        'staged commit failed and rollback was incomplete; journal remains prepared for startup recovery',
      );
    }
    throw error;
  }
  return true;
}

/** Find rotation entrants on/after the previous snapshot's date through the tainted run. */
export function rotationEntrantIds(announcements, previousAt, taintedAt) {
  if (!Number.isFinite(Date.parse(previousAt)) || !Number.isFinite(Date.parse(taintedAt))) {
    throw new Error('rotation window requires valid previous and tainted timestamps');
  }
  const previousDate = previousAt.slice(0, 10);
  const taintedMs = Date.parse(taintedAt);
  const modelIds = new Set();
  for (const announcement of announcements) {
    if (announcement?.type !== 'entrant' && announcement?.type !== 'fresh_challenger') continue;
    if (typeof announcement.at !== 'string') continue;
    const atMs = Date.parse(announcement.at);
    if (!Number.isFinite(atMs) || atMs > taintedMs) continue;
    if (announcement.at.slice(0, 10) < previousDate) continue;
    if (typeof announcement.model_id === 'string' && announcement.model_id) {
      modelIds.add(announcement.model_id);
    }
  }
  return modelIds;
}

/** Restore clean day-50 stats using rotation evidence for members absent from that snapshot. */
export function buildBoundaryRoster(currentRoster, previousRoster, postSnapshotEntrantIds) {
  const previousById = new Map(previousRoster.map((entry) => [entry.model_id, entry]));

  return currentRoster.map((current) => {
    const boundary = structuredClone(current);
    const previous = previousById.get(current.model_id);
    if (previous) {
      Object.assign(boundary, {
        rating: previous.rating,
        wins: previous.wins,
        losses: previous.losses,
        draws: previous.draws,
        matches: previous.matches,
      });
      return boundary;
    }

    if (!postSnapshotEntrantIds.has(current.model_id)) {
      throw new Error(
        `unexpected membership: ${current.model_id} is absent from the previous snapshot `
        + 'and lacks rotation evidence',
      );
    }
    Object.assign(boundary, {
      rating: 50,
      wins: 0,
      losses: 0,
      draws: 0,
      matches: 0,
    });
    return boundary;
  });
}

/** Check the replay's identity and match boundaries; ratings are deliberately unconstrained. */
export function validateReplay(boundaryRoster, replayedRoster, targetDay) {
  const boundaryIds = new Set(boundaryRoster.map((entry) => entry.model_id));
  const replayedIds = new Set(replayedRoster.map((entry) => entry.model_id));
  if (boundaryIds.size !== boundaryRoster.length
      || replayedIds.size !== replayedRoster.length
      || boundaryIds.size !== replayedIds.size
      || [...boundaryIds].some((modelId) => !replayedIds.has(modelId))) {
    throw new Error(`L0 day ${targetDay} replay model-id sets differ`);
  }

  const replayedById = new Map(replayedRoster.map((entry) => [entry.model_id, entry]));
  for (const boundary of boundaryRoster) {
    const replayed = replayedById.get(boundary.model_id);
    const expected = boundary.matches + REPLAY_MATCHES;
    if (replayed.matches !== expected) {
      throw new Error(
        `L0 day ${targetDay} replay validation failed for ${boundary.model_id}: `
        + `matches expected ${expected}, got ${replayed.matches}`,
      );
    }
  }
  return true;
}

async function prepareStagedEvaluation(currentHistory) {
  await fs.mkdir(STAGING_TRACK_DIRECTORY, { recursive: true });
  await fs.cp(
    path.join(TRACK_DIRECTORY, 'fighters'),
    path.join(STAGING_TRACK_DIRECTORY, 'fighters'),
    { recursive: true, force: false, errorOnExist: true },
  );
  await atomicWriteJson(
    STAGING_CURRENT_HISTORY_PATH,
    currentHistory.filter((entry) => entry?.day_index !== TARGET_DAY),
  );

  // The runner derives ROOT_DIR from its own filename. A copied four-file
  // runner keeps its seasons and lock under STAGING_ROOT instead of live.
  for (const filename of [
    'run_top10_season.mjs',
    'season_scoring.mjs',
    'arena_api_client.mjs',
    'owned_lock.mjs',
  ]) {
    await copyFileInto(
      path.join(SCRIPT_DIR, '..', filename),
      path.join(STAGING_ROOT, 'scripts/arena', filename),
    );
  }
}

const collectCapped = (current, chunk) => {
  const combined = current + chunk;
  return combined.length > MAX_CAPTURE_BYTES
    ? combined.slice(combined.length - MAX_CAPTURE_BYTES)
    : combined;
};

function redactorFor(adminToken) {
  const secrets = [adminToken, String(process.env.OPENROUTER_API_KEY || '').trim()]
    .filter(Boolean)
    .sort((left, right) => right.length - left.length);
  return (value) => {
    let sanitized = String(value ?? '');
    for (const secret of secrets) sanitized = sanitized.split(secret).join('[REDACTED]');
    return sanitized;
  };
}

async function runStagedSeasonRunner(args, { env = {}, timeoutMs = RUNNER_TIMEOUT_MS } = {}, adminToken) {
  const redact = redactorFor(adminToken);
  const child = spawn(process.execPath, [STAGED_RUNNER_PATH, ...args], {
    cwd: STAGING_ROOT,
    env: {
      ...process.env,
      ARENA_TOP_MODELS: '10',
      ...env,
      ARENA_API_BASE: ISOLATED_API_BASE,
      ARENA_ADMIN_BEARER_TOKEN: adminToken,
      MGS_ARENA_STORE_PATH: path.join(ISOLATED_DIRECTORY, 'arena_store.json'),
      MGS_ARENA_WASM_DIR: path.join(ISOLATED_DIRECTORY, 'arena_bots'),
      MGS_ARENA_RATINGS_PATH: path.join(ISOLATED_DIRECTORY, 'arena_ratings.json'),
      MGS_ARENA_REDIS_URL: '',
      MGS_REDIS_URL: '',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let stdout = '';
  let stderr = '';
  child.stdout.on('data', (chunk) => {
    stdout = collectCapped(stdout, chunk.toString('utf8'));
  });
  child.stderr.on('data', (chunk) => {
    stderr = collectCapped(stderr, chunk.toString('utf8'));
  });

  let timedOut = false;
  const result = await new Promise((resolve, reject) => {
    let killTimer = null;
    const timeout = setTimeout(() => {
      timedOut = true;
      child.kill('SIGTERM');
      killTimer = setTimeout(() => {
        if (child.exitCode === null) child.kill('SIGKILL');
      }, KILL_GRACE_MS);
      killTimer.unref();
    }, timeoutMs);
    timeout.unref();
    const settle = (callback, value) => {
      clearTimeout(timeout);
      if (killTimer) clearTimeout(killTimer);
      callback(value);
    };
    child.once('error', (error) => settle(reject, error));
    child.once('close', (code, signal) => settle(resolve, { code, signal }));
  });

  if (timedOut) throw new Error(`staged season runner timed out after ${timeoutMs}ms`);
  if (result.code !== 0) {
    const detail = redact(stderr).trim().slice(-MAX_ERROR_CHARS);
    throw new Error(
      `staged season runner exited ${result.signal ? `on ${result.signal}` : `with code ${result.code}`}`
      + (detail ? `: ${detail}` : ''),
    );
  }
  return { stdout, stderr: redact(stderr) };
}

/** Build the isolated server environment without mutating the inherited object. */
export function isolatedServerEnvironment(adminToken, {
  env = process.env,
  isolatedDirectory = ISOLATED_DIRECTORY,
} = {}) {
  return {
    ...env,
    MGS_CONFIG_PATH: '',
    MGS_HOST: '127.0.0.1',
    MGS_PORT: '19080',
    MGS_DISABLE_STUN: '1',
    MGS_TARGET_BOT_COUNT: '0',
    MGS_DIAGNOSTICS_ENABLED: '0',
    MGS_QUIC_PRIMARY: '0',
    MGS_METRICS_ENABLED: '1',
    MGS_METRICS_BIND_ADDR: '127.0.0.1:19090',
    MGS_ADMIN_BEARER_TOKEN: adminToken,
    MGS_ACCESS_LOG_PATH: path.join(isolatedDirectory, 'access.jsonl'),
    MGS_LIVE_REPLAY_DISPUTE_STORE_PATH: path.join(
      isolatedDirectory,
      'live_replay/disputes.jsonl',
    ),
    MGS_LIVE_REPLAY_MATCH_STORE_DIR: path.join(isolatedDirectory, 'live_replay/matches'),
    MGS_BACKUP_DIR: path.join(isolatedDirectory, 'backups'),
    MGS_BACKUP_EXTRA_PATHS: '',
    MGS_ARENA_EXHIBITION_ROSTER_PATH: path.join(isolatedDirectory, 'exhibition_roster.json'),
    MGS_PANIC_LOG_DIR: path.join(isolatedDirectory, 'panic'),
    MGS_PANIC_LOG_FILE: 'server-panic.log',
    MGS_ARENA_STORE_PATH: path.join(isolatedDirectory, 'arena_store.json'),
    MGS_ARENA_WASM_DIR: path.join(isolatedDirectory, 'arena_bots'),
    MGS_ARENA_RATINGS_PATH: path.join(isolatedDirectory, 'arena_ratings.json'),
    MGS_ARENA_SOURCE_DIR: path.join(isolatedDirectory, 'arena_sources'),
    MGS_AUTH_STORE_PATH: path.join(isolatedDirectory, 'auth_store.json'),
    MGS_FEATURE_FLAG_STORE_PATH: path.join(isolatedDirectory, 'feature_flags.json'),
    MGS_SHUTDOWN_STATE_PATH: path.join(isolatedDirectory, 'shutdown_state.json'),
    MGS_ARENA_REDIS_URL: '',
    MGS_REDIS_URL: '',
    MGS_FEATURE_FLAGS_REDIS_URL: '',
    MGS_BACKUP_REDIS_URL: '',
    MGS_LIVE_REPLAY_DISPUTE_REDIS_URL: '',
    MGS_LIVE_REPLAY_MATCH_REDIS_URL: '',
  };
}

function spawnIsolatedServer(adminToken) {
  const status = { child: null, error: null, stopping: false };
  const child = spawn(SERVER_BINARY, [], {
    cwd: ROOT_DIR,
    env: isolatedServerEnvironment(adminToken),
    stdio: ['ignore', 'ignore', 'ignore'],
  });
  status.child = child;
  child.once('error', (error) => {
    status.error = error;
  });
  child.once('exit', (code, signal) => {
    if (!status.stopping) {
      status.error = new Error(`isolated server exited ${signal ? `on ${signal}` : `with code ${code}`}`);
    }
  });
  return status;
}

async function waitForIsolatedServer(status) {
  const deadline = Date.now() + SERVER_READY_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (status.error) {
      throw new Error(`isolated arena server unavailable: ${status.error.message}`);
    }
    try {
      const response = await fetch(`${ISOLATED_API_BASE}/readyz`, {
        signal: AbortSignal.timeout(500),
      });
      if (response.ok) return;
    } catch {
      // The listener is not ready yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`isolated arena server did not become ready at ${ISOLATED_API_BASE}/readyz`);
}

async function stopIsolatedServer(status) {
  const child = status?.child;
  if (!child) return;
  status.stopping = true;
  if (child.exitCode !== null || child.signalCode !== null) return;
  const closed = new Promise((resolve) => child.once('close', resolve));
  const killTimer = setTimeout(() => {
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
  }, KILL_GRACE_MS);
  try {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill('SIGTERM');
      await closed;
    }
  } finally {
    clearTimeout(killTimer);
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
  }
}

async function evaluateStagedTrack({ state, nowMs, log }) {
  await fs.mkdir(path.join(ISOLATED_DIRECTORY, 'arena_bots'), { recursive: true });
  await fs.mkdir(path.join(ISOLATED_DIRECTORY, 'arena_sources'), { recursive: true });
  const adminToken = await readAdminToken();
  const server = spawnIsolatedServer(adminToken);
  try {
    await waitForIsolatedServer(server);
    return await runTrackCycle({
      track: state.tracks[TRACK],
      leagueId: state.league_id,
      trackId: TRACK,
      flags: { shadow: true },
      stateDirectory: STAGING_CONTINUOUS_DIRECTORY,
      trackDirectory: STAGING_TRACK_DIRECTORY,
      rootDirectory: STAGING_ROOT,
      deps: {
        nowMs,
        log,
        leagueCreatedAt: state.created_at,
        apiBase: ISOLATED_API_BASE,
        adminToken,
        runRunner: (args, options) => runStagedSeasonRunner(args, options, adminToken),
      },
    });
  } finally {
    await stopIsolatedServer(server);
  }
}

async function main() {
  const log = (line) => process.stdout.write(`[arena-l0-day51-replay] ${line}\n`);
  const supervisorLock = await acquireOwnedLock(
    path.join(CONTINUOUS_DIR, 'supervisor.lock'),
    {
      activeMessage: (owner) => (
        `continuous league supervisor is already running as PID ${owner.pid}; refusing L0 day 51 replay`
      ),
    },
  );

  try {
    // A prepared journal is handled before any new state read, staging, or server start.
    await recoverOnStartup({
      backupDirectory: BACKUP_DIRECTORY,
      continuousDirectory: CONTINUOUS_DIR,
      trackDirectory: TRACK_DIRECTORY,
      seasonsDirectory: SEASONS_DIRECTORY,
    });

    const statePath = statePathFor(CONTINUOUS_DIR);
    const state = await readJson(statePath);
    validateState(state);
    const previousHistory = await readJson(PREVIOUS_HISTORY_PATH);
    const currentHistory = await readJson(CURRENT_HISTORY_PATH);
    const track = state.tracks[TRACK];
    if (track.day_index !== FINAL_DAY) {
      throw new Error(`${TRACK} day_index must be ${FINAL_DAY}, got ${track.day_index}`);
    }
    const previousSnapshot = requireSnapshot(previousHistory, PREVIOUS_DAY, PREVIOUS_HISTORY_PATH);
    const taintedSnapshot = requireSnapshot(currentHistory, TARGET_DAY, CURRENT_HISTORY_PATH);
    if (track.roster.length !== ROSTER_SIZE || previousSnapshot.roster?.length !== ROSTER_SIZE) {
      throw new Error(
        `${TRACK} current and previous rosters must both contain ${ROSTER_SIZE} entries`,
      );
    }

    const nowMs = Date.parse(taintedSnapshot.at);
    if (!Number.isFinite(nowMs)
        || new Date(nowMs).toISOString().slice(0, 10) !== CURRENT_HISTORY.slice(0, 10)) {
      throw new Error('tainted day 51 snapshot has an invalid or wrong-date at timestamp');
    }
    const seasonIds = seasonIdsFor(taintedSnapshot);
    if (seasonIds.length !== 4 || new Set(seasonIds).size !== 4) {
      throw new Error('tainted day 51 snapshot must name four distinct season IDs');
    }
    const postSnapshotEntrantIds = rotationEntrantIds(
      track.announcements,
      previousSnapshot.at,
      taintedSnapshot.at,
    );
    const boundaryRoster = buildBoundaryRoster(
      track.roster,
      previousSnapshot.roster,
      postSnapshotEntrantIds,
    );
    if (boundaryRoster.length !== ROSTER_SIZE) {
      throw new Error(`${TRACK} boundary roster must contain ${ROSTER_SIZE} entries`);
    }

    // The backup and journal precede staging. Live files are read-only until commit.
    await createBackup(statePath, seasonIds);
    log(`backup prepared at ${BACKUP_DIRECTORY}`);
    let replayedTrack;
    try {
      await prepareStagedEvaluation(currentHistory);
      track.day_index = TARGET_DAY;
      track.roster = boundaryRoster;
      replayedTrack = await evaluateStagedTrack({ state, nowMs, log });
      if (replayedTrack.day_index !== FINAL_DAY) {
        throw new Error(
          `${TRACK} staged replay must end at day_index ${FINAL_DAY}, got ${replayedTrack.day_index}`,
        );
      }
      if (replayedTrack.roster.length !== ROSTER_SIZE) {
        throw new Error(
          `${TRACK} staged replay roster must contain ${ROSTER_SIZE} entries; unexpected retirement`,
        );
      }
      validateReplay(boundaryRoster, replayedTrack.roster, TARGET_DAY);

      const stagedHistory = await readJson(STAGING_CURRENT_HISTORY_PATH);
      const stagedSnapshot = requireSnapshot(stagedHistory, TARGET_DAY, STAGING_CURRENT_HISTORY_PATH);
      const stagedSeasonIds = seasonIdsFor(stagedSnapshot);
      if (stagedSeasonIds.length !== 4 || new Set(stagedSeasonIds).size !== 4
          || [...stagedSeasonIds].sort().join('\0') !== [...seasonIds].sort().join('\0')) {
        throw new Error('staged day 51 snapshot must name the original four distinct season IDs');
      }
      state.updated_at = new Date(nowMs).toISOString();
      validateState(state);
      await writeState(STAGING_CONTINUOUS_DIRECTORY, state);
    } catch (error) {
      // No live commit has begun, so recording rolled_back is sufficient here.
      try {
        await writeJournal(BACKUP_DIRECTORY, 'rolled_back', seasonIds);
      } catch (journalError) {
        throw new AggregateError(
          [error, journalError],
          'staged replay failed and its journal could not be finalized',
        );
      }
      throw error;
    }

    await commitStagedReplay({
      backupDirectory: BACKUP_DIRECTORY,
      stagedRootDirectory: STAGING_ROOT,
      continuousDirectory: CONTINUOUS_DIR,
      trackDirectory: TRACK_DIRECTORY,
      seasonsDirectory: SEASONS_DIRECTORY,
      seasonIds,
    });
    log(`staged replay committed; backup preserved at ${BACKUP_DIRECTORY}`);
    for (const division of divisionSlices(replayedTrack.roster)) {
      const leaders = division.models.slice(0, 3)
        .map((entry) => `${entry.model_id} ${entry.rating}`)
        .join(' | ');
      log(`${division.name} leaders: ${leaders}`);
    }
  } finally {
    await releaseOwnedLock(supervisorLock).catch(() => {});
  }
}

const invokedAsScript = process.argv[1]
  && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedAsScript) {
  main().catch((error) => {
    process.stderr.write(
      `[arena-l0-day51-replay] ${String(error?.message || error).slice(0, MAX_ERROR_CHARS)}\n`,
    );
    process.exitCode = 1;
  });
}
