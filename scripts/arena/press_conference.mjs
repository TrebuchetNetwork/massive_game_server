#!/usr/bin/env node
// press_conference.mjs — daily "post-match press conference" for the Model Arena.
//
// Once per day, pick the most newsworthy league event from the last 24h of the
// continuous league (new leader > retirement/displacement > debutant entry >
// cross-track rating divergence >= DIVERGENCE_MIN > nothing) and ask the
// INVOLVED MODEL ITSELF — via one OpenRouter chat completion on its own
// provider model — for a short public quote in its mascot persona.
//
// The prompt follows the feedback-brief neutrality discipline: measured
// numbers only (rating, W/L/D, matches, days in league, the event itself), no
// coaching or strategy content. Unlike the private brief this is PUBLIC
// commentary, so an editorial voice in the OUTPUT is fine.
//
// Output (atomic write, one file per UTC day):
//   artifacts/arena/continuous/press/<date>.json
//   { event: {type, track, caption}, model_id, slug, mascot, quote,
//     generated_at, prompt_sha256 }
//
// HARD RULE: on ANY failure (no key, HTTP 402/403/429/5xx, timeout, empty or
// unusable model output) the run skips gracefully and writes NO file — a
// quote is never fabricated. At most one paid call per day: an existing
// press/<today>.json ends the run before any provider traffic (use --force to
// regenerate). build_model_pages.mjs renders the latest press file(s) as the
// "Press box" card; see loadPress there.

import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { loadContinuousLeague, defaultIo } from './build_model_pages.mjs';
import { TRACKS } from './continuous/league.mjs';
import { atomicWriteJson } from './continuous/state.mjs';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, '..', '..');

export const PRESS_WINDOW_MS = 24 * 3_600_000;
export const DIVERGENCE_MIN = 5;
export const QUOTE_MAX_CHARS = 280;
export const QUOTE_MAX_SENTENCES = 2;
export const OPENROUTER_CHAT_URL = 'https://openrouter.ai/api/v1/chat/completions';
export const CHAT_TIMEOUT_MS = 90_000;
// Headroom for reasoning models, whose reasoning tokens count against the cap
// on OpenRouter — too small a cap returns content cut mid-sentence.
export const CHAT_MAX_TOKENS = 512;

const sha256 = (value) => createHash('sha256').update(String(value)).digest('hex');

// Event newsworthiness, highest first.
const EVENT_PRIORITY = {
  leader_change: 4,
  retirement: 3,
  debutant: 2,
  divergence: 1,
};

// ---------------------------------------------------------------------------
// Event selection (pure — fixtures in tests)
// ---------------------------------------------------------------------------

/** Highest-rated roster entry of a history snapshot (null when empty). */
function snapshotLeader(snapshot) {
  let best = null;
  for (const entry of snapshot?.roster || []) {
    if (!best || Number(entry.rating) > Number(best.rating)) best = entry;
  }
  return best;
}

const withinWindow = (at, nowMs, windowMs) => {
  const ms = Date.parse(at);
  return Number.isFinite(ms) && ms <= nowMs && ms >= nowMs - windowMs;
};

function displayName(slugOrId) {
  const last = String(slugOrId || '').split('/').pop();
  return last.replace(/-\d{8}$/, '').replace(/:free$/, '') || String(slugOrId || '?');
}

/**
 * Pick the single most newsworthy event of the last `windowMs` from the
 * continuous league state + per-track history snapshots. Returns null when
 * the league was quiet. `snapshots` maps trackId -> snapshots sorted by `at`
 * (the loadContinuousLeague shape). The returned event carries a precomputed
 * human `caption` so the prompt and the page render share one phrasing.
 */
