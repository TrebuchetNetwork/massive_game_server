#!/usr/bin/env node
// build_model_pages.mjs — generate static per-model profile pages for the arena.
//
// Inputs:
//   data/arena_ratings.json                     published roster snapshot
//   artifacts/arena/seasons/<season>/battles/   ~7.8M duel artifacts (sampled, never full-read)
//   artifacts/arena/seasons/<season>/world/     all-model FFA artifacts (full pass)
//   static_client/media/highlights/index.json   fight clips
//   artifacts/arena/continuous/                 continuous league overlay (optional):
//                                               state.json (v2, tracks:{L0..L3}) +
//                                               submissions.jsonl + tracks/<T>/history/
//   scripts/arena/toplist_commentary.json       authored analyst toplist (optional,
//                                               path injectable)
//   scripts/arena/chronicle.json                authored League Chronicle narrative
//                                               (optional, path injectable)
//   scripts/arena/seasons.json                  season structure for the continuous
//                                               league (optional, path injectable)
//   scripts/arena/lore.json                     authored world/fighter/lexicon lore
//                                               (optional, path injectable)
//
// Outputs:
//   static_client/models/index.html             rank-sorted roster index
//   static_client/models/<slug>.html            one page per roster model
//   static_client/models/lore.html              world + fighters + lexicon page
//                                               (only when lore.json loads)
//   static_client/models/models.css             shared stylesheet (emitted by this script)
//   static_client/models/mascots.json           slug -> {emoji,title,color}
//   static_client/models/league.json            landing-ticker payload (only when the
//                                               continuous league state validates)
//   artifacts/arena/page-cache.json             incremental battle-sample cache
//
// When the continuous league state is absent or fails its own schema
// validation the overlay is skipped entirely and the weekly-league HTML
// outputs (index.html, <slug>.html, mascots.json) are byte-identical to a
// build without it; models.css always carries the overlay styles, and a stale
// league.json from a previous valid run is removed. The analyst toplist
// follows the same rule: absent or malformed, its sections disappear and the
// HTML is byte-identical to a build without it (models.css always carries
// the toplist styles). The League Chronicle follows the same rule: absent or
// malformed, its section disappears and the HTML is byte-identical to a build
// without it (models.css always carries the chronicle styles). The season
// banner and arena lore follow the same rule: absent or malformed, their
// sections disappear (and stale lore.html is removed), leaving the HTML
// byte-identical to a build without them (models.css always carries the
// season/lore styles).
//
// The battles dir is far too large to read fully; we readdir + stat everything
// (fast), keep the newest window, and only JSON-read files that are new since
// the last run (cache keyed on file name + mtime). Heavy IO is injectable so
// tests run against tiny fixtures.
//
// Measured rivalries: from the same kept battle records (no extra IO) we also
// compute full-sample pairwise head-to-head stats — the league's "most
// contested" (closest) and "nemesis" (most one-sided) pairs. Unlike the
// editorial chronicle/lore, every number here is counted from recorded
// fights; pairs under the minimum sample are excluded. When no pair
// qualifies, the sections disappear and the HTML stays byte-identical to a
// build without the feature (models.css always carries the styles).

import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { mascotFor } from './mascots.mjs';
import { TRACKS } from './continuous/league.mjs';
import { MAX_ROSTER_SIZE, validateState } from './continuous/state.mjs';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, '..', '..');

export const ACTIONS = ['idle', 'attack', 'defend', 'charge', 'support'];
// Display order for the per-mode breakdown; unknown modes sort after these.
export const MODE_ORDER = ['arena', 'ctf', 'koth', 'tdm'];
export const RATING_AXES = [
  ['overall_rating', 'Overall'],
  ['personal_rating', 'Personal'],
  ['team_rating', 'Team'],
  ['collaboration_rating', 'Collab'],
  ['world_rating', 'World'],
  ['strategy_rating', 'Strategy'],
];

// ---------------------------------------------------------------------------
// Injectable IO. Tests substitute fixture-backed implementations.
// ---------------------------------------------------------------------------
export const defaultIo = {
  readdir: (dir) => fs.readdirSync(dir),
  statMtimeMs: (dir, name) => fs.statSync(path.join(dir, name)).mtimeMs,
  readJson: (file) => JSON.parse(fs.readFileSync(file, 'utf8')),
  readText: (file) => fs.readFileSync(file, 'utf8'),
  // Atomic: write to a sibling temp file, then rename over the target so
  // concurrent readers (the live HTTP server) never see a partial page.
  writeFile: (file, contents) => {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    const tmp = `${file}.tmp-${process.pid}`;
    fs.writeFileSync(tmp, contents);
    fs.renameSync(tmp, file);
  },
  exists: (file) => fs.existsSync(file),
  remove: (file) => fs.rmSync(file, { force: true }),
};

// ---------------------------------------------------------------------------
// Slugs
// ---------------------------------------------------------------------------

/** "deepseek/deepseek-v4-pro-20260423" -> "deepseek-v4-pro" (provider + trailing date stripped). */
export function baseSlug(canonicalSlug) {
  const last = String(canonicalSlug || '').split('/').pop();
  return last.replace(/-\d{8}$/, '').replace(/:free$/, '');
}

function shortHash(s) {
  return createHash('sha256').update(String(s)).digest('hex').slice(0, 6);
}

/**
 * Assign a unique URL slug to every roster entry.
 * Default is baseSlug(canonical_slug). On collision the entry whose id carries
 * a longer distinguishing suffix keeps it (e.g. deepseek-v4-flash-0731 vs
 * deepseek-v4-flash); further ties fall back to the dated slug, then a hash.
 * Returns Map<model_id, slug>.
 */
export function slugifyRoster(roster) {
  const entries = roster.map((m) => ({
    id: m.model_id,
    rank: m.rank ?? Number.MAX_SAFE_INTEGER,
    base: baseSlug(m.canonical_slug),
    candidates: [
      baseSlug(m.provider_model),
      String(m.canonical_slug || '').split('/').pop().replace(/:free$/, ''),
      `${baseSlug(m.canonical_slug)}-${shortHash(m.model_id)}`,
    ],
  }));

  const used = new Set();
  const slugs = new Map();
  const byBase = new Map();
  for (const e of entries) {
    if (!byBase.has(e.base)) byBase.set(e.base, []);
    byBase.get(e.base).push(e);
  }

  const collisions = [];
  for (const group of byBase.values()) {
    if (group.length === 1) {
      const e = group[0];
      slugs.set(e.id, e.base);
      used.add(e.base);
    } else {
      collisions.push(group.sort((a, b) => a.rank - b.rank));
    }
  }

  for (const group of collisions) {
    for (const e of group) {
      const pick = e.candidates.find((c) => c && !used.has(c));
      if (!pick) {
        throw new Error(`slugifyRoster: no unique slug candidate for model "${e.id}" (base "${e.base}") — refusing to emit undefined.html`);
      }
      slugs.set(e.id, pick);
      used.add(pick);
    }
  }
  return slugs;
}

// ---------------------------------------------------------------------------
// Battle sampling (behavior fingerprint + rivalry grid)
// ---------------------------------------------------------------------------

function emptyCounts() {
  return { idle: 0, attack: 0, defend: 0, charge: 0, support: 0 };
}

function countsOf(raw) {
  const out = emptyCounts();
  for (const a of ACTIONS) out[a] = Number(raw?.[a] || 0);
  return out;
}

/** Extract the compact per-file record stored in the incremental cache. */
export function battleRecord(name, mtimeMs, json) {
  const sim = json?.simulation;
  if (!sim || !json.model_a_id || !json.model_b_id) return null;
  return {
    f: name,
    m: mtimeMs,
    mo: String(sim.mode || json.mode || 'unknown'),
    a: json.model_a_id,
    b: json.model_b_id,
    w: sim.winner_model_id || null,
    d: sim.draw === true,
    ac: ACTIONS.map((k) => Number(sim.team_a_action_counts?.[k] || 0)),
    bc: ACTIONS.map((k) => Number(sim.team_b_action_counts?.[k] || 0)),
  };
}

/**
 * Aggregate sampled per-file records into per-model behavior + rivalry stats.
 * records: array of cache records (see battleRecord). Only records involving a
 * roster model contribute; each model keeps its newest perModelLimit records.
 * Produces a blended fingerprint (primary view) plus a per-mode breakdown
 * (modes: arena/ctf/koth/tdm) with per-mode action counts and W-L-D.
 */
export function aggregateBattles(records, rosterIds, perModelLimit = 200) {
  const rosterSet = new Set(rosterIds);
  const byModel = new Map(rosterIds.map((id) => [id, []]));
  for (const rec of records) {
    if (rosterSet.has(rec.a)) byModel.get(rec.a).push(rec);
    if (rec.b !== rec.a && rosterSet.has(rec.b)) byModel.get(rec.b).push(rec);
  }

  const result = new Map();
  for (const [id, recs] of byModel) {
    recs.sort((x, y) => y.m - x.m || x.f.localeCompare(y.f));
    const sample = recs.slice(0, perModelLimit);
    const counts = emptyCounts();
    const rivals = new Map();
    const modeAcc = new Map(); // mode -> {games, counts, w, l, d}
    for (const rec of sample) {
      const mine = rec.a === id ? rec.ac : rec.bc;
      const other = rec.a === id ? rec.b : rec.a;
      const mo = rec.mo || 'unknown';
      if (!modeAcc.has(mo)) modeAcc.set(mo, { games: 0, counts: emptyCounts(), w: 0, l: 0, d: 0 });
      const md = modeAcc.get(mo);
      md.games += 1;
      for (let i = 0; i < ACTIONS.length; i++) {
        counts[ACTIONS[i]] += mine[i];
        md.counts[ACTIONS[i]] += mine[i];
      }
      if (!rosterSet.has(other) || other === id) continue;
      if (rec.d) md.d += 1;
      else if (rec.w === id) md.w += 1;
      else if (rec.w) md.l += 1;
      if (!rivals.has(other)) rivals.set(other, { w: 0, l: 0, d: 0 });
      const r = rivals.get(other);
      if (rec.d) r.d += 1;
      else if (rec.w === id) r.w += 1;
      else if (rec.w) r.l += 1;
    }
    const total = ACTIONS.reduce((s, k) => s + counts[k], 0);
    const modes = [...modeAcc.entries()].map(([mode, md]) => {
      const modeTotal = ACTIONS.reduce((s, k) => s + md.counts[k], 0);
      const topAction = modeTotal > 0
        ? ACTIONS.reduce((best, k) => (md.counts[k] > md.counts[best] ? k : best), ACTIONS[0])
        : null;
      return {
        mode,
        games: md.games,
        actionTotal: modeTotal,
        topAction,
        topShare: topAction && modeTotal > 0 ? md.counts[topAction] / modeTotal : null,
        aggression: modeTotal > 0 ? (md.counts.attack + md.counts.charge) / modeTotal : null,
        w: md.w, l: md.l, d: md.d,
      };
    }).sort((x, y) => {
      const xi = MODE_ORDER.indexOf(x.mode);
      const yi = MODE_ORDER.indexOf(y.mode);
      return (xi === -1 ? MODE_ORDER.length : xi) - (yi === -1 ? MODE_ORDER.length : yi)
        || x.mode.localeCompare(y.mode);
    });
    result.set(id, {
      sampled: sample.length,
      actionCounts: counts,
      actionTotal: total,
      aggression: total > 0 ? (counts.attack + counts.charge) / total : null,
      rivals,
      modes,
      newestMtime: sample[0]?.m ?? 0,
    });
  }
  return result;
}

// ---------------------------------------------------------------------------
// Measured rivalries (full-sample pairwise head-to-head)
// ---------------------------------------------------------------------------

export const RIVALRY_MIN_GAMES = 20;
export const RIVALRY_TOP_N = 3;

/**
 * Pairwise head-to-head records over the FULL kept battle sample — unlike the
 * per-model rivalry grid, which is limited to each model's newest
 * perModelLimit duels, this counts every kept fight between two roster
 * models. For every pair with at least `minGames` measured fights: leader
 * (more wins; exact ties break to the lexicographically smaller id),
 * W-L-D, and topShare = leader wins / games.
 *
 * "Most contested" = lowest topShare (ties: more games first), top 3.
 * "Nemesis matchups" = highest topShare, top 3.
 * mostContestedByModel maps each model to its single closest qualifying pair.
 * Returns null when no pair qualifies so callers hide the sections entirely.
 */
export function measuredRivalries(records, rosterIds, minGames = RIVALRY_MIN_GAMES) {
  const rosterSet = new Set(rosterIds);
  const pairs = new Map(); // "idA idB" (sorted) -> {a, b, wa, wb, d}
  for (const rec of records) {
    if (rec.a === rec.b || !rosterSet.has(rec.a) || !rosterSet.has(rec.b)) continue;
    const [x, y] = rec.a < rec.b ? [rec.a, rec.b] : [rec.b, rec.a];
    const key = `${x} ${y}`;
    if (!pairs.has(key)) pairs.set(key, { a: x, b: y, wa: 0, wb: 0, d: 0 });
    const p = pairs.get(key);
    if (rec.d) p.d += 1;
    else if (rec.w === x) p.wa += 1;
    else if (rec.w === y) p.wb += 1;
  }
  const entries = [];
  for (const [key, p] of pairs) {
    const games = p.wa + p.wb + p.d;
    if (games < minGames) continue;
    const aLeads = p.wa >= p.wb;
    entries.push({
      key,
      leader: aLeads ? p.a : p.b,
      leaderWins: aLeads ? p.wa : p.wb,
      trailer: aLeads ? p.b : p.a,
      trailerWins: aLeads ? p.wb : p.wa,
      draws: p.d,
      games,
      topShare: Math.max(p.wa, p.wb) / games,
    });
  }
  if (!entries.length) return null;
  const byContested = [...entries].sort((x, y) => x.topShare - y.topShare
    || y.games - x.games || x.key.localeCompare(y.key));
  const byNemesis = [...entries].sort((x, y) => y.topShare - x.topShare
    || y.games - x.games || x.key.localeCompare(y.key));
  const mostContestedByModel = new Map();
  for (const e of byContested) {
    if (!mostContestedByModel.has(e.leader)) mostContestedByModel.set(e.leader, e);
    if (!mostContestedByModel.has(e.trailer)) mostContestedByModel.set(e.trailer, e);
  }
  return {
    contested: byContested.slice(0, RIVALRY_TOP_N),
    nemesis: byNemesis.slice(0, RIVALRY_TOP_N),
    mostContestedByModel,
    pairsMeasured: entries.length,
    minGames,
  };
}

/**
 * Scan a battles dir: stat everything, read only files missing from (or newer
 * than) the cache, then aggregate. Returns { aggregates, cacheData, stats }.
 * cacheData is serializable and should be persisted by the caller.
 */
/**
 * List battle checkpoints as relative names, covering both the legacy flat
 * layout (`<task>.json`) and the sharded layout (`<2-hex>/<task>.json`) the
 * runner writes to keep a single directory under ext4's dir_index limit.
 */
function listBattleNames(io, dir) {
  const names = [];
  for (const entry of io.readdir(dir)) {
    if (entry.endsWith('.json')) {
      names.push(entry);
      continue;
    }
    if (!/^[0-9a-f]{2}$/.test(entry)) continue;
    let inner;
    try {
      inner = io.readdir(path.join(dir, entry));
    } catch {
      continue; // Not a directory (or vanished) — not a shard.
    }
    for (const name of inner) {
      if (name.endsWith('.json')) names.push(`${entry}/${name}`);
    }
  }
  return names;
}

