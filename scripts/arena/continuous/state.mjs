// Continuous Model League — state schema, validation, and atomic IO.
//
// Schema v2 (multi-track amendment, 2026-08-24): top-level `tracks` map with
// one full league slice per intervention track (L0/L1/L2/L3), each carrying
// its own roster, retired ledger, announcements, day_index, feedback clock,
// and frozen policy fields. A v1 state file is discarded on load
// (clean-slate migration — the old single-track shadow state has no value).
//
// Mirrors the weekly supervisor's discipline (see weekly_supervisor.mjs):
// strict schema checks on load, fsync-before-rename atomic writes, and a
// refusal to run on malformed state. Small helpers are copied, not imported,
// because the weekly supervisor module runs its own service loop.

import { randomBytes } from 'node:crypto';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import { TRACKS, trackPolicy } from './league.mjs';

// The widest submission budget across tracks (L3: 9); per-track validation
// uses the track's own frozen policy.
const MAX_SUBMISSIONS_CAP = Math.max(...TRACKS.map((trackId) => trackPolicy(trackId).maxSubmissions));

export const SCHEMA_VERSION = 2;
export const MAX_ROSTER_SIZE = 10;
export const MAX_ANNOUNCEMENTS = 200;
export const STATE_FILENAME = 'state.json';

const SHA256_PATTERN = /^[a-f0-9]{64}$/;
const ID_PATTERN = /^\S{1,200}$/;
const LEAGUE_ID_PATTERN = /^[A-Za-z0-9_.:-]{1,128}$/;

const nowIso = () => new Date().toISOString();

function isIsoTimestamp(value) {
  if (typeof value !== 'string') return false;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) && new Date(parsed).toISOString() === value;
}

function isNonNegativeInt(value) {
  return Number.isSafeInteger(value) && value >= 0;
}

async function writeFileDurable(temporaryPath, contents, options) {
  const handle = await fs.open(temporaryPath, 'w', options?.mode ?? 0o600);
  try {
    await handle.writeFile(contents);
    // Flush data pages before the rename so a crash cannot resurrect the
    // target name pointing at an unwritten (zero-byte) inode.
    await handle.sync();
  } finally {
    await handle.close();
  }
}

