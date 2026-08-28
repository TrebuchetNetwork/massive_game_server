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
import { trackPolicy } from '../continuous/league.mjs';

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
    toplistPath: path.join(FIXTURES, 'no-such-toplist.json'),
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
    toplistPath: path.join(FIXTURES, 'no-such-toplist.json'),
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

  // No toplist commentary -> analyst sections hidden.
  const index = fs.readFileSync(path.join(outDir, 'index.html'), 'utf8');
  assert.ok(!index.includes('Analyst Toplist'), 'toplist section hidden when commentary is absent');
  assert.ok(!alpha.includes('analyst-note'), 'analyst quote block hidden when commentary is absent');
});

// ---------------------------------------------------------------------------
// Analyst toplist
// ---------------------------------------------------------------------------

test('analyst toplist: fixture commentary renders ranked cards and model quote blocks', async () => {
  const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-toplist-'));
  await buildPages({
    ratingsPath: path.join(FIXTURES, 'ratings.json'),
    artifactsRoot: FIXTURES,
    highlightsPath: path.join(FIXTURES, 'highlights.json'),
    outDir,
    cachePath: path.join(outDir, 'page-cache.json'),
    toplistPath: path.join(FIXTURES, 'toplist_commentary.json'),
  });

  const index = fs.readFileSync(path.join(outDir, 'index.html'), 'utf8');
  assert.match(index, /Analyst Toplist/);
  assert.match(index, /league day 7/);
  // Cards ordered by rank even though the fixture lists rank 2 first.
  assert.ok(
    index.indexOf('Beta takes the crown.') < index.indexOf('Dethroned, but dangerous.'),
    'toplist cards ordered by rank',
  );
  // Roster entries link to their model page (slug prefix match against the
  // dated canonical slug); the retired gamma gets a subtle league badge.
  assert.match(index, /<a class="toplist__card" href="beta-two\.html">/);
  assert.match(index, /<a class="toplist__card" href="alpha-one\.html">/);
  assert.match(index, /<article class="toplist__card">/);
  assert.match(index, /toplist__badge">league</);
  // Mascot emoji + headline land on every card.
  assert.equal((index.match(/toplist__emoji/g) || []).length, 3);
  assert.equal((index.match(/toplist__headline/g) || []).length, 3);
  // Roster-matched cards show the model display name, not the raw entry slug.
  const toplistBlock = index.split('aria-label="Analyst toplist"')[1].split('</section>')[0];
  assert.match(toplistBlock, /Test: Beta Two/);
  assert.match(toplistBlock, /test\/gamma-three/);

  const alpha = fs.readFileSync(path.join(outDir, 'alpha-one.html'), 'utf8');
  assert.match(alpha, /Analyst note · league day 7/);
  assert.match(alpha, /Dethroned, but dangerous\./);
  assert.match(alpha, /analyst-note__commentary/);
  // Quote block sits near the top, before the ratings panels.
  assert.ok(alpha.indexOf('analyst-note') < alpha.indexOf('Ratings radar'));

  const beta = fs.readFileSync(path.join(outDir, 'beta-two.html'), 'utf8');
  assert.match(beta, /Analyst note · league day 7/);
  assert.match(beta, /Beta takes the crown\./);
});

test('analyst toplist: absent or malformed file keeps byte-identical goldens', async () => {
  const badDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-toplist-bad-'));
  const malformed = path.join(badDir, 'toplist.json');
  fs.writeFileSync(malformed, '{not json');
  const empty = path.join(badDir, 'empty.json');
  fs.writeFileSync(empty, JSON.stringify({ league_day: 3, entries: [] }));

  for (const toplistPath of [path.join(FIXTURES, 'no-such-toplist.json'), malformed, empty]) {
    const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-toplist-out-'));
    await buildPages({
      ratingsPath: path.join(FIXTURES, 'ratings.json'),
      artifactsRoot: FIXTURES,
      highlightsPath: path.join(FIXTURES, 'highlights.json'),
      outDir,
      cachePath: path.join(outDir, 'page-cache.json'),
      toplistPath,
    });
    for (const f of ['index.html', 'alpha-one.html', 'beta-two.html', 'mascots.json']) {
      assert.equal(
        fs.readFileSync(path.join(outDir, f), 'utf8'),
        fs.readFileSync(path.join(FIXTURES, 'golden', f), 'utf8'),
        `${f} must be byte-identical to the golden when toplist commentary is unusable`,
      );
    }
  }
});

// ---------------------------------------------------------------------------
// Continuous Model League overlay
// ---------------------------------------------------------------------------

const NOW_MS = Date.parse('2026-08-23T12:00:00.000Z');
const HEX = (c) => c.repeat(64);

/**
 * Write a valid schema-v2 (four-track) continuous league state directory.
 * Alpha and beta compete in every track with diverging per-track ratings
 * (matrix data); delta retired in L0, gamma retired in L1 (two HoF groups);
 * epsilon fills the rosters so standings top-3 are real. Announcements are
 * spread across tracks (26 total → merged feed caps at 20). Shared
 * submissions.jsonl carries track + stint fields plus a stale-stint record
 * and a torn tail. Alpha's L2 lineage: v1 (entrant) -> v2 accepted -> v3
 * compile_failed; L3 lineage: v1 -> v2 accepted. Per-track history snapshots
 * live under tracks/<T>/history.
 */
function writeContinuousFixture(dir) {
  const mascot = (emoji, title, color) => ({ emoji, title, color });
  const artifact = (version) => ({
    wasm_sha256: HEX('a'), source_sha256: HEX('b'), prompt_sha256: HEX('c'),
    version, parent_version: version > 1 ? version - 1 : null,
  });
  const entry = (over) => ({
    model_id: 'x', slug: 'x', mascot: mascot('🥚', 'X', '#91a098'),
    joined_at: '2026-08-10T12:00:00.000Z', submissions_used: 1,
    artifact: artifact(1), rating: 50, wins: 0, losses: 0, draws: 0, matches: 0,
    days_in_league: 13, status: 'active', ...over,
  });
  const wld = (w, l, d) => ({ wins: w, losses: l, draws: d, matches: w + l + d });

  const alphaAt = (rating, over = {}) => entry({
    model_id: 'test/alpha-one', slug: 'test/alpha-one-20260101',
    mascot: mascot('🦊', 'Alpha', '#caff00'), rating, ...wld(8, 2, 1), ...over,
  });
  const betaAt = (rating) => entry({
    model_id: 'test/beta-two', slug: 'test/beta-two-20260102',
    mascot: mascot('🐙', 'Beta', '#00e0ff'), rating, ...wld(3, 5, 1),
  });
  const epsilonAt = (rating) => entry({
    model_id: 'test/epsilon', slug: 'test/epsilon-20260101',
    mascot: mascot('🐢', 'Epsilon', '#facc15'), rating, ...wld(4, 4, 1),
  });
  const retiredEntry = (name, emoji, rating) => entry({
    model_id: `test/${name}`, slug: `test/${name}-20260101`,
    mascot: mascot(emoji, name[0].toUpperCase() + name.slice(1), '#a78bfa'),
    joined_at: '2026-08-01T12:00:00.000Z', rating, ...wld(2, 9, 1),
    days_in_league: 14,
    retired_at: '2026-08-15T12:00:00.000Z',
    reason: `14 days in league, submissions 1/1: rating ${rating} < 35`,
  });
  const delta = retiredEntry('delta', '🦎', 30);
  const gamma = retiredEntry('gamma', '🦉', 32);

  const fillers = (trackId, from, to) => {
    const out = [];
    for (let i = from; i < to; i++) {
      out.push({
        type: 'entrant', track: trackId, model_id: `old-${i}`, slug: `old-${i}`,
        mascot: mascot('🥚', `Old ${i}`, '#91a098'), provider_rank: 50 + i,
        at: new Date(Date.parse('2026-08-01T00:00:00.000Z') + i * 3600_000).toISOString(),
      });
    }
    return out;
  };
  const alphaMascot = mascot('🦊', 'Alpha', '#caff00');
  const betaMascot = mascot('🐙', 'Beta', '#00e0ff');

  const slice = (trackId, over) => {
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
      ...over,
    };
  };

  const state = {
    schema_version: 2,
    league_id: 'cml-test-0001',
    tracks: {
      L0: slice('L0', {
        day_index: 10,
        roster: [alphaAt(52), betaAt(41), epsilonAt(48)],
        retired: [delta],
        announcements: fillers('L0', 0, 11),
      }),
      L1: slice('L1', {
        day_index: 11,
        roster: [alphaAt(55), betaAt(43), epsilonAt(50)],
        retired: [gamma],
        announcements: [
          ...fillers('L1', 11, 21),
          { type: 'retirement', track: 'L1', model_id: 'test/gamma', slug: gamma.slug, mascot: gamma.mascot, reason: gamma.reason, stats: { rating: 32, wins: 2, losses: 9, draws: 1, matches: 12, days_in_league: 14, submissions_used: 1 }, at: '2026-08-15T11:00:00.000Z' },
        ],
      }),
      L2: slice('L2', {
        day_index: 12,
        roster: [alphaAt(61.5, { submissions_used: 2, artifact: artifact(2) }), betaAt(45), epsilonAt(44)],
        announcements: [
          { type: 'entrant', track: 'L2', model_id: 'test/beta-two', slug: 'test/beta-two-20260102', mascot: betaMascot, provider_rank: 7, at: '2026-08-12T12:00:00.000Z' },
          { type: 'revision', track: 'L2', model_id: 'test/alpha-one', slug: 'test/alpha-one-20260101', mascot: alphaMascot, version: 2, outcome: 'accepted', at: '2026-08-15T12:00:00.000Z' },
          { type: 'revision', track: 'L2', model_id: 'test/alpha-one', slug: 'test/alpha-one-20260101', mascot: alphaMascot, version: 3, outcome: 'compile_failed', at: '2026-08-22T12:00:00.000Z' },
        ],
        last_feedback_at: '2026-08-22T12:00:00.000Z',
      }),
      L3: slice('L3', {
        day_index: 13,
        roster: [alphaAt(66, { submissions_used: 2, artifact: artifact(2) }), betaAt(40), epsilonAt(52)],
        announcements: [
          { type: 'revision', track: 'L3', model_id: 'test/alpha-one', slug: 'test/alpha-one-20260101', mascot: alphaMascot, version: 2, outcome: 'accepted', at: '2026-08-18T12:00:00.000Z' },
        ],
        last_feedback_at: '2026-08-22T12:00:00.000Z',
      }),
    },
    created_at: '2026-08-01T00:00:00.000Z',
    updated_at: '2026-08-23T11:00:00.000Z',
  };
  fs.writeFileSync(path.join(dir, 'state.json'), JSON.stringify(state, null, 2));

  const STINT = '2026-08-10T12:00:00.000Z';
  const submission = (over) => JSON.stringify({
    track: 'L2', stint: STINT,
    model_id: 'test/alpha-one', slug: 'test/alpha-one-20260101', version_attempted: 2, parent_version: 1,
    prompt_sha256: HEX('c'), brief_sha256: null, source_sha256: HEX('b'), wasm_sha256: HEX('a'),
    compile_attempts: 1, outcome: 'accepted', at: '2026-08-15T12:00:00.000Z', ...over,
  });
  const tornTail = '{"model_id":"test/alpha-one","track":"L2","version_attempted":6,"outcome":"acc';
  fs.writeFileSync(
    path.join(dir, 'submissions.jsonl'),
    `${submission({})}\n${submission({
      version_attempted: 3, parent_version: 2, source_sha256: null, wasm_sha256: null,
      outcome: 'compile_failed', at: '2026-08-22T12:00:00.000Z',
    })}\n${submission({
      track: 'L3', version_attempted: 2, at: '2026-08-18T12:00:00.000Z',
    })}\n${submission({
      stint: '2026-07-01T00:00:00.000Z', version_attempted: 5, at: '2026-07-05T12:00:00.000Z',
    })}\n${tornTail}`, // stale stint (previous incarnation) + torn tail, both ignored
  );

  const snap = (trackId, at, day, alphaStats, betaStats) => [{
    at, league_id: 'cml-test-0001', track: trackId, day_index: day,
    season_id: `continuous-cml-test-0001-${trackId}-day${day}`,
    roster: [
      { model_id: 'test/alpha-one', slug: 'test/alpha-one-20260101', rating: 55, ...alphaStats },
      { model_id: 'test/beta-two', slug: 'test/beta-two-20260102', rating: 47, ...betaStats },
    ],
  }];
  const writeHistory = (trackId, entries) => {
    const dirT = path.join(dir, 'tracks', trackId, 'history');
    fs.mkdirSync(dirT, { recursive: true });
    for (const [date, payload] of entries) {
      fs.writeFileSync(path.join(dirT, `${date}.json`), JSON.stringify(payload));
    }
  };
  // L2: v1 window ends at the 08-14 snapshot (3/1/0); v2 runs to 08-22 (8/2/1).
  writeHistory('L2', [
    ['2026-08-14', snap('L2', '2026-08-14T23:00:00.000Z', 4, wld(3, 1, 0), wld(1, 2, 0))],
    ['2026-08-18', snap('L2', '2026-08-18T23:00:00.000Z', 8, wld(6, 2, 1), wld(2, 4, 1))],
    ['2026-08-22', snap('L2', '2026-08-22T23:00:00.000Z', 12, wld(8, 2, 1), wld(3, 5, 1))],
  ]);
  // L3: v1 window ends at 08-17 (2/0/1); v2 accepted 08-18, runs to 08-22 (9/3/1).
  writeHistory('L3', [
    ['2026-08-17', snap('L3', '2026-08-17T23:00:00.000Z', 7, wld(2, 0, 1), wld(1, 1, 0))],
    ['2026-08-22', snap('L3', '2026-08-22T23:00:00.000Z', 12, wld(9, 3, 1), wld(3, 5, 1))],
  ]);
  // L0/L1 never revise: a single snapshot each, so v1 carries a plain delta.
  writeHistory('L0', [['2026-08-22', snap('L0', '2026-08-22T23:00:00.000Z', 10, wld(4, 3, 0), wld(2, 5, 0))]]);
  writeHistory('L1', [['2026-08-22', snap('L1', '2026-08-22T23:00:00.000Z', 11, wld(5, 3, 0), wld(2, 6, 0))]]);
}