export function selectEvent({ state, snapshots, nowMs = Date.now(), windowMs = PRESS_WINDOW_MS }) {
  const candidates = [];

  // 1. Leader change: the top-rated model in a track's latest snapshot
  //    differs from the previous snapshot's leader.
  for (const trackId of TRACKS) {
    const list = Array.isArray(snapshots?.[trackId]) ? snapshots[trackId] : [];
    if (list.length < 2) continue;
    const latest = list[list.length - 1];
    const previous = list[list.length - 2];
    if (!withinWindow(latest?.at, nowMs, windowMs)) continue;
    const now1 = snapshotLeader(latest);
    const before = snapshotLeader(previous);
    if (!now1 || !before || now1.model_id === before.model_id) continue;
    candidates.push({
      type: 'leader_change',
      track: trackId,
      model_id: now1.model_id,
      slug: now1.slug,
      at: latest.at,
      caption: `${displayName(now1.slug)} is the new leader of track ${trackId} `
        + `(rating ${now1.rating}), taking #1 from ${displayName(before.slug)} `
        + `(rating ${before.rating}).`,
    });
  }

  // 2./3. Retirement (incl. displacement) and debutant announcements.
  for (const trackId of TRACKS) {
    const announcements = state?.tracks?.[trackId]?.announcements;
    for (const a of Array.isArray(announcements) ? announcements : []) {
      if (!withinWindow(a?.at, nowMs, windowMs)) continue;
      if (a.type === 'retirement') {
        candidates.push({
          type: 'retirement',
          track: trackId,
          model_id: a.model_id,
          slug: a.slug,
          mascot: a.mascot || null,
          stats: a.stats || null,
          at: a.at,
          caption: `${displayName(a.slug)} retired from track ${trackId} — ${String(a.reason || 'retired')}.`,
        });
      } else if (a.type === 'entrant' || a.type === 'fresh_challenger') {
        const rankNote = Number.isSafeInteger(a.provider_rank)
          ? ` (OpenRouter weekly rank #${a.provider_rank})` : '';
        candidates.push({
          type: 'debutant',
          track: trackId,
          model_id: a.model_id,
          slug: a.slug,
          mascot: a.mascot || null,
          at: a.at,
          caption: a.type === 'fresh_challenger'
            ? `${displayName(a.slug)} enters track ${trackId} as a fresh challenger${rankNote}.`
            : `${displayName(a.slug)} makes its league debut in track ${trackId}${rankNote}.`,
        });
      }
    }
  }

  // 4. Cross-track divergence: one model active in several tracks whose
  //    ratings differ by >= DIVERGENCE_MIN (the intervention experiment
  //    visibly working — or not).
  if (withinWindow(state?.updated_at, nowMs, windowMs)) {
    const perModel = new Map(); // model_id -> { slug, mascot, ratings: Map<track, rating> }
    for (const trackId of TRACKS) {
      for (const entry of state?.tracks?.[trackId]?.roster || []) {
        if (entry.status !== 'active') continue;
        if (!perModel.has(entry.model_id)) {
          perModel.set(entry.model_id, { slug: entry.slug, mascot: entry.mascot, ratings: new Map() });
        }
        perModel.get(entry.model_id).ratings.set(trackId, Number(entry.rating));
      }
    }
    for (const [modelId, info] of perModel) {
      if (info.ratings.size < 2) continue;
      const values = [...info.ratings.values()];
      const spread = Math.max(...values) - Math.min(...values);
      if (spread < DIVERGENCE_MIN) continue;
      const breakdown = [...info.ratings.entries()]
        .map(([t, r]) => `${t} ${r}`).join(', ');
      candidates.push({
        type: 'divergence',
        track: [...info.ratings.entries()].reduce((a, b) => (b[1] > a[1] ? b : a))[0],
        model_id: modelId,
        slug: info.slug,
        mascot: info.mascot || null,
        at: state.updated_at,
        caption: `${displayName(info.slug)} diverges across tracks — rating spread `
          + `${spread.toFixed(2)} (${breakdown}).`,
      });
    }
  }

  if (!candidates.length) return null;
  candidates.sort((a, b) => EVENT_PRIORITY[b.type] - EVENT_PRIORITY[a.type]
    || Date.parse(b.at) - Date.parse(a.at)
    || TRACKS.indexOf(a.track) - TRACKS.indexOf(b.track)
    || String(a.model_id).localeCompare(String(b.model_id)));
  return candidates[0];
}

/** The stats line for the speaking model: real numbers from state (or the
 * retirement announcement's final stats for a model already off the roster). */
export function speakerStats({ state, event }) {
  const entry = state?.tracks?.[event.track]?.roster?.find((e) => e.model_id === event.model_id)
    || (event.type === 'retirement'
      ? state?.tracks?.[event.track]?.retired?.find((e) => e.model_id === event.model_id)
      : null);
  const source = entry || event.stats || {};
  return {
    rating: source.rating ?? null,
    wins: Number(source.wins) || 0,
    losses: Number(source.losses) || 0,
    draws: Number(source.draws) || 0,
    matches: Number(source.matches) || 0,
    days_in_league: Number(source.days_in_league) || 0,
    mascot: entry?.mascot || event.mascot || null,
    slug: entry?.slug || event.slug,
  };
}

// ---------------------------------------------------------------------------
// Prompt + quote sanitation (pure)
// ---------------------------------------------------------------------------

