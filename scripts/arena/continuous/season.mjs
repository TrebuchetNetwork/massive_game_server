// Continuous Model League — season frame computation (pure functions).
//
// scripts/arena/seasons.json gives the never-frozen league narrative shape:
// fixed-length arcs (28 days) with a champion rule and an archive. This
// module computes, from a validated league state and the parsed season
// definitions, everything the site needs around a season boundary:
//
//   - progress: day N of 28, days remaining, the final-countdown window;
//   - the projected champion (current L0 leader under the champion rule);
//   - the frozen finale record once the season ends — the first computation
//     after ends_at wins, and the page generator persists it to
//     artifacts/arena/continuous/seasons/<id>.json. Once frozen, a mid-season
//     leader change or later league play cannot rewrite history.
//
// No IO lives here; file reads/writes are the caller's job (the page
// generator writes the record with its usual atomic temp-file rename).

export const DAY_MS = 86400000;
// A season enters its "final countdown" this many days before ends_at.
export const COUNTDOWN_DAYS = 3;
export const FINALE_SCHEMA_VERSION = 1;

// Division pyramid — mirror of divisionSlices in continuous_league.mjs
// (copied, not imported: the supervisor module runs its own service loop —
// same convention as the copy in build_model_pages.mjs).
export const SEASON_DIVISION_SIZE = 10;
export const SEASON_DIVISION_NAMES = Object.freeze(['premier', 'challenger', 'contender', 'prospect']);

/** Win rate of a roster/retired entry; 0 for a model with no recorded match. */
export function winRate(entry) {
  const matches = Number(entry?.matches) || 0;
  return matches > 0 ? (Number(entry?.wins) || 0) / matches : 0;
}

/**
 * Championship order (the champion_rule in seasons.json): highest L0 rating;
 * ties broken by win rate, then matches played. The final slug tiebreak only
 * keeps the outcome deterministic — slugs are unique, so it never decides
 * between two records of the same model.
 */
export function compareChampionship(a, b) {
  return (Number(b?.rating) || 0) - (Number(a?.rating) || 0)
    || winRate(b) - winRate(a)
    || (Number(b?.matches) || 0) - (Number(a?.matches) || 0)
    || String(a?.slug || '').localeCompare(String(b?.slug) || '');
}

/** Projected champion of a roster (the L0 active slice): first in championship order. */
export function championOf(roster) {
  const sorted = [...(Array.isArray(roster) ? roster : [])].sort(compareChampionship);
  return sorted[0] ?? null;
}

/**
 * Season clock at nowMs. `season` is the parsed definition:
 * { startedMs, endsMs, totalDays }. Status is 'before' | 'active' | 'ended';
 * the phase adds 'countdown' for the last COUNTDOWN_DAYS of an active season.
 * The day counter is clamped to [1, totalDays] — an ended season reads
 * "day 28 of 28".
 */
export function seasonWindow(season, nowMs) {
  const { startedMs, endsMs } = season;
  // Parsed definitions carry totalDays; derive it from the span otherwise.
  const totalDays = Number.isFinite(season.totalDays) && season.totalDays > 0
    ? season.totalDays
    : Math.max(1, Math.round((endsMs - startedMs) / DAY_MS));
  const elapsed = nowMs - startedMs;
  const day = Math.min(totalDays, Math.max(1, Math.floor(elapsed / DAY_MS) + 1));
  const daysRemaining = Math.max(0, Math.ceil((endsMs - nowMs) / DAY_MS));
  const status = nowMs < startedMs ? 'before' : nowMs >= endsMs ? 'ended' : 'active';
  const phase = status === 'active' && daysRemaining <= COUNTDOWN_DAYS ? 'countdown' : status;
  return { day, totalDays, daysRemaining, status, phase };
}

/**
 * Pick the season that governs the page at nowMs. `next` takes over the
 * moment its started_at passes, so the banner switches seasons automatically
 * with no edit to seasons.json; the displaced season is returned as
 * `previous` (its finale can then be frozen).
 */