async function buildWithContinuous(outDir, cmlDir) {
  await buildPages({
    ratingsPath: path.join(FIXTURES, 'ratings.json'),
    artifactsRoot: FIXTURES,
    continuousDir: cmlDir,
    highlightsPath: path.join(FIXTURES, 'highlights.json'),
    outDir,
    cachePath: path.join(outDir, 'page-cache.json'),
    toplistPath: path.join(FIXTURES, 'no-such-toplist.json'),
    nowMs: NOW_MS,
  });
}

test('continuous overlay: multi-track header, standings, matrix, track-badged feed, HoF, league.json', async () => {
  const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-out-'));
  const cmlDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-state-'));
  writeContinuousFixture(cmlDir);
  await buildWithContinuous(outDir, cmlDir);

  const index = fs.readFileSync(path.join(outDir, 'index.html'), 'utf8');

  // Header: league id, per-track day index, per-track feedback cadence.
  assert.match(index, /cml-test-0001/);
  assert.match(index, /L0<\/span> Zero-shot/);
  assert.match(index, /day 10 · 3\/10 slots/);
  assert.match(index, /day 13 · 3\/10 slots/);
  assert.match(index, /never revises/);
  assert.match(index, /feedback in 1d 0h/, 'L2 countdown from last_feedback_at + 48h');
  assert.match(index, /feedback in 6d 0h/, 'L3 countdown from last_feedback_at + 7d');

  // Standings: one table per track, rank-sorted by rating, with subs used/allowed.
  assert.equal((index.match(/standings__track/g) || []).length, 4);
  const l3Block = index.split('L3</span> Weekly feedback <small>')[1];
  assert.ok(l3Block.indexOf('66.0') < l3Block.indexOf('40.0'), 'L3 standings sorted by rating');
  assert.match(index, /2\/3/, 'L2 submissions used/allowed');
  assert.match(index, /2\/9/, 'L3 submissions used/allowed');

  // Experiment matrix: model x track ratings + L3-L0 delta, sorted by delta.
  assert.match(index, /Experiment matrix/);
  assert.match(index, /Δ feedback/);
  const matrix = index.split('<section class="panel matrix"')[1].split('<section class="panel announce"')[0];
  const alphaRow = matrix.split('🦊</span> <b>Alpha</b>')[1].split('</tr>')[0];
  for (const cell of ['52.0', '55.0', '61.5', '66.0']) assert.ok(alphaRow.includes(cell), `alpha matrix cell ${cell}`);
  assert.ok(alphaRow.includes('matrix__delta--pos">+14.0'), 'alpha L3-L0 delta');
  const betaRow = matrix.split('🐙</span> <b>Beta</b>')[1].split('</tr>')[0];
  assert.ok(betaRow.includes('matrix__delta--neg">-1.0'), 'beta L3-L0 delta');
  assert.ok(
    matrix.indexOf('<b>Alpha</b>') < matrix.indexOf('<b>Epsilon</b>')
      && matrix.indexOf('<b>Epsilon</b>') < matrix.indexOf('<b>Beta</b>'),
    'matrix rows sorted by feedback delta',
  );
  assert.match(matrix, /32\.0 🪦/, 'retired cell keeps final rating with marker');

  // Feed: merged across tracks, capped at 20, newest first, track-badged.
  const feedItems = index.match(/<li class="announce__item /g) || [];
  assert.equal(feedItems.length, 20, 'merged feed capped at 20 of 26');
  assert.ok(!index.includes('Old 5'), 'oldest announcements beyond the cap are dropped');
  assert.ok(index.includes('Old 6'));
  assert.ok(index.indexOf('v3 compile failed') < index.indexOf('v2 accepted'), 'newest announcement first');
  assert.ok(index.indexOf('v2 accepted') < index.indexOf('retires to the Hall of Fame'));
  assert.match(index, /track-badge track-badge--L2/);
  assert.match(index, /track-badge track-badge--L3/);
  assert.match(index, /🌱/);
  assert.match(index, /🔧/);
  assert.match(index, /🪦/);

  // Hall of Fame grouped by track (L0: delta, L1: gamma).
  assert.match(index, /Hall of Fame/);
  assert.equal((index.match(/hof__track/g) || []).length, 2);
  const hof = index.split('🪦 retired with honors, per track')[1];
  assert.ok(hof.indexOf('L0</span> Zero-shot') < hof.indexOf('Delta'));
  assert.ok(hof.indexOf('L1</span> Compile-fix') < hof.indexOf('Gamma'));
  assert.match(hof, /32\.0/);
  assert.match(hof, /14 days in league, submissions 1\/1: rating 32 &lt; 35/);

  // Ticker payload: back-compat flat fields + new per-track map.
  const league = JSON.parse(fs.readFileSync(path.join(outDir, 'league.json'), 'utf8'));
  assert.equal(league.day_index, 13, 'legacy day_index = max across tracks');
  assert.equal(league.announcements.length, 10, 'legacy flat announcements kept for the ticker');
  assert.equal(league.announcements[0].type, 'revision');
  assert.equal(league.announcements[0].version, 3);
  assert.equal(league.announcements[0].outcome, 'compile_failed');
  assert.equal(league.announcements[0].track, 'L2');
  for (const t of ['L0', 'L1', 'L2', 'L3']) {
    assert.ok(league.tracks[t], `tracks.${t} present`);
    assert.equal(league.tracks[t].standings.length, 3, 'top-3 standings per track');
  }
  assert.equal(league.tracks.L0.day_index, 10);
  assert.equal(league.tracks.L3.day_index, 13);
  assert.equal(league.tracks.L3.standings[0].rating, 66);
  assert.equal(league.tracks.L3.standings[0].submissions_allowed, 9);
});

test('continuous overlay: model page renders per-track submission lineage', async () => {
  const outDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-out-'));
  const cmlDir = fs.mkdtempSync(path.join(os.tmpdir(), 'modelpages-cml-state-'));
  writeContinuousFixture(cmlDir);
  await buildWithContinuous(outDir, cmlDir);

  const alpha = fs.readFileSync(path.join(outDir, 'alpha-one.html'), 'utf8');
  assert.match(alpha, /Submission lineage/);
  assert.equal((alpha.match(/lineage__track/g) || []).length, 4, 'one lineage block per track');

  const l2 = alpha.split('L2<\/span> Two-iteration')[1].split('lineage__track')[0];
  for (const v of ['v1', 'v2', 'v3']) {
    assert.ok(l2.includes(`lineage__version">${v}<`), `L2 lineage includes ${v}`);
  }
  assert.match(l2, /lineage__node--compile_failed/);
  // L2 v1 window: joined -> 2026-08-14 snapshot (3/1/0 from a clean record).
  assert.match(l2, /\+3W \+1L \+0D while live/);
  // L2 v2 window: 2026-08-14 snapshot -> latest snapshot (8/2/1 - 3/1/0).
  assert.match(l2, /\+5W \+1L \+1D while live/);

  const l3 = alpha.split('L3<\/span> Weekly feedback')[1];
  assert.ok(l3.includes('lineage__version">v2<'), 'L3 lineage includes v2');
  assert.ok(!l3.includes('lineage__version">v3<'), 'L3 has no v3 attempt');
  // L3 v2 window: 08-17 snapshot (2/0/1) -> latest (9/3/1).
  assert.match(l3, /\+7W \+3L \+0D while live/);

  // Stale-stint record (v5) and the torn tail (v6) never render.
  assert.ok(!alpha.includes('lineage__version">v5<'), 'stale-stint submission ignored');
  assert.ok(!alpha.includes('lineage__version">v6<'), 'torn jsonl tail is skipped');

  // Beta has no submissions: four v1-entrant-only blocks.
  const beta = fs.readFileSync(path.join(outDir, 'beta-two.html'), 'utf8');
  assert.equal((beta.match(/lineage__track/g) || []).length, 4);
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