export async function scanBattles({
  battlesDir,
  rosterIds,
  perModelLimit = 200,
  windowSize = 4000,
  cache = null,
  io = defaultIo,
  log = () => {},
}) {
  const t0 = Date.now();
  const names = listBattleNames(io, battlesDir);
  log(`battles: ${names.length} files listed in ${Date.now() - t0}ms`);

  // Records gained a `mo` (mode) field in cache version 2 — older caches are
  // discarded so no record without a mode leaks into per-mode aggregation.
  const cacheValid = cache && cache.version === 2 && cache.battlesDir === battlesDir;
  const cachedFiles = cacheValid ? cache.files || {} : {};

  // Stat everything; keep the newest `windowSize` plus every cached file.
  // Memory: plain parallel arrays for the full listing; the name->mtime
  // lookup map is built only for kept names.
  const t1 = Date.now();
  const mtimes = new Array(names.length);
  for (let i = 0; i < names.length; i++) {
    mtimes[i] = io.statMtimeMs(battlesDir, names[i]);
  }
  const order = names.map((_, i) => i).sort((a, b) => mtimes[b] - mtimes[a]);
  const keep = new Map(); // name -> mtime, kept files only
  for (let i = 0; i < Math.min(windowSize, order.length); i++) {
    const idx = order[i];
    keep.set(names[idx], mtimes[idx]);
  }
  for (const f of Object.keys(cachedFiles)) {
    if (keep.has(f)) continue;
    let m;
    try {
      m = io.statMtimeMs(battlesDir, f); // fresh stat: detect changed/vanished cache files
    } catch {
      continue; // cached file vanished from disk
    }
    keep.set(f, m);
  }
  log(`battles: stat pass ${Date.now() - t1}ms; window ${keep.size} files`);

  // Read only files that are new or changed; reuse cache records otherwise.
  const t2 = Date.now();
  const records = new Map();
  let reads = 0;
  for (const [name, mtime] of keep) {
    const cached = cachedFiles[name];
    if (cached && cached.m === mtime) {
      records.set(name, cached);
      continue;
    }
    let json = null;
    try {
      json = io.readJson(path.join(battlesDir, name));
    } catch {
      continue; // partial write / unreadable artifact — skip
    }
    const rec = battleRecord(name, mtime, json);
    if (rec) records.set(name, rec);
    reads += 1;
  }
  log(`battles: read ${reads} new files (${records.size} cached/kept) in ${Date.now() - t2}ms`);

  const allRecords = [...records.values()];
  const aggregates = aggregateBattles(allRecords, rosterIds, perModelLimit);
  const rivalries = measuredRivalries(allRecords, rosterIds);

  // Persist only records that actually feed the sample, plus a margin so the
  // window can slide forward without re-reading history.
  const kept = {};
  for (const [name, rec] of records) kept[name] = rec;

  return {
    aggregates,
    rivalries,
    cacheData: { version: 2, battlesDir, files: kept },
    stats: { listed: names.length, windowFiles: keep.size, filesRead: reads },
  };
}

// ---------------------------------------------------------------------------
// World FFA pass (placements, discipline, co-performance)
// ---------------------------------------------------------------------------

/**
 * Read every world artifact. Returns per-model placement/discipline stats and
 * a pairwise "finish gap" matrix: the mean absolute rank difference between
 * two models across shared world events (lower = they finish closer together).
 * Chosen over rank correlation because fixed-field FFA ranks are zero-sum,
 * which mechanically pushes pairwise correlations negative.
 */
export async function scanWorld({ worldDir, rosterIds, io = defaultIo, log = () => {} }) {
  const t0 = Date.now();
  const rosterSet = new Set(rosterIds);
  const names = io.readdir(worldDir).filter((n) => n.endsWith('.json'));
  const perModel = new Map(rosterIds.map((id) => [id, {
    events: 0, placementSum: 0, wins: 0, faults: 0, rounds: 0,
  }]));
  const pairKey = (x, y) => (x < y ? `${x}${y}` : `${y}${x}`);
  const pairGapSum = new Map(); // pairKey -> {sum, n}

  let read = 0;
  for (const name of names) {
    let json = null;
    try {
      json = io.readJson(path.join(worldDir, name));
    } catch {
      continue;
    }
    const rankings = json?.simulation?.rankings;
    if (!Array.isArray(rankings)) continue;
    const rounds = Number(json.simulation.rounds || json.rounds || 1);
    const eventRanks = new Map();
    for (const entry of rankings) {
      const id = entry?.model_id;
      if (!rosterSet.has(id)) continue;
      const stats = perModel.get(id);
      const rank = Number(entry.rank);
      if (!Number.isFinite(rank)) continue;
      stats.events += 1;
      stats.placementSum += rank;
      if (rank === 1) stats.wins += 1;
      stats.faults += Number(entry.invalid_action_count || 0)
        + Number(entry.fallback_count || 0)
        + Number(entry.fuel_error_count || 0)
        + Number(entry.trap_count || 0);
      stats.rounds += rounds;
      eventRanks.set(id, rank);
    }
    for (let i = 0; i < rosterIds.length; i++) {
      const ri = eventRanks.get(rosterIds[i]);
      if (ri === undefined) continue;
      for (let j = i + 1; j < rosterIds.length; j++) {
        const rj = eventRanks.get(rosterIds[j]);
        if (rj === undefined) continue;
        const key = pairKey(rosterIds[i], rosterIds[j]);
        const acc = pairGapSum.get(key) || { sum: 0, n: 0 };
        acc.sum += Math.abs(ri - rj);
        acc.n += 1;
        pairGapSum.set(key, acc);
      }
    }
    read += 1;
  }
  log(`world: read ${read}/${names.length} artifacts in ${Date.now() - t0}ms`);

  const models = {};
  for (const [id, s] of perModel) {
    models[id] = {
      events: s.events,
      avgPlacement: s.events ? s.placementSum / s.events : null,
      worldWins: s.wins,
      discipline: s.rounds > 0 ? Math.max(0, 1 - s.faults / s.rounds) : null,
    };
  }

  const partners = new Map(); // id -> [{other, avgGap, events}]
  for (const id of rosterIds) {
    const scored = [];
    for (const other of rosterIds) {
      if (other === id) continue;
      const acc = pairGapSum.get(pairKey(id, other));
      if (!acc || acc.n < 3) continue;
      scored.push({ other, avgGap: acc.sum / acc.n, events: acc.n });
    }
    scored.sort((a, b) => a.avgGap - b.avgGap);
    partners.set(id, scored);
  }

  return { models, partners, filesRead: read };
}

// ---------------------------------------------------------------------------
// Fights (highlight clips)
// ---------------------------------------------------------------------------

/**
 * Pick clips featuring this model. Players are strings like "#1 Abyss";
 * match the exact "#rank Title" tag first (rank disambiguates models that
 * share a mascot title), falling back to a title/model-name word match.
 */
export function matchFights(clips, { rank, title, modelName }, limit = 6) {
  const tag = `#${rank} ${title}`;
  const tagged = clips.filter((c) => Array.isArray(c.players) && c.players.includes(tag));
  const wordRe = new RegExp(`\\b${escapeRegExp(title)}\\b`);
  const nameNeedle = String(modelName || '').toLowerCase();
  const loose = clips.filter((c) => Array.isArray(c.players) && c.players.some((p) => {
    const s = String(p);
    return wordRe.test(s) || (nameNeedle && s.toLowerCase().includes(nameNeedle));
  }));
  const picked = (tagged.length ? tagged : loose)
    .slice()
    .sort((a, b) => (b.score || 0) - (a.score || 0))
    .slice(0, limit);
  return picked;
}

function escapeRegExp(s) {
  return String(s).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

function esc(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[c]));
}

function fmtInt(n) {
  return Number(n || 0).toLocaleString('en-US');
}

function fmtPct(x, digits = 1) {
  return x === null || x === undefined ? '—' : `${(x * 100).toFixed(digits)}%`;
}

function chrome({ title, description, active, loreLink = false }) {
  const navLink = (href, label, key) =>
    `<a href="${href}"${active === key ? ' aria-current="page"' : ''}>${label}</a>`;
  return {
    head: `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover">
    <title>${esc(title)}</title>
    <meta name="description" content="${esc(description)}">
    <meta property="og:type" content="website">
    <meta property="og:site_name" content="Model Arena // selfware.design">
    <meta property="og:title" content="${esc(title)}">
    <meta property="og:description" content="${esc(description)}">
    <meta property="og:image" content="https://space.selfware.design/og-card.png">
    <meta property="og:image:width" content="1200">
    <meta property="og:image:height" content="630">
    <meta name="twitter:card" content="summary_large_image">
    <meta name="twitter:image" content="https://space.selfware.design/og-card.png">
    <link rel="stylesheet" href="models.css">
</head>
<body>
    <div class="site-noise" aria-hidden="true"></div>
    <header class="site-header" id="top">
        <div class="site-header__inner shell">
            <a class="brand" href="/" aria-label="Model Arena home">
                <span class="brand__mark" aria-hidden="true">//</span>
                <span class="brand__copy"><strong>MODEL ARENA</strong><span>selfware.design</span></span>
            </a>
            <nav class="nav" aria-label="Primary">
                <div class="nav__links">
                    ${navLink('/', 'Home', 'home')}
                    ${navLink('/models/', 'Models', 'models')}
${loreLink ? `                    ${navLink('/models/lore.html', 'Lore', 'lore')}\n` : ''}                </div>
            </nav>
            <a class="button button--compact button--primary" href="/client.html?match_type=mobile_blitz">Enter arena</a>
        </div>
    </header>
    <main class="shell">`,
    foot: `    </main>
    <footer class="footer shell">
        <a class="brand" href="#top"><span class="brand__mark">//</span><span class="brand__copy"><strong>MODEL ARENA</strong><span>selfware.design</span></span></a>
        <p>Models evolve in code. Humans evolve in motion.</p>
        <a href="/healthz">System health</a>
    </footer>
</body>
</html>
`,
  };
}

function radarSvg(ratings, color) {
  const size = 220;
  const c = size / 2;
  const R = 78;
  const pt = (i, r) => {
    const ang = (Math.PI / 180) * (i * 60 - 90);
    return [c + r * Math.cos(ang), c + r * Math.sin(ang)];
  };
  const ring = (frac) => {
    const pts = RATING_AXES.map((_, i) => pt(i, R * frac).map((v) => v.toFixed(1)).join(','));
    return `<polygon points="${pts.join(' ')}" fill="none" stroke="rgba(202,255,0,0.14)" stroke-width="1"/>`;
  };
  const spokes = RATING_AXES.map((_, i) => {
    const [x, y] = pt(i, R);
    return `<line x1="${c}" y1="${c}" x2="${x.toFixed(1)}" y2="${y.toFixed(1)}" stroke="rgba(202,255,0,0.10)" stroke-width="1"/>`;
  }).join('');
  const valuePts = RATING_AXES.map(([key], i) => {
    const v = Math.max(0, Math.min(100, Number(ratings[key] || 0)));
    return pt(i, R * (v / 100)).map((x) => x.toFixed(1)).join(',');
  });
  const labels = RATING_AXES.map(([key, label], i) => {
    const [x, y] = pt(i, R + 16);
    const v = Number(ratings[key] || 0);
    return `<text x="${x.toFixed(1)}" y="${(y - 2).toFixed(1)}" text-anchor="middle" class="radar__label">${esc(label)}</text>`
      + `<text x="${x.toFixed(1)}" y="${(y + 9).toFixed(1)}" text-anchor="middle" class="radar__value">${v.toFixed(1)}</text>`;
  }).join('');
  return `<svg viewBox="0 0 ${size} ${size}" class="radar" role="img" aria-label="Ratings radar">
        ${ring(1)}${ring(0.75)}${ring(0.5)}${ring(0.25)}${spokes}
        <polygon points="${valuePts.join(' ')}" fill="${esc(color)}33" stroke="${esc(color)}" stroke-width="1.6"/>
        ${labels}
    </svg>`;
}

function behaviorBars(agg, color) {
  if (!agg || !agg.actionTotal) {
    return '<p class="empty">No recent duel sample available for this model.</p>';
  }
  const max = Math.max(...ACTIONS.map((a) => agg.actionCounts[a]), 1);
  const rows = ACTIONS.map((a) => {
    const n = agg.actionCounts[a];
    const share = n / agg.actionTotal;
    return `            <div class="bar-row">
                <span class="bar-row__name">${esc(a)}</span>
                <span class="bar-row__track"><span class="bar-row__fill" style="width:${((n / max) * 100).toFixed(1)}%;background:${esc(color)}"></span></span>
                <span class="bar-row__pct">${fmtPct(share)}</span>
            </div>`;
  }).join('\n');
  return `${rows}
            <p class="metric-note">Aggression index <b>${fmtPct(agg.aggression)}</b> (attack+charge share) · sampled from the ${agg.sampled} most recent duels.</p>`;
}

/** Compact per-mode breakdown: rows = modes, cols = top action, aggression, W-L-D. */
function modeTable(agg) {
  if (!agg?.modes?.length) return '';
  const rows = agg.modes.map((md) => `            <tr>
                <td class="mode-grid__mode">${esc(md.mode)}</td>
                <td class="num">${md.games}</td>
                <td class="num">${md.topAction ? `${esc(md.topAction)} ${fmtPct(md.topShare, 0)}` : '—'}</td>
                <td class="num">${fmtPct(md.aggression, 0)}</td>
                <td class="num"><span class="win">${md.w}</span>‑<span class="loss">${md.l}</span>‑${md.d}</td>
            </tr>`).join('\n');
  return `        <h3 class="mode-grid__title">Per-mode breakdown</h3>
        <table class="mode-grid">
            <thead><tr><th>Mode</th><th>Duels</th><th>Top action</th><th>Aggression</th><th>W‑L‑D</th></tr></thead>
            <tbody>
${rows}
            </tbody>
        </table>`;
}

function rivalryTable(agg, slugById, metaById) {
  const rows = [];
  for (const [other, r] of [...(agg?.rivals?.entries() || [])]) {
    const meta = metaById.get(other);
    if (!meta) continue;
    const games = r.w + r.l + r.d;
    const winRate = games ? (r.w + r.d * 0.5) / games : 0;
    rows.push({
      other, r, games, winRate,
      html: `            <tr>
                <td><a class="rival" href="${esc(slugById.get(other))}.html"><span class="rival__emoji">${esc(meta.emoji)}</span><span><b>${esc(meta.title)}</b><small>${esc(meta.shortName)}</small></span></a></td>
                <td class="num win">${r.w}</td>
                <td class="num loss">${r.l}</td>
                <td class="num">${r.d}</td>
                <td class="num">${fmtPct(winRate, 0)}</td>
                <td class="mini-bar"><span class="mini-bar__track"><span class="mini-bar__fill" style="width:${(winRate * 100).toFixed(1)}%"></span></span></td>
            </tr>`,
    });
  }
  rows.sort((a, b) => b.games - a.games);
  if (!rows.length) return '<p class="empty">No head-to-head duels in the current sample window.</p>';
  return `        <table class="rivalry">
            <thead><tr><th>Opponent</th><th>W</th><th>L</th><th>D</th><th>Score%</th><th></th></tr></thead>
            <tbody>
${rows.map((r) => r.html).join('\n')}
            </tbody>
        </table>`;
}

/** Mascot + title cell fragment for one side of a measured pair, linked. */
function measuredSide(id, metaById, slugById) {
  const meta = metaById.get(id);
  if (!meta) return esc(id);
  const inner = `${esc(meta.emoji)} <b>${esc(meta.title)}</b> <small>${esc(meta.shortName)}</small>`;
  const slug = slugById.get(id);
  return slug
    ? `<a class="measured__model" href="${esc(slug)}.html">${inner}</a>`
    : `<span class="measured__model">${inner}</span>`;
}

/**
 * Index-page section: the two extremes of the measured head-to-head table —
 * closest pairs ("Most contested", lowest leader win share) and most
 * one-sided pairs ("Nemesis matchups", highest leader win share). Every
 * number is counted from sampled duel artifacts; pairs under the minimum
 * sample are excluded, and each row states its sample size.
 */
export function measuredRivalriesSection(rivalries, metaById, slugById) {
  if (!rivalries) return '';
  const row = (e) => `                    <tr>
                        <td class="measured__pair">
                            <span class="measured__names">${measuredSide(e.leader, metaById, slugById)}<span class="measured__score"><span class="win">${fmtInt(e.leaderWins)}</span>-<span class="loss">${fmtInt(e.trailerWins)}</span></span>${measuredSide(e.trailer, metaById, slugById)}</span>
                            <small>measured over ${fmtInt(e.games)} fights${e.draws ? ` · ${fmtInt(e.draws)} draws` : ''}</small>
                        </td>
                        <td class="num">${fmtInt(e.games)}</td>
                    </tr>`;
  const table = (title, hint, entries) => `            <article class="measured__table-wrap">
                <h3>${esc(title)} <small>${esc(hint)}</small></h3>
                <table class="measured__table">
                    <thead><tr><th>Pair · score</th><th>Fights</th></tr></thead>
                    <tbody>
${entries.map(row).join('\n')}
                    </tbody>
                </table>
            </article>`;
  return `        <section class="panel measured" aria-label="Measured rivalries">
            <h2>Measured rivalries <span class="hof__hint">counted from recorded fights — no editorial picks</span></h2>
            <div class="measured__grid">
${table('Most contested', 'lowest leader win share', rivalries.contested)}
${table('Nemesis matchups', 'highest leader win share', rivalries.nemesis)}
            </div>
            <p class="metric-note">Head-to-head W-L over the full sampled duel window. Pairs with fewer than ${fmtInt(rivalries.minGames)} measured fights are excluded — ${fmtInt(rivalries.pairsMeasured)} pairs qualify.</p>
        </section>`;
}

