import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  COUNTDOWN_DAYS,
  DAY_MS,
  FINALE_SCHEMA_VERSION,
  championOf,
  compareChampionship,
  computeSeasonView,
  divisionTop3,
  freezeRecord,
  resolveSeasonFrame,
  seasonNumbers,
  seasonRetirements,
  seasonWindow,
  validateFinaleRecord,
  winRate,
} from '../season.mjs';

const START_MS = Date.parse('2026-08-24T00:00:00.000Z');
const END_MS = Date.parse('2026-09-21T00:00:00.000Z');
const SEASON = {
  id: 'S1',
  name: 'Genesis',
  startedMs: START_MS,
  endsMs: END_MS,
  totalDays: 28,
  theme: 'Ten walk in.',
  championRule: 'Highest L0 rating at season end.',
};

const mascot = (emoji, title, color = '#caff00') => ({ emoji, title, color });
const entry = (over = {}) => ({
  model_id: 'vendor/model-a',
  slug: 'vendor/model-a-20260801',
  mascot: mascot('🦊', 'Alpha'),
  joined_at: '2026-08-24T00:00:00.000Z',
  rating: 50,
  wins: 0,
  losses: 0,
  draws: 0,
  matches: 0,
  days_in_league: 10,
  status: 'active',
  ...over,
});
const wld = (w, l, d) => ({ wins: w, losses: l, draws: d, matches: w + l + d });

function stateWith(l0Roster, { l0Retired = [], otherRetired = {} } = {}) {
  const slice = (roster, retired) => ({ roster, retired });
  return {
    tracks: {
      L0: slice(l0Roster, l0Retired),
      L1: slice([], otherRetired.L1 || []),
      L2: slice([], otherRetired.L2 || []),
      L3: slice([], otherRetired.L3 || []),
    },
  };
}

test('seasonWindow counts days from started_at and clamps to the season length', () => {
  assert.deepEqual(seasonWindow(SEASON, START_MS), {
    day: 1, totalDays: 28, daysRemaining: 28, status: 'active', phase: 'active',
  });
  // Mid-season: 2026-09-16 12:00 is day 24 (23.5 days in).
  const mid = seasonWindow(SEASON, Date.parse('2026-09-16T12:00:00.000Z'));
  assert.equal(mid.day, 24);
  assert.equal(mid.daysRemaining, 5);
  assert.equal(mid.phase, 'active');
  // The last day still reads day 28, then the season ends at ends_at.
  assert.equal(seasonWindow(SEASON, END_MS - 1).day, 28);
  assert.equal(seasonWindow(SEASON, END_MS - 1).status, 'active');
  assert.equal(seasonWindow(SEASON, END_MS).status, 'ended');
  assert.equal(seasonWindow(SEASON, END_MS + 9 * DAY_MS).day, 28);
  // Before the season starts the counter clamps to day 1.
  assert.equal(seasonWindow(SEASON, START_MS - DAY_MS).status, 'before');
  assert.equal(seasonWindow(SEASON, START_MS - DAY_MS).day, 1);
});

test('seasonWindow flags the final-countdown phase within 3 days of the end', () => {
  assert.equal(COUNTDOWN_DAYS, 3);
  // 2026-09-18T00:00Z is exactly 3 days out.
  const at = seasonWindow(SEASON, Date.parse('2026-09-18T00:00:00.000Z'));
  assert.equal(at.daysRemaining, 3);
  assert.equal(at.phase, 'countdown');
  // A second past the boundary drops to 2 days and stays in countdown.
  const inside = seasonWindow(SEASON, Date.parse('2026-09-19T12:00:00.000Z'));
  assert.equal(inside.daysRemaining, 2);
  assert.equal(inside.phase, 'countdown');
  // 4 days out is still the regular active phase.
  assert.equal(seasonWindow(SEASON, Date.parse('2026-09-16T12:00:00.000Z')).phase, 'active');
});

test('championOf follows the champion rule: rating, win rate, then matches', () => {
  const leader = entry({ slug: 'a/leader', rating: 66, ...wld(8, 2, 1) });
  const chaser = entry({ slug: 'a/chaser', rating: 60, ...wld(20, 0, 0) });
  assert.equal(championOf([chaser, leader]).slug, 'a/leader', 'highest rating wins');

  // Equal rating: higher win rate takes it.
  const hot = entry({ slug: 'a/hot', rating: 60, ...wld(9, 1, 0) }); // 90%
  const cold = entry({ slug: 'a/cold', rating: 60, ...wld(5, 5, 0) }); // 50%
  assert.equal(championOf([cold, hot]).slug, 'a/hot');

  // Equal rating and win rate: more matches played takes it.
  const busy = entry({ slug: 'a/busy', rating: 60, ...wld(10, 5, 0) });
  const quiet = entry({ slug: 'a/quiet', rating: 60, ...wld(2, 1, 0) });
  assert.equal(championOf([quiet, busy]).slug, 'a/busy');

  // Full tie: deterministic slug order (never reached by one model twice).
  const first = entry({ slug: 'a/aaa', rating: 60, ...wld(2, 1, 0) });
  const second = entry({ slug: 'a/bbb', rating: 60, ...wld(2, 1, 0) });
  assert.equal(championOf([second, first]).slug, 'a/aaa');

  assert.equal(championOf([]), null);
  assert.equal(championOf(undefined), null);
});

