import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {
  cumulativeRoster,
  deterministicSeedPack,
  isoWeekId,
  migrateLegacyUnboundState,
  rankingPathFor,
  validateCommittedArtifactBinding,
  validateLegacyUnboundState,
  validateEpochSnapshot,
  validateState,
} from './weekly_supervisor.mjs';

const digest = (value) => createHash('sha256').update(value).digest('hex');

function supervisorStateFixture(completed = false) {
  const modelIds = Array.from({ length: 10 }, (_, index) => `model-${index}`);
  const candidateRankingSha256 = 'c'.repeat(64);
  const state = {
    schema_version: 1,
    week_id: '2026-W30',
    status: completed ? 'active' : 'candidate',
    season_id: 'weekly-test',
    candidate_ranking_sha256: candidateRankingSha256,
    entrant_model_ids: modelIds,
    reasoning_policies: modelIds.map((modelId, index) => ({
      model_id: modelId,
      provider_model: `provider/model-${index}`,
      reasoning_policy: {
        version: 'capability_minimum_v1',
        mode: 'disabled',
        effort: null,
        exclude: true,
      },
    })),
    team_size: 10,
    modes: ['arena', 'ctf', 'koth', 'tdm'],
    rating_weights: { personal: 0.4, team: 0.35, collaboration: 0.25 },
    strategy_weights: { duel: 0.75, world: 0.25 },
    world_squad_size: 3,
    world_max_ticks: 600,
    generation: { completed, completed_at: completed ? '2026-07-24T00:00:00.000Z' : null },
    seed_pack_size: 4,
    points_by_rank: [1000, 700, 500, 360, 250, 180, 120, 80, 50, 30],
    epochs: [],
  };
  if (completed) {
    state.ranking_sha256 = candidateRankingSha256;
    state.arena_contract = {
      prompt_sha256: 'a'.repeat(64),
      prompt_version: 'arena-rust-v3.1.0',
      max_completion_tokens: 16_384,
      provider_sort_policy: 'throughput',
      temperature_policy: 'provider_default',
      reasoning_policy_version: 'capability_minimum_v1',
      provider_require_parameters: true,
      reasoning_exclude: true,
      response_transport_policy: 'sse_v1',
      source_limit_bytes: 50 * 1024,
      collaboration_abi_version: 'bot_tick_v2/1',
      simulator_rules_version: 'arena-v2',
    };
    state.artifact_bindings = modelIds.map((modelId) => ({
      model_id: modelId,
      wasm_bytes: 100,
      wasm_sha256: 'e'.repeat(64),
    }));
  }
  return state;
}

function legacyEpochSnapshot(state, seeds) {
  const providerIds = state.reasoning_policies.map((entry) => entry.provider_model);
  const roster = state.entrant_model_ids.map((modelId, index) => ({
    rank: index + 1,
    provider_rank: index + 1,
    model_id: modelId,
    model_name: `Model ${index}`,
    provider_model: providerIds[index],
    personal_rating: 50,
    team_rating: 50,
    collaboration_rating: 50,
    overall_rating: 50,
    world_rating: 50,
    strategy_rating: 50,
    compiled: true,
    simulated: false,
    wins: 1,
    losses: 1,
    draws: 0,
    matches_played: 2,
    evaluation_engagements: 2,
    personal_score_for: 1,
    personal_score_against: 1,
    team_objective_for: 1,
    team_objective_against: 1,
    collaboration_score_for: 1,
    collaboration_score_against: 1,
    world_points: 1,
    world_round_wins: 1,
    world_eliminations: 1,
    world_deaths: 1,
    world_collaboration_score: 1,
    source_bytes: 100,
    source_limit_bytes: 50 * 1024,
    source_sha256: 'b'.repeat(64),
    wasm_bytes: 100,
    compile_attempts: 1,
    integrity_status: 'verified_wasm',
  }));
  return {
    schema_version: 1,
    active: true,
    season_id: state.season_id,
    generated_at: '2026-07-24T00:01:00.000Z',
    ranking: { models: providerIds.map((id) => ({ id })) },
    methodology: {
      seed_sets: seeds,
      side_swapped: true,
      prompt_sha256: state.arena_contract.prompt_sha256,
      prompt_version: state.arena_contract.prompt_version,
      max_completion_tokens: state.arena_contract.max_completion_tokens,
      provider_sort_policy: state.arena_contract.provider_sort_policy,
      temperature_policy: state.arena_contract.temperature_policy,
      reasoning_policy_version: state.arena_contract.reasoning_policy_version,
      provider_require_parameters: state.arena_contract.provider_require_parameters,
      reasoning_exclude: state.arena_contract.reasoning_exclude,
      reasoning_policies: state.reasoning_policies,
      response_transport_policy: state.arena_contract.response_transport_policy,
      source_limit_bytes: state.arena_contract.source_limit_bytes,
      collaboration_abi_version: state.arena_contract.collaboration_abi_version,
      simulator_rules_version: state.arena_contract.simulator_rules_version,
      team_size: state.team_size,
      modes: state.modes,
      personal_weight: state.rating_weights.personal,
      team_weight: state.rating_weights.team,
      collaboration_weight: state.rating_weights.collaboration,
      duel_strategy_weight: state.strategy_weights.duel,
      world_strategy_weight: state.strategy_weights.world,
      world_squad_size: state.world_squad_size,
      world_max_ticks: state.world_max_ticks,
    },
    integrity: {
      verified: true,
      simulator_rules_version: state.arena_contract.simulator_rules_version,
      battle_requests: 45 * 4 * seeds.length * 2,
      total_engagements: (90 * seeds.length) + (270 * seeds.length * state.team_size),
      world_requests: seeds.length,
      world_fighter_rounds: seeds.length * state.entrant_model_ids.length * state.world_squad_size,
    },
    roster,
  };
}

