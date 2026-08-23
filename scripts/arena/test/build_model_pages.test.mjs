// Tests for build_model_pages.mjs — runs entirely against tiny fixtures, never
// the real multi-million-file artifacts tree.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

import {
  aggregateBattles,
  baseSlug,
  buildPages,
  defaultIo,
  matchFights,
  scanBattles,
  slugifyRoster,
} from '../build_model_pages.mjs';

const FIXTURES = path.join(path.dirname(fileURLToPath(import.meta.url)), 'fixtures');
const A = 'test-0001-alpha-alpha-one';
const B = 'test-0002-beta-beta-two';

test('baseSlug strips provider prefix and trailing date', () => {
  assert.equal(baseSlug('deepseek/deepseek-v4-pro-20260423'), 'deepseek-v4-pro');
  assert.equal(baseSlug('tencent/hy3-20260706'), 'hy3');
  assert.equal(baseSlug('xiaomi/mimo-v2.5-20260422'), 'mimo-v2.5');
  assert.equal(baseSlug('nvidia/nemotron-3-ultra-550b-a55b-20260604'), 'nemotron-3-ultra-550b-a55b');
});

test('slugifyRoster resolves collisions by keeping the longer suffix', () => {
  const roster = [
    { model_id: 'm1', rank: 1, canonical_slug: 'deepseek/deepseek-v4-flash-20260731', provider_model: 'deepseek/deepseek-v4-flash-0731' },
    { model_id: 'm2', rank: 2, canonical_slug: 'deepseek/deepseek-v4-pro-20260423', provider_model: 'deepseek/deepseek-v4-pro' },
    { model_id: 'm3', rank: 3, canonical_slug: 'deepseek/deepseek-v4-flash-20260423', provider_model: 'deepseek/deepseek-v4-flash' },
  ];
  const slugs = slugifyRoster(roster);
  assert.equal(slugs.get('m2'), 'deepseek-v4-pro');
  // Both flash entries strip to the same base; the -0731 id keeps its suffix.
  assert.equal(slugs.get('m1'), 'deepseek-v4-flash-0731');
  assert.equal(slugs.get('m3'), 'deepseek-v4-flash');
  assert.equal(new Set(slugs.values()).size, 3);
});

test('slugifyRoster falls back to dated slug when suffixes also collide', () => {
  const roster = [
    { model_id: 'x1', rank: 1, canonical_slug: 'p/dup-20260101', provider_model: 'p/dup' },
    { model_id: 'x2', rank: 2, canonical_slug: 'p/dup-20260202', provider_model: 'p/dup' },
  ];
  const slugs = slugifyRoster(roster);
  assert.equal(slugs.get('x1'), 'dup');
  assert.equal(slugs.get('x2'), 'dup-20260202');
});

test('aggregateBattles builds W/L/D rivalry grid respecting side swap', () => {
  const rec = (f, m, a, b, w, d, ac, bc) => ({ f, m, a, b, w, d, ac, bc });
  const records = [
    rec('b1.json', 4000, A, B, A, false, [0, 10, 2, 4, 1], [1, 2, 9, 0, 3]),
    rec('b2.json', 3000, A, B, A, false, [0, 8, 3, 5, 0], [0, 3, 8, 1, 2]),
    rec('b3.json', 2000, B, A, null, true, [2, 4, 6, 1, 2], [1, 6, 4, 3, 1]), // B is model_a
    rec('b4.json', 1000, A, 'ghost', A, false, [0, 9, 2, 2, 0], [0, 1, 9, 0, 0]),
  ];
  const aggs = aggregateBattles(records, [A, B], 200);

  const aAgg = aggs.get(A);
  assert.equal(aAgg.sampled, 4);
  assert.deepEqual([...aAgg.rivals.keys()], [B]);
  assert.deepEqual(aAgg.rivals.get(B), { w: 2, l: 0, d: 1 });
  // side-aware action counts: ac for b1/b2/b4, bc for b3
  assert.equal(aAgg.actionCounts.attack, 10 + 8 + 6 + 9);
  assert.equal(aAgg.actionCounts.idle, 0 + 0 + 1 + 0);

  const bAgg = aggs.get(B);
  assert.equal(bAgg.sampled, 3);
  assert.deepEqual(bAgg.rivals.get(A), { w: 0, l: 2, d: 1 });
  assert.equal(bAgg.actionCounts.defend, 9 + 8 + 6);

  // aggression = (attack + charge) / total over side-attributed counts
  const aTotal = Object.values(aAgg.actionCounts).reduce((s, n) => s + n, 0);
  const expected = (aAgg.actionCounts.attack + aAgg.actionCounts.charge) / aTotal;
  assert.ok(Math.abs(aAgg.aggression - expected) < 1e-12);
});