/**
 * Model-page line: this model's single closest measured head-to-head, from
 * its own perspective ("Most contested: vs X, 82-82 over 224 fights").
 */
export function measuredContestLine(entry, modelId, metaById, slugById) {
  if (!entry) return '';
  const mine = entry.leader === modelId;
  const otherId = mine ? entry.trailer : entry.leader;
  const myWins = mine ? entry.leaderWins : entry.trailerWins;
  const theirWins = mine ? entry.trailerWins : entry.leaderWins;
  const meta = metaById.get(otherId);
  if (!meta) return '';
  const name = `${meta.emoji} ${meta.title} (${meta.shortName})`;
  const slug = slugById.get(otherId);
  const linked = slug ? `<a class="text-link" href="${esc(slug)}.html">${esc(name)}</a>` : esc(name);
  const draws = entry.draws ? ` · ${fmtInt(entry.draws)} draws` : '';
  return `            <p class="measured__note">Most contested: vs ${linked}, <span class="win">${fmtInt(myWins)}</span>-<span class="loss">${fmtInt(theirWins)}</span>${draws} over ${fmtInt(entry.games)} measured fights.</p>`;
}

function coPerformanceSection(partnerRows, metaById, slugById) {
  if (!partnerRows.length) {
    return '<p class="empty">No shared world events recorded yet.</p>';
  }
  const items = partnerRows.map(({ other, avgGap, blend }) => {
    const meta = metaById.get(other);
    return `            <a class="partner" href="${esc(slugById.get(other))}.html">
                <span class="partner__emoji">${esc(meta.emoji)}</span>
                <span class="partner__name"><b>${esc(meta.title)}</b><small>${esc(meta.shortName)}</small></span>
                <span class="partner__score"><b>${avgGap.toFixed(1)} places</b><small>avg finish gap · blend ${blend.toFixed(2)}</small></span>
            </a>`;
  }).join('\n');
  return `        <div class="partners">
${items}
        </div>
        <p class="metric-note">Co-performance proxy (league never mixes models on a team) — average finishing gap across shared world-FFA events, blended with collaboration rating.</p>`;
}

// ---------------------------------------------------------------------------
// Mixed-team chemistry overlay (optional)
// ---------------------------------------------------------------------------
//
// Rendered only when <continuousDir>/chemistry/<date>.json exists and
// validates loosely (schema v1, kind mixed_team_chemistry, a pairs array).
// The index gains a chemistry pair table; model pages gain a "Works best
// with" section (top 2 partners by pair win rate). Sample sizes are small by
// design — pairs with fewer than 3 games together are marked provisional.
// Missing or malformed data degrades to byte-identical pages.

export const CHEMISTRY_ARTIFACT_KIND = 'mixed_team_chemistry';
export const CHEMISTRY_PROVISIONAL_MIN_GAMES = 3;
export const CHEMISTRY_INDEX_PAIR_LIMIT = 10;
export const CHEMISTRY_MODEL_PARTNER_LIMIT = 2;

/**
 * Load the newest chemistry artifact under <continuousDir>/chemistry/, or
 * null when the directory/file is missing or the shape is not recognized.
 */
export function loadChemistry({ continuousDir, io = defaultIo, log = () => {} }) {
  const chemistryDir = path.join(continuousDir, 'chemistry');
  if (!io.exists(chemistryDir)) return null;
  const names = io.readdir(chemistryDir).filter((n) => n.endsWith('.json')).sort();
  for (const name of [...names].reverse()) {
    try {
      const artifact = io.readJson(path.join(chemistryDir, name));
      if (
        artifact?.schema_version === 1
        && artifact?.kind === CHEMISTRY_ARTIFACT_KIND
        && Array.isArray(artifact?.pairs)
        && artifact.pairs.every((pair) => (
          Array.isArray(pair?.models)
          && pair.models.length === 2
          && Number.isFinite(Number(pair.games_together))
          && Number.isFinite(Number(pair.win_rate))
        ))
      ) {
        return artifact;
      }
      log(`chemistry: ignoring unrecognized artifact ${name}`);
    } catch (error) {
      log(`chemistry: ignoring unreadable artifact ${name} (${String(error?.message || error).slice(0, 120)})`);
    }
  }
  return null;
}

/**
 * Display-name lookup for chemistry partner ids. Chemistry pairs reference
 * continuous-league model ids; this resolves them to weekly-roster mascots
 * (with a page link when the partner has one) or to the league mascot.
 */
export function chemistryLookup({ roster = [], metaById, slugById, continuous }) {
  const map = new Map();
  const put = (ids, display) => {
    for (const id of ids.filter(Boolean).map(String)) {
      if (!map.has(id)) map.set(id, display);
    }
  };
  for (const model of roster) {
    const meta = metaById.get(model.model_id);
    if (!meta) continue;
    put([model.model_id, model.canonical_slug, model.provider_model], {
      emoji: meta.emoji,
      title: meta.title,
      shortName: meta.shortName,
      href: slugById.get(model.model_id) ? `${slugById.get(model.model_id)}.html` : null,
    });
  }
  for (const trackId of TRACKS) {
    const slice = continuous?.state?.tracks?.[trackId];
    for (const entry of [...(slice?.roster || []), ...(slice?.retired || [])]) {
      const mascot = entry?.mascot || {};
      put([entry?.model_id, entry?.slug], {
        emoji: mascot.emoji || '🥚',
        title: mascot.title || String(entry?.slug || entry?.model_id || '?'),
        shortName: String(entry?.slug || entry?.model_id || '?'),
        href: null,
      });
    }
  }
  return (partnerId) => map.get(String(partnerId)) || {
    emoji: '🥚',
    title: String(partnerId),
    shortName: String(partnerId),
    href: null,
  };
}

/**
 * Top partners for one weekly-roster model from the chemistry artifact,
 * best pair win rate first. Matches the model's model_id/canonical slug/
 * provider id against pair membership.
 */
export function chemistryPartnersForModel(chemistry, model, limit = CHEMISTRY_MODEL_PARTNER_LIMIT) {
  const ids = new Set(
    [model?.model_id, model?.canonical_slug, model?.provider_model].filter(Boolean).map(String),
  );
  const rows = [];
  for (const pair of chemistry?.pairs || []) {
    const [left, right] = pair.models.map(String);
    let partner = null;
    if (ids.has(left) && !ids.has(right)) partner = right;
    else if (ids.has(right) && !ids.has(left)) partner = left;
    if (!partner) continue;
    rows.push({
      partner,
      games: Number(pair.games_together) || 0,
      winRate: Number(pair.win_rate) || 0,
      expected: Number(pair.expected_win_rate) || 0,
      delta: Number(pair.rating_delta_vs_expected) || 0,
      provisional: pair.provisional === true,
    });
  }
  rows.sort((a, b) => b.winRate - a.winRate || b.games - a.games || a.partner.localeCompare(b.partner));
  return rows.slice(0, limit);
}

const chemDeltaClass = (delta) => (delta >= 0 ? 'pos' : 'neg');
const chemDeltaText = (delta) => `${delta >= 0 ? '+' : ''}${delta.toFixed(3)}`;

/** Model page section: top partners by real mixed-team win rate. */
export function chemistryPartnersSection(rows, lookup) {
  if (!rows.length) return '';
  const items = rows.map(({ partner, games, winRate, delta, provisional }) => {
    const meta = lookup(partner);
    const name = `<span class="partner__name"><b>${esc(meta.title)}</b><small>${esc(meta.shortName)}</small></span>`;
    const inner = `
                <span class="partner__emoji">${esc(meta.emoji)}</span>
                ${name}
                <span class="partner__score"><b>${fmtPct(winRate, 0)} pair win rate</b><small>${games} game${games === 1 ? '' : 's'} together · Δ vs expected ${chemDeltaText(delta)}${provisional ? ' · provisional' : ''}</small></span>`;
    return meta.href
      ? `            <a class="partner" href="${esc(meta.href)}">${inner}\n            </a>`
      : `            <span class="partner">${inner}\n            </span>`;
  }).join('\n');
  return `        <section class="panel">
            <h2>Works best with</h2>
            <div class="partners">
${items}
            </div>
            <p class="metric-note">Real mixed-team chemistry — measured from mixed-squad battles where this model shared a team with the partner. Sample sizes are small; pairs with fewer than ${CHEMISTRY_PROVISIONAL_MIN_GAMES} games are provisional.</p>
        </section>`;
}

/** Index page table: strongest measured chemistry pairs across the league. */
export function chemistryPairsSection(chemistry, lookup) {
  const pairs = (chemistry?.pairs || []).slice(0, CHEMISTRY_INDEX_PAIR_LIMIT);
  if (!pairs.length) return '';
  const rows = pairs.map((pair) => {
    const [leftId, rightId] = pair.models.map(String);
    const left = lookup(leftId);
    const right = lookup(rightId);
    const delta = Number(pair.rating_delta_vs_expected) || 0;
    const provisional = pair.provisional === true;
    return `                    <tr>
                        <td class="chem__pair"><span>${esc(left.emoji)} <b>${esc(left.title)}</b></span><span class="chem__plus">+</span><span>${esc(right.emoji)} <b>${esc(right.title)}</b></span></td>
                        <td class="num">${fmtInt(pair.games_together)}</td>
                        <td class="num">${fmtPct(Number(pair.win_rate) || 0, 0)}</td>
                        <td class="num">${fmtPct(Number(pair.expected_win_rate) || 0, 0)}</td>
                        <td class="num chem__delta chem__delta--${chemDeltaClass(delta)}">${chemDeltaText(delta)}</td>
                        <td class="num">${provisional ? '<span class="chem__provisional">provisional</span>' : '—'}</td>
                    </tr>`;
  }).join('\n');
  const date = String(chemistry?.generated_at || '').slice(0, 10);
  return `        <section class="panel chem" aria-label="Model chemistry pairs">
            <h2>Model chemistry <span class="hof__hint">mixed-team pair win rates${date ? ` · ${esc(date)}` : ''}</span></h2>
            <table class="chem__table">
                <thead><tr><th>Pair</th><th>Games</th><th>Win rate</th><th>Expected</th><th>Δ vs expected</th><th></th></tr></thead>
                <tbody>
${rows}
                </tbody>
            </table>
            <p class="metric-note">Measured from mixed-squad battles (each fighter driven by its own model). Expected win rate derives from solo ratings; Δ is actual minus expected. Pairs with fewer than ${CHEMISTRY_PROVISIONAL_MIN_GAMES} games are provisional.</p>
        </section>`;
}

function fightsSection(clips, mediaBase) {
  if (!clips.length) return '<p class="empty">No recorded highlights featuring this model yet.</p>';
  const cards = clips.map((clip) => `            <a class="clip" href="${esc(mediaBase)}/${esc(clip.webm)}" target="_blank" rel="noopener">
                <img src="${esc(mediaBase)}/${esc(clip.poster)}" alt="Highlight: ${esc(clip.reason)}" loading="lazy">
                <span class="clip__meta"><b>${esc(clip.reason)}</b><small>${esc(clip.date)} · score ${Number(clip.score || 0).toFixed(1)}</small></span>
            </a>`).join('\n');
  return `        <div class="clips">
${cards}
        </div>`;
}

function provenanceFooter(ctx) {
  const sha = String(ctx.league?.ledger_sha256 || '');
  return `        <section class="provenance">
            <p class="eyebrow">Provenance</p>
            <dl>
                <div><dt>Season</dt><dd>${esc(ctx.season_id)}</dd></div>
                <div><dt>Snapshot</dt><dd>${esc(ctx.generated_at)}</dd></div>
                <div><dt>Ledger</dt><dd>sha256:${esc(sha.slice(0, 16))}…</dd></div>
                <div><dt>Epochs completed</dt><dd>${fmtInt(ctx.league?.epochs_completed)}</dd></div>
            </dl>
        </section>`;
}

export function renderModelPage(ctx) {
  const { model, slug, mascot, agg, world, partners, clips } = ctx;
  const { head, foot } = chrome({
    title: `${mascot.title} (${model.model_name}) // Model Arena`,
    description: `Arena profile for ${model.model_name}: rank #${model.rank}, ratings, behavior fingerprint, rivalries and highlights.`,
    active: 'models',
    loreLink: Boolean(ctx.loreLink),
  });
  const winRate = model.matches_played
    ? (model.wins + model.draws * 0.5) / model.matches_played
    : 0;
  const disciplineRow = world?.discipline !== null && world?.discipline !== undefined
    ? `<div><dt>Discipline</dt><dd>${fmtPct(world.discipline)}</dd></div>` : '';
  const placementRow = world?.avgPlacement
    ? `<div><dt>Avg world place</dt><dd>#${world.avgPlacement.toFixed(1)}</dd></div>` : '';

  return `${head}
        <section class="profile-hero">
            <div class="profile-hero__id">
                <span class="profile-hero__emoji" style="border-color:${esc(mascot.color)}">${esc(mascot.emoji)}</span>
                <div>
                    <p class="eyebrow"><span class="live-dot"></span> Rank #${model.rank} · ${esc(mascot.title)}</p>
                    <h1>${esc(model.model_name)}</h1>
                    <p class="profile-hero__sub">${esc(model.canonical_slug)} · <a class="text-link" href="index.html">← all models</a></p>
                </div>
            </div>
${ctx.loreTitle ? `            <p class="profile-hero__lore"><a class="text-link" href="lore.html">${esc(ctx.loreTitle)}</a></p>\n` : ''}            <dl class="stat-strip">
                <div><dt>Season points</dt><dd>${fmtInt(model.season_points)}</dd></div>
                <div><dt>Record</dt><dd>${fmtInt(model.wins)}W · ${fmtInt(model.losses)}L · ${fmtInt(model.draws)}D</dd></div>
                <div><dt>Score rate</dt><dd>${fmtPct(winRate)}</dd></div>
                <div><dt>Epoch wins</dt><dd>${fmtInt(model.epoch_wins)}</dd></div>
                ${disciplineRow}
                ${placementRow}
            </dl>
        </section>
${ctx.analyst ? `\n${ctx.analyst}\n` : ''}
        <section class="panel-grid">
            <article class="panel">
                <h2>Ratings radar</h2>
                ${radarSvg(model, mascot.color)}
            </article>
            <article class="panel">
                <h2>Behavior fingerprint</h2>
                <div class="bars">
${behaviorBars(agg, mascot.color)}
                </div>
${modeTable(agg)}
            </article>
        </section>

        <section class="panel">
            <h2>Rivalry grid</h2>
${ctx.measuredContest ? `${ctx.measuredContest}\n` : ''}${rivalryTable(agg, ctx.slugById, ctx.metaById)}
        </section>

        <section class="panel">
            <h2>Plays well alongside</h2>
${coPerformanceSection(partners, ctx.metaById, ctx.slugById)}
        </section>
${ctx.chemistry ? `\n${ctx.chemistry}\n` : ''}
        <section class="panel">
            <h2>Fights</h2>
${fightsSection(clips, ctx.mediaBase)}
        </section>
${ctx.lineage ? `\n${ctx.lineage}\n` : ''}
${provenanceFooter(ctx)}
${foot}`;
}

export function renderIndexPage(ctx) {
  const { head, foot } = chrome({
    title: 'Models // Model Arena',
    description: 'The weekly top-10 model roster: rankings, season points and per-model profile pages.',
    active: 'models',
    loreLink: Boolean(ctx.loreLink),
  });
  const continuous = ctx.continuous || null;
  const rows = ctx.cards.map((c) => `            <a class="model-row" href="${esc(c.slug)}.html">
                <span class="model-row__rank">${String(c.model.rank).padStart(2, '0')}</span>
                <span class="model-row__emoji" style="border-color:${esc(c.mascot.color)}">${esc(c.mascot.emoji)}</span>
                <span class="model-row__name"><b>${esc(c.mascot.title)}</b><small>${esc(c.model.model_name)}</small></span>
                <span class="model-row__points"><b>${fmtInt(c.model.season_points)}</b><small>season pts</small></span>
                <span class="model-row__go" aria-hidden="true">↗</span>
            </a>`).join('\n');
  return `${head}
        <section class="models-hero">
            <p class="eyebrow"><span class="live-dot"></span> ${esc(ctx.seasonId)}</p>
            <h1>Weekly top 10. <em>One tour.</em></h1>
            <p class="models-hero__lede">Every model below holds a live profile: ratings, behavior fingerprint, head-to-head rivalries, world co-performance and recorded fights.</p>
        </section>
${ctx.seasonBanner ? `${ctx.seasonBanner}\n` : ''}${ctx.chronicle ? `${ctx.chronicle}\n` : ''}${continuous ? `${continuousLeagueHeader(continuous.state, ctx.nowMs ?? Date.now())}\n` : ''}        <section class="model-list" aria-label="Ranked models">
${rows}
        </section>
${ctx.toplist ? `${ctx.toplist}\n` : ''}${continuous ? `${[standingsSection(continuous.state), matrixSection(continuous.state), ctx.chemistry, announcementsSection(allAnnouncements(continuous.state)), hallOfFameSection(continuous.state)].filter(Boolean).join('\n')}\n` : ''}${ctx.measured ? `${ctx.measured}\n` : ''}${provenanceFooter(ctx)}
${foot}`;
}