async function legacyMigrationFixture(rootDirectory) {
  const weekDirectory = path.join(
    rootDirectory,
    'artifacts/arena/weekly-supervisor/2026-W30',
  );
  const statePath = path.join(weekDirectory, 'state.json');
  const publishPath = path.join(rootDirectory, 'data/arena_ratings.json');
  const state = supervisorStateFixture(true);
  delete state.artifact_bindings;
  state.frozen_at = '2026-07-24T00:00:00.000Z';

  const ranking = {
    schema_version: 1,
    retrieved_at: '2026-07-23T00:00:00.000Z',
    source: 'frozen-test',
    window: 'weekly',
    sort: 'top-weekly',
    models: state.reasoning_policies.map((entry, index) => ({
      provider_rank: index + 1,
      id: entry.provider_model,
      reasoning_policy: entry.reasoning_policy,
    })),
  };
  const rankingBytes = `${JSON.stringify(ranking, null, 2)}\n`;
  state.candidate_ranking_sha256 = digest(rankingBytes);
  state.ranking_sha256 = state.candidate_ranking_sha256;
  state.roster_sha256 = digest(ranking.models.map((model) => model.id).join('\n'));

  const seeds = deterministicSeedPack(state.week_id, 0, state.seed_pack_size);
  const snapshot = legacyEpochSnapshot(state, seeds);
  const snapshotBytes = `${JSON.stringify(snapshot, null, 2)}\n`;
  const epochPath = path.join(weekDirectory, 'epochs/epoch-000000.json');
  state.epochs = [{
    index: 0,
    epoch_id: '2026-W30-E000001',
    completed_at: snapshot.generated_at,
    seeds,
    battle_requests: snapshot.integrity.battle_requests,
    total_engagements: snapshot.integrity.total_engagements,
    world_requests: snapshot.integrity.world_requests,
    world_fighter_rounds: snapshot.integrity.world_fighter_rounds,
    artifact_path: path.relative(rootDirectory, epochPath).split(path.sep).join('/'),
    artifact_sha256: digest(snapshotBytes),
    standings: snapshot.roster.map((entry) => ({
      model_id: entry.model_id,
      epoch_rank: entry.rank,
      overall_rating: entry.overall_rating,
      world_rating: entry.world_rating,
      strategy_rating: entry.strategy_rating,
      points_awarded: state.points_by_rank[entry.rank - 1],
    })),
  }];
  const ledgerSha256 = digest(JSON.stringify(state.epochs));
  const validationSnapshot = {
    ...snapshot,
    roster: snapshot.roster.map((entry) => ({
      ...entry,
      wasm_sha256: 'e'.repeat(64),
    })),
  };
  const ratings = {
    ...snapshot,
    integrity: {
      ...snapshot.integrity,
      epochs_completed: 1,
    },
    league: {
      format: 'weekly_continuous_v1',
      week_id: state.week_id,
      epochs_completed: 1,
      ledger_sha256: ledgerSha256,
    },
    roster: cumulativeRoster([validationSnapshot], state).map((entry) => {
      const { wasm_sha256: _wasmSha256, ...legacyEntry } = entry;
      return legacyEntry;
    }),
  };
  const ratingsBytes = `${JSON.stringify(ratings, null, 2)}\n`;

  const seasonDirectory = path.join(
    rootDirectory,
    'artifacts/arena/seasons',
    state.season_id,
  );
  const generationDirectory = path.join(seasonDirectory, 'generations');
  await fs.mkdir(path.dirname(epochPath), { recursive: true });
  await fs.mkdir(generationDirectory, { recursive: true });
  await fs.mkdir(path.dirname(publishPath), { recursive: true });
  await fs.writeFile(path.join(weekDirectory, 'ranking.json'), rankingBytes);
  await fs.writeFile(epochPath, snapshotBytes);
  await fs.writeFile(statePath, `${JSON.stringify(state, null, 2)}\n`);
  await fs.writeFile(publishPath, ratingsBytes);
  await fs.writeFile(path.join(seasonDirectory, 'season.json'), snapshotBytes);
  await fs.writeFile(path.join(seasonDirectory, 'server-status.json'), JSON.stringify({
    provider_configured: false,
    prompt_sha256: state.arena_contract.prompt_sha256,
    prompt_version: state.arena_contract.prompt_version,
    max_tokens: state.arena_contract.max_completion_tokens,
    provider_sort_policy: state.arena_contract.provider_sort_policy,
    temperature_policy: state.arena_contract.temperature_policy,
    reasoning_policy_version: state.arena_contract.reasoning_policy_version,
    provider_require_parameters: state.arena_contract.provider_require_parameters,
    reasoning_exclude: state.arena_contract.reasoning_exclude,
    response_transport_policy: state.arena_contract.response_transport_policy,
    source_limit_bytes: state.arena_contract.source_limit_bytes,
    collaboration_abi_version: state.arena_contract.collaboration_abi_version,
    simulator_rules_version: state.arena_contract.simulator_rules_version,
  }));
  for (const modelId of state.entrant_model_ids) {
    await fs.writeFile(
      path.join(generationDirectory, `${modelId}.json`),
      JSON.stringify({
        schema_version: 2,
        stage: 'compiled',
        model_id: modelId,
        compiled: true,
        wasm_bytes: 100,
        wasm_sha256: 'e'.repeat(64),
      }),
    );
  }
  const bindingPath = path.join(seasonDirectory, 'artifact-binding.json');
  const bindingBytes = '{"test":"verified-binding"}\n';
  await fs.writeFile(bindingPath, bindingBytes);
  const entrants = state.reasoning_policies.map((entry, index) => ({
    provider_rank: index + 1,
    model_id: entry.model_id,
    model_name: `Model ${index}`,
    provider_model: entry.provider_model,
    canonical_slug: null,
    reasoning_policy: entry.reasoning_policy,
  }));
  return {
    state,
    statePath,
    weekDirectory,
    publishPath,
    seasonDirectory,
    generationDirectory,
    epochPath,
    snapshotBytes,
    artifactBindingReader: async () => {
      const bindings = await Promise.all(state.entrant_model_ids.map(async (modelId) => {
        const checkpoint = JSON.parse(await fs.readFile(
          path.join(generationDirectory, `${modelId}.json`),
          'utf8',
        ));
        if (checkpoint?.schema_version !== 2
            || checkpoint.stage !== 'compiled'
            || checkpoint.model_id !== modelId
            || checkpoint.compiled !== true
            || !Number.isSafeInteger(checkpoint.wasm_bytes)
            || !/^[a-f0-9]{64}$/.test(String(checkpoint.wasm_sha256 || ''))) {
          throw new Error('weekly arena artifact binding is invalid');
        }
        return {
          model_id: modelId,
          wasm_bytes: checkpoint.wasm_bytes,
          wasm_sha256: checkpoint.wasm_sha256,
        };
      }));
      return {
        bindingPath,
        manifestSha256: digest(bindingBytes),
        bindings,
      };
    },
    rankingLoader: async () => ranking,
    entrantBuilder: () => entrants,
  };
}

