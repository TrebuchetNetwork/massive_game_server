import { test } from 'node:test';
import assert from 'node:assert/strict';

import { mascotFor } from '../../mascots.mjs';
import {
  FEEDBACK_INTERVAL_MS,
  MAX_SUBMISSIONS,
  RETIRE_COOLDOWN_MS,
  applyBattleRatings,
  eligibleChallengers,
  feedbackDue,
  nextVersion,
  shouldRetire,
  trackPolicy,
  winRate,
} from '../league.mjs';

const NOW = Date.parse('2026-08-23T00:00:00.000Z');
const DAY_MS = 24 * 60 * 60 * 1000;

const sha = (ch) => ch.repeat(64);

function model(overrides = {}) {
  const modelId = overrides.model_id || 'vendor/model-a';
  return {
    model_id: modelId,
    slug: `${modelId}-20260801`,
    mascot: mascotFor(modelId),
    joined_at: new Date(NOW - 5 * DAY_MS).toISOString(),
    submissions_used: MAX_SUBMISSIONS,
    artifact: {
      wasm_sha256: sha('a'),
      source_sha256: sha('b'),
      prompt_sha256: sha('c'),
      version: 1,
      parent_version: null,
    },
    rating: 50,
    wins: 5,
    losses: 5,
    draws: 0,
    matches: 10,
    days_in_league: 5,
    status: 'active',
    ...overrides,
  };
}

function stateWith(overrides = {}) {
  return {
    schema_version: 1,
    league_id: 'cml-test',
    day_index: 5,
    roster: [],
    retired: [],
    announcements: [],
    last_feedback_at: null,
    created_at: '2026-08-18T00:00:00.000Z',
    updated_at: '2026-08-23T00:00:00.000Z',
    ...overrides,
  };
}

test('winRate is wins/matches and 0 with no matches', () => {
  assert.equal(winRate(model({ wins: 3, matches: 12 })), 0.25);
  assert.equal(winRate(model({ wins: 0, matches: 0 })), 0);
});

test('shouldRetire requires tenure, exhausted submissions, and a failed bar', () => {
  // All conditions met, low rating → retire.
  assert.equal(shouldRetire(model({ rating: 34.99 }), NOW), true);
  // All conditions met, rating fine but win rate below 25% → retire.
  assert.equal(shouldRetire(model({ rating: 80, wins: 2, losses: 8 }), NOW), true);
  // Win rate exactly 0.25 is NOT below the bar, and rating is fine → stay.
  assert.equal(shouldRetire(model({ rating: 50, wins: 3, losses: 9, matches: 12 }), NOW), false);
  // Rating exactly 35 is NOT below the bar → stay.
  assert.equal(shouldRetire(model({ rating: 35, wins: 5, losses: 5 }), NOW), false);
  // Too young → stay, even with a terrible record.
  assert.equal(shouldRetire(model({ days_in_league: 2, rating: 0, wins: 0, losses: 10 }), NOW), false);
  // Submissions left → stay.
  assert.equal(shouldRetire(model({ submissions_used: 2, rating: 0 }), NOW), false);
  // Good record → stay.
  assert.equal(shouldRetire(model(), NOW), false);
  // Zero matches → win rate 0 → below the bar when the rest holds.
  assert.equal(shouldRetire(model({ rating: 60, wins: 0, losses: 0, matches: 0 }), NOW), true);
});

test('feedbackDue: null is due, 48h gate otherwise', () => {
  assert.equal(feedbackDue(stateWith(), NOW), true);
  const recent = new Date(NOW - (FEEDBACK_INTERVAL_MS - 1000)).toISOString();
  assert.equal(feedbackDue(stateWith({ last_feedback_at: recent }), NOW), false);
  const exact = new Date(NOW - FEEDBACK_INTERVAL_MS).toISOString();
  assert.equal(feedbackDue(stateWith({ last_feedback_at: exact }), NOW), true);
  const stale = new Date(NOW - 10 * FEEDBACK_INTERVAL_MS).toISOString();
  assert.equal(feedbackDue(stateWith({ last_feedback_at: stale }), NOW), true);
});

