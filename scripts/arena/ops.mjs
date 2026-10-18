// ops.mjs — collectors and assembly for the public "System" page
// (static_client/models/ops.html): scheduled jobs, league stats, arena stats.
//
// Collectors run at BUILD time (the daily 04:17 build bakes the page) and are
// injectable for tests. Every section degrades to an honest "unavailable"
// mark when its data source is missing — the page must never crash the build.
//
// The jobs table reads the host's systemd user timers via
//   systemctl --user list-timers --no-pager --output=json
//   systemctl --user show <unit>.service -p Result -p ExecMainStatus
//   systemctl --user is-enabled <unit>.timer
// Pure parsers (parseTimerList, parseServiceShow, parseMetrics) are exported
// for tests; the collectors are thin exec/fetch wrappers around them.

import { execSync } from 'node:child_process';

import { TRACKS } from './continuous/league.mjs';

export const METRICS_URL = 'http://127.0.0.1:9090/metrics';

/**
 * The jobs a visitor should know about. `schedule` is the human-facing wall
 * clock claim (server-local); `blurb` is the plain-language one-liner.
 */
export const OPS_JOBS = Object.freeze([
  Object.freeze({
    unit: 'massive-game-media-daily',
    schedule: 'daily 04:17',
    blurb: 'Rebuilds this site: model pages, league standings, fight clips, press quotes, benchmark.',
  }),
  Object.freeze({
    unit: 'massive-game-continuous-league',
    schedule: 'daily 05:23',
    blurb: 'Runs one league day: division battles in every track, then retirements, feedback revisions and recruitment.',
  }),
  Object.freeze({
    unit: 'massive-game-selfware-maintenance',
    schedule: 'daily 06:17',
    blurb: 'Self-maintenance sweep (disk hygiene, housekeeping). Currently disabled — shown for completeness.',
  }),
]);

const defaultExec = (cmd) => execSync(cmd, {
  encoding: 'utf8',
  stdio: ['ignore', 'pipe', 'ignore'],
  timeout: 10_000,
});

/** systemctl list-timers JSON -> Map<timerUnit, {nextMs, lastMs, activates}>. */
export function parseTimerList(text) {
  const out = new Map();
  let entries;
  try {
    entries = JSON.parse(text);
  } catch {
    return out;
  }
  if (!Array.isArray(entries)) return out;
  for (const e of entries) {
    if (!e || typeof e.unit !== 'string') continue;
    // systemctl reports timestamps in microseconds since the epoch.
    out.set(e.unit, {
      nextMs: Number.isFinite(e.next) && e.next > 0 ? e.next / 1000 : null,
      lastMs: Number.isFinite(e.last) && e.last > 0 ? e.last / 1000 : null,
      activates: typeof e.activates === 'string' ? e.activates : null,
    });
  }
  return out;
}

/** systemctl show -p Result -p ExecMainStatus -> { result, exitStatus }. */
export function parseServiceShow(text) {
  const out = { result: null, exitStatus: null };
  for (const line of String(text || '').split('\n')) {
    if (line.startsWith('Result=')) out.result = line.slice('Result='.length).trim() || null;
    if (line.startsWith('ExecMainStatus=')) {
      const n = Number(line.slice('ExecMainStatus='.length).trim());
      out.exitStatus = Number.isFinite(n) ? n : null;
    }
  }
  return out;
}

/** Prometheus text exposition -> the two arena gauges the page reports. */
export function parseMetrics(text) {
  const get = (name) => {
    const m = new RegExp(`^${name} ([0-9.eE+-]+)$`, 'm').exec(String(text || ''));
    return m ? Number(m[1]) : null;
  };
  return {
    exhibitionActive: get('game_arena_exhibition_runtimes_active'),
    framesTotal: get('game_frames_total'),
  };
}

/**
 * Collect the scheduled-jobs table from the host's systemd user timers.
 * Per-job failures degrade that job's row; a total failure (no systemctl,
 * no user bus) marks the whole table unavailable. Never throws.
 */
export function collectSystemdJobs({ exec = defaultExec } = {}) {
  let byUnit;
  try {
    byUnit = parseTimerList(exec('systemctl --user list-timers --no-pager --output=json'));
  } catch {
    byUnit = new Map();
  }
  const available = byUnit.size > 0;
  const jobs = OPS_JOBS.map((job) => {
    const row = {
      ...job,
      available,
      nextMs: null,
      lastMs: null,
      result: null,
      exitStatus: null,
      enabled: null,
    };
    if (!available) return row;
    const timer = byUnit.get(`${job.unit}.timer`);
    row.nextMs = timer?.nextMs ?? null;
    row.lastMs = timer?.lastMs ?? null;
    try {
      const shown = parseServiceShow(exec(`systemctl --user show ${job.unit}.service -p Result -p ExecMainStatus`));
      row.result = shown.result;
      row.exitStatus = shown.exitStatus;
    } catch {
      // service never ran or query failed — leave nulls
    }
    try {
      row.enabled = exec(`systemctl --user is-enabled ${job.unit}.timer`).trim();
    } catch (error) {
      // is-enabled exits non-zero for 'disabled' and prints it on stdout.
      row.enabled = String(error?.stdout || '').trim() || 'disabled';
    }
    return row;
  });
  return { available, jobs };
}

/**
 * Fetch the arena metrics endpoint at build time. Returns parseMetrics output,
 * or null when the endpoint is unreachable. Never throws.
 */
export async function fetchArenaMetrics({ url = METRICS_URL, timeoutMs = 3000, fetchImpl = null } = {}) {
  try {
    const text = fetchImpl
      ? await fetchImpl(url)
      : await (await fetch(url, { signal: AbortSignal.timeout(timeoutMs) })).text();
    return parseMetrics(text);
  } catch {
    return null;
  }
}

/**
 * Assemble the pure ops-page model from collected parts. `state` is the
 * validated continuous league state (or null when the overlay is inactive).
 */
export function buildOpsModel({
  jobs,
  metrics,
  state,
  submissionsCount,
  highlightsCount,
  pressCount,
  generatedAt,
}) {
  const league = state
    ? {
      league_id: state.league_id,
      day_index: Math.max(...TRACKS.map((t) => state.tracks[t]?.day_index ?? 0)),
      tracks: TRACKS.map((trackId) => {
        const slice = state.tracks[trackId];
        return {
          track: trackId,
          day_index: slice.day_index,
          roster: slice.roster.length,
          retired: slice.retired.length,
          announcements: slice.announcements.length,
        };
      }),
      retiredTotal: TRACKS.reduce((n, t) => n + state.tracks[t].retired.length, 0),
      announcementsTotal: TRACKS.reduce((n, t) => n + state.tracks[t].announcements.length, 0),
      submissionsTotal: submissionsCount ?? null,
    }
    : null;
  return {
    generatedAt,
    jobs,
    metrics,
    league,
    highlightsCount: highlightsCount ?? null,
    pressCount: pressCount ?? null,
  };
}