test('winRate treats a matchless model as 0', () => {
  assert.equal(winRate(entry({})), 0);
  assert.equal(winRate(entry(wld(3, 1, 0))), 0.75);
  assert.ok(compareChampionship(entry({ rating: 50, ...wld(0, 0, 0) }), entry({ rating: 50, ...wld(1, 3, 0) })) > 0,
    'a matchless model loses the win-rate tiebreak');
});

test('resolveSeasonFrame hands over to next at its started_at', () => {
  const next = { ...SEASON, id: 'S2', name: 'Ascension', startedMs: END_MS, endsMs: END_MS + 28 * DAY_MS };
  const before = resolveSeasonFrame({ current: SEASON, next }, END_MS - 1);
  assert.equal(before.current.id, 'S1');
  assert.equal(before.previous, null);
  const after = resolveSeasonFrame({ current: SEASON, next }, END_MS);
  assert.equal(after.current.id, 'S2');
  assert.equal(after.previous.id, 'S1');
  // No next season configured: current stays even past its end.
  const alone = resolveSeasonFrame({ current: SEASON }, END_MS + DAY_MS);
  assert.equal(alone.current.id, 'S1');
  assert.equal(alone.previous, null);
});

test('divisionTop3 partitions by rating and keeps 3 per division', () => {
  const roster = [];
  for (let i = 0; i < 12; i += 1) {
    roster.push(entry({ slug: `a/m${String(i).padStart(2, '0')}`, rating: 100 - i }));
  }
  const divisions = divisionTop3(roster);
  assert.equal(divisions.length, 2);
  assert.deepEqual(divisions.map((d) => d.name), ['premier', 'challenger']);
  assert.deepEqual(divisions[0].models.map((m) => m.slug), ['a/m00', 'a/m01', 'a/m02']);
  assert.deepEqual(divisions[1].models.map((m) => m.slug), ['a/m10', 'a/m11'], 'partial division keeps what it has');
});

test('seasonRetirements filters to the season window across tracks, oldest first', () => {
  const inS1 = entry({ slug: 'a/gone', retired_at: '2026-09-10T00:00:00.000Z', reason: 'rating < 35' });
  const early = entry({ slug: 'a/early', retired_at: '2026-08-30T00:00:00.000Z', reason: 'rating < 35' });
  const preSeason = entry({ slug: 'a/old', retired_at: '2026-08-20T00:00:00.000Z', reason: 'x' });
  const atEnd = entry({ slug: 'a/late', retired_at: '2026-09-21T00:00:00.000Z', reason: 'x' }); // == ends_at: excluded
  const state = stateWith([], {
    l0Retired: [inS1, preSeason],
    otherRetired: { L1: [early, atEnd] },
  });
  const found = seasonRetirements(state, { startedMs: START_MS, endsMs: END_MS });
  assert.deepEqual(found.map((r) => `${r.track}:${r.entry.slug}`), ['L1:a/early', 'L0:a/gone']);
});

test('seasonNumbers counts fights, debuts and retirements inside the window', () => {
  const roster = [
    entry({ slug: 'a/x', ...wld(6, 2, 2), joined_at: '2026-08-24T01:00:00.000Z' }), // 10 matches
    entry({ slug: 'a/y', ...wld(2, 2, 0), joined_at: '2026-07-01T00:00:00.000Z' }), // pre-season debut
  ];
  const retired = [
    entry({ slug: 'a/z', ...wld(1, 1, 0), retired_at: '2026-09-01T00:00:00.000Z' }), // 2 matches
    entry({ slug: 'a/w', ...wld(4, 4, 0), joined_at: '2026-07-15T00:00:00.000Z', retired_at: '2026-10-01T00:00:00.000Z' }), // outside the season
  ];
  const numbers = seasonNumbers(
    { roster, retired },
    { startedMs: START_MS, endsMs: END_MS, day: 28 },
  );
  // (10 + 4 + 2 + 8) / 2 = 12 fights; debuts: a/x + a/z; retirements: a/z.
  assert.deepEqual(numbers, { totalFights: 12, modelsDebuted: 2, modelsRetired: 1, days: 28 });
});