test('isoWeekId follows UTC ISO-8601 year boundaries', () => {
  assert.equal(isoWeekId('2019-12-30T00:00:00Z'), '2020-W01');
  assert.equal(isoWeekId('2020-01-05T23:59:59Z'), '2020-W01');
  assert.equal(isoWeekId('2020-12-31T12:00:00Z'), '2020-W53');
  assert.equal(isoWeekId('2021-01-01T12:00:00Z'), '2020-W53');
  assert.equal(isoWeekId('2021-01-04T00:00:00Z'), '2021-W01');
  assert.equal(isoWeekId('2026-12-31T23:59:59Z'), '2026-W53');
  assert.equal(isoWeekId('2027-01-03T23:59:59Z'), '2026-W53');
});

test('isoWeekId uses UTC rather than the host timezone', () => {
  assert.equal(isoWeekId('2026-01-05T00:15:00+14:00'), '2026-W01');
  assert.equal(isoWeekId('2026-01-05T00:15:00-12:00'), '2026-W02');
});

test('deterministicSeedPack is stable and contains valid unique u32 seeds', () => {
  const expected = [3693741182, 2053209647, 412678112, 3067113873];
  assert.deepEqual(deterministicSeedPack('2026-W30', 7, 4), expected);
  assert.equal(expected.length, 4);
  assert.equal(new Set(expected).size, 4);
  assert.ok(expected.every((seed) => Number.isSafeInteger(seed) && seed >= 0 && seed <= 0xffff_ffff));
});

test('rotating seed packs do not overlap adjacent epochs', () => {
  const packs = Array.from({ length: 100 }, (_, epoch) => (
    deterministicSeedPack('2026-W30', epoch, 4)
  ));
  const flattened = packs.flat();
  assert.equal(new Set(flattened).size, flattened.length);
  assert.notDeepEqual(packs[0], packs[1]);
  assert.notDeepEqual(
    deterministicSeedPack('2026-W30', 0, 4),
    deterministicSeedPack('2026-W31', 0, 4),
  );
});

test('seed and week helpers reject invalid input', () => {
  assert.throws(() => isoWeekId('not-a-date'), /valid date/);
  assert.throws(() => deterministicSeedPack('week-30', 0, 4), /ISO week/);
  assert.throws(() => deterministicSeedPack('2026-W54', 0, 4), /ISO week/);
  assert.throws(() => deterministicSeedPack('2026-W30', -1, 4), /epoch index/);
  assert.throws(() => deterministicSeedPack('2026-W30', 0, 0), /pack size/);
});

test('candidate state requires a ranking digest and rankingPathFor binds the exact file bytes', async (t) => {
  const weekDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-weekly-ranking-'));
  t.after(() => fs.rm(weekDirectory, { recursive: true, force: true }));
  const rankingPath = path.join(weekDirectory, 'candidate-ranking.json');
  const rankingBytes = '{"models":[{"id":"provider/model-0"}]}\n';
  await fs.writeFile(rankingPath, rankingBytes);

  const state = supervisorStateFixture();
  state.candidate_ranking_sha256 = digest(rankingBytes);
  assert.equal(validateState(state, '2026-W30', 4), state);
  assert.equal(await rankingPathFor(weekDirectory, state), rankingPath);

  const missingDigest = { ...state, candidate_ranking_sha256: null };
  assert.throws(
    () => validateState(missingDigest, '2026-W30', 4),
    /candidate ranking digest/,
  );
  await fs.writeFile(rankingPath, `${rankingBytes} `);
  await assert.rejects(
    rankingPathFor(weekDirectory, state),
    /candidate weekly ranking hash mismatch/,
  );
});

test('completed state freezes the full generation contract and bounded completion budget', () => {
  const state = supervisorStateFixture(true);
  assert.equal(validateState(state, '2026-W30', 4), state);
  for (const maxCompletionTokens of [2_049, 16_384]) {
    const boundary = structuredClone(state);
    boundary.arena_contract.max_completion_tokens = maxCompletionTokens;
    assert.equal(validateState(boundary, '2026-W30', 4), boundary);
  }
  for (const maxCompletionTokens of [2_048, 16_385]) {
    const outside = structuredClone(state);
    outside.arena_contract.max_completion_tokens = maxCompletionTokens;
    assert.throws(() => validateState(outside, '2026-W30', 4), /invalid arena contract/);
  }
  for (const [field, value] of [
    ['prompt_version', null],
    ['source_limit_bytes', (50 * 1024) - 1],
    ['collaboration_abi_version', 'bot_tick_v2'],
  ]) {
    const changed = structuredClone(state);
    changed.arena_contract[field] = value;
    assert.throws(() => validateState(changed, '2026-W30', 4), /invalid arena contract/);
  }
  const changedRanking = structuredClone(state);
  changedRanking.ranking_sha256 = 'd'.repeat(64);
  assert.throws(() => validateState(changedRanking, '2026-W30', 4), /ranking digest/);

  const changedArtifact = structuredClone(state);
  changedArtifact.artifact_bindings[0].wasm_sha256 = 'E'.repeat(64);
  assert.throws(
    () => validateState(changedArtifact, '2026-W30', 4),
    /artifact binding is invalid/,
  );
});

