// Continuous Model League — pure league logic. No IO, no timers.
//
// All functions are deterministic and side-effect free: they never mutate
// their arguments and return new objects where state changes.

export const MAX_SUBMISSIONS = 3;
export const RETIRE_RATING = 35;
export const RETIRE_WINRATE = 0.25;
export const RETIRE_MIN_DAYS = 3;
export const FEEDBACK_INTERVAL_MS = 48 * 60 * 60 * 1000;
export const RETIRE_COOLDOWN_MS = 7 * 24 * 60 * 60 * 1000;

const WEEK_MS = 7 * 24 * 60 * 60 * 1000;

/**
 * Intervention track policies (multi-track amendment, 2026-08-24):
 *
 *   L0 zero-shot:        1 submission, 1 compile attempt, never revised
 *   L1 compile-fix:      1 submission, 3 compile attempts (raw compiler error
 *                        surfaced in logs/ledger), never revised
 *   L2 two-iteration:    3 submissions (1 initial + 2 revisions), feedback
 *                        rounds at least 48h apart
 *   L3 weekly-feedback:  9 submissions (1 initial + 8 revisions), one
 *                        revision round every 7 days
 *
 * feedbackIntervalMs === null means the track never revises. Compile
 * "attempts" are retries of the same source against the compiler (only
 * transient failures can benefit; a deterministic rustc error fails all
 * attempts); they never consume a submission.
 */
export const TRACKS = Object.freeze(['L0', 'L1', 'L2', 'L3']);
export const TRACK_POLICIES = Object.freeze({
  L0: Object.freeze({
    maxSubmissions: 1, compileAttempts: 1, feedbackIntervalMs: null, maxRevisions: 0,
  }),
  L1: Object.freeze({
    maxSubmissions: 1, compileAttempts: 3, feedbackIntervalMs: null, maxRevisions: 0,
  }),
  L2: Object.freeze({
    maxSubmissions: 3, compileAttempts: 3, feedbackIntervalMs: FEEDBACK_INTERVAL_MS, maxRevisions: 2,
  }),
  L3: Object.freeze({
    maxSubmissions: 9, compileAttempts: 3, feedbackIntervalMs: WEEK_MS, maxRevisions: 8,
  }),
});

/** The pre-multitrack behavior, kept as the default for legacy callers. */
export const DEFAULT_TRACK_POLICY = TRACK_POLICIES.L2;

export function trackPolicy(trackId) {
  const policy = TRACK_POLICIES[trackId];
  if (!policy) throw new Error(`unknown league track: ${trackId}`);
  return policy;
}

const DAY_MS = 24 * 60 * 60 * 1000;

/** Win rate in [0, 1]; a model with no recorded matches rates 0. */
export function winRate(model) {
  const matches = Number(model?.matches) || 0;
  return matches > 0 ? (Number(model.wins) || 0) / matches : 0;
}

/**
 * Retirement bar (spec): retire when ALL hold —
 *   days_in_league >= 3 AND submissions_used === policy.maxSubmissions
 *   AND (rating < 35 OR winRate < 0.25).
 * `nowMs` is accepted for signature symmetry with the other gates; tenure is
 * read from the model's `days_in_league` counter, which the daily cycle
 * maintains (whole days since joined_at). `policy` defaults to the L2 track
 * policy (the pre-multitrack behavior).
 */
export function shouldRetire(model, nowMs, policy = DEFAULT_TRACK_POLICY) { // eslint-disable-line no-unused-vars
  if (!model || typeof model !== 'object') return false;
  if ((model.days_in_league ?? 0) < RETIRE_MIN_DAYS) return false;
  if (model.submissions_used !== policy.maxSubmissions) return false;
  return (Number(model.rating) || 0) < RETIRE_RATING
    || winRate(model) < RETIRE_WINRATE;
}

/**
 * True when the track's feedback interval elapsed since the last feedback
 * round (null → due). Tracks with feedbackIntervalMs === null (L0/L1) never
 * revise. `policy` defaults to the L2 track policy.
 */
export function feedbackDue(state, nowMs, policy = DEFAULT_TRACK_POLICY) {
  if (policy.feedbackIntervalMs === null) return false;
  if (state?.last_feedback_at == null) return true;
  const elapsed = Number(nowMs) - Date.parse(state.last_feedback_at);
  return Number.isFinite(elapsed) && elapsed >= policy.feedbackIntervalMs;
}