test('computeSeasonView projects the champion from the live L0 roster', () => {
  const state = stateWith([
    entry({ slug: 'a/leader', rating: 66, ...wld(8, 2, 1) }),
    entry({ slug: 'a/chaser', rating: 65.5, ...wld(9, 1, 0) }),
  ]);
  const view = computeSeasonView({
    season: SEASON, state, nowMs: Date.parse('2026-09-19T12:00:00.000Z'),
  });
  assert.equal(view.window.phase, 'countdown');
  assert.equal(view.window.daysRemaining, 2);
  assert.equal(view.projected.slug, 'a/leader');
  assert.equal(view.divisions[0].models.length, 2);
  assert.equal(view.numbers.days, 27, '2026-09-19 is 26.5 days in -> day 27');
});

test('freezeRecord captures champion, divisions, retirements and numbers', () => {
  const retiredInSeason = entry({
    slug: 'a/gone', rating: 30, ...wld(2, 9, 1),
    retired_at: '2026-09-10T00:00:00.000Z', reason: 'rating 30 < 35',
  });
  const state = stateWith(
    [entry({ slug: 'a/leader', rating: 66, ...wld(8, 2, 1), days_in_league: 27 })],
    { l0Retired: [retiredInSeason] },
  );
  const frozenAt = Date.parse('2026-09-21T06:00:00.000Z');
  const record = freezeRecord({ season: SEASON, state, nowMs: frozenAt });

  assert.equal(record.version, FINALE_SCHEMA_VERSION);
  assert.equal(record.season.id, 'S1');
  assert.equal(record.season.ends_at, '2026-09-21T00:00:00.000Z');
  assert.equal(record.frozen_at, '2026-09-21T06:00:00.000Z');
  assert.equal(record.champion.slug, 'a/leader');
  assert.equal(record.champion.rating, 66);
  assert.equal(record.champion.matches, 11);
  assert.ok(Math.abs(record.champion.win_rate - 8 / 11) < 1e-12);
  assert.equal(record.divisions[0].name, 'premier');
  assert.equal(record.retirements.length, 1);
  assert.equal(record.retirements[0].track, 'L0');
  assert.equal(record.retirements[0].reason, 'rating 30 < 35');
  assert.equal(record.numbers.days, 28, 'an ended season reports its full length');
  assert.equal(validateFinaleRecord(record, 'S1'), record, 'a fresh record validates');
});

test('freezeRecord returns null with no L0 roster to crown', () => {
  assert.equal(freezeRecord({ season: SEASON, state: stateWith([]), nowMs: END_MS }), null);
});

test('a frozen record is immune to later leader changes (frozen vs projected)', () => {
  const state = stateWith([
    entry({ slug: 'a/leader', rating: 66, ...wld(8, 2, 1) }),
    entry({ slug: 'a/chaser', rating: 60, ...wld(9, 1, 0) }),
  ]);
  const record = freezeRecord({ season: SEASON, state, nowMs: END_MS });
  assert.equal(record.champion.slug, 'a/leader');

  // The league plays on: the chaser overtakes. The frozen record must not
  // move; only a fresh projection reflects the new leader.
  state.tracks.L0.roster[0].rating = 55;
  state.tracks.L0.roster[1].rating = 70;
  assert.equal(record.champion.slug, 'a/leader');
  assert.equal(record.champion.rating, 66);
  const view = computeSeasonView({ season: SEASON, state, nowMs: END_MS + DAY_MS });
  assert.equal(view.projected.slug, 'a/chaser', 'projection follows live state');
  // Re-freezing from the mutated state would crown the chaser — which is why
  // the first computation after ends_at is persisted and never recomputed.
  assert.equal(freezeRecord({ season: SEASON, state, nowMs: END_MS + DAY_MS }).champion.slug, 'a/chaser');
});

test('validateFinaleRecord rejects malformed records', () => {
  const state = stateWith([entry({ slug: 'a/leader', rating: 66, ...wld(8, 2, 1) })]);
  const good = freezeRecord({ season: SEASON, state, nowMs: END_MS });
  assert.equal(validateFinaleRecord(good, 'S1'), good);
  assert.throws(() => validateFinaleRecord(good, 'S2'), /invalid season finale record/);
  assert.throws(() => validateFinaleRecord({ ...good, version: 2 }, 'S1'));
  assert.throws(() => validateFinaleRecord({ ...good, champion: null }, 'S1'));
  assert.throws(() => validateFinaleRecord({ ...good, divisions: [{ name: 'premier' }] }, 'S1'));
  assert.throws(() => validateFinaleRecord({ ...good, numbers: { total_fights: 1.5 } }, 'S1'));
  assert.throws(() => validateFinaleRecord(null, 'S1'));
});
