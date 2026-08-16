// Highlight window selection: score sliding windows over a normalized replay
// by kill density, kill clusters, and end-of-match closeness.

const MIN_WINDOW_MS = 10_000;
const MAX_WINDOW_MS = 45_000;
const BASE_WINDOW_MS = 20_000;
const STEP_MS = 2_000;
const CLUSTER_SPAN_MS = 5_000;
const CLUSTER_MIN_KILLS = 3;

/** Extract alive->dead transitions as kill events {t, id, name, x, y, team}. */
export function killEvents(replay) {
  const events = [];
  const wasAlive = new Map();
  for (const frame of replay.frames) {
    for (const p of frame.players) {
      const prev = wasAlive.get(p.id);
      if (prev && prev.alive && !p.alive) {
        events.push({ t: frame.t, id: p.id, name: p.name, x: p.x, y: p.y, team: p.team });
      }
      wasAlive.set(p.id, p);
    }
  }
  return events.sort((a, b) => a.t - b.t);
}

/** Max number of kills inside any CLUSTER_SPAN_MS sub-window of [startMs, endMs]. */
function maxCluster(kills, startMs, endMs) {
  let best = 0;
  const inWin = kills.filter((k) => k.t >= startMs && k.t <= endMs);
  for (let i = 0; i < inWin.length; i++) {
    let n = 0;
    for (let j = i; j < inWin.length && inWin[j].t - inWin[i].t <= CLUSTER_SPAN_MS; j++) n++;
    if (n > best) best = n;
  }
  return best;
}

function frameAtOrBefore(frames, t) {
  let lo = 0, hi = frames.length - 1, ans = 0;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (frames[mid].t <= t) { ans = mid; lo = mid + 1; } else hi = mid - 1;
  }
  return frames[ans];
}

/** Closeness of the two teams at time t: 1 = perfectly even, plus low-hp drama bonus. */
function closenessAt(replay, t) {
  const frame = frameAtOrBefore(replay.frames, t);
  const hp = new Map();
  for (const p of frame.players) {
    if (!p.alive) continue;
    hp.set(p.team, (hp.get(p.team) || 0) + Math.max(0, p.hp || 0));
  }
  const totals = [...hp.values()];
  if (totals.length < 2) return { even: 0, lowHp: 0 };
  const sum = totals.reduce((a, b) => a + b, 0);
  const max = Math.max(...totals);
  const even = sum > 0 ? 1 - (max - (sum - max)) / sum : 0; // two-team evenness
  const alive = frame.players.filter((p) => p.alive);
  const avgHp = alive.length ? alive.reduce((a, p) => a + Math.max(0, p.hp || 0), 0) / alive.length : 100;
  return { even, lowHp: avgHp < 50 ? 1 - avgHp / 50 : 0 };
}

function clampWindow(startMs, endMs, durationMs) {
  let s = Math.max(0, startMs);
  let e = Math.min(durationMs, endMs);
  if (e - s > MAX_WINDOW_MS) e = s + MAX_WINDOW_MS;
  if (e - s < MIN_WINDOW_MS && durationMs >= MIN_WINDOW_MS) {
    e = Math.min(durationMs, s + MIN_WINDOW_MS);
    s = Math.max(0, e - MIN_WINDOW_MS);
  }
  return { startMs: Math.round(s), endMs: Math.round(e) };
}

function scoreWindow(replay, kills, startMs, endMs) {
  const inWin = kills.filter((k) => k.t >= startMs && k.t <= endMs);
  const cluster = maxCluster(kills, startMs, endMs);
  const prox = Math.max(0, 1 - (replay.durationMs - endMs) / 30_000); // 1 at match end
  const { even, lowHp } = closenessAt(replay, endMs);
  const endScore = prox * (5 + even * 10 + lowHp * 5);
  const score = inWin.length * 10 + (cluster >= CLUSTER_MIN_KILLS ? cluster * 15 : 0) + endScore;

  let reason;
  if (cluster >= CLUSTER_MIN_KILLS) reason = `Kill cluster x${cluster}`;
  else if (inWin.length >= 2) reason = `${inWin.length} kills`;
  else if (prox > 0.5) reason = 'End-of-match showdown';
  else if (inWin.length === 1) reason = 'Opening kill';
  else reason = 'Positional battle';
  return { score, reason, kills: inWin.length, cluster };
}

/**
 * selectHighlights(replay, {maxClips}) -> top non-overlapping windows,
 * chronological order: [{startMs, endMs, reason, score}].
 */
export function selectHighlights(replay, { maxClips = 3 } = {}) {
  const duration = replay.durationMs;
  if (!replay.frames.length || duration <= 0) return [];
  if (duration <= MIN_WINDOW_MS) {
    return [{ startMs: 0, endMs: duration, reason: 'Full match', score: 1 }];
  }

  const kills = killEvents(replay);
  const candidates = [];
  const base = Math.min(BASE_WINDOW_MS, duration);
  for (let s = 0; s + base <= duration + STEP_MS; s += STEP_MS) {
    const startMs = Math.min(s, duration - base);
    const { score, reason } = scoreWindow(replay, kills, startMs, startMs + base);
    candidates.push({ startMs, endMs: startMs + base, score, reason });
  }
  candidates.sort((a, b) => b.score - a.score);

  const picked = [];
  for (const cand of candidates) {
    if (picked.length >= maxClips) break;
    const overlaps = picked.some((p) => {
      const inter = Math.min(p.endMs, cand.endMs) - Math.max(p.startMs, cand.startMs);
      return inter > 0.3 * Math.min(p.endMs - p.startMs, cand.endMs - cand.startMs);
    });
    if (overlaps) continue;

    // Refit the window around the kills it contains (pad, then clamp 10-45s).
    const inWin = kills.filter((k) => k.t >= cand.startMs - 5_000 && k.t <= cand.endMs + 5_000);
    let { startMs, endMs } = cand;
    if (inWin.length) {
      startMs = Math.min(startMs, inWin[0].t - 3_000);
      endMs = Math.max(endMs, inWin[inWin.length - 1].t + 3_000);
    }
    const fitted = clampWindow(startMs, endMs, duration);
    // Refitting can grow the window into an already-picked clip; reject if so.
    const refitOverlaps = picked.some((p) => {
      const inter = Math.min(p.endMs, fitted.endMs) - Math.max(p.startMs, fitted.startMs);
      return inter > 0.3 * Math.min(p.endMs - p.startMs, fitted.endMs - fitted.startMs);
    });
    if (refitOverlaps) continue;
    picked.push({ ...fitted, reason: cand.reason, score: Math.round(cand.score * 10) / 10 });
  }
  return picked.sort((a, b) => a.startMs - b.startMs);
}
