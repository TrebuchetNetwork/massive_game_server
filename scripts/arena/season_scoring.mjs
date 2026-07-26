const DEFAULT_WEIGHTS = Object.freeze({
  personal: 0.4,
  team: 0.35,
  collaboration: 0.25,
});

const COLLABORATION_FIELDS = Object.freeze({
  a: [
    'total_team_a_collaboration_score',
    'team_a_collaboration_score',
    'total_team_a_collaboration',
    'collaboration_a',
  ],
  b: [
    'total_team_b_collaboration_score',
    'team_b_collaboration_score',
    'total_team_b_collaboration',
    'collaboration_b',
  ],
});

const finiteNumber = (value, fallback = 0) => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
};

const clamp01 = (value) => Math.max(0, Math.min(1, finiteNumber(value)));

const mean = (values) => {
  if (!Array.isArray(values) || values.length === 0) return 0;
  return values.reduce((sum, value) => sum + finiteNumber(value), 0) / values.length;
};

const roundRating = (value) => Math.round(Math.max(0, Math.min(100, value)) * 100) / 100;

const share = (mine, theirs, emptyValue = 0.5) => {
  const left = Math.max(0, finiteNumber(mine));
  const right = Math.max(0, finiteNumber(theirs));
  const total = left + right;
  return total > 0 ? left / total : emptyValue;
};

const resultPoint = (simulation, modelId) => {
  if (simulation?.draw || !simulation?.winner_model_id) return 0.5;
  return simulation.winner_model_id === modelId ? 1 : 0;
};

const collaborationValue = (simulation, side) => {
  for (const field of COLLABORATION_FIELDS[side]) {
    if (Object.hasOwn(simulation || {}, field)) {
      const value = Number(simulation[field]);
      if (Number.isFinite(value) && value >= 0) return value;
    }
  }
  return null;
};

const blankAccumulator = (entrant) => ({
  entrant,
  personal: [],
  team: [],
  collaboration: [],
  wins: 0,
  losses: 0,
  draws: 0,
  matchesPlayed: 0,
  evaluationEngagements: 0,
  personalScoreFor: 0,
  personalScoreAgainst: 0,
  teamObjectiveFor: 0,
  teamObjectiveAgainst: 0,
  collaborationScoreFor: 0,
  collaborationScoreAgainst: 0,
});

function addRecord(accumulator, simulation, modelId) {
  accumulator.matchesPlayed += 1;
  accumulator.evaluationEngagements += Math.max(
    1,
    Math.trunc(finiteNumber(simulation?.total_engagements, 1)),
  );
  if (simulation?.draw || !simulation?.winner_model_id) {
    accumulator.draws += 1;
  } else if (simulation.winner_model_id === modelId) {
    accumulator.wins += 1;
  } else {
    accumulator.losses += 1;
  }
}

function scoreSide(accumulator, leg, side) {
  const simulation = leg.simulation;
  const isA = side === 'a';
  const modelId = isA ? leg.model_a_id : leg.model_b_id;
  const opponentId = isA ? leg.model_b_id : leg.model_a_id;
  const mineScore = finiteNumber(
    isA ? simulation.total_team_a_score : simulation.total_team_b_score,
  );
  const theirScore = finiteNumber(
    isA ? simulation.total_team_b_score : simulation.total_team_a_score,
  );
  const mineObjective = finiteNumber(
    isA ? simulation.total_team_a_objective : simulation.total_team_b_objective,
  );
  const theirObjective = finiteNumber(
    isA ? simulation.total_team_b_objective : simulation.total_team_a_objective,
  );
  const result = resultPoint(simulation, modelId);

  if (!opponentId || opponentId === modelId) {
    throw new Error(`invalid season leg for '${modelId}'`);
  }

  addRecord(accumulator, simulation, modelId);

  if (leg.category === 'personal') {
    // A solo rating combines the objective verdict with damage/score production.
    accumulator.personalScoreFor += Math.max(0, mineScore);
    accumulator.personalScoreAgainst += Math.max(0, theirScore);
    accumulator.personal.push(0.7 * result + 0.3 * share(mineScore, theirScore));
    return;
  }

  if (leg.category !== 'team') {
    throw new Error(`unknown season leg category '${leg.category}'`);
  }

  // Team rating is mode-agnostic: objective share is normalized within each leg
  // before CTF, KOTH, and TDM are averaged together.
  accumulator.team.push(0.65 * result + 0.35 * share(mineObjective, theirObjective));
  accumulator.teamObjectiveFor += Math.max(0, mineObjective);
  accumulator.teamObjectiveAgainst += Math.max(0, theirObjective);

  const mineCollaboration = collaborationValue(simulation, side);
  const theirCollaboration = collaborationValue(simulation, isA ? 'b' : 'a');
  if (mineCollaboration === null || theirCollaboration === null) {
    throw new Error(
      `collaboration telemetry missing for ${leg.mode || 'team'} leg ${leg.model_a_id} vs ${leg.model_b_id}`,
    );
  }

  // Collaboration is primarily direct support telemetry from the v2 team
  // sandbox, with a smaller result component so support must still convert into
  // competitive team play. Two teams that perform no support receive zero for
  // the support component rather than an artificial 50/50 score.
  const supportShare = share(mineCollaboration, theirCollaboration, 0);
  accumulator.collaborationScoreFor += mineCollaboration;
  accumulator.collaborationScoreAgainst += theirCollaboration;
  accumulator.collaboration.push(0.75 * supportShare + 0.25 * result);
}