test('aggregateBattles keeps only the newest perModelLimit records', () => {
  const records = [];
  for (let i = 0; i < 5; i++) {
    records.push({ f: `f${i}.json`, m: i, a: A, b: B, w: i % 2 ? B : A, d: false, ac: [0, 1, 0, 0, 0], bc: [0, 0, 1, 0, 0] });
  }
  const aggs = aggregateBattles(records, [A, B], 2);
  const aAgg = aggs.get(A);
  assert.equal(aAgg.sampled, 2);
  // newest two: m=4 (A wins) and m=3 (B wins)
  assert.deepEqual(aAgg.rivals.get(B), { w: 1, l: 1, d: 0 });
});

test('scanBattles uses cache to avoid re-reading unchanged files', async () => {
  const battlesDir = path.join(FIXTURES, 'seasons', 'test-season-0001', 'battles');
  const io = { ...defaultIo };
  const first = await scanBattles({ battlesDir, rosterIds: [A, B], io });
  assert.equal(first.stats.filesRead, 4);
  assert.equal(first.aggregates.get(A).rivals.get(B).w, 2);

  const second = await scanBattles({ battlesDir, rosterIds: [A, B], io, cache: first.cacheData });
  assert.equal(second.stats.filesRead, 0, 'unchanged files must be served from cache');
  assert.equal(second.aggregates.get(A).rivals.get(B).w, 2);

  // Touch one file -> only that file is re-read.
  const changed = path.join(battlesDir, 'b4.json');
  const now = Date.now();
  fs.utimesSync(changed, now / 1000, now / 1000);
  const third = await scanBattles({ battlesDir, rosterIds: [A, B], io, cache: second.cacheData });
  assert.equal(third.stats.filesRead, 1);
  // restore deterministic fixture mtimes
  const restore = { 'b1.json': 4000, 'b2.json': 3000, 'b3.json': 2000, 'b4.json': 1000 };
  for (const [f, t] of Object.entries(restore)) fs.utimesSync(path.join(battlesDir, f), t, t);
});

test('aggregateBattles builds a per-mode breakdown', () => {
  const rec = (f, m, mo, a, b, w, d, ac, bc) => ({ f, m, mo, a, b, w, d, ac, bc });
  const records = [
    rec('m1.json', 5000, 'arena', A, B, A, false, [0, 10, 2, 4, 1], [1, 2, 9, 0, 3]),
    rec('m2.json', 4000, 'arena', A, B, B, false, [0, 6, 2, 2, 0], [0, 3, 8, 1, 2]),
    rec('m3.json', 3000, 'ctf', B, A, A, false, [0, 1, 9, 0, 2], [0, 7, 3, 1, 0]), // side swap
    rec('m4.json', 2000, 'tdm', A, B, null, true, [1, 1, 1, 1, 1], [1, 1, 1, 1, 1]),
  ];
  const aAgg = aggregateBattles(records, [A, B], 200).get(A);
  assert.deepEqual(aAgg.modes.map((m) => m.mode), ['arena', 'ctf', 'tdm'], 'modes in MODE_ORDER');

  const arena = aAgg.modes.find((m) => m.mode === 'arena');
  assert.equal(arena.games, 2);
  assert.deepEqual([arena.w, arena.l, arena.d], [1, 1, 0]);
  assert.equal(arena.topAction, 'attack');
  const arenaTotal = 10 + 2 + 4 + 1 + 6 + 2 + 2;
  assert.ok(Math.abs(arena.aggression - (10 + 4 + 6 + 2) / arenaTotal) < 1e-12);
  assert.ok(Math.abs(arena.topShare - 16 / arenaTotal) < 1e-12);

  // ctf: A is model_b, so A's counts come from bc; A won.
  const ctf = aAgg.modes.find((m) => m.mode === 'ctf');
  assert.equal(ctf.games, 1);
  assert.deepEqual([ctf.w, ctf.l, ctf.d], [1, 0, 0]);
  assert.equal(ctf.topAction, 'attack'); // bc attack = 7 > defend 3
  const tdm = aAgg.modes.find((m) => m.mode === 'tdm');
  assert.deepEqual([tdm.w, tdm.l, tdm.d], [0, 0, 1]);
});

