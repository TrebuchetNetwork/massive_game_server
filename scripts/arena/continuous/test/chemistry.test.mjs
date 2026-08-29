// Tests for continuous/chemistry.mjs — schedule coverage, aggregation math,
// sim attribution validation, and the evaluation runner with injected IO.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  aggregateChemistry,
  allPairs,
  buildChemistryArtifact,
  buildChemistrySchedule,
  eloFromRating,
  expectedSquadWinRate,
  MIXED_BATTLE_ROUTE,
  pairKey,
  resolveServerModelIds,
  runChemistryEvaluation,
  selectChemistryModels,
  summarizeMixedMatch,
  validateMixedSimulation,
} from '../chemistry.mjs';

const TEN = Array.from({ length: 10 }, (_, i) => `model_${i}`);

test('allPairs enumerates C(n,2) pairs in canonical order', () => {
  const pairs = allPairs(TEN);
  assert.equal(pairs.length, 45);
  assert.equal(pairKey('b', 'a'), 'a|b');
  assert.deepEqual(pairs[0], ['model_0', 'model_1']);
  const keys = new Set(pairs.map(([a, b]) => pairKey(a, b)));
  assert.equal(keys.size, 45);
});

test('schedule covers all 45 pairs at least K times (K = 1, 2, 3)', () => {
  for (const k of [1, 2, 3]) {
    const schedule = buildChemistrySchedule(TEN, { k, seed: 1 });
    assert.equal(schedule.complete, true, `k=${k} schedule completes`);
    assert.equal(schedule.squad_size, 5);
    const coverage = Object.entries(schedule.coverage);
    assert.equal(coverage.length, 45, 'every model pair tracked');
    for (const [key, count] of coverage) {
      assert.ok(count >= k, `${key} covered ${count} >= ${k}`);
    }
    // Bounded cost: never more matches than the pair budget implies.
    assert.ok(schedule.matches.length <= 45 * k / 10 + 6, 'schedule stays bounded');
  }
});

test('schedule is deterministic for a fixed seed and varies partners/opponents', () => {
  const first = buildChemistrySchedule(TEN, { k: 2, seed: 7 });
  const second = buildChemistrySchedule(TEN, { k: 2, seed: 7 });
  assert.deepEqual(first, second);

  // model_0 partners with several distinct teammates and meets varied opponents.
  const teammates = new Set();
  const opponents = new Set();
  for (const match of first.matches) {
    const sides = [match.team_a_models, match.team_b_models];
    const sideIndex = sides.findIndex((squad) => squad.includes('model_0'));
    assert.notEqual(sideIndex, -1, 'model_0 plays every scheduled match it appears in');
    if (sideIndex === -1) continue;
    for (const id of sides[sideIndex]) if (id !== 'model_0') teammates.add(id);
    for (const id of sides[1 - sideIndex]) opponents.add(id);
  }
  assert.ok(teammates.size >= 3, `partners vary (got ${teammates.size})`);
  assert.ok(opponents.size >= 4, `opponents vary (got ${opponents.size})`);

  // Match seeds are deterministic, distinct, and JSON-safe integers.
  const seeds = first.matches.map((match) => match.seed);
  assert.equal(new Set(seeds).size, seeds.length);
  for (const seed of seeds) assert.ok(Number.isSafeInteger(seed) && seed >= 0);
});

test('schedule rejects invalid configurations', () => {
  assert.throws(() => buildChemistrySchedule(['a', 'b', 'c'], {}), /at least 4 models/);
  assert.throws(() => buildChemistrySchedule(TEN, { squadSize: 6 }), /too large/);
  assert.throws(() => buildChemistrySchedule(TEN, { squadSize: 1 }), />= 2/);
  assert.throws(() => buildChemistrySchedule(TEN, { k: 0 }), /positive integer/);
});

test('eloFromRating maps the 0-100 league scale onto an Elo-like scale', () => {
  assert.equal(eloFromRating(50), 0);
  assert.equal(eloFromRating(100), 400);
  assert.equal(eloFromRating(0), -400);
  assert.equal(eloFromRating(undefined), 0, 'missing rating defaults to 50');
  assert.equal(expectedSquadWinRate([50], [50]), 0.5);
  const better = expectedSquadWinRate([60, 50], [40, 50]);
  assert.ok(better > 0.5 && better < 1);
  assert.ok(Math.abs(better + expectedSquadWinRate([40, 50], [60, 50]) - 1) < 1e-12);
});