/**
 * Neutral, stats-only press prompt. The persona framing and the event caption
 * carry the editorial color; every number is a measured league stat. No
 * coaching, no strategy suggestions — same discipline as the feedback brief.
 */
export function buildPressPrompt({ event, stats, mascot }) {
  const name = displayName(stats.slug || event.slug);
  const persona = mascot
    ? `mascot "${mascot.title}" ${mascot.emoji}`
    : 'league fighter';
  const lines = [
    `You are ${name}, ${persona} in the Model Arena — a league where AI models `
      + 'compete as game bots. You are speaking at the post-match press '
      + 'conference, in your mascot persona.',
    '',
    `Event: ${event.caption}`,
    '',
    `Your measured stats in track ${event.track}: rating ${stats.rating ?? 'n/a'}, `
      + `record ${stats.wins}W-${stats.losses}L-${stats.draws}D in ${stats.matches} matches, `
      + `${stats.days_in_league} days in the league.`,
    '',
    `Write your public quote about this event: ${QUOTE_MAX_SENTENCES === 2 ? '1-2' : '1'} short `
      + `sentences, under ${QUOTE_MAX_CHARS} characters. Editorial voice is welcome; `
      + 'react to the event and your numbers only. Plain text: no markdown, no '
      + 'quotation marks, no newlines, no hashtags, no strategy advice.',
  ];
  return lines.join('\n');
}

/**
 * Sanitize raw model output into a publishable quote: collapse newlines,
 * strip markdown/quotation marks, keep at most maxSentences sentences, cap at
 * maxChars (word-boundary truncation). Returns null for unusable output —
 * the caller then writes NOTHING.
 */
export function sanitizeQuote(raw, { maxChars = QUOTE_MAX_CHARS, maxSentences = QUOTE_MAX_SENTENCES } = {}) {
  if (typeof raw !== 'string') return null;
  let text = raw
    .replace(/```[\s\S]*?```/g, ' ') // fenced code blocks
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1') // markdown links -> label
    .replace(/[\r\n]+/g, ' ')
    .replace(/[*_`#>~]/g, '') // markdown emphasis/heading/quote markers
    .replace(/["“”„«»]/g, '') // quotation marks (apostrophes stay)
    .replace(/^\s*[-–—•]+\s*/, '') // leading bullet
    .replace(/\s+/g, ' ')
    .trim();
  if (!text) return null;

  // Sentence cap: split after . ! ? followed by whitespace/end.
  const sentences = text.match(/[^.!?]+[.!?]+(?:\s|$)|[^.!?]+$/g) || [];
  if (!sentences.length) return null;
  text = sentences.slice(0, maxSentences).map((s) => s.trim()).filter(Boolean).join(' ').trim();
  if (!text) return null;

  // Drop a trailing fragment cut off mid-sentence (provider hit the token
  // cap): when earlier sentences are complete, an unterminated tail is
  // truncation, never a stylistic choice. A single fragment with no complete
  // sentence at all is unusable.
  if (!/[.!?…]$/.test(text)) {
    const complete = text.match(/^([\s\S]*[.!?])\s+[^.!?]+$/);
    if (complete) text = complete[1].trim();
    else return null;
  }

  if (text.length > maxChars) {
    const cut = text.slice(0, maxChars - 1);
    text = `${cut.slice(0, Math.max(cut.lastIndexOf(' '), 0))}…`;
  }
  return text || null;
}

// ---------------------------------------------------------------------------
// OpenRouter chat (key resolution mirrors continuous/generation.mjs)
// ---------------------------------------------------------------------------

/** OpenRouter key: env, then env-pointed file. Null when unconfigured. */
export async function readOpenRouterKey({ env = process.env, readFile = fs.readFile } = {}) {
  const direct = String(env.OPENROUTER_API_KEY || '').trim();
  if (direct) return direct;
  const filePath = String(env.OPENROUTER_API_KEY_FILE || '').trim();
  if (!filePath) return null;
  try {
    return (await readFile(filePath, 'utf8')).trim() || null;
  } catch {
    return null;
  }
}

/**
 * One chat completion on the model's OWN provider id — the model speaks for
 * itself. Throws on any HTTP/transport/protocol failure; the caller treats a
 * throw as "no press today".
 */
export async function callOpenRouterChat({
  apiKey,
  model,
  prompt,
  fetchImpl = fetch,
  timeoutMs = CHAT_TIMEOUT_MS,
}) {
  const response = await fetchImpl(OPENROUTER_CHAT_URL, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${apiKey}`,
      'Content-Type': 'application/json',
      Accept: 'application/json',
    },
    body: JSON.stringify({
      model,
      messages: [{ role: 'user', content: prompt }],
      max_tokens: CHAT_MAX_TOKENS,
    }),
    signal: AbortSignal.timeout(timeoutMs),
  });
  if (!response.ok) {
    throw new Error(`OpenRouter chat completion failed with HTTP ${response.status}`);
  }
  const payload = await response.json();
  const content = payload?.choices?.[0]?.message?.content;
  if (typeof content !== 'string' || !content.trim()) {
    throw new Error('OpenRouter chat completion returned no usable content');
  }
  return content;
}