export function resolveSeasonFrame(seasons, nowMs) {
  const { current, next = null } = seasons;
  if (next && Number.isFinite(next.startedMs) && nowMs >= next.startedMs) {
    return { current: next, previous: current };
  }
  return { current, previous: null };
}

/** Partition a track roster into rating-ordered division slices. */
export function divisionSlices(roster, size = SEASON_DIVISION_SIZE) {
  const sorted = [...(Array.isArray(roster) ? roster : [])].sort((a, b) => (
    (Number(b.rating) || 0) - (Number(a.rating) || 0)
    || String(a.slug).localeCompare(String(b.slug))
  ));
  const slices = [];
  for (let index = 0; index < sorted.length; index += size) {
    const name = SEASON_DIVISION_NAMES[index / size] ?? `division-${index / size + 1}`;
    slices.push({ name, models: sorted.slice(index, index + size), offset: index });
  }
  return slices;
}

/** Top 3 of every division of the given roster (rating order). */
export function divisionTop3(roster) {
  return divisionSlices(roster).map((d) => ({ name: d.name, models: d.models.slice(0, 3) }));
}

function inWindow(iso, startedMs, endsMs) {
  const ms = Date.parse(iso);
  return Number.isFinite(ms) && ms >= startedMs && ms < endsMs;
}

/**
 * All retirements across tracks inside the season window [startedMs, endsMs),
 * oldest first, each tagged with its track.
 */
export function seasonRetirements(state, { startedMs, endsMs }, tracks = Object.keys(state?.tracks || {})) {
  const out = [];
  for (const trackId of tracks) {
    for (const entry of state?.tracks?.[trackId]?.retired || []) {
      if (inWindow(entry.retired_at, startedMs, endsMs)) out.push({ track: trackId, entry });
    }
  }
  return out.sort((a, b) => String(a.entry.retired_at).localeCompare(String(b.entry.retired_at)));
}

/**
 * "Season in numbers" strip, counted on the L0 (zero-shot) baseline track:
 * total fights (each fight is counted by both participants, hence /2),
 * models that debuted inside the season window, models retired inside it,
 * and the season day counter.
 */
export function seasonNumbers(l0Slice, { startedMs, endsMs, day }) {
  const all = [...(l0Slice?.roster || []), ...(l0Slice?.retired || [])];
  const totalFights = Math.round(all.reduce((sum, e) => sum + (Number(e.matches) || 0), 0) / 2);
  const modelsDebuted = all.filter((e) => inWindow(e.joined_at, startedMs, endsMs)).length;
  const modelsRetired = (l0Slice?.retired || [])
    .filter((e) => inWindow(e.retired_at, startedMs, endsMs)).length;
  return { totalFights, modelsDebuted, modelsRetired, days: day };
}

/**
 * The live finale view for an in-progress season: progress, projected
 * champion, per-division top 3, season retirements and numbers — all derived
 * from the current state (nothing frozen yet).
 */
export function computeSeasonView({ season, state, nowMs, tracks }) {
  const window = seasonWindow(season, nowMs);
  const l0 = state?.tracks?.L0;
  return {
    id: season.id,
    name: season.name,
    startedMs: season.startedMs,
    endsMs: season.endsMs,
    window,
    projected: championOf(l0?.roster),
    divisions: divisionTop3(l0?.roster),
    retirements: seasonRetirements(state, { startedMs: season.startedMs, endsMs: season.endsMs }, tracks),
    numbers: seasonNumbers(l0, {
      startedMs: season.startedMs, endsMs: season.endsMs, day: window.day,
    }),
  };
}