test('aggregation computes per-pair win rates and delta vs expected', () => {
  // Hand-computed fixture (see module docs): ratings m1=60, m2=50, m3=40, m4=50.
  const ratings = { m1: 60, m2: 50, m3: 40, m4: 50 };
  const matches = [
    { team_a_models: ['m1', 'm2'], team_b_models: ['m3', 'm4'], winner_side: 'team_a', draw: false },
    { team_a_models: ['m1', 'm3'], team_b_models: ['m2', 'm4'], winner_side: 'team_b', draw: false },
    { team_a_models: ['m1', 'm4'], team_b_models: ['m2', 'm3'], winner_side: null, draw: true },
  ];
  const { pairs } = aggregateChemistry({ matches, ratings });
  assert.equal(pairs.length, 6);
  const byKey = new Map(pairs.map((pair) => [pair.models.join('|'), pair]));

  const e1 = 1 / (1 + 10 ** (-80 / 400)); // squad (60,50) vs (40,50): mean Elo diff 80
  const m1m2 = byKey.get('m1|m2');
  assert.equal(m1m2.games_together, 1);
  assert.deepEqual([m1m2.wins, m1m2.draws, m1m2.losses], [1, 0, 0]);
  assert.equal(m1m2.win_rate, 1);
  assert.equal(m1m2.expected_win_rate, Math.round(e1 * 10_000) / 10_000);
  assert.equal(m1m2.rating_delta_vs_expected, Math.round((1 - e1) * 10_000) / 10_000);
  assert.equal(m1m2.provisional, true, 'single game is provisional');

  const m3m4 = byKey.get('m3|m4');
  assert.equal(m3m4.win_rate, 0);
  assert.equal(m3m4.rating_delta_vs_expected, Math.round((0 - (1 - e1)) * 10_000) / 10_000);

  const m1m3 = byKey.get('m1|m3');
  assert.equal(m1m3.win_rate, 0, 'm1+m3 lost from team_a');
  assert.equal(m1m3.expected_win_rate, 0.5, 'mirror-strength squads expect 0.5');
  assert.equal(m1m3.rating_delta_vs_expected, -0.5);

  const m1m4 = byKey.get('m1|m4');
  assert.equal(m1m4.win_rate, 0.5, 'draws count as half a win');
  assert.equal(m1m4.rating_delta_vs_expected, Math.round((0.5 - e1) * 10_000) / 10_000);

  // Pairs with enough games lose the provisional flag.
  const seasoned = aggregateChemistry({
    matches: [...matches, ...matches, ...matches],
    ratings,
  });
  assert.equal(seasoned.pairs.find((pair) => pair.models.join('|') === 'm1|m2').provisional, false);
});

function fakeSimulation(body) {
  const roster = [
    ...body.team_a_models.map((model_id, slot) => ({ side: 'team_a', slot, model_id })),
    ...body.team_b_models.map((model_id, slot) => ({ side: 'team_b', slot, model_id })),
  ];
  return {
    simulation: {
      mode: 'mixed_team',
      match_mode: body.mode,
      rules_version: 'test-rules',
      seed: body.seed,
      team_a_models: body.team_a_models,
      team_b_models: body.team_b_models,
      team_size: body.team_a_models.length,
      rounds: body.rounds,
      max_ticks: body.max_ticks,
      winner_side: 'team_a',
      draw: false,
      total_team_a_objective: 3,
      total_team_b_objective: 1,
      total_team_a_score: 100,
      total_team_b_score: 40,
      fighters: roster.map((fighter) => ({
        ...fighter,
        runtime: 'wasm_v2',
        eliminations: 1,
        deaths: 0,
        personal_score: 10,
        collaboration_score: 2,
      })),
    },
  };
}

test('validateMixedSimulation accepts a well-formed attributed response', () => {
  const match = {
    match_id: 'chem-001',
    mode: 'tdm',
    rounds: 1,
    seed: 42,
    team_a_models: ['a', 'b'],
    team_b_models: ['c', 'd'],
  };
  const { simulation } = fakeSimulation({ ...match, ...{ max_ticks: 240 } });
  assert.equal(validateMixedSimulation(simulation, match), simulation);

  const summary = summarizeMixedMatch(match, simulation);
  assert.equal(summary.winner_side, 'team_a');
  assert.equal(summary.fighters.length, 4);
  assert.deepEqual(summary.fighters.map((f) => f.model_id), ['a', 'b', 'c', 'd']);
});

test('validateMixedSimulation rejects misattributed fighters and bad envelopes', () => {
  const match = {
    match_id: 'chem-001',
    mode: 'tdm',
    rounds: 1,
    seed: 42,
    team_a_models: ['a', 'b'],
    team_b_models: ['c', 'd'],
  };
  const good = () => fakeSimulation({ ...match, max_ticks: 240 }).simulation;

  const swapped = good();
  swapped.fighters[0].model_id = 'b';
  assert.throws(() => validateMixedSimulation(swapped, match), /attribution mismatch/);

  const missing = good();
  missing.fighters.pop();
  assert.throws(() => validateMixedSimulation(missing, match), /missing per-fighter attribution/);

  const dupSlot = good();
  dupSlot.fighters[1].slot = 0;
  assert.throws(() => validateMixedSimulation(dupSlot, match), /invalid fighter entry/);

  const badDraw = good();
  badDraw.draw = true;
  assert.throws(() => validateMixedSimulation(badDraw, match), /inconsistent winner\/draw/);

  const badMode = good();
  badMode.mode = 'team_battle';
  assert.throws(() => validateMixedSimulation(badMode, match), /frozen request/);

  const badRoster = good();
  badRoster.team_a_models = ['a', 'zz'];
  assert.throws(() => validateMixedSimulation(badRoster, match), /frozen request/);
});