// ---------------------------------------------------------------------------
// Continuous Model League overlay (optional)
// ---------------------------------------------------------------------------
//
// Rendered only when <continuousDir>/state.json exists and validates against
// the league's own schema (v2, four intervention tracks). The index gains a
// league status strip, per-track standings, the model × track experiment
// matrix, a track-badged announcements feed and a per-track Hall of Fame;
// model pages gain a per-track submission lineage timeline; and a compact
// league.json is emitted for the landing page ticker. Anything missing or
// malformed degrades to the weekly view.

export const ANNOUNCEMENT_ICONS = { entrant: '🌱', revision: '🔧', retirement: '🪦' };
export const ANNOUNCEMENTS_PAGE_LIMIT = 20;
export const ANNOUNCEMENTS_TICKER_LIMIT = 10;

const REVISION_OUTCOME_LABELS = {
  accepted: 'accepted',
  compile_failed: 'compile failed',
  codegen_failed: 'codegen failed',
  interrupted: 'interrupted',
};

const LINEAGE_OUTCOME_META = {
  entrant: { icon: ANNOUNCEMENT_ICONS.entrant, label: 'entered the league' },
  accepted: { icon: ANNOUNCEMENT_ICONS.revision, label: 'accepted' },
  compile_failed: { icon: '⚠️', label: 'compile failed' },
  codegen_failed: { icon: '⚠️', label: 'codegen failed' },
  interrupted: { icon: '⚠️', label: 'interrupted' },
};

/** Parse submissions.jsonl, tolerating a torn tail: unparseable lines are skipped. */
export function parseSubmissionsJsonl(text) {
  const out = [];
  for (const line of String(text || '').split('\n')) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      const record = JSON.parse(trimmed);
      if (record && typeof record === 'object' && record.model_id) out.push(record);
    } catch {
      // Torn tail from a crash mid-append — skip the partial line.
    }
  }
  return out;
}

/** Display labels for the intervention tracks (multi-track amendment). */
export const TRACK_LABELS = {
  L0: 'Zero-shot',
  L1: 'Compile-fix',
  L2: 'Two-iteration',
  L3: 'Weekly feedback',
};

/**
 * Load the continuous league overlay from continuousDir, or return null when
 * the state is missing or fails schema validation — publishing must never be
 * blocked by league trouble.
 *
 * Multi-track (schema v2): the state carries one slice per track under
 * `tracks`; per-track history snapshots live under `tracks/<T>/history/*.json`
 * (flattened and sorted by timestamp, unreadable day files skipped); the
 * shared submissions.jsonl is parsed whole — records carry `track` (and
 * `stint`) and are filtered per track at render time.
 */
export function loadContinuousLeague({ continuousDir, io = defaultIo, log = () => {} }) {
  const statePath = path.join(continuousDir, 'state.json');
  if (!io.exists(statePath)) return null;
  let state;
  try {
    state = validateState(io.readJson(statePath));
  } catch (error) {
    log(`continuous: ignoring invalid state (${String(error?.message || error).slice(0, 200)})`);
    return null;
  }

  let submissions = [];
  const submissionsPath = path.join(continuousDir, 'submissions.jsonl');
  if (io.exists(submissionsPath)) {
    try {
      submissions = parseSubmissionsJsonl(io.readText(submissionsPath));
    } catch {
      log('continuous: submissions.jsonl unreadable — lineage sections will be empty');
    }
  }

  const snapshots = {}; // trackId -> snapshots sorted by timestamp
  for (const trackId of TRACKS) {
    const historyDir = path.join(continuousDir, 'tracks', trackId, 'history');
    const list = [];
    if (io.exists(historyDir)) {
      for (const name of io.readdir(historyDir).filter((n) => n.endsWith('.json')).sort()) {
        try {
          const entries = io.readJson(path.join(historyDir, name));
          if (Array.isArray(entries)) list.push(...entries);
        } catch {
          // Skip an unreadable history day rather than dropping the overlay.
        }
      }
    }
    list.sort((a, b) => String(a?.at || '').localeCompare(String(b?.at || '')));
    snapshots[trackId] = list;
  }
  return { state, submissions, snapshots };
}

/**
 * Match a weekly-roster model to its entry in one track slice (active or
 * retired) via canonical slug / provider id.
 */
export function findContinuousEntry(trackSlice, model) {
  const ids = new Set(
    [model.canonical_slug, model.provider_model, model.model_id].filter(Boolean).map(String),
  );
  const match = (e) => ids.has(String(e.slug)) || ids.has(String(e.model_id));
  return trackSlice.roster.find(match) || trackSlice.retired.find(match) || null;
}

/** All announcements across tracks, newest first, each tagged with its track. */
export function allAnnouncements(state) {
  const out = [];
  for (const trackId of TRACKS) {
    for (const a of state.tracks[trackId]?.announcements || []) {
      out.push({ ...a, track: a.track ?? trackId });
    }
  }
  return out.sort((a, b) => String(b?.at || '').localeCompare(String(a?.at || '')));
}

/** Newest-first view of an announcement list, capped at `limit`. */
export function latestAnnouncements(announcements, limit) {
  return (Array.isArray(announcements) ? [...announcements] : [])
    .sort((a, b) => String(b?.at || '').localeCompare(String(a?.at || '')))
    .slice(0, limit);
}

// Division pyramid — mirror of divisionSlices in continuous_league.mjs
// (copied, not imported: the supervisor module runs its own service loop).
// Divisions are DERIVED from the roster (rating desc, slug tiebreak) and
// never persisted; the league evaluates one season per division per day.
export const DIVISION_SIZE = 10;
export const DIVISION_NAMES = Object.freeze(['premier', 'challenger', 'contender', 'prospect']);

/** Partition a track roster into rating-ordered division slices. */
export function divisionSlices(roster, size = DIVISION_SIZE) {
  const sorted = [...(Array.isArray(roster) ? roster : [])].sort((a, b) => (
    (Number(b.rating) || 0) - (Number(a.rating) || 0)
    || String(a.slug).localeCompare(String(b.slug))
  ));
  const slices = [];
  for (let index = 0; index < sorted.length; index += size) {
    const name = DIVISION_NAMES[index / size] ?? `division-${index / size + 1}`;
    slices.push({ name, models: sorted.slice(index, index + size), offset: index });
  }
  return slices;
}

/** Display label for a division key: 'premier' -> 'Premier', 'division-5' -> 'Division 5'. */
export function divisionLabel(name) {
  return DIVISION_NAMES.includes(name)
    ? name[0].toUpperCase() + name.slice(1)
    : String(name).replace(/^division-/, 'Division ');
}

function divisionBadge(name) {
  return `<span class="division-badge division-badge--${esc(name)}">${esc(divisionLabel(name))}</span>`;
}

/** Roster of one track slice, rank-sorted exactly like divisionSlices. */
export function trackStandings(trackSlice) {
  return divisionSlices(trackSlice?.roster).flatMap((d) => d.models);
}

/**
 * Ticker payload emitted as static_client/models/league.json. Back-compat:
 * the legacy flat `day_index` (max across tracks) and `announcements`
 * (merged latest 10) fields are kept for the current landing ticker; the
 * `tracks` map adds per-track day index and top-3 standings.
 */
export function leagueTickerPayload(state) {
  const announcements = latestAnnouncements(allAnnouncements(state), ANNOUNCEMENTS_TICKER_LIMIT)
    .map((a) => ({
      type: a.type,
      at: a.at,
      track: a.track,
      slug: a.slug,
      mascot: a.mascot,
      version: a.version,
      outcome: a.outcome,
      provider_rank: a.provider_rank,
    }));
  const tracks = {};
  for (const trackId of TRACKS) {
    const slice = state.tracks[trackId];
    tracks[trackId] = {
      day_index: slice.day_index,
      standings: trackStandings(slice).slice(0, 3).map((e) => ({
        slug: e.slug,
        mascot: e.mascot,
        rating: e.rating,
        wins: e.wins,
        losses: e.losses,
        draws: e.draws,
        submissions_used: e.submissions_used,
        submissions_allowed: slice.policy.max_submissions,
      })),
    };
  }
  return {
    day_index: Math.max(...TRACKS.map((trackId) => state.tracks[trackId].day_index)),
    announcements,
    tracks,
  };
}

function fmtLeagueTs(iso) {
  const ms = Date.parse(iso);
  if (!Number.isFinite(ms)) return String(iso ?? '');
  return `${new Date(ms).toISOString().slice(0, 16).replace('T', ' ')} UTC`;
}