test('eligibleChallengers drops roster models and recently retired ones', () => {
  const ranking = [
    { id: 'vendor/on-roster', canonical_slug: 'vendor/on-roster-20260801' },
    { id: 'vendor/retired-recent', canonical_slug: 'vendor/retired-recent-20260801' },
    { id: 'vendor/retired-long-ago', canonical_slug: 'vendor/retired-long-ago-20260801' },
    { id: 'vendor/fresh', canonical_slug: 'vendor/fresh-20260801' },
  ];
  const state = stateWith({
    roster: [model({ model_id: 'vendor/on-roster' })],
    retired: [
      {
        ...model({ model_id: 'vendor/retired-recent' }),
        retired_at: new Date(NOW - (RETIRE_COOLDOWN_MS - DAY_MS)).toISOString(),
        reason: 'rating_below_bar',
      },
      {
        ...model({ model_id: 'vendor/retired-long-ago' }),
        retired_at: new Date(NOW - RETIRE_COOLDOWN_MS).toISOString(),
        reason: 'rating_below_bar',
      },
    ],
  });
  const eligible = eligibleChallengers(ranking, state, NOW);
  assert.deepEqual(
    eligible.map((entry) => entry.id),
    ['vendor/retired-long-ago', 'vendor/fresh'],
  );
});

test('eligibleChallengers matches canonical slugs and preserves ranking order', () => {
  const ranking = [
    { id: 'vendor/new-a', canonical_slug: 'vendor/shared-slug' },
    { id: 'vendor/new-b', canonical_slug: 'vendor/other-slug' },
  ];
  const state = stateWith({
    roster: [model({ model_id: 'vendor/entrant-xyz', slug: 'vendor/shared-slug' })],
  });
  const eligible = eligibleChallengers(ranking, state, NOW);
  assert.deepEqual(eligible.map((entry) => entry.id), ['vendor/new-b']);
});

test('applyBattleRatings accumulates W/L/D and recomputes the rating', () => {
  const roster = [
    model({ model_id: 'vendor/model-a', rating: 50, wins: 0, losses: 0, draws: 0, matches: 0 }),
    model({ model_id: 'vendor/model-b', rating: 50, wins: 0, losses: 0, draws: 0, matches: 0 }),
  ];
  const season = {
    roster: [
      { model_id: 'vendor/model-a', wins: 7, losses: 2, draws: 1, matches_played: 10 },
      { model_id: 'vendor/model-b', wins: 2, losses: 7, draws: 1, matches_played: 10 },
    ],
  };
  const updated = applyBattleRatings(roster, season);
  // rating = 100 * (wins + 0.5 * draws) / matches
  assert.equal(updated[0].rating, 75);
  assert.equal(updated[1].rating, 25);
  assert.deepEqual(
    updated.map((entry) => [entry.wins, entry.losses, entry.draws, entry.matches]),
    [[7, 2, 1, 10], [2, 7, 1, 10]],
  );
  // Original roster untouched (pure function).
  assert.equal(roster[0].matches, 0);

  // Second season accumulates onto the first.
  const again = applyBattleRatings(updated, season);
  assert.equal(again[0].rating, 75);
  assert.equal(again[0].matches, 20);
  assert.equal(again[1].wins, 4);
});

test('applyBattleRatings is monotonic in wins', () => {
  const build = (wins) => applyBattleRatings(
    [model({ wins: 0, losses: 0, draws: 0, matches: 0 })],
    { roster: [{ model_id: 'vendor/model-a', wins, losses: 10 - wins, draws: 0, matches_played: 10 }] },
  )[0].rating;
  const ratings = Array.from({ length: 11 }, (_, wins) => build(wins));
  for (let index = 1; index < ratings.length; index += 1) {
    assert.ok(ratings[index] > ratings[index - 1], `${ratings}`);
  }
  assert.equal(ratings[0], 0);
  assert.equal(ratings[10], 100);
});

test('applyBattleRatings rejects a season missing a roster model', () => {
  const roster = [model({ model_id: 'vendor/model-a' })];
  assert.throws(
    () => applyBattleRatings(roster, { roster: [{ model_id: 'vendor/other', wins: 1, losses: 0, draws: 0, matches_played: 1 }] }),
    /missing roster model/,
  );
});

test('applyBattleRatings matches season entries via provider_model', () => {
  const roster = [model({ model_id: 'vendor/model-a', wins: 0, losses: 0, draws: 0, matches: 0 })];
  const season = {
    roster: [{
      model_id: 'orw-20260823-deadbeef-vendor-model-a',
      provider_model: 'vendor/model-a',
      wins: 10,
      losses: 0,
      draws: 0,
      matches_played: 10,
    }],
  };
  const updated = applyBattleRatings(roster, season);
  assert.equal(updated[0].rating, 100);
  assert.equal(updated[0].wins, 10);
});