test('runChemistryEvaluation drives the mixed route and aggregates all pairs', async () => {
  const calls = [];
  const models = TEN.map((model_id, i) => ({ model_id, rating: 40 + i * 2 }));
  const evaluation = await runChemistryEvaluation({
    models,
    k: 1,
    seed: 3,
    mode: 'tdm',
    rounds: 1,
    maxTicks: 120,
    apiJson: async (request) => {
      calls.push(request);
      return fakeSimulation(request.body);
    },
  });
  assert.equal(calls.length, evaluation.schedule.matches.length);
  for (const call of calls) {
    assert.equal(call.method, 'POST');
    assert.equal(call.route, MIXED_BATTLE_ROUTE);
    assert.equal(call.body.mode, 'tdm');
    assert.equal(call.body.max_ticks, 120);
  }
  assert.equal(evaluation.aggregation.pairs.length, 45);
  // Team A always wins in the fake sim, so every pair is decided, not drawn.
  assert.ok(evaluation.aggregation.pairs.every((pair) => pair.draws === 0));

  const artifact = buildChemistryArtifact({
    generatedAt: '2026-08-29T00:00:00.000Z',
    leagueId: 'cml-test',
    track: 'L2',
    models,
    k: 1,
    seed: 3,
    mode: 'tdm',
    rounds: 1,
    maxTicks: 120,
    evaluation,
  });
  assert.equal(artifact.schema_version, 1);
  assert.equal(artifact.kind, 'mixed_team_chemistry');
  assert.equal(artifact.matches.length, calls.length);
  assert.equal(artifact.pairs.length, 45);
  assert.equal(artifact.schedule_complete, true);
  assert.ok(Object.keys(artifact.coverage).length === 45);
});

test('selectChemistryModels picks the top-N active roster entries by rating', () => {
  const roster = [
    { model_id: 'low', rating: 30, status: 'active' },
    { model_id: 'high', rating: 70, status: 'active' },
    { model_id: 'retired', rating: 99, status: 'retired' },
    { model_id: 'mid', rating: 50 },
  ];
  assert.deepEqual(
    selectChemistryModels(roster, 2).map((entry) => entry.model_id),
    ['high', 'mid'],
  );
  assert.equal(selectChemistryModels(roster, 10).length, 3);
});

test('resolveServerModelIds binds league ids to the latest matching day season', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'chem-resolve-'));
  const seasonDir = (day) => path.join(root, 'seasons', `continuous-cml-x-L2-day${day}`);
  const rosterEntry = (provider, wasm) => ({
    provider_model: provider,
    model_id: `orw-day-${provider.replace('/', '-')}`,
    wasm_sha256: wasm,
  });
  fs.mkdirSync(seasonDir(3), { recursive: true });
  fs.writeFileSync(path.join(seasonDir(3), 'season.json'), JSON.stringify({
    roster: [rosterEntry('p/a', 'aaa'), rosterEntry('p/b', 'bbb')],
  }));
  fs.mkdirSync(seasonDir(4), { recursive: true });
  fs.writeFileSync(path.join(seasonDir(4), 'season.json'), JSON.stringify({
    roster: [rosterEntry('p/a', 'aaa'), rosterEntry('p/b', 'stale')],
  }));

  const models = [
    { model_id: 'p/a', rating: 60, wasm_sha256: 'aaa' },
    { model_id: 'p/b', rating: 50, wasm_sha256: 'bbb' },
  ];
  // Day 4 is newer but carries stale bytes for p/b -> day 3 wins.
  const resolved = await resolveServerModelIds({
    artifactsRoot: root, leagueId: 'cml-x', track: 'L2', models,
  });
  assert.equal(resolved.season_id, 'continuous-cml-x-L2-day3');
  assert.deepEqual(
    resolved.models.map((entry) => entry.server_model_id),
    ['orw-day-p-a', 'orw-day-p-b'],
  );

  // No matching season dirs at all -> ids pass through unchanged.
  const empty = fs.mkdtempSync(path.join(os.tmpdir(), 'chem-resolve-empty-'));
  const passthrough = await resolveServerModelIds({
    artifactsRoot: empty, leagueId: 'cml-x', track: 'L2', models,
  });
  assert.equal(passthrough.season_id, null);
  assert.deepEqual(
    passthrough.models.map((entry) => entry.server_model_id),
    ['p/a', 'p/b'],
  );

  // Season dirs exist but none binds the current artifacts -> hard error.
  await assert.rejects(
    resolveServerModelIds({
      artifactsRoot: root,
      leagueId: 'cml-x',
      track: 'L2',
      models: [{ model_id: 'p/b', rating: 50, wasm_sha256: 'new' }],
    }),
    /no completed L2 day season/,
  );
});