/**
 * Whole days a roster entry has been in the league at `nowMs`
 * (floored, clamped at 0).
 */
export function daysInLeague(model, nowMs) {
  const joined = Date.parse(model?.joined_at || '');
  if (!Number.isFinite(joined)) return 0;
  return Math.max(0, Math.floor((Number(nowMs) - joined) / DAY_MS));
}

/**
 * Filter a live OpenRouter ranking (array of `{ id, canonical_slug, ... }`,
 * best first) down to eligible challengers: drop models currently on the
 * roster and models retired within the last 7 days. Ranking order is
 * preserved. Matching checks both the provider id and the canonical slug
 * against roster/retired `model_id` and `slug`.
 */
export function eligibleChallengers(rankingModels, state, nowMs) {
  const excluded = new Set();
  for (const entry of state?.roster || []) {
    excluded.add(entry.model_id);
    excluded.add(entry.slug);
  }
  for (const entry of state?.retired || []) {
    const retiredAt = Date.parse(entry.retired_at || '');
    if (Number.isFinite(retiredAt) && Number(nowMs) - retiredAt < RETIRE_COOLDOWN_MS) {
      excluded.add(entry.model_id);
      excluded.add(entry.slug);
    }
  }
  return (Array.isArray(rankingModels) ? rankingModels : []).filter((entry) => {
    const id = String(entry?.id || '');
    if (!id) return false;
    const slug = String(entry?.canonical_slug || '');
    return !excluded.has(id) && !(slug && excluded.has(slug));
  });
}

const roundRating = (value) => Math.round(Math.max(0, Math.min(100, value)) * 100) / 100;

/**
 * Rating formula (0–100), recomputed deterministically from cumulative
 * battle statistics:
 *
 *   match_points = wins + 0.5 * draws        (standard match scoring)
 *   rating       = 100 * match_points / matches        (matches > 0)
 *
 * A model with no recorded matches keeps its current rating (fresh entrants
 * start at 50). The formula is stable (same inputs → same output), strictly
 * monotonic in wins for fixed losses/draws/matches, bounded to [0, 100], and
 * draws count as half a win.
 *
 * `applyBattleRatings` folds one evaluation season (`season.json` from the
 * top-10 runner, roster entries carrying wins/losses/draws/matches_played)
 * into the league roster: each entry's W/L/D/matches accumulate the season's
 * numbers and the rating is recomputed from the cumulative totals. A roster
 * model missing from the season means the evaluation was incomplete and is
 * rejected; season entries not on the roster are ignored. Returns a new
 * roster array.
 */
export function applyBattleRatings(roster, seasonJson) {
  const seasonRoster = Array.isArray(seasonJson?.roster) ? seasonJson.roster : [];
  const byModelId = new Map();
  for (const entry of seasonRoster) {
    if (entry?.model_id) byModelId.set(entry.model_id, entry);
    if (entry?.provider_model && !byModelId.has(entry.provider_model)) {
      byModelId.set(entry.provider_model, entry);
    }
  }
  return (Array.isArray(roster) ? roster : []).map((model) => {
    const seasonEntry = byModelId.get(model.model_id) ?? byModelId.get(model.slug);
    if (!seasonEntry) {
      throw new Error(`evaluation season is missing roster model ${model.model_id}`);
    }
    const wins = model.wins + (Number(seasonEntry.wins) || 0);
    const losses = model.losses + (Number(seasonEntry.losses) || 0);
    const draws = model.draws + (Number(seasonEntry.draws) || 0);
    const matches = model.matches + (Number(seasonEntry.matches_played) || 0);
    const rating = matches > 0
      ? roundRating(100 * (wins + 0.5 * draws) / matches)
      : model.rating;
    return { ...model, wins, losses, draws, matches, rating };
  });
}

/**
 * Advance a model's artifact lineage one submission: version increments and
 * parent_version links back to the current version. Returns the new artifact
 * binding (the sha fields still describe the previous build; the feedback
 * step overwrites them after a successful compile).
 */
export function nextVersion(model) {
  const artifact = model?.artifact;
  if (!artifact || !Number.isSafeInteger(artifact.version) || artifact.version < 1) {
    throw new Error('nextVersion requires a model with a valid artifact binding');
  }
  return {
    ...artifact,
    version: artifact.version + 1,
    parent_version: artifact.version,
  };
}