test('scanBattles discards caches with a stale schema version', async () => {
  const battlesDir = path.join(FIXTURES, 'seasons', 'test-season-0001', 'battles');
  const first = await scanBattles({ battlesDir, rosterIds: [A, B], io: defaultIo });
  const stale = { ...first.cacheData, version: 1 };
  const second = await scanBattles({ battlesDir, rosterIds: [A, B], io: defaultIo, cache: stale });
  assert.equal(second.stats.filesRead, 4, 'version mismatch must force a full re-read');
  assert.equal(second.cacheData.version, 2);
});

test('slugifyRoster throws instead of emitting an undefined slug', () => {
  const dup = { model_id: 'same-id', canonical_slug: 'p/dup-20260101', provider_model: 'p/dup' };
  // 4 fully identical entries exhaust base, dated, and hash candidates.
  assert.throws(
    () => slugifyRoster([1, 2, 3, 4].map((rank) => ({ ...dup, rank }))),
    /no unique slug candidate.*same-id/,
  );
});

test('buildPages tolerates a fresh season with no artifact dirs', async () => {
  const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-fresh-'));
  const emptyRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-empty-'));
  await buildPages({
    ratingsPath: path.join(FIXTURES, 'ratings.json'),
    artifactsRoot: emptyRoot, // no seasons/<id>/{battles,world} at all
    highlightsPath: path.join(FIXTURES, 'highlights.json'),
    outDir,
    cachePath: path.join(outDir, 'page-cache.json'),
  });
  const alpha = fs.readFileSync(path.join(outDir, 'alpha-one.html'), 'utf8');
  assert.match(alpha, /No recent duel sample available/);
  assert.match(alpha, /No shared world events recorded yet/);
  assert.doesNotMatch(alpha, /undefined/);
});

test('matchFights prefers exact "#rank Title" tag over loose title match', () => {
  const clips = [
    { date: 'd1', webm: 'w1', poster: 'p1', reason: 'x', score: 10, players: ['#1 Abyss', '#2 Talon'] },
    { date: 'd2', webm: 'w2', poster: 'p2', reason: 'y', score: 20, players: ['#9 Abyss', '#3 Surge'] },
    { date: 'd3', webm: 'w3', poster: 'p3', reason: 'z', score: 30, players: ['#1 Abyss'] },
  ];
  // rank 9 Abyss: only the tagged clip, not the rank-1 ones.
  const nine = matchFights(clips, { rank: 9, title: 'Abyss', modelName: 'M' });
  assert.deepEqual(nine.map((c) => c.webm), ['w2']);
  // rank 1 Abyss: two clips, highest score first.
  const one = matchFights(clips, { rank: 1, title: 'Abyss', modelName: 'M' });
  assert.deepEqual(one.map((c) => c.webm), ['w3', 'w1']);
});