test('legacy eligibility rejects every binding-era marker, even null or partial metadata', () => {
  const legacy = supervisorStateFixture(true);
  delete legacy.artifact_bindings;
  legacy.status = 'active';
  legacy.epochs = [{ placeholder: true }];
  for (const [field, value] of [
    ['artifact_binding_version', null],
    ['artifact_binding_started_at', null],
    ['ledger_generation', 1],
    ['artifact_binding_manifest_sha256', null],
    ['artifact_binding_manifest_path', null],
  ]) {
    const marked = structuredClone(legacy);
    marked[field] = value;
    assert.throws(
      () => validateLegacyUnboundState(marked, '2026-W30', 4),
      /not an eligible legacy unbound season/,
    );
  }
});

test('legacy migration archives unbound history and starts a strict digest-bound ledger', async (t) => {
  const rootDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-weekly-migrate-'));
  t.after(() => fs.rm(rootDirectory, { recursive: true, force: true }));
  const fixture = await legacyMigrationFixture(rootDirectory);
  assert.equal(
    validateLegacyUnboundState(fixture.state, fixture.state.week_id, fixture.state.seed_pack_size),
    fixture.state,
  );
  const runnerCalls = [];
  const migrated = await migrateLegacyUnboundState({
    ...fixture,
    rootDirectory,
    runner: async (args) => { runnerCalls.push(args); },
    timestamp: () => '2026-07-25T12:00:00.000Z',
  });

  assert.deepEqual(runnerCalls, [[
    '--ranking-file', path.join(fixture.weekDirectory, 'ranking.json'),
    '--season-id', fixture.state.season_id,
    '--rehydrate-only',
  ]]);
  assert.equal(migrated.epochs.length, 0);
  assert.equal(migrated.artifact_bindings.length, 10);
  assert.equal(migrated.artifact_binding_version, 1);
  assert.equal(migrated.ledger_generation, 2);
  assert.equal(migrated.legacy_history[0].epoch_count, 1);
  assert.equal(migrated.legacy_history[0].wasm_sha256_bound, false);
  assert.equal(validateState(migrated, '2026-W30', 4), migrated);
  assert.equal(await fs.readFile(fixture.statePath, 'utf8').then(JSON.parse).then(
    (persisted) => persisted.artifact_bindings.length,
  ), 10);
  await assert.rejects(fs.access(fixture.epochPath));
  const archivedEpoch = path.join(
    rootDirectory,
    migrated.legacy_history[0].epochs_path,
    'epoch-000000.json',
  );
  assert.equal(await fs.readFile(archivedEpoch, 'utf8'), fixture.snapshotBytes);
  assert.equal(
    JSON.parse(await fs.readFile(
      path.join(rootDirectory, migrated.legacy_history[0].state_path),
      'utf8',
    )).epochs.length,
    1,
  );

  const tamperedHistory = structuredClone(migrated);
  tamperedHistory.legacy_history[0].ratings_path = '../escape.json';
  assert.throws(
    () => validateState(tamperedHistory, '2026-W30', 4),
    /invalid legacy history/,
  );
});

test('legacy migration keeps state and epochs untouched when one binding is incomplete', async (t) => {
  const rootDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-weekly-partial-'));
  t.after(() => fs.rm(rootDirectory, { recursive: true, force: true }));
  const fixture = await legacyMigrationFixture(rootDirectory);
  const incompletePath = path.join(
    fixture.generationDirectory,
    `${fixture.state.entrant_model_ids.at(-1)}.json`,
  );
  const incomplete = JSON.parse(await fs.readFile(incompletePath, 'utf8'));
  delete incomplete.wasm_sha256;
  await fs.writeFile(incompletePath, JSON.stringify(incomplete));
  const originalState = await fs.readFile(fixture.statePath, 'utf8');

  await assert.rejects(
    migrateLegacyUnboundState({
      ...fixture,
      rootDirectory,
      runner: async () => {},
      timestamp: () => '2026-07-25T12:00:00.000Z',
    }),
    /artifact binding is invalid/,
  );
  assert.equal(await fs.readFile(fixture.statePath, 'utf8'), originalState);
  assert.equal(await fs.readFile(fixture.epochPath, 'utf8'), fixture.snapshotBytes);
  await assert.rejects(fs.access(path.join(fixture.weekDirectory, 'legacy')));
});

test('legacy migration resumes after a crash between epoch rename and state commit', async (t) => {
  const rootDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-weekly-crash-'));
  t.after(() => fs.rm(rootDirectory, { recursive: true, force: true }));
  const fixture = await legacyMigrationFixture(rootDirectory);
  const ledgerSha256 = digest(JSON.stringify(fixture.state.epochs));
  const legacyDirectory = path.join(
    fixture.weekDirectory,
    'legacy',
    `unbound_wasm_v1-${ledgerSha256.slice(0, 12)}`,
  );
  await fs.mkdir(legacyDirectory, { recursive: true });
  await fs.writeFile(
    path.join(legacyDirectory, 'state.json'),
    `${JSON.stringify(fixture.state, null, 2)}\n`,
  );
  await fs.copyFile(fixture.publishPath, path.join(legacyDirectory, 'ratings.json'));
  await fs.copyFile(
    path.join(fixture.seasonDirectory, 'season.json'),
    path.join(legacyDirectory, 'season.json'),
  );
  await fs.rename(
    path.join(fixture.weekDirectory, 'epochs'),
    path.join(legacyDirectory, 'epochs'),
  );

  const migrated = await migrateLegacyUnboundState({
    ...fixture,
    rootDirectory,
    runner: async () => {},
    timestamp: () => '2026-07-25T12:00:00.000Z',
  });
  assert.equal(migrated.epochs.length, 0);
  assert.equal(validateState(migrated, '2026-W30', 4), migrated);
  assert.equal(
    await fs.readFile(path.join(legacyDirectory, 'epochs/epoch-000000.json'), 'utf8'),
    fixture.snapshotBytes,
  );
});