export function buildSeasonRatings({
  entrants,
  legs,
  weights = DEFAULT_WEIGHTS,
  sourceLimitBytes = 50 * 1024,
}) {
  if (!Array.isArray(entrants) || entrants.length < 2) {
    throw new Error('at least two entrants are required');
  }
  if (!Array.isArray(legs) || legs.length === 0) {
    throw new Error('season legs are required');
  }

  const weightTotal = finiteNumber(weights.personal)
    + finiteNumber(weights.team)
    + finiteNumber(weights.collaboration);
  if (Math.abs(weightTotal - 1) > 0.000001) {
    throw new Error('rating weights must sum to 1');
  }

  const accumulators = new Map(
    entrants.map((entrant) => [entrant.model_id, blankAccumulator(entrant)]),
  );

  for (const leg of legs) {
    const left = accumulators.get(leg.model_a_id);
    const right = accumulators.get(leg.model_b_id);
    if (!left || !right) {
      throw new Error(`season leg references an unknown entrant`);
    }
    scoreSide(left, leg, 'a');
    scoreSide(right, leg, 'b');
  }

  const unrated = [...accumulators.values()].filter(
    (entry) => entry.personal.length === 0 || entry.team.length === 0 || entry.collaboration.length === 0,
  );
  if (unrated.length > 0) {
    throw new Error(`incomplete evaluation for: ${unrated.map((entry) => entry.entrant.model_id).join(', ')}`);
  }

  const roster = [...accumulators.values()].map((entry) => {
    const personalRating = roundRating(mean(entry.personal) * 100);
    const teamRating = roundRating(mean(entry.team) * 100);
    const collaborationRating = roundRating(mean(entry.collaboration) * 100);
    const overallRating = roundRating(
      personalRating * weights.personal
      + teamRating * weights.team
      + collaborationRating * weights.collaboration,
    );

    return {
      provider_rank: entry.entrant.provider_rank,
      model_id: entry.entrant.model_id,
      model_name: entry.entrant.model_name,
      provider_model: entry.entrant.provider_model,
      canonical_slug: entry.entrant.canonical_slug || null,
      personal_rating: personalRating,
      team_rating: teamRating,
      collaboration_rating: collaborationRating,
      overall_rating: overallRating,
      compiled: entry.entrant.compiled === true,
      simulated: entry.entrant.simulated === true,
      source_bytes: Math.max(0, Math.trunc(finiteNumber(entry.entrant.source_bytes))),
      source_limit_bytes: sourceLimitBytes,
      source_sha256: entry.entrant.source_sha256 || null,
      wasm_bytes: Number.isFinite(Number(entry.entrant.wasm_bytes))
        ? Math.max(0, Math.trunc(Number(entry.entrant.wasm_bytes)))
        : null,
      wasm_sha256: entry.entrant.wasm_sha256 || null,
      compile_attempts: entry.entrant.compile_attempts || null,
      wins: entry.wins,
      losses: entry.losses,
      draws: entry.draws,
      matches_played: entry.matchesPlayed,
      evaluation_engagements: entry.evaluationEngagements,
      personal_score_for: entry.personalScoreFor,
      personal_score_against: entry.personalScoreAgainst,
      team_objective_for: entry.teamObjectiveFor,
      team_objective_against: entry.teamObjectiveAgainst,
      collaboration_score_for: entry.collaborationScoreFor,
      collaboration_score_against: entry.collaborationScoreAgainst,
      integrity_status: entry.entrant.compiled === true && entry.entrant.simulated === false
        ? 'verified_wasm'
        : 'unverified',
    };
  });

  roster.sort((left, right) => (
    right.overall_rating - left.overall_rating
    || right.personal_rating - left.personal_rating
    || right.team_rating - left.team_rating
    || right.collaboration_rating - left.collaboration_rating
    || left.provider_rank - right.provider_rank
  ));
  roster.forEach((entry, index) => {
    entry.rank = index + 1;
  });
  return roster;
}

