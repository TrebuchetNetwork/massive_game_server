import assert from 'node:assert/strict';
import test from 'node:test';
import {
  addWorldRatings,
  assertBattleIntegrity,
  buildSeasonRatings,
} from './season_scoring.mjs';

const entrants = [
  {
    provider_rank: 1,
    model_id: 'model_a',
    model_name: 'Model A',
    provider_model: 'provider/model-a',
    compiled: true,
    simulated: false,
    source_bytes: 1200,
    source_sha256: 'a'.repeat(64),
    wasm_bytes: 2000,
    wasm_sha256: 'c'.repeat(64),
    compile_attempts: 1,
  },
  {
    provider_rank: 2,
    model_id: 'model_b',
    model_name: 'Model B',
    provider_model: 'provider/model-b',
    compiled: true,
    simulated: false,
    source_bytes: 1100,
    source_sha256: 'b'.repeat(64),
    wasm_bytes: 1900,
    wasm_sha256: 'd'.repeat(64),
    compile_attempts: 1,
  },
];

const simulation = (overrides = {}) => ({
  winner_model_id: 'model_a',
  draw: false,
  total_engagements: 1,
  total_team_a_score: 10,
  total_team_b_score: 10,
  total_team_a_objective: 4,
  total_team_b_objective: 1,
  total_team_a_collaboration_score: 8,
  total_team_b_collaboration_score: 2,
  team_a_v2_fighters: 1,
  team_b_v2_fighters: 1,
  fallback_count: 0,
  trap_count: 0,
  invalid_action_count: 0,
  fuel_error_count: 0,
  warnings: [],
  ...overrides,
});

test('rates personal, team, and direct collaboration telemetry independently', () => {
  const roster = buildSeasonRatings({
    entrants,
    legs: [
      {
        category: 'personal',
        mode: 'arena',
        model_a_id: 'model_a',
        model_b_id: 'model_b',
        simulation: simulation(),
      },
      {
        category: 'team',
        mode: 'ctf',
        model_a_id: 'model_a',
        model_b_id: 'model_b',
        simulation: simulation(),
      },
    ],
  });

  assert.equal(roster[0].model_id, 'model_a');
  assert.equal(roster[0].personal_rating, 85);
  assert.equal(roster[0].team_rating, 93);
  assert.equal(roster[0].collaboration_rating, 85);
  assert.equal(roster[0].overall_rating, 87.8);
  assert.equal(roster[0].personal_score_for, 10);
  assert.equal(roster[0].team_objective_for, 4);
  assert.equal(roster[0].collaboration_score_for, 8);
  assert.equal(roster[0].wasm_sha256, 'c'.repeat(64));
  assert.equal(roster[1].personal_rating, 15);
  assert.equal(roster[1].team_rating, 7);
  assert.equal(roster[1].collaboration_rating, 15);
  assert.equal(roster[1].overall_rating, 12.2);
});

test('maps side-swapped results back to model identity', () => {
  const roster = buildSeasonRatings({
    entrants,
    legs: [
      {
        category: 'personal',
        mode: 'arena',
        model_a_id: 'model_b',
        model_b_id: 'model_a',
        simulation: simulation({
          winner_model_id: 'model_a',
          total_team_a_score: 2,
          total_team_b_score: 8,
        }),
      },
      {
        category: 'team',
        mode: 'tdm',
        model_a_id: 'model_b',
        model_b_id: 'model_a',
        simulation: simulation({
          winner_model_id: 'model_a',
          total_team_a_objective: 1,
          total_team_b_objective: 4,
          total_team_a_collaboration_score: 2,
          total_team_b_collaboration_score: 8,
        }),
      },
    ],
  });

  assert.equal(roster[0].model_id, 'model_a');
  assert.ok(roster[0].overall_rating > roster[1].overall_rating);
});

test('world placement can change the strategy leader without rewriting duel ratings', () => {
  const roster = [
    {
      ...entrants[0],
      rank: 1,
      overall_rating: 90,
      collaboration_rating: 80,
    },
    {
      ...entrants[1],
      rank: 2,
      overall_rating: 80,
      collaboration_rating: 80,
    },
  ];
  const result = addWorldRatings(roster, [{
    simulation: {
      rankings: [
        { model_id: 'model_a', points: 220, round_wins: 0, eliminations: 1, deaths: 2, collaboration_score: 3 },
        { model_id: 'model_b', points: 1000, round_wins: 1, eliminations: 4, deaths: 0, collaboration_score: 8 },
      ],
    },
  }]);

  assert.equal(result[0].model_id, 'model_b');
  assert.equal(result[0].world_rating, 100);
  assert.equal(result[0].strategy_rating, 85);
  assert.equal(result[1].overall_rating, 90);
  assert.equal(result[1].world_rating, 22);
  assert.equal(result[1].strategy_rating, 73);
});

test('refuses to invent a collaboration score when telemetry is absent', () => {
  assert.throws(
    () => buildSeasonRatings({
      entrants,
      legs: [
        {
          category: 'personal',
          mode: 'arena',
          model_a_id: 'model_a',
          model_b_id: 'model_b',
          simulation: simulation(),
        },
        {
          category: 'team',
          mode: 'tdm',
          model_a_id: 'model_a',
          model_b_id: 'model_b',
          simulation: {
            ...simulation(),
            total_team_a_collaboration_score: undefined,
            total_team_b_collaboration_score: undefined,
          },
        },
      ],
    }),
    /collaboration telemetry missing/,
  );
});

test('integrity checks reject fallback runtimes and wrong engagement counts', () => {
  assert.equal(assertBattleIntegrity(simulation(), {
    expectedEngagements: 1,
    requireCollaboration: true,
  }), true);

  assert.throws(() => assertBattleIntegrity(simulation({
    warnings: ['round=1 slot=1: wasm not found; fallback runtime used'],
  }), {
    expectedEngagements: 1,
    requireCollaboration: true,
  }), /unverified fighter runtime/);

  assert.throws(() => assertBattleIntegrity(simulation({ total_engagements: 10 }), {
    expectedEngagements: 1,
    requireCollaboration: true,
  }), /unexpected engagement count/);

  assert.throws(() => assertBattleIntegrity(simulation({ team_b_v2_fighters: 0 }), {
    expectedEngagements: 1,
    requireCollaboration: true,
  }), /v2 fighter integrity failed/);

  assert.throws(() => assertBattleIntegrity(simulation({ trap_count: 1 }), {
    expectedEngagements: 1,
    requireCollaboration: true,
  }), /trap_count=1/);

  assert.equal(assertBattleIntegrity(simulation({
    total_engagements: 3,
    team_a_v2_fighters: 1,
    team_b_v2_fighters: 1,
  }), {
    expectedEngagements: 3,
    expectedV2Fighters: 1,
    requireCollaboration: true,
  }), true);
});