test('legacy migration revalidates the exact manifest immediately before state commit', async (t) => {
  const rootDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-weekly-binding-race-'));
  t.after(() => fs.rm(rootDirectory, { recursive: true, force: true }));
  const fixture = await legacyMigrationFixture(rootDirectory);
  const originalReader = fixture.artifactBindingReader;
  let reads = 0;
  const originalState = await fs.readFile(fixture.statePath, 'utf8');

  await assert.rejects(
    migrateLegacyUnboundState({
      ...fixture,
      rootDirectory,
      runner: async () => {},
      artifactBindingReader: async (args) => {
        const binding = await originalReader(args);
        reads += 1;
        return reads === 1
          ? binding
          : { ...binding, manifestSha256: 'f'.repeat(64) };
      },
      timestamp: () => '2026-07-25T12:00:00.000Z',
    }),
    /committed artifact binding changed after validation/,
  );
  assert.equal(reads, 2);
  assert.equal(await fs.readFile(fixture.statePath, 'utf8'), originalState);
});

test('completed generation validation rejects a manifest hash change before use', async (t) => {
  const rootDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-weekly-binding-use-'));
  t.after(() => fs.rm(rootDirectory, { recursive: true, force: true }));
  const fixture = await legacyMigrationFixture(rootDirectory);
  const migrated = await migrateLegacyUnboundState({
    ...fixture,
    rootDirectory,
    runner: async () => {},
    timestamp: () => '2026-07-25T12:00:00.000Z',
  });
  await validateCommittedArtifactBinding({
    state: migrated,
    weekDirectory: fixture.weekDirectory,
    rootDirectory,
    artifactBindingReader: fixture.artifactBindingReader,
    rankingLoader: fixture.rankingLoader,
    entrantBuilder: fixture.entrantBuilder,
  });
  await assert.rejects(
    validateCommittedArtifactBinding({
      state: migrated,
      weekDirectory: fixture.weekDirectory,
      rootDirectory,
      artifactBindingReader: async (args) => ({
        ...await fixture.artifactBindingReader(args),
        manifestSha256: '0'.repeat(64),
      }),
      rankingLoader: fixture.rankingLoader,
      entrantBuilder: fixture.entrantBuilder,
    }),
    /committed artifact binding changed after validation/,
  );
});

test('legacy migration rejects ambiguous epoch directories and conflicting archives', async (t) => {
  const ambiguousRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-weekly-ambiguous-'));
  const conflictRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-weekly-conflict-'));
  t.after(() => Promise.all([
    fs.rm(ambiguousRoot, { recursive: true, force: true }),
    fs.rm(conflictRoot, { recursive: true, force: true }),
  ]));

  const ambiguous = await legacyMigrationFixture(ambiguousRoot);
  const ambiguousLedger = digest(JSON.stringify(ambiguous.state.epochs));
  const ambiguousLegacy = path.join(
    ambiguous.weekDirectory,
    'legacy',
    `unbound_wasm_v1-${ambiguousLedger.slice(0, 12)}`,
    'epochs',
  );
  await fs.mkdir(ambiguousLegacy, { recursive: true });
  await fs.writeFile(path.join(ambiguousLegacy, 'epoch-000000.json'), ambiguous.snapshotBytes);
  await assert.rejects(
    migrateLegacyUnboundState({
      ...ambiguous,
      rootDirectory: ambiguousRoot,
      runner: async () => {},
      timestamp: () => '2026-07-25T12:00:00.000Z',
    }),
    /both current and legacy epoch directories exist/,
  );
  assert.equal((await fs.readFile(ambiguous.statePath, 'utf8')).includes('artifact_bindings'), false);

  const conflict = await legacyMigrationFixture(conflictRoot);
  const conflictLedger = digest(JSON.stringify(conflict.state.epochs));
  const conflictLegacy = path.join(
    conflict.weekDirectory,
    'legacy',
    `unbound_wasm_v1-${conflictLedger.slice(0, 12)}`,
  );
  await fs.mkdir(conflictLegacy, { recursive: true });
  await fs.writeFile(path.join(conflictLegacy, 'ratings.json'), 'tampered');
  await assert.rejects(
    migrateLegacyUnboundState({
      ...conflict,
      rootDirectory: conflictRoot,
      runner: async () => {},
      timestamp: () => '2026-07-25T12:00:00.000Z',
    }),
    /legacy archive conflicts with current source/,
  );
  assert.equal((await fs.readFile(conflict.statePath, 'utf8')).includes('artifact_bindings'), false);
  assert.equal(await fs.readFile(conflict.epochPath, 'utf8'), conflict.snapshotBytes);
});