/** Atomically write `value` as JSON: fsync the temp file, then rename. */
export async function atomicWriteJson(targetPath, value) {
  await fs.mkdir(path.dirname(targetPath), { recursive: true });
  const temporaryPath = `${targetPath}.tmp-${process.pid}-${Date.now()}`;
  try {
    await writeFileDurable(temporaryPath, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
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

function validateMascot(mascot, context) {
  if (!mascot || typeof mascot !== 'object'
      || typeof mascot.emoji !== 'string' || !mascot.emoji.trim()
      || typeof mascot.title !== 'string' || !mascot.title.trim()
      || typeof mascot.color !== 'string' || !mascot.color.trim()) {
    throw new Error(`continuous league state has an invalid mascot for ${context}`);
  }
}

function validateArtifact(artifact, context) {
  if (!artifact || typeof artifact !== 'object'
      || !SHA256_PATTERN.test(String(artifact.wasm_sha256 || ''))
      || !SHA256_PATTERN.test(String(artifact.source_sha256 || ''))
      || !SHA256_PATTERN.test(String(artifact.prompt_sha256 || ''))
      || !Number.isSafeInteger(artifact.version)
      || artifact.version < 1
      || !(artifact.parent_version === null
        || (Number.isSafeInteger(artifact.parent_version) && artifact.parent_version >= 0))) {
    throw new Error(`continuous league state has an invalid artifact binding for ${context}`);
  }
}

function validateModelRecord(entry, context, maxSubmissions = MAX_SUBMISSIONS_CAP) {
  if (!entry || typeof entry !== 'object'
      || !ID_PATTERN.test(String(entry.model_id || ''))
      || !ID_PATTERN.test(String(entry.slug || ''))
      || !isIsoTimestamp(entry.joined_at)
      || !Number.isSafeInteger(entry.submissions_used)
      || entry.submissions_used < 0
      || entry.submissions_used > maxSubmissions
      || !Number.isFinite(entry.rating)
      || entry.rating < 0
      || entry.rating > 100
      || !isNonNegativeInt(entry.wins)
      || !isNonNegativeInt(entry.losses)
      || !isNonNegativeInt(entry.draws)
      || !isNonNegativeInt(entry.matches)
      || entry.matches !== entry.wins + entry.losses + entry.draws
      || !isNonNegativeInt(entry.days_in_league)
      || entry.status !== 'active') {
    throw new Error(`continuous league state has an invalid roster entry for ${context}`);
  }
  validateMascot(entry.mascot, context);
  validateArtifact(entry.artifact, context);
}

function validateRetiredEntry(entry, context, maxSubmissions) {
  validateModelRecord(entry, context, maxSubmissions);
  if (!isIsoTimestamp(entry.retired_at)
      || typeof entry.reason !== 'string' || !entry.reason.trim()) {
    throw new Error(`continuous league state has an invalid retired entry for ${context}`);
  }
}

function validateAnnouncement(entry, index) {
  if (!entry || typeof entry !== 'object'
      || typeof entry.type !== 'string' || !entry.type.trim()
      || !isIsoTimestamp(entry.at)
      || (entry.track != null && !TRACKS.includes(entry.track))) {
    throw new Error(`continuous league state has an invalid announcement at index ${index}`);
  }
}

/** Validate one track slice against its frozen policy. Returns the slice. */
export function validateTrackSlice(slice, trackId) {
  if (!TRACKS.includes(trackId)) {
    throw new Error(`continuous league state has an unknown track: ${trackId}`);
  }
  if (!slice || typeof slice !== 'object') {
    throw new Error(`continuous league track ${trackId} is missing`);
  }
  const expected = trackPolicy(trackId);
  const policy = slice.policy;
  if (!policy || typeof policy !== 'object'
      || policy.max_submissions !== expected.maxSubmissions
      || policy.compile_attempts !== expected.compileAttempts
      || policy.feedback_interval_ms !== expected.feedbackIntervalMs
      || policy.max_revisions !== expected.maxRevisions) {
    throw new Error(`continuous league track ${trackId} has an invalid frozen policy`);
  }
  if (!isNonNegativeInt(slice.day_index)) {
    throw new Error(`continuous league track ${trackId} has an invalid day index`);
  }
  if (!Array.isArray(slice.roster) || slice.roster.length > MAX_ROSTER_SIZE) {
    throw new Error(`continuous league track ${trackId} roster must contain at most ${MAX_ROSTER_SIZE} models`);
  }
  slice.roster.forEach((entry) => (
    validateModelRecord(entry, `${trackId}:${entry?.model_id || 'unknown'}`, policy.max_submissions)
  ));
  if (new Set(slice.roster.map((entry) => entry.model_id)).size !== slice.roster.length) {
    throw new Error(`continuous league track ${trackId} roster has duplicate model IDs`);
  }
  if (!Array.isArray(slice.retired)) {
    throw new Error(`continuous league track ${trackId} has an invalid retired ledger`);
  }
  slice.retired.forEach((entry) => (
    validateRetiredEntry(entry, `${trackId}:${entry?.model_id || 'unknown'}`, policy.max_submissions)
  ));
  if (new Set(slice.retired.map((entry) => entry.model_id)).size !== slice.retired.length) {
    throw new Error(`continuous league track ${trackId} retired ledger has duplicate model IDs`);
  }
  if (!Array.isArray(slice.announcements) || slice.announcements.length > MAX_ANNOUNCEMENTS) {
    throw new Error(`continuous league track ${trackId} announcements must be capped at ${MAX_ANNOUNCEMENTS}`);
  }
  slice.announcements.forEach(validateAnnouncement);
  if (!(slice.last_feedback_at === null || isIsoTimestamp(slice.last_feedback_at))) {
    throw new Error(`continuous league track ${trackId} has an invalid feedback timestamp`);
  }
  return slice;
}

/** Strictly validate a league state document; returns the state on success. */
export function validateState(state) {
  if (!state || typeof state !== 'object' || state.schema_version !== SCHEMA_VERSION) {
    throw new Error('invalid continuous league state schema version');
  }
  if (!LEAGUE_ID_PATTERN.test(String(state.league_id || ''))) {
    throw new Error('continuous league state is missing its league ID');
  }
  if (!state.tracks || typeof state.tracks !== 'object' || Array.isArray(state.tracks)
      || Object.keys(state.tracks).sort().join(',') !== [...TRACKS].sort().join(',')) {
    throw new Error(`continuous league state must contain exactly the tracks ${TRACKS.join(', ')}`);
  }
  for (const trackId of TRACKS) validateTrackSlice(state.tracks[trackId], trackId);
  if (!isIsoTimestamp(state.created_at) || !isIsoTimestamp(state.updated_at)) {
    throw new Error('continuous league state has invalid lifecycle timestamps');
  }
  return state;
}

/** One empty track slice with the track's frozen policy. */
function createTrackSlice(trackId) {
  const policy = trackPolicy(trackId);
  return {
    day_index: 0,
    policy: {
      max_submissions: policy.maxSubmissions,
      compile_attempts: policy.compileAttempts,
      feedback_interval_ms: policy.feedbackIntervalMs,
      max_revisions: policy.maxRevisions,
    },
    roster: [],
    retired: [],
    announcements: [],
    last_feedback_at: null,
  };
}

/** Create a fresh, valid, empty league state (schema v2, four tracks). */
export function createState({ now = new Date(), leagueId } = {}) {
  const stamp = now instanceof Date ? now : new Date(now);
  if (!Number.isFinite(stamp.getTime())) throw new Error('createState requires a valid date');
  const day = stamp.toISOString().slice(0, 10).replace(/-/g, '');
  const id = leagueId || `cml-${day}-${randomBytes(4).toString('hex')}`;
  return validateState({
    schema_version: SCHEMA_VERSION,
    league_id: id,
    tracks: Object.fromEntries(TRACKS.map((trackId) => [trackId, createTrackSlice(trackId)])),
    created_at: stamp.toISOString(),
    updated_at: stamp.toISOString(),
  });
}

export function statePathFor(stateDirectory) {
  return path.join(stateDirectory, STATE_FILENAME);
}

/**
 * Load and validate the persisted state, or create + persist a fresh one.
 * A schema-v1 state file is discarded (clean-slate migration to the
 * four-track v2 schema — the pre-amendment shadow state has no value) and
 * reported via `migrated: true`; any other malformed state is refused.
 */
export async function loadOrCreateState(stateDirectory, options = {}) {
  const statePath = statePathFor(stateDirectory);
  await fs.mkdir(stateDirectory, { recursive: true });
  if (await fileExists(statePath)) {
    const persisted = await readJson(statePath);
    if (persisted?.schema_version === 1) {
      const state = createState(options);
      await atomicWriteJson(statePath, state);
      return { statePath, state, created: true, migrated: true };
    }
    return { statePath, state: validateState(persisted), created: false, migrated: false };
  }
  const state = createState(options);
  await atomicWriteJson(statePath, state);
  return { statePath, state, created: true, migrated: false };
}

/** Validate, then atomically persist the state. */
export async function writeState(stateDirectory, state) {
  const statePath = statePathFor(stateDirectory);
  await atomicWriteJson(statePath, validateState(state));
  return statePath;
}