test('nextVersion increments the version and links the parent', () => {
  const entry = model();
  const artifact = nextVersion(entry);
  assert.equal(artifact.version, 2);
  assert.equal(artifact.parent_version, 1);
  const third = nextVersion({ artifact });
  assert.equal(third.version, 3);
  assert.equal(third.parent_version, 2);
  // Input untouched.
  assert.equal(entry.artifact.version, 1);
  assert.throws(() => nextVersion({ artifact: { version: 0 } }), Error);
});

// --- Multi-track policies (amendment 2026-08-24) -----------------------------

test('trackPolicy returns the exact track configs', () => {
  assert.deepEqual(trackPolicy('L0'), {
    maxSubmissions: 1, compileAttempts: 1, feedbackIntervalMs: null, maxRevisions: 0,
  });
  assert.deepEqual(trackPolicy('L1'), {
    maxSubmissions: 1, compileAttempts: 3, feedbackIntervalMs: null, maxRevisions: 0,
  });
  assert.deepEqual(trackPolicy('L2'), {
    maxSubmissions: 3, compileAttempts: 3, feedbackIntervalMs: FEEDBACK_INTERVAL_MS, maxRevisions: 2,
  });
  assert.deepEqual(trackPolicy('L3'), {
    maxSubmissions: 9, compileAttempts: 3, feedbackIntervalMs: 7 * 24 * 60 * 60 * 1000,
    maxRevisions: 8,
  });
  assert.throws(() => trackPolicy('L9'), /unknown league track/);
});

test('L0/L1 exhaust submissions at entry, so their bar is rating-based after day 3', () => {
  const weak = model({
    submissions_used: 1,
    rating: 20,
    wins: 1,
    losses: 9,
    days_in_league: 4,
  });
  assert.equal(shouldRetire(weak, NOW, trackPolicy('L0')), true);
  assert.equal(shouldRetire(weak, NOW, trackPolicy('L1')), true);
  // Same model under L2 still has submissions left: not retired.
  assert.equal(shouldRetire(weak, NOW, trackPolicy('L2')), false);
  // A healthy L0 model is never retired regardless of policy.
  assert.equal(shouldRetire(model({ submissions_used: 1, rating: 60 }), NOW, trackPolicy('L0')), false);
});

test('feedback cadence per track policy', () => {
  const dayAgo = { last_feedback_at: new Date(NOW - 24 * 60 * 60 * 1000).toISOString() };
  const threeDaysAgo = { last_feedback_at: new Date(NOW - 3 * 24 * 60 * 60 * 1000).toISOString() };
  const eightDaysAgo = { last_feedback_at: new Date(NOW - 8 * 24 * 60 * 60 * 1000).toISOString() };
  // L0/L1 never revise.
  assert.equal(feedbackDue({ last_feedback_at: null }, NOW, trackPolicy('L0')), false);
  assert.equal(feedbackDue(threeDaysAgo, NOW, trackPolicy('L1')), false);
  // L2: 48h.
  assert.equal(feedbackDue(dayAgo, NOW, trackPolicy('L2')), false);
  assert.equal(feedbackDue(threeDaysAgo, NOW, trackPolicy('L2')), true);
  // L3: 7 days.
  assert.equal(feedbackDue(threeDaysAgo, NOW, trackPolicy('L3')), false);
  assert.equal(feedbackDue(eightDaysAgo, NOW, trackPolicy('L3')), true);
});

test('eligibleChallengers skips models whose generation failed within 7 days', () => {
  const state = stateWith();
  const ranking = [
    { id: 'vendor/failed-recently', canonical_slug: 'vendor/failed-recently-20260101' },
    { id: 'vendor/failed-long-ago', canonical_slug: 'vendor/failed-long-ago-20260101' },
    { id: 'vendor/never-failed', canonical_slug: 'vendor/never-failed-20260101' },
    { id: 'vendor/failed-by-slug', canonical_slug: 'vendor/slug-only-20260101' },
  ];
  const failures = {
    'vendor/failed-recently': new Date(NOW - 2 * DAY_MS).toISOString(),
    'vendor/failed-long-ago': new Date(NOW - 8 * DAY_MS).toISOString(),
    'vendor/slug-only-20260101': new Date(NOW - 1 * DAY_MS).toISOString(),
  };
  const eligible = eligibleChallengers(ranking, state, NOW, failures);
  assert.deepEqual(
    eligible.map((entry) => entry.id),
    ['vendor/failed-long-ago', 'vendor/never-failed'],
  );
  // Without the ledger nothing is cooldown-excluded.
  assert.equal(eligibleChallengers(ranking, state, NOW).length, 4);
  assert.equal(eligibleChallengers(ranking, state, NOW, {}).length, 4);
});