export function addWorldRatings(
  roster,
  worldCheckpoints,
  strategyWeights = { duel: 0.75, world: 0.25 },
) {
  if (!Array.isArray(roster) || roster.length < 2 || !Array.isArray(worldCheckpoints) || worldCheckpoints.length === 0) {
    throw new Error('world ratings require a roster and completed world checkpoints');
  }
  const weightTotal = finiteNumber(strategyWeights.duel) + finiteNumber(strategyWeights.world);
  if (Math.abs(weightTotal - 1) > 0.000001) throw new Error('strategy weights must sum to 1');
  const totals = new Map(roster.map((entry) => [entry.model_id, {
    points: 0,
    roundWins: 0,
    eliminations: 0,
    deaths: 0,
    collaboration: 0,
  }]));
  for (const checkpoint of worldCheckpoints) {
    if (!Array.isArray(checkpoint?.simulation?.rankings)) {
      throw new Error('world checkpoint is missing rankings');
    }
    const seen = new Set();
    for (const result of checkpoint.simulation.rankings) {
      const total = totals.get(result.model_id);
      if (!total || seen.has(result.model_id)) {
        throw new Error(`world result references duplicate or unknown model ${result.model_id}`);
      }
      seen.add(result.model_id);
      total.points += Math.max(0, finiteNumber(result.points));
      total.roundWins += Math.max(0, finiteNumber(result.round_wins));
      total.eliminations += Math.max(0, finiteNumber(result.eliminations));
      total.deaths += Math.max(0, finiteNumber(result.deaths));
      total.collaboration += Math.max(0, finiteNumber(result.collaboration_score));
    }
    if (seen.size !== roster.length) throw new Error('world checkpoint has an incomplete roster');
  }
  const maximumPoints = worldCheckpoints.length * 1_000;
  for (const entry of roster) {
    const total = totals.get(entry.model_id);
    entry.world_rating = roundRating((total.points / maximumPoints) * 100);
    entry.strategy_rating = roundRating(
      entry.overall_rating * strategyWeights.duel
      + entry.world_rating * strategyWeights.world,
    );
    entry.world_points = total.points;
    entry.world_round_wins = total.roundWins;
    entry.world_eliminations = total.eliminations;
    entry.world_deaths = total.deaths;
    entry.world_collaboration_score = total.collaboration;
  }
  roster.sort((left, right) => (
    right.strategy_rating - left.strategy_rating
    || right.overall_rating - left.overall_rating
    || right.world_rating - left.world_rating
    || right.collaboration_rating - left.collaboration_rating
    || left.provider_rank - right.provider_rank
  ));
  roster.forEach((entry, index) => { entry.rank = index + 1; });
  return roster;
}

export function assertBattleIntegrity(
  simulation,
  {
    expectedEngagements,
    expectedV2Fighters = expectedEngagements,
    requireCollaboration,
    requireV2 = true,
  },
) {
  if (!simulation || typeof simulation !== 'object') {
    throw new Error('arena simulation response is missing');
  }
  if (Math.trunc(finiteNumber(simulation.total_engagements)) !== expectedEngagements) {
    throw new Error(
      `unexpected engagement count: expected ${expectedEngagements}, got ${simulation.total_engagements}`,
    );
  }
  const dangerousWarning = (simulation.warnings || []).find((warning) => (
    /fallback|trap|fuel|wasm not found|runtime unavailable|instantiate failed/i.test(String(warning))
  ));
  if (dangerousWarning) {
    throw new Error(`unverified fighter runtime: ${dangerousWarning}`);
  }
  for (const field of ['fallback_count', 'trap_count', 'fuel_error_count']) {
    if (Math.trunc(finiteNumber(simulation[field], -1)) !== 0) {
      throw new Error(`unverified fighter runtime: ${field}=${simulation[field]}`);
    }
  }
  if (requireV2) {
    const normalizedExpectedV2Fighters = Math.max(1, expectedV2Fighters);
    const leftV2 = Math.trunc(finiteNumber(simulation.team_a_v2_fighters, -1));
    const rightV2 = Math.trunc(finiteNumber(simulation.team_b_v2_fighters, -1));
    if (leftV2 !== normalizedExpectedV2Fighters || rightV2 !== normalizedExpectedV2Fighters) {
      throw new Error(
        `v2 fighter integrity failed: expected ${normalizedExpectedV2Fighters} per side, got ${leftV2}/${rightV2}`,
      );
    }
  }
  if (requireCollaboration) {
    const left = collaborationValue(simulation, 'a');
    const right = collaborationValue(simulation, 'b');
    if (left === null || right === null) {
      throw new Error('v2 collaboration telemetry is required for team ratings');
    }
  }
  return true;
}

export { DEFAULT_WEIGHTS };