test('legacy migration rejects semantically unrelated ratings before creating an archive', async (t) => {
  const rootDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-weekly-publication-'));
  t.after(() => fs.rm(rootDirectory, { recursive: true, force: true }));
  const fixture = await legacyMigrationFixture(rootDirectory);
  const unrelated = JSON.parse(await fs.readFile(fixture.publishPath, 'utf8'));
  unrelated.season_id = 'different-season';
  await fs.writeFile(fixture.publishPath, JSON.stringify(unrelated));
  const originalState = await fs.readFile(fixture.statePath, 'utf8');

  await assert.rejects(
    migrateLegacyUnboundState({
      ...fixture,
      rootDirectory,
      runner: async () => {},
      timestamp: () => '2026-07-25T12:00:00.000Z',
    }),
    /ratings artifact is not the legacy ledger publication/,
  );
  assert.equal(await fs.readFile(fixture.statePath, 'utf8'), originalState);
  assert.equal(await fs.readFile(fixture.epochPath, 'utf8'), fixture.snapshotBytes);
  await assert.rejects(fs.access(path.join(fixture.weekDirectory, 'legacy')));
});

test('legacy migration rejects ratings already marked as a bound ledger', async (t) => {
  const rootDirectory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-weekly-bound-publication-'));
  t.after(() => fs.rm(rootDirectory, { recursive: true, force: true }));
  const fixture = await legacyMigrationFixture(rootDirectory);
  const bound = JSON.parse(await fs.readFile(fixture.publishPath, 'utf8'));
  bound.league.ledger_generation = 2;
  await fs.writeFile(fixture.publishPath, JSON.stringify(bound));
  const originalState = await fs.readFile(fixture.statePath, 'utf8');

  await assert.rejects(
    migrateLegacyUnboundState({
      ...fixture,
      rootDirectory,
      runner: async () => {},
      timestamp: () => '2026-07-25T12:00:00.000Z',
    }),
    /ratings artifact is not the legacy ledger publication/,
  );
  assert.equal(await fs.readFile(fixture.statePath, 'utf8'), originalState);
  assert.equal(await fs.readFile(fixture.epochPath, 'utf8'), fixture.snapshotBytes);
  await assert.rejects(fs.access(path.join(fixture.weekDirectory, 'legacy')));
});

test('cumulative roster awards tennis points and averages completed epochs', () => {
  const entrant = (modelId, rank, overall, world, wins) => ({
    model_id: modelId,
    model_name: modelId,
    provider_rank: modelId === 'a' ? 1 : 2,
    rank,
    personal_rating: overall,
    team_rating: overall,
    collaboration_rating: overall,
    overall_rating: overall,
    world_rating: world,
    strategy_rating: Math.round(((overall * 0.75) + (world * 0.25)) * 100) / 100,
    wins,
    losses: 10 - wins,
    draws: 0,
    matches_played: 10,
    evaluation_engagements: 10,
    personal_score_for: wins * 10,
    personal_score_against: (10 - wins) * 10,
    team_objective_for: wins * 2,
    team_objective_against: (10 - wins) * 2,
    collaboration_score_for: wins * 3,
    collaboration_score_against: (10 - wins) * 3,
    world_points: wins * 100,
    world_round_wins: wins,
    world_eliminations: wins * 4,
    world_deaths: 10 - wins,
    world_collaboration_score: wins * 5,
  });
  const methodology = {
    personal_weight: 0.4,
    team_weight: 0.35,
    collaboration_weight: 0.25,
  };
  const snapshots = [
    { methodology, roster: [entrant('a', 1, 90, 90, 8), entrant('b', 2, 60, 100, 2)] },
    { methodology, roster: [entrant('b', 1, 80, 100, 7), entrant('a', 2, 70, 30, 3)] },
  ];
  const roster = cumulativeRoster(snapshots, {
    points_by_rank: [1000, 700, 500, 360, 250, 180, 120, 80, 50, 30],
  });
  const modelA = roster.find((entry) => entry.model_id === 'a');
  const modelB = roster.find((entry) => entry.model_id === 'b');
  assert.equal(roster[0].model_id, 'b', 'strategy rating breaks equal league points');
  assert.equal(modelA.season_points, 1700);
  assert.equal(modelA.epochs_played, 2);
  assert.equal(modelA.epoch_wins, 1);
  assert.equal(modelA.overall_rating, 80);
  assert.equal(modelA.world_rating, 60);
  assert.equal(modelA.strategy_rating, 75);
  assert.equal(modelA.wins, 11);
  assert.equal(modelA.matches_played, 20);
  assert.equal(modelA.personal_score_for, 110);
  assert.equal(modelA.personal_score_against, 90);
  assert.equal(modelA.team_objective_for, 22);
  assert.equal(modelA.collaboration_score_for, 33);
  assert.equal(modelA.world_points, 1100);
  assert.equal(modelA.world_round_wins, 11);
  assert.equal(modelA.world_eliminations, 44);
  assert.equal(modelA.world_deaths, 9);
  assert.equal(modelA.world_collaboration_score, 55);
  assert.equal(modelB.season_points, 1700);
  assert.equal(modelB.overall_rating, 70);
  assert.equal(modelB.strategy_rating, 77.5);
});