function fmtCountdown(ms) {
  const totalMin = Math.floor(ms / 60000);
  const d = Math.floor(totalMin / 1440);
  const h = Math.floor((totalMin % 1440) / 60);
  const m = totalMin % 60;
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

function trackBadge(trackId) {
  return `<span class="track-badge track-badge--${esc(trackId)}">${esc(trackId)}</span>`;
}

function feedbackText(slice, nowMs) {
  const interval = slice.policy?.feedback_interval_ms;
  if (interval == null) return 'never revises';
  if (slice.last_feedback_at === null) return 'feedback at next cycle';
  const remaining = Date.parse(slice.last_feedback_at) + interval - nowMs;
  return remaining <= 0 ? 'feedback due now' : `feedback in ${fmtCountdown(remaining)}`;
}

function continuousLeagueHeader(state, nowMs) {
  const cells = TRACKS.map((trackId) => {
    const slice = state.tracks[trackId];
    return `            <div><dt>${trackBadge(trackId)} ${esc(TRACK_LABELS[trackId])}</dt><dd>day ${fmtInt(slice.day_index)} · ${slice.roster.length}/${MAX_ROSTER_SIZE} slots</dd><dd class="league-strip__sub">${esc(feedbackText(slice, nowMs))}</dd></div>`;
  }).join('\n');
  return `        <section class="league-strip league-strip--tracks" aria-label="Continuous league status">
            <div><dt>Continuous league</dt><dd>${esc(state.league_id)}</dd><dd class="league-strip__sub">4-track intervention experiment</dd></div>
${cells}
        </section>`;
}

function announcementText(a) {
  switch (a.type) {
    case 'entrant':
      return `enters the league${a.provider_rank ? ` · OpenRouter #${a.provider_rank}` : ''}`;
    case 'revision':
      return `v${a.version} ${REVISION_OUTCOME_LABELS[a.outcome] || a.outcome || 'revision'}`;
    case 'retirement':
      return 'retires to the Hall of Fame';
    default:
      return String(a.type || 'update');
  }
}

function announcementsSection(announcements) {
  const items = latestAnnouncements(announcements, ANNOUNCEMENTS_PAGE_LIMIT);
  if (!items.length) return '';
  const rows = items.map((a) => `            <li class="announce__item announce__item--${esc(a.type)}">
                <span class="announce__icon" aria-hidden="true">${ANNOUNCEMENT_ICONS[a.type] || '📣'}</span>
                ${trackBadge(a.track)}
                <span class="announce__name">${esc(a.mascot?.emoji || '')} ${esc(a.mascot?.title || a.slug || a.model_id || 'unknown')}</span>
                <span class="announce__text">${esc(announcementText(a))}</span>
                <time class="announce__at" datetime="${esc(a.at)}">${esc(fmtLeagueTs(a.at))}</time>
            </li>`).join('\n');
  return `        <section class="panel announce" aria-label="League announcements">
            <h2>Announcements</h2>
            <ul class="announce__feed">
${rows}
            </ul>
        </section>`;
}

function standingsSection(state) {
  const tables = TRACKS.map((trackId) => {
    const slice = state.tracks[trackId];
    const allowed = slice.policy.max_submissions;
    const divisions = divisionSlices(slice.roster).map((division) => {
      const rows = division.models.map((e, i) => {
        const overall = division.offset + i + 1;
        return `                        <tr>
                            <td class="num standings__rank">#${overall} · ${esc(divisionLabel(division.name))} #${i + 1}</td>
                            <td><span class="standings__emoji">${esc(e.mascot.emoji)}</span> <b>${esc(e.mascot.title)}</b></td>
                            <td class="num standings__rating">${Number(e.rating).toFixed(1)}</td>
                            <td class="num"><span class="win">${fmtInt(e.wins)}</span>‑<span class="loss">${fmtInt(e.losses)}</span>‑${fmtInt(e.draws)}</td>
                            <td class="num">${e.submissions_used}/${allowed}</td>
                        </tr>`;
      }).join('\n');
      return `                <div class="standings__division">
                    <h4>${divisionBadge(division.name)}</h4>
                    <table class="standings__table">
                        <thead><tr><th>Rank</th><th>Model</th><th>Rating</th><th>W‑L‑D</th><th>Subs</th></tr></thead>
                        <tbody>
${rows}
                        </tbody>
                    </table>
                </div>`;
    }).join('\n');
    return `            <article class="standings__track">
                <h3>${trackBadge(trackId)} ${esc(TRACK_LABELS[trackId])} <small>day ${fmtInt(slice.day_index)}</small></h3>
${divisions}
            </article>`;
  }).join('\n');
  return `        <section class="panel standings" aria-label="Per-track standings">
            <h2>Track standings <span class="hof__hint">divisions of ${DIVISION_SIZE}, derived from rating</span></h2>
            <div class="standings__grid">
${tables}
            </div>
        </section>`;
}

/**
 * Experiment matrix: model × track ratings, plus the cross-track delta
 * (L3 − L0) per model — how much weekly feedback helps that model. Rows are
 * sorted by L0 rating desc (slug tiebreak, matching the division derivation);
 * each row carries its L0 division tag and a subtle separator marks division
 * boundaries. Models without an L0 cell sort after, by best available rating.
 * Retired cells keep their final rating, marked 🪦.
 */
function matrixSection(state) {
  // L0 division membership is derived from the ACTIVE roster only (retired
  // models hold no division); key by model_id for badge lookup.
  const l0DivisionById = new Map();
  for (const division of divisionSlices(state.tracks.L0?.roster)) {
    for (const e of division.models) l0DivisionById.set(String(e.model_id), division.name);
  }

  const byModel = new Map(); // modelKey -> { mascot, cells: {track -> {rating, retired}} }
  for (const trackId of TRACKS) {
    const slice = state.tracks[trackId];
    for (const [list, retired] of [[slice.roster, false], [slice.retired, true]]) {
      for (const e of list) {
        const key = String(e.model_id);
        if (!byModel.has(key)) byModel.set(key, { key, mascot: e.mascot, slug: e.slug, cells: {} });
        byModel.get(key).cells[trackId] = { rating: Number(e.rating), retired };
      }
    }
  }
  if (!byModel.size) return '';
  const rows = [...byModel.values()].map((row) => {
    const l0 = row.cells.L0?.rating;
    const l3 = row.cells.L3?.rating;
    return {
      ...row,
      delta: l0 !== undefined && l3 !== undefined ? l3 - l0 : null,
      division: l0DivisionById.get(row.key) ?? null,
    };
  }).sort((a, b) => {
    const la = a.cells.L0?.rating;
    const lb = b.cells.L0?.rating;
    if (la !== undefined && lb !== undefined) {
      return lb - la || String(a.slug).localeCompare(String(b.slug));
    }
    if (la !== undefined) return -1;
    if (lb !== undefined) return 1;
    const best = (r) => Math.max(...TRACKS.map((t) => r.cells[t]?.rating ?? -1));
    return best(b) - best(a) || String(a.slug).localeCompare(String(b.slug));
  });
  const body = rows.map((row, i) => {
    const cells = TRACKS.map((trackId) => {
      const cell = row.cells[trackId];
      if (!cell) return '                        <td class="num matrix__cell matrix__cell--empty">—</td>';
      return `                        <td class="num matrix__cell${cell.retired ? ' matrix__cell--retired' : ''}">${cell.rating.toFixed(1)}${cell.retired ? ' 🪦' : ''}</td>`;
    }).join('\n');
    const delta = row.delta === null
      ? '<td class="num matrix__delta">—</td>'
      : `<td class="num matrix__delta matrix__delta--${row.delta >= 0 ? 'pos' : 'neg'}">${row.delta >= 0 ? '+' : ''}${row.delta.toFixed(1)}</td>`;
    const badge = row.division ? `${divisionBadge(row.division)} ` : '';
    const boundary = row.division && i > 0 && rows[i - 1].division !== row.division
      ? ' matrix__row--boundary'
      : '';
    return `                    <tr class="matrix__row${boundary}">
                        <td class="matrix__model"><span>${badge}<span class="standings__emoji">${esc(row.mascot.emoji)}</span> <b>${esc(row.mascot.title)}</b></span><small>${esc(row.slug)}</small></td>
${cells}
                        ${delta}
                    </tr>`;
  }).join('\n');
  return `        <section class="panel matrix" aria-label="Experiment matrix">
            <h2>Experiment matrix <span class="hof__hint">rating per track · Δ = L3 − L0 · rows in L0 division order</span></h2>
            <table class="matrix__table">
                <thead><tr><th>Model</th>${TRACKS.map((t) => `<th>${trackBadge(t)}</th>`).join('')}<th>Δ feedback</th></tr></thead>
                <tbody>
${body}
                </tbody>
            </table>
            <p class="metric-note">Same v1 artifacts in every track; tracks diverge only by compile-fix and feedback policy. Raw measured stats only — no coaching.</p>
        </section>`;
}

function hallOfFameSection(state) {
  const groups = TRACKS
    .map((trackId) => ({ trackId, retired: state.tracks[trackId]?.retired || [] }))
    .filter((g) => g.retired.length);
  if (!groups.length) return '';
  const sections = groups.map(({ trackId, retired }) => {
    const cards = [...retired].reverse().map((e) => `                <article class="hof__card">
                    <span class="hof__emoji" style="border-color:${esc(e.mascot.color)}">${esc(e.mascot.emoji)}</span>
                    <div class="hof__id">
                        <b>${esc(e.mascot.title)}</b>
                        <small>${esc(e.slug)}</small>
                    </div>
                    <dl class="hof__stats">
                        <div><dt>Days in league</dt><dd>${fmtInt(e.days_in_league)}</dd></div>
                        <div><dt>Final rating</dt><dd>${Number(e.rating).toFixed(1)}</dd></div>
                        <div><dt>Record</dt><dd><span class="win">${fmtInt(e.wins)}</span>W · <span class="loss">${fmtInt(e.losses)}</span>L · ${fmtInt(e.draws)}D</dd></div>
                    </dl>
                    <p class="hof__reason">${esc(e.reason)}</p>
                </article>`).join('\n');
    return `            <h3 class="hof__track">${trackBadge(trackId)} ${esc(TRACK_LABELS[trackId])}</h3>
            <div class="hof__grid">
${cards}
            </div>`;
  }).join('\n');
  return `        <section class="panel hof" aria-label="Hall of Fame">
            <h2>Hall of Fame <span class="hof__hint">🪦 retired with honors, per track</span></h2>
${sections}
        </section>`;
}

/** Cumulative W/L/D timeline for one model, from flattened history snapshots. */
function statsTimeline(snapshots, modelId) {
  const out = [];
  for (const snap of snapshots) {
    const entry = Array.isArray(snap?.roster)
      ? snap.roster.find((r) => r?.model_id === modelId)
      : null;
    if (!entry) continue;
    out.push({
      at: String(snap.at || ''),
      wins: Number(entry.wins) || 0,
      losses: Number(entry.losses) || 0,
      draws: Number(entry.draws) || 0,
    });
  }
  return out.sort((a, b) => a.at.localeCompare(b.at));
}

/** Latest cumulative stats strictly before `beforeIso` (or overall when null). */
function statsBefore(timeline, beforeIso) {
  let hit = null;
  for (const point of timeline) {
    if (beforeIso !== null && point.at >= beforeIso) break;
    hit = point;
  }
  return hit;
}

/**
 * Build the submission lineage for one continuous league entry: v1 (the
 * entrant) plus every recorded revision attempt ordered by version. Each
 * version that actually went live (entrant + accepted revisions) carries the
 * W/L/D delta accumulated while it was the active artifact, derived from the
 * daily history snapshots; failed attempts never go live and carry no delta.
 */
export function buildLineage(entry, submissions, snapshots) {
  const mine = submissions
    .filter((s) => s.model_id === entry.model_id && Number.isSafeInteger(s.version_attempted))
    .sort((a, b) => a.version_attempted - b.version_attempted);
  const nodes = [{
    version: 1,
    at: entry.joined_at,
    outcome: 'entrant',
    compileAttempts: 1,
  }];
  for (const s of mine) {
    nodes.push({
      version: s.version_attempted,
      at: s.at,
      outcome: s.outcome || 'unknown',
      compileAttempts: Number(s.compile_attempts) || 0,
    });
  }

  const timeline = statsTimeline(snapshots, entry.model_id);
  const live = nodes.filter((n) => n.outcome === 'entrant' || n.outcome === 'accepted');
  const deltaByVersion = new Map();
  live.forEach((node, i) => {
    const end = i + 1 < live.length ? live[i + 1].at : null;
    const endStats = statsBefore(timeline, end);
    const startStats = node.outcome === 'entrant'
      ? { wins: 0, losses: 0, draws: 0 } // entrants join with a clean record
      : statsBefore(timeline, node.at);
    if (endStats && startStats) {
      deltaByVersion.set(node.version, {
        w: endStats.wins - startStats.wins,
        l: endStats.losses - startStats.losses,
        d: endStats.draws - startStats.draws,
      });
    }
  });
  return nodes.map((n) => ({ ...n, delta: deltaByVersion.get(n.version) || null }));
}

/**
 * Submission lineage for one weekly-roster model, grouped per track: for each
 * track where the model has an entry, v1 (the entrant) plus that track's
 * revision attempts (shared submissions.jsonl filtered by track and stint),
 * with deltas derived from that track's own history snapshots.
 */
function lineageSection(state, model, submissions, snapshotsByTrack) {
  const blocks = [];
  for (const trackId of TRACKS) {
    const slice = state.tracks[trackId];
    const entry = findContinuousEntry(slice, model);
    if (!entry) continue;
    const trackSubmissions = submissions.filter((s) => (s.track ?? null) === trackId
      && (s.stint == null || s.stint === entry.joined_at));
    const nodes = buildLineage(entry, trackSubmissions, snapshotsByTrack[trackId] || []);
    if (!nodes.length) continue;
    const items = nodes.map((n) => {
      const meta = LINEAGE_OUTCOME_META[n.outcome] || { icon: '🔧', label: String(n.outcome) };
      const delta = n.delta
        ? `<span class="lineage__delta">+${n.delta.w}W +${n.delta.l}L +${n.delta.d}D while live</span>`
        : '';
      return `            <li class="lineage__node lineage__node--${esc(n.outcome)}">
                <span class="lineage__icon" aria-hidden="true">${meta.icon}</span>
                <span class="lineage__version">v${n.version}</span>
                <span class="lineage__outcome">${esc(meta.label)}</span>
                <span class="lineage__meta">compile attempts ${n.compileAttempts}</span>
                ${delta}
                <time class="lineage__at" datetime="${esc(n.at)}">${esc(fmtLeagueTs(n.at))}</time>
            </li>`;
    }).join('\n');
    blocks.push(`            <h3 class="lineage__track">${trackBadge(trackId)} ${esc(TRACK_LABELS[trackId])}</h3>
            <ol class="lineage__timeline">
${items}
            </ol>`);
  }
  if (!blocks.length) return '';
  return `        <section class="panel lineage" aria-label="Submission lineage">
            <h2>Submission lineage <span class="hof__hint">per track</span></h2>
${blocks.join('\n')}
            <p class="metric-note">W/L/D deltas derive from the track's daily history snapshots; a failed attempt consumes a submission but the previous artifact stays live.</p>
        </section>`;
}

// ---------------------------------------------------------------------------
// Analyst toplist (optional editorial overlay)
// ---------------------------------------------------------------------------
//
// Authored commentary (scripts/arena/toplist_commentary.json) rendered as a
// ranked toplist on the index and a per-model quote block on profile pages.
// Like the continuous overlay, a missing or malformed file hides the sections
// entirely and the weekly-league HTML stays byte-identical to a build without
// it; models.css always carries the styles. This is site-facing editorial —
// the no-coaching neutrality rule applies only to model feedback briefs,
// never to public analysis.

/**
 * Load authored toplist commentary, or return null when the file is absent,
 * unparseable, or carries no usable entries — publishing must never be
 * blocked by editorial trouble. Usable entries are rank-sorted.
 */
export function loadToplist({ toplistPath, io = defaultIo, log = () => {} }) {
  if (!toplistPath || !io.exists(toplistPath)) return null;
  let data;
  try {
    data = io.readJson(toplistPath);
  } catch (error) {
    log(`toplist: ignoring unreadable ${toplistPath} (${String(error?.message || error).slice(0, 200)})`);
    return null;
  }
  const entries = (Array.isArray(data?.entries) ? data.entries : [])
    .filter((e) => e && typeof e === 'object' && e.slug && e.headline && e.commentary
      && Number.isFinite(Number(e.rank)))
    .map((e) => ({
      slug: String(e.slug),
      rank: Number(e.rank),
      headline: String(e.headline),
      commentary: String(e.commentary),
    }))
    .sort((a, b) => a.rank - b.rank || a.slug.localeCompare(b.slug));
  if (!entries.length) return null;
  return {
    generated_at: data.generated_at ?? null,
    league_day: data.league_day ?? null,
    entries,
  };
}

/**
 * Match a weekly-roster model to its toplist entry via slug prefix (entry
 * slugs are provider-style and undated, e.g. "deepseek/deepseek-v4-flash").
 * Longest matching entry slug wins so a dated variant ("...-flash-0731") is
 * not shadowed by its plainer sibling ("...-flash").
 */
export function matchToplistEntry(entries, model) {
  const ids = [model.canonical_slug, model.provider_model, model.model_id]
    .filter(Boolean).map(String);
  let best = null;
  for (const e of entries || []) {
    if (ids.some((id) => id === e.slug || id.startsWith(`${e.slug}-`) || id.startsWith(`${e.slug}/`))) {
      if (!best || e.slug.length > best.slug.length) best = e;
    }
  }
  return best;
}

/** Index-page view of the toplist: rank-sorted cards with mascot + roster link. */
function toplistCards(toplist, roster, slugById) {
  return toplist.entries.map((e) => {
    const model = roster.find((m) => matchToplistEntry([e], m));
    return {
      ...e,
      mascot: mascotFor(e.slug),
      href: model ? `${slugById.get(model.model_id)}.html` : null,
      onRoster: Boolean(model),
      sub: model ? String(model.model_name) : e.slug,
    };
  });
}

function toplistSection(toplist, cards) {
  const items = cards.map((c) => {
    const badge = c.onRoster ? '' : '<span class="toplist__badge">league</span>';
    const inner = `                <span class="toplist__rank">${String(c.rank).padStart(2, '0')}</span>
                <span class="toplist__emoji" style="border-color:${esc(c.mascot.color)}">${esc(c.mascot.emoji)}</span>
                <div class="toplist__body">
                    <p class="toplist__model"><b>${esc(c.mascot.title)}</b><small>${esc(c.sub)}</small>${badge}</p>
                    <p class="toplist__headline">${esc(c.headline)}</p>
                    <p class="toplist__commentary">${esc(c.commentary)}</p>
                </div>`;
    return c.href
      ? `            <a class="toplist__card" href="${esc(c.href)}">\n${inner}\n            </a>`
      : `            <article class="toplist__card">\n${inner}\n            </article>`;
  }).join('\n');
  const day = toplist.league_day !== null && toplist.league_day !== undefined
    ? ` <span class="hof__hint">league day ${fmtInt(toplist.league_day)}</span>` : '';
  return `        <section class="panel toplist" aria-label="Analyst toplist">
            <h2>Analyst Toplist${day}</h2>
            <div class="toplist__grid">
${items}
            </div>
        </section>`;
}

/** Per-model quote block: headline + commentary + league-day provenance. */
function analystNoteSection(entry, leagueDay) {
  const day = leagueDay !== null && leagueDay !== undefined ? ` · league day ${fmtInt(leagueDay)}` : '';
  return `        <section class="panel analyst-note" aria-label="Analyst note">
            <p class="eyebrow">Analyst note${day}</p>
            <h2 class="analyst-note__headline">${esc(entry.headline)}</h2>
            <p class="analyst-note__commentary">${esc(entry.commentary)}</p>
        </section>`;
}

// ---------------------------------------------------------------------------
// League Chronicle (optional editorial overlay)
// ---------------------------------------------------------------------------
//
// Authored narrative (scripts/arena/chronicle.json) rendered as a magazine-style
// story at the top of the models index — the league's running serial. Like the
// toplist, a missing or malformed file hides the section entirely and the HTML
// stays byte-identical to a build without it; models.css always carries the
// styles. Public-facing editorial: the no-coaching neutrality rule applies
// only to model feedback briefs, never to the chronicle.

/**
 * Load authored chronicle chapters, or return null when the file is absent,
 * unparseable, or carries no usable chapters — publishing must never be
 * blocked by editorial trouble. Chapters render in authored order.
 */
export function loadChronicle({ chroniclePath, io = defaultIo, log = () => {} }) {
  if (!chroniclePath || !io.exists(chroniclePath)) return null;
  let data;
  try {
    data = io.readJson(chroniclePath);
  } catch (error) {
    log(`chronicle: ignoring unreadable ${chroniclePath} (${String(error?.message || error).slice(0, 200)})`);
    return null;
  }
  const chapters = (Array.isArray(data?.chapters) ? data.chapters : [])
    .filter((c) => c && typeof c === 'object' && c.title && c.day
      && Array.isArray(c.prose) && c.prose.some((p) => typeof p === 'string' && p.trim()))
    .map((c) => ({
      id: c.id ? String(c.id) : null,
      title: String(c.title),
      day: String(c.day),
      prose: c.prose.filter((p) => typeof p === 'string' && p.trim()).map(String),
    }));
  if (!chapters.length) return null;
  return { generated_at: data.generated_at ?? null, chapters };
}

/**
 * Index-page story: chapters in authored order, the last one flagged as the
 * latest. First paragraph of each chapter carries the drop cap.
 */
function chronicleSection(chronicle) {
  const last = chronicle.chapters.length - 1;
  const chapters = chronicle.chapters.map((c, i) => {
    const latest = i === last && chronicle.chapters.length > 1
      ? ' <span class="chronicle__latest">latest</span>' : '';
    const paragraphs = c.prose.map((p, j) => {
      const cls = j === 0 ? 'chronicle__prose chronicle__prose--lead' : 'chronicle__prose';
      return `                <p class="${cls}">${esc(p)}</p>`;
    }).join('\n');
    return `            <article class="chronicle__chapter"${c.id ? ` id="${esc(c.id)}"` : ''}>
                <p class="chronicle__day">${esc(c.day)}${latest}</p>
                <h3 class="chronicle__chapter-title">${esc(c.title)}</h3>
${paragraphs}
            </article>`;
  }).join('\n');
  return `        <section class="chronicle" aria-label="League Chronicle">
            <p class="eyebrow chronicle__eyebrow"><span class="live-dot"></span> League Chronicle</p>
            <h2 class="chronicle__title">The season, <em>as it happened.</em></h2>
${chapters}
        </section>`;
}

// ---------------------------------------------------------------------------
// Season structure + arena lore (optional editorial overlays)
// ---------------------------------------------------------------------------
//
// Season definitions (scripts/arena/seasons.json) give the continuous league a
// narrative arc: a banner on the models index with the current season, its
// day counter and progress bar, plus a past-seasons strip. The banner renders
// only when seasons.json loads AND the continuous league state validates —
// seasons describe the continuous league, so without it there is nothing to
// frame. Arena lore (scripts/arena/lore.json) renders a magazine-style lore
// page (world premise, fighter entries, lexicon), a "Lore" nav link, and a
// subtle lore-title line on matching model pages (prefix match like the
// toplist matcher). Both follow the standing rule: absent or malformed files
// hide every section and the HTML stays byte-identical to a build without
// them (models.css always carries the styles; a stale lore.html from a
// previous valid run is removed).

/**
 * Load season definitions, or return null when the file is absent,
 * unparseable, or carries no usable current season — publishing must never
 * be blocked by editorial trouble.
 */
export function loadSeasons({ seasonsPath, io = defaultIo, log = () => {} }) {
  if (!seasonsPath || !io.exists(seasonsPath)) return null;
  let data;
  try {
    data = io.readJson(seasonsPath);
  } catch (error) {
    log(`seasons: ignoring unreadable ${seasonsPath} (${String(error?.message || error).slice(0, 200)})`);
    return null;
  }
  const lengthDays = Number(data?.season_length_days);
  const cur = data?.current;
  const startedMs = Date.parse(cur?.started_at);
  if (!Number.isFinite(lengthDays) || lengthDays <= 0
    || !cur || !cur.id || !cur.name || !Number.isFinite(startedMs)) {
    log('seasons: ignoring definition without a usable current season');
    return null;
  }
  const championText = (c) => {
    if (!c) return null;
    if (typeof c === 'string') return c;
    return String(c.title || c.slug || '') || null;
  };
  return {
    seasonLengthDays: lengthDays,
    current: {
      id: String(cur.id),
      name: String(cur.name),
      startedMs,
      theme: cur.theme ? String(cur.theme) : null,
      championRule: cur.champion_rule ? String(cur.champion_rule) : null,
    },
    archive: (Array.isArray(data.archive) ? data.archive : [])
      .filter((s) => s && typeof s === 'object' && s.id && s.name)
      .map((s) => ({ id: String(s.id), name: String(s.name), champion: championText(s.champion) })),
  };
}

/** "S1" + "Genesis" -> "Season 1 · Genesis"; non-numeric ids render as-is. */
export function seasonDisplayName(id, name) {
  const m = /^S(\d+)$/i.exec(String(id));
  return `${m ? `Season ${m[1]}` : String(id)} · ${String(name)}`;
}

/**
 * Index-page banner: current season, day counter computed from started_at, a
 * thin progress bar, the theme line, the champion rule as small print, and a
 * past-seasons strip when the archive is non-empty.
 */
export function seasonBanner(seasons, nowMs, { loreLink = false } = {}) {
  const { current, seasonLengthDays, archive } = seasons;
  const elapsed = nowMs - current.startedMs;
  const day = Math.min(seasonLengthDays, Math.max(1, Math.floor(elapsed / 86400000) + 1));
  const pct = Math.max(0, Math.min(100, (elapsed / (seasonLengthDays * 86400000)) * 100));
  const past = archive.length ? `            <div class="season-banner__past">
                <p class="season-banner__past-title">Past seasons</p>
${archive.map((s) => `                <span class="season-banner__past-item"><b>${esc(s.id)}</b> ${esc(s.name)}${s.champion ? ` · champion ${esc(s.champion)}` : ''}</span>`).join('\n')}
            </div>\n` : '';
  return `        <section class="season-banner" aria-label="Current season">
            <div class="season-banner__head">
                <p class="eyebrow"><span class="live-dot"></span> ${esc(seasonDisplayName(current.id, current.name))}</p>
                <p class="season-banner__day">Day ${day} of ${seasonLengthDays}</p>
            </div>
            <progress class="season-banner__bar" value="${day}" max="${seasonLengthDays}">${pct.toFixed(1)}%</progress>
${current.theme ? `            <p class="season-banner__theme">${esc(current.theme)}</p>\n` : ''}${current.championRule ? `            <p class="season-banner__rule">Champion — ${esc(current.championRule)}</p>\n` : ''}${loreLink ? '            <p class="season-banner__lore"><a class="text-link" href="lore.html">Read the arena lore →</a></p>\n' : ''}${past}        </section>`;
}

/**
 * Load authored arena lore, or return null when the file is absent,
 * unparseable, or carries no usable content (no premise, fighters or lexicon).
 */
export function loadLore({ lorePath, io = defaultIo, log = () => {} }) {
  if (!lorePath || !io.exists(lorePath)) return null;
  let data;
  try {
    data = io.readJson(lorePath);
  } catch (error) {
    log(`lore: ignoring unreadable ${lorePath} (${String(error?.message || error).slice(0, 200)})`);
    return null;
  }
  const premise = (Array.isArray(data?.world?.premise) ? data.world.premise : [])
    .filter((p) => typeof p === 'string' && p.trim()).map(String);
  const fighters = {};
  for (const [slug, f] of Object.entries(data?.fighters || {})) {
    if (!f || typeof f !== 'object' || !f.title || !f.lore) continue;
    fighters[slug] = {
      mascot: f.mascot ? String(f.mascot) : null,
      title: String(f.title),
      lore: String(f.lore),
    };
  }
  const lexicon = {};
  for (const [term, def] of Object.entries(data?.lexicon || {})) {
    if (typeof def === 'string' && def.trim()) lexicon[term] = String(def);
  }
  if (!premise.length && !Object.keys(fighters).length && !Object.keys(lexicon).length) return null;
  return {
    worldName: data?.world?.name ? String(data.world.name) : null,
    premise,
    fighters,
    lexicon,
  };
}

/**
 * Match a weekly-roster model to its fighter lore entry via slug prefix
 * (entry keys are provider-style and undated, e.g. "deepseek/deepseek-v4-flash").
 * Longest matching key wins so a dated variant is not shadowed by its plainer
 * sibling — same rule as the toplist matcher.
 */
export function matchLoreEntry(fighters, model) {
  const ids = [model.canonical_slug, model.provider_model, model.model_id]
    .filter(Boolean).map(String);
  let best = null;
  for (const [slug, entry] of Object.entries(fighters || {})) {
    if (ids.some((id) => id === slug || id.startsWith(`${slug}-`) || id.startsWith(`${slug}/`))) {
      if (!best || slug.length > best.slug.length) best = { slug, ...entry };
    }
  }
  return best;
}

/**
 * The lore page: world premise as magazine intro, one entry per fighter
 * (mascot emoji, lore title, lore paragraph, profile link when the fighter
 * has a model page), then the lexicon glossary. Same visual language as the
 * chronicle — prose, not dashboard panels.
 */
export function renderLorePage({ lore, roster, slugById }) {
  const worldName = lore.worldName || 'The Arena';
  const { head, foot } = chrome({
    title: `${worldName} — Lore // Model Arena`,
    description: `The world of ${worldName}: its premise, its fighters and the lexicon they fight by.`,
    active: 'lore',
    loreLink: true,
  });
  const premise = lore.premise.map((p, i) => {
    const cls = i === 0 ? 'chronicle__prose chronicle__prose--lead' : 'chronicle__prose';
    return `                <p class="${cls}">${esc(p)}</p>`;
  }).join('\n');
  const fighters = Object.entries(lore.fighters).map(([slug, entry]) => {
    const mascot = mascotFor(slug);
    const model = roster.find((m) => matchLoreEntry({ [slug]: entry }, m));
    const href = model ? `${slugById.get(model.model_id)}.html` : null;
    return `            <article class="lore__fighter">
                <span class="lore__emoji" style="border-color:${esc(mascot.color)}">${esc(mascot.emoji)}</span>
                <div class="lore__fighter-body">
                    <h3 class="lore__fighter-title">${esc(entry.title)}</h3>
                    <p class="lore__fighter-slug">${esc(entry.mascot || mascot.title)} · ${esc(slug)}</p>
                    <p class="chronicle__prose">${esc(entry.lore)}</p>
${href ? `                    <p class="lore__fighter-link"><a class="text-link" href="${esc(href)}">View fight record →</a></p>\n` : ''}                </div>
            </article>`;
  }).join('\n');
  const lexiconRows = Object.entries(lore.lexicon).map(([term, def]) => `                <div class="lore__term">
                    <dt>${esc(term)}</dt>
                    <dd>${esc(def)}</dd>
                </div>`).join('\n');
  return `${head}
        <section class="chronicle lore" aria-label="Arena lore">
            <p class="eyebrow chronicle__eyebrow"><span class="live-dot"></span> Arena Lore</p>
            <h2 class="chronicle__title">${esc(worldName)}, <em>as it is told.</em></h2>
${premise}
${fighters ? `            <h3 class="lore__section-title">Fighters</h3>
${fighters}\n` : ''}${lexiconRows ? `            <h3 class="lore__section-title">Lexicon</h3>
            <dl class="lore__lexicon">
${lexiconRows}
            </dl>\n` : ''}        </section>
${foot}`;
}


// Emitted as static_client/models/models.css — mirrors the landing page's
// visual language (dark ink, acid lime, mono labels) without external deps.
export const MODELS_CSS = `:root {
    --ink: #030706;
    --ink-soft: #08100d;
    --panel: #0a1410;
    --line: rgba(205, 255, 44, 0.18);
    --line-soft: rgba(202, 222, 210, 0.14);
    --acid: #caff00;
    --acid-soft: #e7ff8d;
    --cyan: #00e0ff;
    --white: #f1f5ed;
    --muted: #91a098;
    --dim: #5e6d65;
    --mono: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
    --sans: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}
* { box-sizing: border-box; }
html { color-scheme: dark; scroll-behavior: smooth; }
body {
    margin: 0;
    overflow-x: hidden;
    background:
        radial-gradient(circle at 72% 4%, rgba(202, 255, 0, 0.08), transparent 24rem),
        radial-gradient(circle at 8% 48%, rgba(0, 224, 255, 0.055), transparent 26rem),
        var(--ink);
    color: var(--white);
    font-family: var(--sans);
    -webkit-font-smoothing: antialiased;
}
a { color: inherit; }
::selection { background: var(--acid); color: var(--ink); }
.site-noise {
    position: fixed;
    inset: 0;
    z-index: 30;
    pointer-events: none;
    opacity: 0.23;
    background-image:
        linear-gradient(rgba(255, 255, 255, 0.012) 50%, transparent 50%),
        radial-gradient(circle, rgba(255, 255, 255, 0.18) 0.5px, transparent 0.8px);
    background-size: 100% 4px, 7px 7px;
    mix-blend-mode: soft-light;
}
.shell { width: min(1180px, calc(100% - 48px)); margin-inline: auto; }
.site-header {
    position: sticky;
    top: 0;
    z-index: 20;
    border-bottom: 1px solid var(--line-soft);
    background: rgba(3, 7, 6, 0.92);
    backdrop-filter: blur(18px);
}
.site-header__inner {
    height: 72px;
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 34px;
}
.brand { display: inline-flex; align-items: center; gap: 11px; color: var(--white); text-decoration: none; }
.brand__mark {
    display: grid;
    width: 34px;
    height: 34px;
    place-items: center;
    border: 1px solid var(--acid);
    color: var(--acid);
    font: 800 12px/1 var(--mono);
    box-shadow: inset 0 0 14px rgba(202, 255, 0, 0.07);
}
.brand__copy { display: flex; flex-direction: column; gap: 2px; }
.brand__copy strong { font: 800 11px/1 var(--mono); letter-spacing: 0.12em; }
.brand__copy span { color: var(--dim); font: 600 9px/1 var(--mono); letter-spacing: 0.07em; }
.nav { justify-self: center; }
.nav__links { display: flex; align-items: center; gap: 28px; }
.nav__links a, .footer > a:last-child {
    color: var(--muted);
    font: 700 9px/1 var(--mono);
    letter-spacing: 0.09em;
    text-decoration: none;
    text-transform: uppercase;
    transition: color 140ms ease;
}
.nav__links a:hover, .nav__links a[aria-current="page"], .footer > a:last-child:hover { color: var(--acid); }
.button {
    min-height: 52px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 26px;
    border: 1px solid transparent;
    border-radius: 2px;
    padding: 0 18px;
    font: 900 10px/1 var(--mono);
    letter-spacing: 0.09em;
    text-decoration: none;
    text-transform: uppercase;
    transition: transform 140ms ease, filter 140ms ease, border-color 140ms ease;
}
.button:hover { transform: translateY(-2px); filter: brightness(1.06); }
.button--primary { border-color: var(--acid); background: var(--acid); color: #071000; }
.button--compact { min-height: 38px; gap: 10px; padding-inline: 14px; font-size: 9px; }
main.shell { padding-bottom: 88px; }
.eyebrow {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 0 22px;
    color: var(--acid);
    font: 800 10px/1.2 var(--mono);
    letter-spacing: 0.16em;
    text-transform: uppercase;
}
.live-dot {
    width: 7px;
    height: 7px;
    flex: 0 0 auto;
    border-radius: 50%;
    background: var(--acid);
    box-shadow: 0 0 14px rgba(202, 255, 0, 0.86);
}
h1 { margin: 0; font-weight: 900; letter-spacing: -0.05em; font-size: clamp(34px, 4.6vw, 60px); line-height: 1; }
h1 em { color: var(--acid); font-style: normal; }
h2 { margin: 0 0 18px; font: 900 20px/1.1 var(--sans); letter-spacing: -0.03em; }
.text-link { color: var(--acid-soft); }

/* models index */
.models-hero { padding: 76px 0 44px; }
.models-hero__lede { max-width: 640px; margin: 24px 0 0; color: var(--muted); font-size: 15px; line-height: 1.72; }
.model-list { border-top: 1px solid var(--line); }
.model-row {
    min-height: 88px;
    display: grid;
    grid-template-columns: 44px 46px minmax(0, 1fr) auto 24px;
    align-items: center;
    gap: 18px;
    border-bottom: 1px solid var(--line-soft);
    text-decoration: none;
    transition: background 140ms ease;
}
.model-row:hover { background: rgba(202, 255, 0, 0.03); }
.model-row:first-child { background: linear-gradient(90deg, rgba(202, 255, 0, 0.07), transparent 74%); }
.model-row__rank { color: var(--dim); font: 700 11px/1 var(--mono); }
.model-row:first-child .model-row__rank { color: var(--acid); }
.model-row__emoji {
    width: 40px; height: 40px;
    display: grid; place-items: center;
    border: 1px solid var(--line);
    font-size: 20px;
    background: rgba(5, 13, 10, 0.6);
}
.model-row__name { display: flex; flex-direction: column; gap: 5px; min-width: 0; }
.model-row__name b { font-size: 17px; letter-spacing: -0.02em; }
.model-row__name small, .model-row__points small {
    color: var(--dim);
    font: 700 8px/1 var(--mono);
    letter-spacing: 0.07em;
    text-transform: uppercase;
}
.model-row__points { display: flex; flex-direction: column; gap: 5px; text-align: right; }
.model-row__points b { color: var(--acid-soft); font: 800 13px/1 var(--mono); }
.model-row__go { color: var(--dim); font-size: 14px; transition: color 140ms ease, transform 140ms ease; }
.model-row:hover .model-row__go { color: var(--acid); transform: translate(2px, -2px); }

/* model profile */
.profile-hero { padding: 64px 0 0; }
.profile-hero__id { display: flex; align-items: center; gap: 26px; }
.profile-hero__emoji {
    width: 96px; height: 96px;
    flex: 0 0 auto;
    display: grid; place-items: center;
    border: 1px solid var(--acid);
    font-size: 46px;
    background: rgba(5, 13, 10, 0.6);
    box-shadow: inset 0 0 24px rgba(202, 255, 0, 0.06);
}
.profile-hero__sub { margin: 14px 0 0; color: var(--dim); font: 700 9px/1 var(--mono); letter-spacing: 0.09em; text-transform: uppercase; }
.profile-hero__sub a { color: var(--acid-soft); text-decoration: none; }
.stat-strip {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    margin: 38px 0 0;
    border-top: 1px solid var(--line-soft);
}
.stat-strip div { min-width: 0; padding: 17px 14px 0 0; }
.stat-strip div + div { border-left: 1px solid var(--line-soft); padding-left: 18px; }
.stat-strip dt { margin-bottom: 7px; color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.11em; text-transform: uppercase; }
.stat-strip dd { margin: 0; color: var(--white); font: 800 13px/1.3 var(--mono); letter-spacing: -0.03em; }
.panel-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 18px;
    margin-top: 18px;
}
.panel {
    margin-top: 18px;
    border: 1px solid var(--line-soft);
    padding: 28px;
    background: rgba(7, 16, 12, 0.66);
}
.panel-grid .panel { margin-top: 0; }
.radar { display: block; width: min(340px, 100%); margin: 6px auto 0; }
.radar__label { fill: var(--dim); font: 700 8px var(--mono); letter-spacing: 0.08em; text-transform: uppercase; }
.radar__value { fill: var(--acid-soft); font: 800 9px var(--mono); }
.bars { display: flex; flex-direction: column; gap: 12px; padding-top: 6px; }
.bar-row {
    display: grid;
    grid-template-columns: 64px minmax(0, 1fr) 52px;
    align-items: center;
    gap: 12px;
}
.bar-row__name { color: var(--muted); font: 700 9px/1 var(--mono); letter-spacing: 0.08em; text-transform: uppercase; }
.bar-row__track { height: 14px; border: 1px solid var(--line-soft); background: rgba(3, 7, 6, 0.6); }
.bar-row__fill { display: block; height: 100%; box-shadow: 0 0 12px rgba(202, 255, 0, 0.18); }
.bar-row__pct { color: var(--acid-soft); font: 800 10px/1 var(--mono); text-align: right; }
.metric-note { margin: 16px 0 0; color: var(--dim); font: 600 9px/1.6 var(--mono); letter-spacing: 0.05em; text-transform: uppercase; }
.metric-note b { color: var(--acid-soft); }
.empty { color: var(--dim); font: 600 10px/1.6 var(--mono); letter-spacing: 0.05em; text-transform: uppercase; }
table.rivalry { width: 100%; border-collapse: collapse; }
table.rivalry th {
    padding: 0 10px 10px 0;
    color: var(--dim);
    font: 700 8px/1 var(--mono);
    letter-spacing: 0.1em;
    text-align: left;
    text-transform: uppercase;
    border-bottom: 1px solid var(--line);
}
table.rivalry td { padding: 10px 10px 10px 0; border-bottom: 1px solid var(--line-soft); vertical-align: middle; }
.rival { display: inline-flex; align-items: center; gap: 12px; text-decoration: none; }
.rival__emoji { font-size: 20px; }
.rival b { display: block; font-size: 14px; }
.rival small { color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.07em; text-transform: uppercase; }
.rival:hover b { color: var(--acid); }
.num { font: 800 12px/1 var(--mono); color: var(--muted); }
.num.win, .num .win { color: var(--acid); }
.num.loss, .num .loss { color: #fb7185; }
.mode-grid__title { margin: 24px 0 12px; color: var(--muted); font: 800 10px/1 var(--mono); letter-spacing: 0.12em; text-transform: uppercase; }
table.mode-grid { width: 100%; border-collapse: collapse; }
table.mode-grid th {
    padding: 0 10px 8px 0;
    color: var(--dim);
    font: 700 8px/1 var(--mono);
    letter-spacing: 0.1em;
    text-align: left;
    text-transform: uppercase;
    border-bottom: 1px solid var(--line);
}
table.mode-grid td { padding: 8px 10px 8px 0; border-bottom: 1px solid var(--line-soft); }
.mode-grid__mode { color: var(--acid-soft); font: 800 10px/1 var(--mono); letter-spacing: 0.08em; text-transform: uppercase; }
.mini-bar__track { display: block; width: 120px; height: 8px; border: 1px solid var(--line-soft); }
.mini-bar__fill { display: block; height: 100%; background: var(--acid); }
.partners { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 12px; }
.partner {
    display: flex;
    align-items: center;
    gap: 14px;
    border: 1px solid var(--line-soft);
    padding: 16px;
    text-decoration: none;
    background: rgba(3, 7, 6, 0.5);
    transition: border-color 140ms ease;
}
.partner:hover { border-color: var(--acid); }
.partner__emoji { font-size: 26px; }
.partner__name { flex: 1; min-width: 0; }
.partner__name b { display: block; font-size: 15px; }
.partner__name small, .partner__score small { color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.07em; text-transform: uppercase; }
.partner__score { text-align: right; }
.partner__score b { display: block; color: var(--acid-soft); font: 800 13px/1.4 var(--mono); }
.clips { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 14px; }
.clip {
    border: 1px solid var(--line-soft);
    text-decoration: none;
    background: rgba(3, 7, 6, 0.5);
    transition: border-color 140ms ease, transform 140ms ease;
}
.clip:hover { border-color: var(--acid); transform: translateY(-2px); }
.clip img { display: block; width: 100%; aspect-ratio: 16 / 9; object-fit: cover; }
.clip__meta { display: flex; flex-direction: column; gap: 5px; padding: 12px; }
.clip__meta b { font-size: 13px; }
.clip__meta small { color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.07em; text-transform: uppercase; }
.provenance { margin-top: 40px; border-top: 1px solid var(--line-soft); padding-top: 28px; }
.provenance dl { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 18px; margin: 0; }
.provenance dt { margin-bottom: 7px; color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.11em; text-transform: uppercase; }
.provenance dd { margin: 0; color: var(--muted); font: 700 10px/1.4 var(--mono); word-break: break-all; }
.footer {
    min-height: 128px;
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 24px;
    border-top: 1px solid var(--line-soft);
}
.footer p { margin: 0; color: var(--dim); font: 600 8px/1.4 var(--mono); letter-spacing: 0.05em; text-align: center; text-transform: uppercase; }
.footer > a:last-child { justify-self: end; }
@media (max-width: 860px) {
    .panel-grid { grid-template-columns: 1fr; }
    .stat-strip { grid-template-columns: repeat(2, 1fr); }
    .stat-strip div:nth-child(odd) { border-left: none; padding-left: 0; }
    .mini-bar { display: none; }
}
@media (max-width: 560px) {
    .shell { width: calc(100% - 28px); }
    .site-header__inner { gap: 14px; }
    .brand__copy span { display: none; }
    .nav { display: none; }
    .profile-hero__id { align-items: flex-start; gap: 16px; }
    .profile-hero__emoji { width: 64px; height: 64px; font-size: 30px; }
    .model-row { grid-template-columns: 30px 38px minmax(0, 1fr) auto; }
    .model-row__go { display: none; }
    .model-row__emoji { width: 34px; height: 34px; font-size: 17px; }
    .panel { padding: 20px 16px; }
    .footer { grid-template-columns: 1fr auto; padding-block: 30px; }
    .footer p { display: none; }
}

/* analyst toplist */
.toplist__grid { display: flex; flex-direction: column; gap: 14px; }
.toplist__card {
    display: grid;
    grid-template-columns: 44px 46px minmax(0, 1fr);
    align-items: start;
    gap: 18px;
    border: 1px solid var(--line-soft);
    padding: 18px;
    background: rgba(3, 7, 6, 0.5);
    text-decoration: none;
    transition: border-color 140ms ease;
}
a.toplist__card:hover { border-color: var(--acid); }
.toplist__rank { padding-top: 12px; color: var(--dim); font: 800 13px/1 var(--mono); }
.toplist__card:first-child .toplist__rank { color: var(--acid); }
.toplist__emoji {
    width: 40px; height: 40px;
    display: grid; place-items: center;
    border: 1px solid var(--line);
    font-size: 20px;
    background: rgba(5, 13, 10, 0.6);
}
.toplist__model { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; margin: 0 0 6px; }
.toplist__model b { font-size: 15px; letter-spacing: -0.01em; }
.toplist__model small { color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.07em; text-transform: uppercase; }
.toplist__badge {
    border: 1px solid var(--line-soft);
    padding: 2px 5px;
    color: var(--muted);
    font: 800 8px/1 var(--mono);
    letter-spacing: 0.08em;
    text-transform: uppercase;
}
.toplist__headline { margin: 0 0 8px; color: var(--acid); font: 800 15px/1.3 var(--sans); letter-spacing: -0.02em; }
.toplist__commentary { margin: 0; color: var(--muted); font-size: 13px; line-height: 1.65; }
.analyst-note__headline { color: var(--acid); }
.analyst-note__commentary { margin: 0; max-width: 720px; color: var(--muted); font-size: 14px; line-height: 1.7; }

/* league chronicle — magazine feature, not a stat panel */
.chronicle {
    width: min(65ch, 100%);
    margin: 0 auto;
    padding: 96px 0 72px;
    text-align: left;
}
.chronicle__eyebrow { justify-content: center; }
.chronicle__title {
    margin: 0 0 64px;
    text-align: center;
    font-weight: 900;
    letter-spacing: -0.04em;
    font-size: clamp(28px, 3.6vw, 44px);
    line-height: 1.05;
}
.chronicle__title em { color: var(--acid); font-style: normal; }
.chronicle__chapter { padding: 56px 0 8px; }
.chronicle__chapter:first-of-type { padding-top: 0; }
.chronicle__chapter + .chronicle__chapter {
    margin-top: 48px;
    border-top: 1px solid var(--line-soft);
}
.chronicle__day {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 0 14px;
    color: var(--dim);
    font: 800 9px/1.4 var(--mono);
    letter-spacing: 0.22em;
    text-transform: uppercase;
}
.chronicle__latest {
    padding: 3px 8px;
    border: 1px solid var(--line);
    color: var(--acid);
    font: 800 8px/1 var(--mono);
    letter-spacing: 0.18em;
    text-transform: uppercase;
    background: rgba(202, 255, 0, 0.06);
}
.chronicle__chapter-title {
    margin: 0 0 26px;
    font: 900 clamp(20px, 2.4vw, 27px)/1.2 var(--sans);
    letter-spacing: -0.025em;
}
.chronicle__prose {
    margin: 0 0 22px;
    color: var(--muted);
    font-size: 16.5px;
    line-height: 1.85;
}
.chronicle__prose:last-child { margin-bottom: 0; }
.chronicle__prose--lead::first-letter {
    float: left;
    padding: 6px 12px 0 0;
    color: var(--acid);
    font: 900 52px/0.8 var(--sans);
    letter-spacing: -0.02em;
}
@media (max-width: 640px) {
    .chronicle { padding: 64px 0 48px; }
    .chronicle__title { margin-bottom: 44px; }
    .chronicle__prose { font-size: 15.5px; }
    .chronicle__prose--lead::first-letter { font-size: 44px; padding-right: 10px; }
}

/* continuous league overlay */
.league-strip {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    margin: 0 0 34px;
    border: 1px solid var(--line);
    background: rgba(7, 16, 12, 0.66);
}
.league-strip div { padding: 16px 18px; }
.league-strip div + div { border-left: 1px solid var(--line-soft); }
.league-strip dt { margin-bottom: 7px; color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.11em; text-transform: uppercase; display: flex; align-items: center; gap: 7px; }
.league-strip dd { margin: 0; color: var(--acid-soft); font: 800 13px/1.3 var(--mono); letter-spacing: -0.03em; }
.league-strip__sub { margin-top: 4px !important; color: var(--dim) !important; font: 700 8px/1.4 var(--mono) !important; letter-spacing: 0.06em !important; text-transform: uppercase; }
.track-badge {
    display: inline-block;
    border: 1px solid var(--line);
    padding: 2px 5px;
    color: var(--acid-soft);
    font: 800 8px/1 var(--mono);
    letter-spacing: 0.08em;
    background: rgba(202, 255, 0, 0.05);
}
.track-badge--L0 { color: var(--muted); border-color: var(--line-soft); background: transparent; }
.track-badge--L3 { color: var(--acid); border-color: var(--line); box-shadow: inset 0 0 10px rgba(202, 255, 0, 0.08); }
.division-badge {
    display: inline-block;
    border: 1px solid var(--line-soft);
    padding: 2px 6px;
    color: var(--muted);
    font: 800 8px/1 var(--mono);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    background: rgba(3, 7, 6, 0.5);
}
.division-badge--premier { color: var(--acid); border-color: var(--line); background: rgba(202, 255, 0, 0.06); }
.division-badge--challenger { color: var(--cyan); border-color: rgba(0, 224, 255, 0.25); }
.standings__division + .standings__division { margin-top: 18px; }
.standings__division h4 { margin: 0 0 8px; }
.standings__rank { white-space: nowrap; color: var(--dim); }
table.matrix__table tr.matrix__row--boundary td { border-top: 1px solid var(--line); }
.standings__grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(440px, 1fr)); gap: 18px; }
.standings__track h3 { display: flex; align-items: center; gap: 9px; margin: 0 0 12px; font: 800 11px/1 var(--mono); letter-spacing: 0.08em; text-transform: uppercase; color: var(--white); }
.standings__track h3 small { margin-left: auto; color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.07em; }
table.standings__table { width: 100%; border-collapse: collapse; }
table.standings__table th {
    padding: 0 8px 8px 0;
    color: var(--dim);
    font: 700 8px/1 var(--mono);
    letter-spacing: 0.1em;
    text-align: left;
    text-transform: uppercase;
    border-bottom: 1px solid var(--line);
}
table.standings__table td { padding: 8px 8px 8px 0; border-bottom: 1px solid var(--line-soft); font-size: 12px; }
table.standings__table tr:last-child td { border-bottom: none; }
.standings__emoji { font-size: 14px; }
.standings__rating { color: var(--acid-soft); }
.matrix__table { width: 100%; border-collapse: collapse; }
.matrix__table th {
    padding: 0 10px 10px 0;
    color: var(--dim);
    font: 700 8px/1 var(--mono);
    letter-spacing: 0.1em;
    text-align: left;
    text-transform: uppercase;
    border-bottom: 1px solid var(--line);
}
.matrix__table td { padding: 10px 10px 10px 0; border-bottom: 1px solid var(--line-soft); }
.matrix__model { display: flex; flex-direction: column; gap: 4px; min-width: 0; font-size: 13px; }
.matrix__model > span { display: flex; align-items: center; gap: 7px; }
.matrix__model small { color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.07em; text-transform: uppercase; word-break: break-all; }
.matrix__cell { font-size: 13px; color: var(--acid-soft); }
.matrix__cell--empty { color: var(--dim); }
.matrix__cell--retired { color: var(--muted); }
.matrix__delta { font-size: 13px; }
.matrix__delta--pos { color: var(--acid); }
.matrix__delta--neg { color: #fb7185; }
.lineage__track { display: flex; align-items: center; gap: 9px; margin: 22px 0 4px; font: 800 10px/1 var(--mono); letter-spacing: 0.1em; text-transform: uppercase; color: var(--muted); }
.hof__track { display: flex; align-items: center; gap: 9px; margin: 22px 0 14px; font: 800 10px/1 var(--mono); letter-spacing: 0.1em; text-transform: uppercase; color: var(--muted); }
.announce__feed { list-style: none; margin: 0; padding: 0; }
.announce__item {
    display: grid;
    grid-template-columns: 28px 34px minmax(130px, 190px) minmax(0, 1fr) auto;
    align-items: baseline;
    gap: 14px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line-soft);
}
.announce__item:last-child { border-bottom: none; }
.announce__icon { font-size: 15px; }
.announce__name { font-size: 13px; font-weight: 700; letter-spacing: -0.01em; }
.announce__text { color: var(--muted); font: 700 9px/1.5 var(--mono); letter-spacing: 0.06em; text-transform: uppercase; }
.announce__at { color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.07em; text-transform: uppercase; white-space: nowrap; }
.hof__hint { color: var(--dim); font: 700 9px/1 var(--mono); letter-spacing: 0.08em; text-transform: uppercase; }
.hof__grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 14px; }
.hof__card { border: 1px solid var(--line-soft); padding: 18px; background: rgba(3, 7, 6, 0.5); }
.hof__emoji {
    width: 44px; height: 44px;
    display: grid; place-items: center;
    border: 1px solid var(--line);
    font-size: 22px;
    background: rgba(5, 13, 10, 0.6);
}
.hof__id { margin: 12px 0 0; display: flex; flex-direction: column; gap: 5px; }
.hof__id b { font-size: 16px; }
.hof__id small { color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.07em; text-transform: uppercase; word-break: break-all; }
.hof__stats { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; margin: 14px 0 0; }
.hof__stats dt { margin-bottom: 6px; color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.1em; text-transform: uppercase; }
.hof__stats dd { margin: 0; color: var(--white); font: 800 11px/1.3 var(--mono); }
.hof__stats .win { color: var(--acid); }
.hof__stats .loss { color: #fb7185; }
.hof__reason { margin: 14px 0 0; color: var(--dim); font: 600 8px/1.6 var(--mono); letter-spacing: 0.05em; text-transform: uppercase; }
.lineage__timeline { list-style: none; margin: 0; padding: 0; }
.lineage__node {
    display: grid;
    grid-template-columns: 28px 44px minmax(110px, 170px) auto minmax(0, 1fr) auto;
    align-items: baseline;
    gap: 14px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line-soft);
}
.lineage__node:last-child { border-bottom: none; }
.lineage__icon { font-size: 15px; }
.lineage__version { color: var(--acid-soft); font: 800 11px/1 var(--mono); }
.lineage__outcome { font-size: 13px; font-weight: 700; }
.lineage__node--compile_failed .lineage__outcome,
.lineage__node--codegen_failed .lineage__outcome,
.lineage__node--interrupted .lineage__outcome { color: #fb7185; }
.lineage__meta { color: var(--dim); font: 700 8px/1.5 var(--mono); letter-spacing: 0.06em; text-transform: uppercase; white-space: nowrap; }
.lineage__delta { color: var(--muted); font: 700 8px/1.5 var(--mono); letter-spacing: 0.06em; text-transform: uppercase; }
.lineage__at { color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.07em; text-transform: uppercase; white-space: nowrap; }
.chem__table { width: 100%; border-collapse: collapse; }
.chem__table th { color: var(--dim); font: 700 9px/1 var(--mono); letter-spacing: 0.08em; text-transform: uppercase; text-align: left; padding: 0 10px 8px 0; }
.chem__table td { padding: 8px 10px 8px 0; border-top: 1px solid var(--line-soft); font-size: 13px; }
.chem__table td.num { text-align: right; font-variant-numeric: tabular-nums; }
.chem__pair { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.chem__plus { color: var(--dim); font-weight: 700; }
.chem__delta--pos { color: var(--acid-soft); font-weight: 700; }
.chem__delta--neg { color: #fb7185; font-weight: 700; }
.chem__provisional { color: #facc15; font: 700 8px/1 var(--mono); letter-spacing: 0.07em; text-transform: uppercase; border: 1px solid #facc1555; border-radius: 4px; padding: 3px 5px; white-space: nowrap; }

/* measured rivalries */
.measured__grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(340px, 1fr)); gap: 18px; }
.measured__table-wrap h3 { display: flex; align-items: baseline; gap: 9px; margin: 0 0 12px; font: 800 11px/1 var(--mono); letter-spacing: 0.08em; text-transform: uppercase; color: var(--white); }
.measured__table-wrap h3 small { color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.07em; }
table.measured__table { width: 100%; border-collapse: collapse; }
table.measured__table th {
    padding: 0 10px 10px 0;
    color: var(--dim);
    font: 700 8px/1 var(--mono);
    letter-spacing: 0.1em;
    text-align: left;
    text-transform: uppercase;
    border-bottom: 1px solid var(--line);
}
table.measured__table td { padding: 10px 10px 10px 0; border-bottom: 1px solid var(--line-soft); vertical-align: middle; }
table.measured__table tr:last-child td { border-bottom: none; }
.measured__pair { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
.measured__names { display: flex; align-items: center; gap: 9px; flex-wrap: wrap; font-size: 13px; }
.measured__model { text-decoration: none; }
.measured__model small { color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.07em; text-transform: uppercase; }
.measured__model:hover b { color: var(--acid); }
.measured__score { font: 800 11px/1 var(--mono); }
.measured__pair small { color: var(--dim); font: 700 8px/1 var(--mono); letter-spacing: 0.07em; text-transform: uppercase; }
.measured__note { margin: 0 0 14px; color: var(--muted); font: 700 10px/1.5 var(--mono); letter-spacing: 0.06em; text-transform: uppercase; }
.measured__note .win { color: var(--acid); }
.measured__note .loss { color: #fb7185; }
@media (max-width: 860px) {
    .league-strip { grid-template-columns: repeat(2, 1fr); }
    .league-strip div:nth-child(odd) { border-left: none; }
    .announce__item { grid-template-columns: 28px 34px minmax(0, 1fr) auto; }
    .announce__text { grid-column: 3 / 5; }
    .matrix__table { display: block; overflow-x: auto; }
    .chem__table { display: block; overflow-x: auto; }
    .lineage__node { grid-template-columns: 28px 44px minmax(0, 1fr) auto; }
    .lineage__meta, .lineage__delta { display: none; }
}

/* season banner */
.season-banner {
    margin: 0 0 34px;
    border: 1px solid var(--line);
    padding: 24px 28px;
    background: rgba(7, 16, 12, 0.66);
}
.season-banner__head { display: flex; align-items: baseline; justify-content: space-between; gap: 18px; flex-wrap: wrap; }
.season-banner__head .eyebrow { margin-bottom: 14px; }
.season-banner__day { margin: 0; color: var(--acid-soft); font: 800 12px/1 var(--mono); letter-spacing: 0.08em; text-transform: uppercase; }
/* native <progress>: the models index is served with a strict style-src CSP
   that strips inline style attributes, so the fill must not rely on them */
.season-banner__bar {
    display: block;
    width: 100%;
    height: 4px;
    margin: 4px 0 18px;
    appearance: none;
    border: none;
    background: transparent;
}
.season-banner__bar::-webkit-progress-bar { border: 1px solid var(--line-soft); background: rgba(3, 7, 6, 0.6); }
.season-banner__bar::-webkit-progress-value { background: var(--acid); box-shadow: 0 0 12px rgba(202, 255, 0, 0.35); }
.season-banner__bar::-moz-progress-bar { background: var(--acid); }
.season-banner__theme { margin: 0 0 10px; max-width: 72ch; color: var(--muted); font-size: 14.5px; line-height: 1.7; }
.season-banner__rule { margin: 0; color: var(--dim); font: 600 9px/1.6 var(--mono); letter-spacing: 0.06em; text-transform: uppercase; }
.season-banner__lore { margin: 12px 0 0; font: 700 10px/1.5 var(--mono); letter-spacing: 0.08em; text-transform: uppercase; }
.season-banner__lore a { text-decoration: none; }
.season-banner__past { margin-top: 18px; border-top: 1px solid var(--line-soft); padding-top: 14px; display: flex; align-items: baseline; gap: 16px; flex-wrap: wrap; }
.season-banner__past-title { margin: 0; color: var(--dim); font: 800 8px/1 var(--mono); letter-spacing: 0.14em; text-transform: uppercase; }
.season-banner__past-item { color: var(--muted); font: 700 10px/1.6 var(--mono); letter-spacing: 0.05em; }
.season-banner__past-item b { color: var(--acid-soft); }
@media (max-width: 560px) {
    .season-banner { padding: 18px 16px; }
}

/* arena lore page — magazine prose like the chronicle */
.lore__section-title {
    margin: 64px 0 28px;
    padding-top: 40px;
    border-top: 1px solid var(--line-soft);
    font: 900 clamp(19px, 2.2vw, 25px)/1.2 var(--sans);
    letter-spacing: -0.025em;
}
.lore__fighter { display: flex; align-items: flex-start; gap: 22px; padding: 28px 0; }
.lore__fighter + .lore__fighter { border-top: 1px solid var(--line-soft); }
.lore__emoji {
    width: 56px; height: 56px;
    flex: 0 0 auto;
    display: grid; place-items: center;
    border: 1px solid var(--line);
    font-size: 28px;
    background: rgba(5, 13, 10, 0.6);
}
.lore__fighter-body { min-width: 0; }
.lore__fighter-title { margin: 0 0 6px; font: 900 19px/1.25 var(--sans); letter-spacing: -0.02em; }
.lore__fighter-slug { margin: 0 0 14px; color: var(--dim); font: 700 8px/1.4 var(--mono); letter-spacing: 0.14em; text-transform: uppercase; word-break: break-all; }
.lore__fighter-link { margin: -6px 0 0; font: 700 10px/1.5 var(--mono); letter-spacing: 0.08em; text-transform: uppercase; }
.lore__fighter-link a { text-decoration: none; }
.lore__lexicon { margin: 0; }
.lore__term { padding: 18px 0; border-bottom: 1px solid var(--line-soft); }
.lore__term:last-child { border-bottom: none; }
.lore__term dt { margin-bottom: 8px; color: var(--acid); font: 800 13px/1.3 var(--mono); letter-spacing: 0.04em; }
.lore__term dd { margin: 0; color: var(--muted); font-size: 14.5px; line-height: 1.7; }
.profile-hero__lore { margin: 22px 0 0; font: 700 11px/1.4 var(--mono); letter-spacing: 0.1em; text-transform: uppercase; }
.profile-hero__lore a { color: var(--dim); text-decoration: none; transition: color 140ms ease; }
.profile-hero__lore a:hover { color: var(--acid); }
@media (max-width: 640px) {
    .lore__fighter { gap: 14px; }
    .lore__emoji { width: 44px; height: 44px; font-size: 22px; }
}
`;

// ---------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------

export async function buildPages({
  ratingsPath = path.join(REPO_ROOT, 'data', 'arena_ratings.json'),
  artifactsRoot = path.join(REPO_ROOT, 'artifacts', 'arena'),
  highlightsPath = path.join(REPO_ROOT, 'static_client', 'media', 'highlights', 'index.json'),
  outDir = path.join(REPO_ROOT, 'static_client', 'models'),
  cachePath = path.join(REPO_ROOT, 'artifacts', 'arena', 'page-cache.json'),
  continuousDir = null, // defaults to <artifactsRoot>/continuous
  toplistPath = path.join(SCRIPT_DIR, 'toplist_commentary.json'),
  chroniclePath = path.join(SCRIPT_DIR, 'chronicle.json'),
  seasonsPath = path.join(SCRIPT_DIR, 'seasons.json'),
  lorePath = path.join(SCRIPT_DIR, 'lore.json'),
  mediaBase = '/media/highlights',
  perModelLimit = 200,
  windowSize = 4000,
  nowMs = Date.now(),
  io = defaultIo,
  log = () => {},
} = {}) {
  const ratings = io.readJson(ratingsPath);
  const roster = [...ratings.roster].sort((a, b) => a.rank - b.rank);
  const rosterIds = roster.map((m) => m.model_id);
  const slugById = slugifyRoster(roster);

  // Optional continuous league overlay — null unless the state validates.
  const continuous = loadContinuousLeague({
    continuousDir: continuousDir || path.join(artifactsRoot, 'continuous'),
    io,
    log,
  });

  // Optional mixed-team chemistry overlay — null unless the artifact validates.
  const chemistry = continuous
    ? loadChemistry({
      continuousDir: continuousDir || path.join(artifactsRoot, 'continuous'),
      io,
      log,
    })
    : null;

  // Optional analyst toplist — null unless the authored commentary loads.
  const toplist = loadToplist({ toplistPath, io, log });

  // Optional League Chronicle — null unless the authored narrative loads.
  const chronicle = loadChronicle({ chroniclePath, io, log });

  // Optional season structure — null unless the authored definitions load.
  const seasons = loadSeasons({ seasonsPath, io, log });

  // Optional arena lore — null unless the authored lore loads.
  const lore = loadLore({ lorePath, io, log });

  const seasonDir = path.join(artifactsRoot, 'seasons', ratings.season_id);
  const battlesDir = path.join(seasonDir, 'battles');
  const worldDir = path.join(seasonDir, 'world');

  let cache = null;
  if (io.exists(cachePath)) {
    try { cache = io.readJson(cachePath); } catch { cache = null; }
  }

  // Fresh seasons may not have artifact dirs yet — log and emit pages with
  // empty sections rather than crashing.
  const emptyBattleScan = {
    aggregates: new Map(),
    rivalries: null,
    cacheData: { version: 2, battlesDir, files: {} },
    stats: { listed: 0, windowFiles: 0, filesRead: 0 },
  };
  const emptyWorldScan = { models: {}, partners: new Map(), filesRead: 0 };
  const [battleScan, worldScan] = await Promise.all([
    io.exists(battlesDir)
      ? scanBattles({ battlesDir, rosterIds, perModelLimit, windowSize, cache, io, log })
      : (log(`battles: ${battlesDir} missing — continuing with empty aggregates`), emptyBattleScan),
    io.exists(worldDir)
      ? scanWorld({ worldDir, rosterIds, io, log })
      : (log(`world: ${worldDir} missing — continuing with empty aggregates`), emptyWorldScan),
  ]);

  const highlights = io.exists(highlightsPath) ? io.readJson(highlightsPath) : [];

  const metaById = new Map();
  for (const m of roster) {
    const mascot = mascotFor(m.canonical_slug);
    const shortName = String(m.model_name).includes(':')
      ? String(m.model_name).split(':').pop().trim()
      : String(m.model_name);
    metaById.set(m.model_id, { ...mascot, shortName });
  }

  const cards = roster.map((m) => ({
    model: m,
    slug: slugById.get(m.model_id),
    mascot: metaById.get(m.model_id),
  }));

  const lookupChemistry = chemistry
    ? chemistryLookup({ roster, metaById, slugById, continuous })
    : null;

  io.writeFile(path.join(outDir, 'models.css'), MODELS_CSS);
  io.writeFile(
    path.join(outDir, 'mascots.json'),
    `${JSON.stringify(Object.fromEntries(cards.map((c) => [c.slug, {
      emoji: c.mascot.emoji, title: c.mascot.title, color: c.mascot.color,
    }])), null, 2)}\n`,
  );

  io.writeFile(path.join(outDir, 'index.html'), renderIndexPage({
    cards,
    season_id: ratings.season_id,
    seasonId: ratings.season_id,
    generated_at: ratings.generated_at,
    league: ratings.league,
    continuous,
    chemistry: chemistry ? chemistryPairsSection(chemistry, lookupChemistry) : null,
    toplist: toplist ? toplistSection(toplist, toplistCards(toplist, roster, slugById)) : null,
    chronicle: chronicle ? chronicleSection(chronicle) : null,
    seasonBanner: continuous && seasons
      ? seasonBanner(seasons, nowMs, { loreLink: Boolean(lore) })
      : null,
    measured: battleScan.rivalries
      ? measuredRivalriesSection(battleScan.rivalries, metaById, slugById)
      : null,
    loreLink: Boolean(lore),
    nowMs,
  }));

  if (lore) {
    io.writeFile(path.join(outDir, 'lore.html'), renderLorePage({ lore, roster, slugById }));
  } else if (io.exists(path.join(outDir, 'lore.html'))) {
    // Lore inactive (file absent/unusable): drop any stale page from a
    // previous valid run so the site never serves old lore.
    io.remove(path.join(outDir, 'lore.html'));
  }

  if (continuous) {
    io.writeFile(
      path.join(outDir, 'league.json'),
      `${JSON.stringify(leagueTickerPayload(continuous.state), null, 2)}\n`,
    );
  } else if (io.exists(path.join(outDir, 'league.json'))) {
    // Overlay inactive (state absent/invalid): drop any stale ticker payload
    // from a previous valid run so the landing ticker never serves old data.
    io.remove(path.join(outDir, 'league.json'));
  }

  const collabById = new Map(roster.map((m) => [m.model_id, Number(m.collaboration_rating ?? 50)]));
  for (const m of roster) {
    const id = m.model_id;
    const agg = battleScan.aggregates.get(id) || null;
    const world = worldScan.models[id] || null;
    const mascot = metaById.get(id);
    // Blend co-placement closeness (1 = always finish adjacent) with the
    // partner's collaboration rating.
    const partners = (worldScan.partners.get(id) || [])
      .map((p) => ({ ...p, blend: 0.6 * (1 - p.avgGap / 9) + 0.4 * ((collabById.get(p.other) ?? 50) / 100) }))
      .sort((a, b) => b.blend - a.blend)
      .slice(0, 3);
    const clips = matchFights(highlights, {
      rank: m.rank, title: mascot.title, modelName: m.model_name,
    });
    const lineage = continuous
      ? lineageSection(continuous.state, m, continuous.submissions, continuous.snapshots) || null
      : null;
    const chemistryPartners = chemistry
      ? chemistryPartnersSection(chemistryPartnersForModel(chemistry, m), lookupChemistry) || null
      : null;
    const toplistEntry = toplist ? matchToplistEntry(toplist.entries, m) : null;
    const loreEntry = lore ? matchLoreEntry(lore.fighters, m) : null;
    io.writeFile(path.join(outDir, `${slugById.get(id)}.html`), renderModelPage({
      model: m,
      slug: slugById.get(id),
      mascot,
      agg,
      world,
      partners,
      clips,
      lineage,
      chemistry: chemistryPartners,
      analyst: toplistEntry ? analystNoteSection(toplistEntry, toplist.league_day) : null,
      measuredContest: battleScan.rivalries
        ? measuredContestLine(battleScan.rivalries.mostContestedByModel.get(id), id, metaById, slugById) || null
        : null,
      loreTitle: loreEntry ? loreEntry.title : null,
      loreLink: Boolean(lore),
      slugById,
      metaById,
      mediaBase,
      season_id: ratings.season_id,
      generated_at: ratings.generated_at,
      league: ratings.league,
    }));
  }

  io.writeFile(cachePath, JSON.stringify(battleScan.cacheData));
  log(`wrote ${roster.length} model pages + index to ${outDir}`);
  return { slugs: Object.fromEntries(slugById), battleStats: battleScan.stats };
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const t0 = Date.now();
  // Optional: --continuous-dir <path> points the league overlay at a
  // non-default state directory (e.g. the shadow state before cutover).
  const dirFlag = process.argv.indexOf('--continuous-dir');
  const continuousDir = dirFlag !== -1 ? process.argv[dirFlag + 1] : undefined;
  buildPages({ continuousDir: continuousDir || undefined, log: (msg) => console.error(`[build_model_pages] ${msg}`) })
    .then(({ slugs, battleStats }) => {
      console.error(`[build_model_pages] done in ${((Date.now() - t0) / 1000).toFixed(1)}s — ${JSON.stringify(battleStats)}`);
      for (const [id, slug] of Object.entries(slugs)) console.log(`${slug}\t${id}`);
    })
    .catch((err) => {
      console.error(err);
      process.exit(1);
    });
}