test('golden render: 2-model mini roster matches checked-in goldens', async (t) => {
  const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-'));
  const cachePath = path.join(outDir, 'page-cache.json');
  await buildPages({
    ratingsPath: path.join(FIXTURES, 'ratings.json'),
    artifactsRoot: FIXTURES, // fixtures/<season_id>/{battles,world}
    highlightsPath: path.join(FIXTURES, 'highlights.json'),
    outDir,
    cachePath,
  });

  const expected = ['index.html', 'alpha-one.html', 'beta-two.html', 'models.css', 'mascots.json'];
  for (const f of expected) {
    assert.ok(fs.existsSync(path.join(outDir, f)), `missing output ${f}`);
  }

  const update = process.env.UPDATE_GOLDEN === '1';
  for (const f of ['index.html', 'alpha-one.html', 'beta-two.html', 'mascots.json']) {
    const actual = fs.readFileSync(path.join(outDir, f), 'utf8');
    const goldenPath = path.join(FIXTURES, 'golden', f);
    if (update) {
      fs.mkdirSync(path.dirname(goldenPath), { recursive: true });
      fs.writeFileSync(goldenPath, actual);
    }
    assert.equal(actual, fs.readFileSync(goldenPath, 'utf8'), `${f} differs from golden (run with UPDATE_GOLDEN=1 to refresh)`);
  }

  // Sanity: provenance and honest co-performance label land in the page.
  const alpha = fs.readFileSync(path.join(outDir, 'alpha-one.html'), 'utf8');
  assert.match(alpha, /test-season-0001/);
  assert.match(alpha, /sha256:abababababababab/);
  assert.match(alpha, /Co-performance proxy \(league never mixes models on a team\)/);
  assert.match(alpha, /avg finish gap/);
  assert.match(alpha, /Per-mode breakdown/);
  assert.match(alpha, /mode-grid__mode">arena</);

  // No continuous league state under the fixtures root -> no overlay outputs.
  assert.ok(!fs.existsSync(path.join(outDir, 'league.json')), 'league.json requires a valid continuous state');
});

// ---------------------------------------------------------------------------
// Continuous Model League overlay
// ---------------------------------------------------------------------------

const NOW_MS = Date.parse('2026-08-23T12:00:00.000Z');
const HEX = (c) => c.repeat(64);

/**
 * Write a valid continuous league state directory: state.json (2 active, 1
 * retired, 25 announcements), submissions.jsonl with a torn tail, and three
 * daily history snapshots. Alpha's lineage: v1 (entrant) -> v2 accepted ->
 * v3 compile_failed, matching its artifact version 2 / submissions_used 2.
 */
function writeContinuousFixture(dir) {
  fs.mkdirSync(path.join(dir, 'history'), { recursive: true });
  const mascot = (emoji, title, color) => ({ emoji, title, color });
  const artifact = (version) => ({
    wasm_sha256: HEX('a'), source_sha256: HEX('b'), prompt_sha256: HEX('c'),
    version, parent_version: version > 1 ? version - 1 : null,
  });
  const entry = (over) => ({
    model_id: 'x', slug: 'x', mascot: mascot('🥚', 'X', '#91a098'),
    joined_at: '2026-08-01T00:00:00.000Z', submissions_used: 1,
    artifact: artifact(1), rating: 50, wins: 0, losses: 0, draws: 0, matches: 0,
    days_in_league: 0, status: 'active', ...over,
  });
  const alpha = entry({
    model_id: 'test/alpha-one', slug: 'test/alpha-one-20260101',
    mascot: mascot('🦊', 'Alpha', '#caff00'),
    joined_at: '2026-08-10T12:00:00.000Z', submissions_used: 2,
    artifact: artifact(2), rating: 61.5, wins: 8, losses: 2, draws: 1, matches: 11,
    days_in_league: 13,
  });
  const beta = entry({
    model_id: 'test/beta-two', slug: 'test/beta-two-20260102',
    mascot: mascot('🐙', 'Beta', '#00e0ff'),
    joined_at: '2026-08-12T12:00:00.000Z',
    rating: 45, wins: 3, losses: 5, draws: 1, matches: 9, days_in_league: 11,
  });
  const gamma = entry({
    model_id: 'test/gamma', slug: 'test/gamma-20260101',
    mascot: mascot('🦉', 'Gamma', '#a78bfa'),
    joined_at: '2026-08-01T12:00:00.000Z', submissions_used: 3,
    artifact: artifact(3), rating: 32, wins: 2, losses: 9, draws: 1, matches: 12,
    days_in_league: 14,
    retired_at: '2026-08-15T12:00:00.000Z',
    reason: '14 days in league, submissions 3/3: rating 32 < 35',
  });

  const announcements = [];
  for (let i = 0; i < 21; i++) {
    announcements.push({
      type: 'entrant', model_id: `old-${i}`, slug: `old-${i}`,
      mascot: mascot('🥚', `Old ${i}`, '#91a098'), provider_rank: 50 + i,
      at: new Date(Date.parse('2026-08-01T00:00:00.000Z') + i * 3600_000).toISOString(),
    });
  }
  announcements.push(
    { type: 'entrant', model_id: 'test/beta-two', slug: beta.slug, mascot: beta.mascot, provider_rank: 7, at: '2026-08-12T12:00:00.000Z' },
    { type: 'retirement', model_id: 'test/gamma', slug: gamma.slug, mascot: gamma.mascot, reason: gamma.reason, stats: { rating: 32, wins: 2, losses: 9, draws: 1, matches: 12, days_in_league: 14, submissions_used: 3 }, at: '2026-08-15T11:00:00.000Z' },
    { type: 'revision', model_id: 'test/alpha-one', slug: alpha.slug, mascot: alpha.mascot, version: 2, outcome: 'accepted', at: '2026-08-15T12:00:00.000Z' },
    { type: 'revision', model_id: 'test/alpha-one', slug: alpha.slug, mascot: alpha.mascot, version: 3, outcome: 'compile_failed', at: '2026-08-22T12:00:00.000Z' },
  );

  const state = {
    schema_version: 1, league_id: 'cml-test-0001', day_index: 13,
    roster: [alpha, beta], retired: [gamma], announcements,
    last_feedback_at: '2026-08-22T12:00:00.000Z',
    created_at: '2026-08-01T00:00:00.000Z', updated_at: '2026-08-23T11:00:00.000Z',
  };
  fs.writeFileSync(path.join(dir, 'state.json'), JSON.stringify(state, null, 2));

  const submission = (over) => JSON.stringify({
    model_id: 'test/alpha-one', slug: alpha.slug, version_attempted: 2, parent_version: 1,
    prompt_sha256: HEX('c'), brief_sha256: null, source_sha256: HEX('b'), wasm_sha256: HEX('a'),
    compile_attempts: 1, outcome: 'accepted', at: '2026-08-15T12:00:00.000Z', ...over,
  });
  const tornTail = '{"model_id":"test/alpha-one","version_attempted":4,"outcome":"acc';
  fs.writeFileSync(
    path.join(dir, 'submissions.jsonl'),
    `${submission({})}\n${submission({
      version_attempted: 3, parent_version: 2, source_sha256: null, wasm_sha256: null,
      outcome: 'compile_failed', at: '2026-08-22T12:00:00.000Z',
    })}\n${tornTail}`, // no trailing newline: simulates a crash mid-append
  );

  const wld = (w, l, d) => ({ wins: w, losses: l, draws: d, matches: w + l + d });
  const snap = (at, day, alphaStats, betaStats) => [{
    at, league_id: 'cml-test-0001', day_index: day, season_id: `continuous-cml-test-0001-day${day}`,
    roster: [
      { model_id: 'test/alpha-one', slug: alpha.slug, rating: 55, ...alphaStats },
      { model_id: 'test/beta-two', slug: beta.slug, rating: 47, ...betaStats },
    ],
  }];
  fs.writeFileSync(path.join(dir, 'history', '2026-08-14.json'), JSON.stringify(snap('2026-08-14T23:00:00.000Z', 4, wld(3, 1, 0), wld(1, 2, 0))));
  fs.writeFileSync(path.join(dir, 'history', '2026-08-18.json'), JSON.stringify(snap('2026-08-18T23:00:00.000Z', 8, wld(6, 2, 1), wld(2, 4, 1))));
  fs.writeFileSync(path.join(dir, 'history', '2026-08-22.json'), JSON.stringify(snap('2026-08-22T23:00:00.000Z', 12, wld(8, 2, 1), wld(3, 5, 1))));
}

async function buildWithContinuous(outDir, cmlDir) {
  await buildPages({
    ratingsPath: path.join(FIXTURES, 'ratings.json'),
    artifactsRoot: FIXTURES,
    continuousDir: cmlDir,
    highlightsPath: path.join(FIXTURES, 'highlights.json'),
    outDir,
    cachePath: path.join(outDir, 'page-cache.json'),
    nowMs: NOW_MS,
  });
}

test('continuous overlay: index header, announcements feed order/cap, hall of fame, league.json', async () => {
  const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-out-'));
  const cmlDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-state-'));
  writeContinuousFixture(cmlDir);
  await buildWithContinuous(outDir, cmlDir);

  const index = fs.readFileSync(path.join(outDir, 'index.html'), 'utf8');
  assert.match(index, /Continuous league/);
  assert.match(index, /day 13/);
  assert.match(index, /2\/10/);
  assert.match(index, /in 1d 0h/, 'countdown from last_feedback_at + 48h');

  // Feed: capped at 20, newest first, oldest 5 of 25 dropped.
  const feedItems = index.match(/<li class="announce__item /g) || [];
  assert.equal(feedItems.length, 20);
  assert.ok(!index.includes('Old 0'), 'announcements beyond the cap are dropped');
  assert.ok(index.includes('Old 20'));
  assert.ok(index.indexOf('v3 compile failed') < index.indexOf('v2 accepted'), 'newest announcement first');
  assert.ok(index.indexOf('v2 accepted') < index.indexOf('retires to the Hall of Fame'));
  assert.match(index, /🌱/);
  assert.match(index, /🔧/);
  assert.match(index, /🪦/);

  // Hall of Fame lists the retired entry with final stats.
  assert.match(index, /Hall of Fame/);
  assert.match(index, /🦉/);
  assert.match(index, /Gamma/);
  assert.match(index, /32\.0/);
  assert.match(index, />14</);
  assert.match(index, /win">2<\/span>W · <span class="loss">9<\/span>L · 1D/);
  assert.match(index, /14 days in league, submissions 3\/3: rating 32 &lt; 35/);

  // Ticker payload for the landing page.
  const league = JSON.parse(fs.readFileSync(path.join(outDir, 'league.json'), 'utf8'));
  assert.equal(league.day_index, 13);
  assert.equal(league.announcements.length, 10);
  assert.equal(league.announcements[0].type, 'revision');
  assert.equal(league.announcements[0].version, 3);
  assert.equal(league.announcements[0].outcome, 'compile_failed');
});

test('continuous overlay: model page renders submission lineage with W/L/D deltas', async () => {
  const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-out-'));
  const cmlDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-state-'));
  writeContinuousFixture(cmlDir);
  await buildWithContinuous(outDir, cmlDir);

  const alpha = fs.readFileSync(path.join(outDir, 'alpha-one.html'), 'utf8');
  assert.match(alpha, /Submission lineage/);
  for (const v of ['v1', 'v2', 'v3']) {
    assert.ok(alpha.includes(`lineage__version">${v}<`), `lineage includes ${v}`);
  }
  assert.ok(!alpha.includes('lineage__version">v4<'), 'torn jsonl tail is skipped');
  assert.match(alpha, /lineage__node--entrant/);
  assert.match(alpha, /lineage__node--accepted/);
  assert.match(alpha, /lineage__node--compile_failed/);
  assert.match(alpha, /entered the league/);
  assert.match(alpha, /compile attempts 1/);
  // v1 window: joined -> 2026-08-14 snapshot (3/1/0 from a clean record).
  assert.match(alpha, /\+3W \+1L \+0D while live/);
  // v2 window: 2026-08-14 snapshot -> latest snapshot (8/2/1 - 3/1/0).
  assert.match(alpha, /\+5W \+1L \+1D while live/);

  // Beta has no submissions: lineage shows only the v1 entrant node.
  const beta = fs.readFileSync(path.join(outDir, 'beta-two.html'), 'utf8');
  assert.match(beta, /Submission lineage/);
  assert.ok(beta.includes('lineage__version">v1<'));
  assert.ok(!beta.includes('lineage__version">v2<'));
});

test('invalid continuous state falls back to byte-identical weekly output', async () => {
  const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-out-'));
  const cmlDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-state-'));
  fs.writeFileSync(path.join(cmlDir, 'state.json'), JSON.stringify({ schema_version: 99 }));
  await buildWithContinuous(outDir, cmlDir);

  for (const f of ['index.html', 'alpha-one.html', 'beta-two.html', 'mascots.json']) {
    assert.equal(
      fs.readFileSync(path.join(outDir, f), 'utf8'),
      fs.readFileSync(path.join(FIXTURES, 'golden', f), 'utf8'),
      `${f} must be byte-identical to the golden when the overlay is inactive`,
    );
  }
  assert.ok(!fs.existsSync(path.join(outDir, 'league.json')));
});

test('stale league.json is removed when the overlay becomes inactive', async () => {
  const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-out-'));
  const cmlDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-state-'));
  writeContinuousFixture(cmlDir);

  // Valid state -> ticker payload written.
  await buildWithContinuous(outDir, cmlDir);
  assert.ok(fs.existsSync(path.join(outDir, 'league.json')), 'league.json written for a valid state');

  // State turns invalid -> rebuild drops the stale payload instead of
  // serving it to the landing ticker forever.
  fs.writeFileSync(path.join(cmlDir, 'state.json'), JSON.stringify({ schema_version: 99 }));
  await buildWithContinuous(outDir, cmlDir);
  assert.ok(!fs.existsSync(path.join(outDir, 'league.json')), 'stale league.json removed');
  assert.equal(
    fs.readFileSync(path.join(outDir, 'index.html'), 'utf8'),
    fs.readFileSync(path.join(FIXTURES, 'golden', 'index.html'), 'utf8'),
    'index.html falls back to the byte-identical weekly view',
  );

  // Overlay never active -> nothing to remove, no crash on a fresh outDir.
  const cleanOut = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-out-'));
  const emptyCml = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-state-'));
  await buildWithContinuous(cleanOut, emptyCml);
  assert.ok(!fs.existsSync(path.join(cleanOut, 'league.json')));
});