/** Chat-completion model id for a league entry: the provider id (the `~`
 * fast-lane provisional marker is not part of the provider id). */
export function providerModelFor(modelId) {
  return String(modelId || '').replace(/^~+/, '');
}

// ---------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------

export function pressFileFor(continuousDir, date) {
  return path.join(continuousDir, 'press', `${date}.json`);
}

/**
 * Run the daily press conference. Status-first result, never throws for
 * expected trouble:
 *   { status: 'unavailable' }            league state missing/invalid
 *   { status: 'quiet' }                  no newsworthy event in the window
 *   { status: 'exists', file }           today's quote already generated
 *   { status: 'failed', reason }         provider/key/output trouble — NO file
 *   { status: 'written', file, record }  quote persisted atomically
 */
export async function runPressConference({
  continuousDir = path.join(REPO_ROOT, 'artifacts', 'arena', 'continuous'),
  now = () => new Date(),
  env = process.env,
  fetchImpl = fetch,
  force = false,
  log = () => {},
} = {}) {
  const nowDate = now();
  const nowMs = nowDate.getTime();

  const league = loadContinuousLeague({ continuousDir, io: defaultIo, log });
  if (!league) return { status: 'unavailable' };

  const event = selectEvent({ state: league.state, snapshots: league.snapshots, nowMs });
  if (!event) {
    log('press: no newsworthy event in the last 24h — quiet day');
    return { status: 'quiet' };
  }

  const date = nowDate.toISOString().slice(0, 10);
  const file = pressFileFor(continuousDir, date);
  if (!force && await fs.access(file).then(() => true, () => false)) {
    log(`press: ${file} already exists — skipping (one call per day)`);
    return { status: 'exists', file };
  }

  const fail = (reason) => {
    log(`press: skipping — ${reason} (no file written)`);
    return { status: 'failed', reason };
  };

  const apiKey = await readOpenRouterKey({ env });
  if (!apiKey) return fail('no OPENROUTER_API_KEY / OPENROUTER_API_KEY_FILE');

  const stats = speakerStats({ state: league.state, event });
  const mascot = stats.mascot;
  const prompt = buildPressPrompt({ event, stats, mascot });
  const model = providerModelFor(event.model_id);

  let raw;
  try {
    raw = await callOpenRouterChat({ apiKey, model, prompt, fetchImpl });
  } catch (error) {
    return fail(String(error?.message || error).slice(0, 300));
  }

  const quote = sanitizeQuote(raw);
  if (!quote) return fail('model output did not survive sanitation');

  const record = {
    event: { type: event.type, track: event.track, caption: event.caption },
    model_id: event.model_id,
    slug: stats.slug || event.slug,
    mascot: mascot
      ? { key: mascot.key ?? null, emoji: mascot.emoji, title: mascot.title, color: mascot.color }
      : null,
    quote,
    generated_at: nowDate.toISOString(),
    prompt_sha256: sha256(prompt),
  };
  await atomicWriteJson(file, record);
  log(`press: ${event.type} → ${model} said: ${quote}`);
  return { status: 'written', file, record };
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const args = process.argv.slice(2);
  const dirFlag = args.indexOf('--continuous-dir');
  const continuousDir = dirFlag !== -1 ? args[dirFlag + 1] : undefined;
  const force = args.includes('--force');
  runPressConference({
    continuousDir: continuousDir || undefined,
    force,
    log: (msg) => console.error(`[press_conference] ${msg}`),
  })
    .then((result) => {
      console.log(JSON.stringify(result.record || result, null, 2));
      // 'failed' exits 1 so the daily runner can tell trouble apart from a
      // quiet day; in both cases NO file was written.
      process.exit(result.status === 'failed' ? 1 : 0);
    })
    .catch((error) => {
      console.error(error);
      process.exit(1);
    });
}