test('epoch validation requires a complete pinned duel and world evaluation', () => {
  const seeds = [101, 202];
  const modelIds = Array.from({ length: 10 }, (_, index) => `model-${index}`);
  const providerIds = Array.from({ length: 10 }, (_, index) => `provider/model-${index}`);
  const reasoningPolicies = modelIds.map((modelId, index) => ({
    model_id: modelId,
    provider_model: providerIds[index],
    reasoning_policy: {
      version: 'capability_minimum_v1',
      mode: index === 9 ? 'minimum' : 'disabled',
      effort: index === 9 ? 'low' : null,
      exclude: true,
    },
  }));
  const digest = (value) => createHash('sha256').update(value).digest('hex');
  const state = {
    season_id: 'weekly-test',
    team_size: 10,
    modes: ['arena', 'ctf', 'koth', 'tdm'],
    rating_weights: { personal: 0.4, team: 0.35, collaboration: 0.25 },
    strategy_weights: { duel: 0.75, world: 0.25 },
    world_squad_size: 3,
    world_max_ticks: 600,
    entrant_model_ids: modelIds,
    artifact_bindings: modelIds.map((modelId) => ({
      model_id: modelId,
      wasm_bytes: 100,
      wasm_sha256: 'e'.repeat(64),
    })),
    reasoning_policies: reasoningPolicies,
    roster_sha256: digest(providerIds.join('\n')),
    arena_contract: {
      prompt_sha256: 'a'.repeat(64),
      prompt_version: 'arena-rust-v3.1.0',
      max_completion_tokens: 16_384,
      provider_sort_policy: 'throughput',
      temperature_policy: 'provider_default',
      reasoning_policy_version: 'capability_minimum_v1',
      provider_require_parameters: true,
      reasoning_exclude: true,
      response_transport_policy: 'sse_v1',
      source_limit_bytes: 50 * 1024,
      collaboration_abi_version: 'bot_tick_v2/1',
      simulator_rules_version: 'arena-v2',
    },
  };
  const roster = modelIds.map((modelId, index) => ({
    rank: index + 1,
    model_id: modelId,
    personal_rating: 50,
    team_rating: 50,
    collaboration_rating: 50,
    overall_rating: 50,
    world_rating: 50,
    strategy_rating: 50,
    compiled: true,
    simulated: false,
    wins: 1,
    losses: 1,
    draws: 0,
    matches_played: 2,
    evaluation_engagements: 2,
    personal_score_for: 1,
    personal_score_against: 1,
    team_objective_for: 1,
    team_objective_against: 1,
    collaboration_score_for: 1,
    collaboration_score_against: 1,
    world_points: 1,
    world_round_wins: 1,
    world_eliminations: 1,
    world_deaths: 1,
    world_collaboration_score: 1,
    source_bytes: 100,
    source_limit_bytes: 50 * 1024,
    source_sha256: 'b'.repeat(64),
    wasm_bytes: 100,
    wasm_sha256: 'e'.repeat(64),
    compile_attempts: 1,
    integrity_status: 'verified_wasm',
  }));
  const snapshot = {
    schema_version: 1,
    active: true,
    season_id: state.season_id,
    ranking: { models: providerIds.map((id) => ({ id })) },
    methodology: {
      seed_sets: seeds,
      side_swapped: true,
      prompt_sha256: state.arena_contract.prompt_sha256,
      prompt_version: state.arena_contract.prompt_version,
      max_completion_tokens: state.arena_contract.max_completion_tokens,
      provider_sort_policy: state.arena_contract.provider_sort_policy,
      temperature_policy: state.arena_contract.temperature_policy,
      reasoning_policy_version: state.arena_contract.reasoning_policy_version,
      provider_require_parameters: state.arena_contract.provider_require_parameters,
      reasoning_exclude: state.arena_contract.reasoning_exclude,
      reasoning_policies: state.reasoning_policies,
      response_transport_policy: state.arena_contract.response_transport_policy,
      source_limit_bytes: state.arena_contract.source_limit_bytes,
      collaboration_abi_version: state.arena_contract.collaboration_abi_version,
      simulator_rules_version: state.arena_contract.simulator_rules_version,
      team_size: 10,
      modes: state.modes,
      personal_weight: 0.4,
      team_weight: 0.35,
      collaboration_weight: 0.25,
      duel_strategy_weight: 0.75,
      world_strategy_weight: 0.25,
      world_squad_size: 3,
      world_max_ticks: 600,
    },
    integrity: {
      verified: true,
      simulator_rules_version: state.arena_contract.simulator_rules_version,
      battle_requests: 45 * 4 * seeds.length * 2,
      total_engagements: (90 * seeds.length) + (270 * seeds.length * 10),
      world_requests: seeds.length,
      world_fighter_rounds: seeds.length * 10 * 3,
    },
    roster,
  };

  assert.equal(validateEpochSnapshot(snapshot, state, seeds), snapshot);
  assert.throws(
    () => validateEpochSnapshot({
      ...snapshot,
      methodology: { ...snapshot.methodology, provider_sort_policy: 'price' },
    }, state, seeds),
    /prompt\/source contract differs/,
  );
  assert.throws(
    () => validateEpochSnapshot({
      ...snapshot,
      methodology: { ...snapshot.methodology, temperature_policy: 'fixed_zero' },
    }, state, seeds),
    /prompt\/source contract differs/,
  );
  assert.throws(
    () => validateEpochSnapshot({
      ...snapshot,
      methodology: { ...snapshot.methodology, prompt_version: 'arena-rust-v3.0.1' },
    }, state, seeds),
    /prompt\/source contract differs/,
  );
  assert.throws(
    () => validateEpochSnapshot({
      ...snapshot,
      methodology: { ...snapshot.methodology, max_completion_tokens: 8_192 },
    }, state, seeds),
    /prompt\/source contract differs/,
  );
  assert.throws(
    () => validateEpochSnapshot({
      ...snapshot,
      methodology: { ...snapshot.methodology, source_limit_bytes: (50 * 1024) - 1 },
    }, state, seeds),
    /prompt\/source contract differs/,
  );
  assert.throws(
    () => validateEpochSnapshot({
      ...snapshot,
      methodology: { ...snapshot.methodology, collaboration_abi_version: 'bot_tick_v2' },
    }, state, seeds),
    /prompt\/source contract differs/,
  );
  assert.throws(
    () => validateEpochSnapshot({
      ...snapshot,
      methodology: {
        ...snapshot.methodology,
        reasoning_policies: snapshot.methodology.reasoning_policies.map((entry, index) => (
          index === 9
            ? { ...entry, reasoning_policy: { ...entry.reasoning_policy, effort: 'medium' } }
            : entry
        )),
      },
    }, state, seeds),
    /prompt\/source contract differs/,
  );
  assert.throws(
    () => validateEpochSnapshot({
      ...snapshot,
      methodology: { ...snapshot.methodology, response_transport_policy: 'json_v1' },
    }, state, seeds),
    /prompt\/source contract differs/,
  );
  assert.throws(
    () => validateEpochSnapshot({
      ...snapshot,
      integrity: { ...snapshot.integrity, world_requests: 1 },
    }, state, seeds),
    /world evaluation is incomplete/,
  );
  assert.throws(
    () => validateEpochSnapshot({
      ...snapshot,
      roster: [{ ...roster[0], strategy_rating: 70 }, ...roster.slice(1)],
    }, state, seeds),
    /roster integrity failed/,
  );
  assert.throws(
    () => validateEpochSnapshot({
      ...snapshot,
      roster: [{ ...roster[0], wasm_sha256: 'f'.repeat(64) }, ...roster.slice(1)],
    }, state, seeds),
    /roster integrity failed/,
  );
});