/** Compact, JSON-stable snapshot of one model's finale-relevant record. */
function finaleModelSnapshot(entry) {
  return {
    model_id: String(entry.model_id),
    slug: String(entry.slug),
    mascot: { emoji: entry.mascot.emoji, title: entry.mascot.title, color: entry.mascot.color },
    rating: Number(entry.rating),
    wins: Number(entry.wins) || 0,
    losses: Number(entry.losses) || 0,
    draws: Number(entry.draws) || 0,
    matches: Number(entry.matches) || 0,
    win_rate: winRate(entry),
    days_in_league: Number(entry.days_in_league) || 0,
  };
}

/**
 * The frozen finale record for an ended season, computed from the state as
 * it stands at freeze time (the first build at/after ends_at). Persisted to
 * artifacts/arena/continuous/seasons/<id>.json by the page generator; once
 * written it is never recomputed — later league play belongs to the next
 * season. Returns null when the L0 track has no champion to crown.
 */
export function freezeRecord({ season, state, nowMs, tracks }) {
  const champion = championOf(state?.tracks?.L0?.roster);
  if (!champion) return null;
  const view = computeSeasonView({ season, state, nowMs, tracks });
  return {
    version: FINALE_SCHEMA_VERSION,
    season: {
      id: season.id,
      name: season.name,
      started_at: new Date(season.startedMs).toISOString(),
      ends_at: new Date(season.endsMs).toISOString(),
    },
    frozen_at: new Date(nowMs).toISOString(),
    champion: finaleModelSnapshot(champion),
    divisions: view.divisions.map((d) => ({
      name: d.name,
      models: d.models.map(finaleModelSnapshot),
    })),
    retirements: view.retirements.map(({ track, entry }) => ({
      track,
      ...finaleModelSnapshot(entry),
      retired_at: String(entry.retired_at),
      reason: String(entry.reason),
    })),
    numbers: {
      total_fights: view.numbers.totalFights,
      models_debuted: view.numbers.modelsDebuted,
      models_retired: view.numbers.modelsRetired,
      days: view.numbers.days,
    },
  };
}

function isMascot(m) {
  return m && typeof m === 'object'
    && typeof m.emoji === 'string' && typeof m.title === 'string' && typeof m.color === 'string';
}

function isModelSnapshot(entry) {
  return entry && typeof entry === 'object'
    && typeof entry.slug === 'string' && typeof entry.model_id === 'string'
    && isMascot(entry.mascot)
    && Number.isFinite(entry.rating)
    && Number.isFinite(entry.win_rate)
    && Number.isSafeInteger(entry.wins) && Number.isSafeInteger(entry.losses)
    && Number.isSafeInteger(entry.draws) && Number.isSafeInteger(entry.matches);
}

/**
 * Structural check for a persisted finale record read back from disk.
 * Returns the record; throws on anything unusable (the caller treats a
 * malformed file like a missing one and refreezes).
 */
export function validateFinaleRecord(record, seasonId) {
  if (!record || typeof record !== 'object'
      || record.version !== FINALE_SCHEMA_VERSION
      || record.season?.id !== seasonId
      || typeof record.season?.name !== 'string'
      || !Number.isFinite(Date.parse(record.frozen_at))
      || !isModelSnapshot(record.champion)
      || !Array.isArray(record.divisions)
      || record.divisions.some((d) => (
        typeof d?.name !== 'string' || !Array.isArray(d.models) || d.models.some((m) => !isModelSnapshot(m))
      ))
      || !Array.isArray(record.retirements)
      || record.retirements.some((r) => (
        !isModelSnapshot(r) || typeof r.track !== 'string'
        || typeof r.retired_at !== 'string' || typeof r.reason !== 'string'
      ))
      || !record.numbers || typeof record.numbers !== 'object'
      || !Number.isSafeInteger(record.numbers.total_fights)
      || !Number.isSafeInteger(record.numbers.models_debuted)
      || !Number.isSafeInteger(record.numbers.models_retired)
      || !Number.isSafeInteger(record.numbers.days)) {
    throw new Error(`invalid season finale record for ${seasonId}`);
  }
  return record;
}