test('recorded mid-season revision permits exactly one artifact swap per model', () => {
  const seeds = [101, 202];
  const state = supervisorStateFixture(true);
  state.week_id = '2026-W31';
  const revisedSha = 'f'.repeat(64);
  state.revision = {
    completed: true,
    epoch_index: 1,
    completed_at: '2026-07-27T12:00:00.000Z',
    entries: [
      {
        model_id: 'model-0',
        status: 'improved',
        wasm_bytes_after: 200,
        wasm_sha256_after: revisedSha,
      },
      ...state.entrant_model_ids.slice(1).map((modelId) => ({
        model_id: modelId,
        status: 'kept_gen1',
      })),
    ],
  };
  validateState(state, '2026-W31', 4);

  const providerIds = state.reasoning_policies.map((entry) => entry.provider_model);
  state.roster_sha256 = digest(providerIds.join('\n'));
  const buildSnapshot = (modelZeroBytes, modelZeroSha) => {
    const roster = state.entrant_model_ids.map((modelId, index) => ({
      rank: index + 1,
      model_id: modelId,
      personal_rating: 50,
      team_rating: 50,
      collaboration_rating: 50,
      overall_rating: 50,
      world_rating: 50,
      strategy_rating: 50,
      compiled: true,
      simulated: false,
      wins: 1,
      losses: 1,
      draws: 0,
      matches_played: 2,
      evaluation_engagements: 2,
      personal_score_for: 1,
      personal_score_against: 1,
      team_objective_for: 1,
      team_objective_against: 1,
      collaboration_score_for: 1,
      collaboration_score_against: 1,
      world_points: 1,
      world_round_wins: 1,
      world_eliminations: 1,
      world_deaths: 1,
      world_collaboration_score: 1,
      source_bytes: 100,
      source_limit_bytes: 50 * 1024,
      source_sha256: 'b'.repeat(64),
      wasm_bytes: index === 0 ? modelZeroBytes : 100,
      wasm_sha256: index === 0 ? modelZeroSha : 'e'.repeat(64),
      compile_attempts: 1,
      integrity_status: 'verified_wasm',
    }));
    return {
      schema_version: 1,
      active: true,
      season_id: state.season_id,
      generated_at: '2026-07-27T12:00:00.000Z',
      ranking: { models: providerIds.map((id) => ({ id })) },
      methodology: {
        seed_sets: seeds,
        side_swapped: true,
        prompt_sha256: state.arena_contract.prompt_sha256,
        prompt_version: state.arena_contract.prompt_version,
        max_completion_tokens: state.arena_contract.max_completion_tokens,
        provider_sort_policy: state.arena_contract.provider_sort_policy,
        temperature_policy: state.arena_contract.temperature_policy,
        reasoning_policy_version: state.arena_contract.reasoning_policy_version,
        provider_require_parameters: state.arena_contract.provider_require_parameters,
        reasoning_exclude: state.arena_contract.reasoning_exclude,
        reasoning_policies: state.reasoning_policies,
        response_transport_policy: state.arena_contract.response_transport_policy,
        source_limit_bytes: state.arena_contract.source_limit_bytes,
        collaboration_abi_version: state.arena_contract.collaboration_abi_version,
        simulator_rules_version: state.arena_contract.simulator_rules_version,
        team_size: 10,
        modes: state.modes,
        personal_weight: 0.4,
        team_weight: 0.35,
        collaboration_weight: 0.25,
        duel_strategy_weight: 0.75,
        world_strategy_weight: 0.25,
        world_squad_size: 3,
        world_max_ticks: 600,
      },
      integrity: {
        verified: true,
        simulator_rules_version: state.arena_contract.simulator_rules_version,
        battle_requests: 45 * 4 * seeds.length * 2,
        total_engagements: (90 * seeds.length) + (270 * seeds.length * 10),
        world_requests: seeds.length,
        world_fighter_rounds: seeds.length * 10 * 3,
      },
      roster,
    };
  };

  const pre = buildSnapshot(100, 'e'.repeat(64));
  const post = buildSnapshot(200, revisedSha);
  const tampered = buildSnapshot(300, 'd'.repeat(64));

  assert.equal(validateEpochSnapshot(pre, state, seeds, 0), pre);
  assert.equal(validateEpochSnapshot(post, state, seeds, 1), post);
  assert.throws(() => validateEpochSnapshot(post, state, seeds, 0), /roster integrity failed/);
  assert.throws(() => validateEpochSnapshot(tampered, state, seeds, 1), /roster integrity failed/);
  // callers that do not thread the epoch index stay conservative (frozen artifacts only)
  assert.throws(() => validateEpochSnapshot(post, state, seeds), /roster integrity failed/);
  // cumulative standings tolerate the recorded swap...
  assert.doesNotThrow(() => cumulativeRoster([pre, post], state));
  // ...but an unrecorded swap is still fatal
  const unrecorded = supervisorStateFixture(true);
  unrecorded.week_id = '2026-W31';
  assert.throws(
    () => cumulativeRoster([pre, post], unrecorded),
    /compiled artifact changed across epochs/,
  );
});
