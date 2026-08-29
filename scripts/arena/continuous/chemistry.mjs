// Continuous Model League — mixed-team chemistry evaluation.
//
// Answers "which models work best together" with real battles: every match is
// a mixed-squad team battle (team A = half the roster, team B = the other
// half, each fighter driven by its own model's WASM) run through the
// additive /api/arena/matches/simulate_mixed_team_battle endpoint. The
// deterministic schedule guarantees every model pair shares a squad at least
// K times; the aggregation compares each pair's actual win rate against the
// win rate expected from the models' solo ratings.
//
// Logic functions are pure and side-effect free; all IO lives in main() and
// runChemistryEvaluation (HTTP via the shared arena client, writes via the
// league's atomicWriteJson).

import { promises as fs } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

import { arenaApiJson } from '../arena_api_client.mjs';
import { atomicWriteJson, statePathFor } from './state.mjs';

export const CHEMISTRY_SCHEMA_VERSION = 1;
export const CHEMISTRY_KIND = 'mixed_team_chemistry';
export const DEFAULT_K = 2;
export const DEFAULT_SEED = 1;
export const DEFAULT_MODE = 'tdm';
export const DEFAULT_ROUNDS = 1;
export const DEFAULT_MAX_TICKS = 240;
export const DEFAULT_TOP_MODELS = 10;
export const PROVISIONAL_MIN_GAMES = 3;
export const MIXED_BATTLE_ROUTE = '/api/arena/matches/simulate_mixed_team_battle';
const SCHEDULE_CANDIDATES_PER_MATCH = 64;
const MAX_SCHEDULE_MATCHES = 256;

/** Canonical pair key: order-independent, so (a,b) and (b,a) collapse. */
export function pairKey(left, right) {
  return [String(left), String(right)].sort().join('|');
}

/** All C(n,2) model pairs as sorted [a, b] tuples, deterministically ordered. */
export function allPairs(modelIds) {
  const ids = [...modelIds].sort();
  const pairs = [];
  for (let i = 0; i < ids.length; i += 1) {
    for (let j = i + 1; j < ids.length; j += 1) {
      pairs.push([ids[i], ids[j]]);
    }
  }
  return pairs;
}

/** Deterministic PRNG (mulberry32) so a (seed, roster) pair always yields the same schedule. */
function mulberry32(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6D2B79F5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function matchSeed(seed, matchIndex) {
  return (
    Math.imul((seed ^ 0x9E3779B9) >>> 0, 0x85EBCA6B)
    + Math.imul((matchIndex + 1) >>> 0, 0xC2B2AE35)
  ) >>> 0;
}

function shuffled(values, random) {
  const copy = [...values];
  for (let i = copy.length - 1; i > 0; i -= 1) {
    const j = Math.floor(random() * (i + 1));
    [copy[i], copy[j]] = [copy[j], copy[i]];
  }
  return copy;
}

function sameSquadPairs(squad) {
  const pairs = [];
  for (let i = 0; i < squad.length; i += 1) {
    for (let j = i + 1; j < squad.length; j += 1) {
      pairs.push(pairKey(squad[i], squad[j]));
    }
  }
  return pairs;
}

/**
 * Deterministic mixed-match schedule covering every model pair as teammates
 * at least `k` times. Each match splits the roster into two squads of
 * `squadSize` (default floor(n/2)); candidates are drawn from a seeded PRNG
 * and the partition covering the most still-under-covered pairs wins, so
 * partners and opponents vary across the schedule.
 *
 * Returns { k, seed, squad_size, complete, matches, coverage } where each
 * match carries its own deterministic battle seed and `coverage` maps pair
 * keys to games-together counts.
 */
export function buildChemistrySchedule(modelIds, { k = DEFAULT_K, seed = DEFAULT_SEED, squadSize, maxMatches = MAX_SCHEDULE_MATCHES } = {}) {
  const ids = [...new Set((Array.isArray(modelIds) ? modelIds : []).map(String))].sort();
  if (ids.length < 4) throw new Error('chemistry schedule needs at least 4 models');
  if (!Number.isSafeInteger(k) || k < 1) throw new Error('k must be a positive integer');
  if (!Number.isSafeInteger(seed) || seed < 0) throw new Error('seed must be a non-negative integer');
  const size = squadSize ?? Math.floor(ids.length / 2);
  if (!Number.isSafeInteger(size) || size < 2) throw new Error('squadSize must be an integer >= 2');
  if (2 * size > ids.length) throw new Error('squadSize too large for the roster');

  const target = new Map(allPairs(ids).map(([a, b]) => [pairKey(a, b), 0]));
  const random = mulberry32(seed);
  const matches = [];

  const uncoveredDeficit = (squads) => {
    let deficit = 0;
    for (const squad of squads) {
      for (const key of sameSquadPairs(squad)) {
        const count = target.get(key) ?? k;
        if (count < k) deficit += k - count;
      }
    }
    return deficit;
  };

  while ([...target.values()].some((count) => count < k) && matches.length < maxMatches) {
    let best = null;
    let bestDeficit = -1;
    for (let candidate = 0; candidate < SCHEDULE_CANDIDATES_PER_MATCH; candidate += 1) {
      const draw = shuffled(ids, random);
      const squads = [draw.slice(0, size), draw.slice(size, 2 * size)];
      const deficit = uncoveredDeficit(squads);
      if (deficit > bestDeficit) {
        bestDeficit = deficit;
        best = squads;
      }
      if (deficit === 0) break;
    }
    const [teamA, teamB] = best.map((squad) => [...squad].sort());
    for (const squad of best) {
      for (const key of sameSquadPairs(squad)) {
        if (target.has(key)) target.set(key, target.get(key) + 1);
      }
    }
    matches.push({
      match_id: `chem-${String(matches.length + 1).padStart(3, '0')}`,
      seed: matchSeed(seed, matches.length),
      team_a_models: teamA,
      team_b_models: teamB,
    });
  }

  return {
    k,
    seed,
    squad_size: size,
    complete: [...target.values()].every((count) => count >= k),
    matches,
    coverage: Object.fromEntries([...target.entries()].sort(([a], [b]) => a.localeCompare(b))),
  };
}

/** League ratings are 0–100 win-percentage scores; map onto an Elo-like scale. */
export function eloFromRating(rating) {
  const value = Number(rating);
  return ((Number.isFinite(value) ? value : 50) - 50) * 8;
}

/** Logistic expectation that squad A beats squad B from mean squad Elos. */
export function expectedSquadWinRate(squadRatings, opponentRatings) {
  const mean = (values) => values.reduce((sum, value) => sum + value, 0) / values.length;
  const diff = mean(squadRatings.map(eloFromRating)) - mean(opponentRatings.map(eloFromRating));
  return 1 / (1 + 10 ** (-diff / 400));
}

const round4 = (value) => Math.round(value * 10_000) / 10_000;

/**
 * Fold executed mixed matches into per-pair chemistry stats. Each entry in
 * `matches` needs { team_a_models, team_b_models, winner_side, draw }.
 * `ratings` maps model_id to its solo rating (missing → 50). Every pair stat
 * carries games_together, wins/draws/losses, win_rate (draws count half),
 * expected_win_rate (mean of per-game logistic expectations from solo
 * ratings), rating_delta_vs_expected (actual − expected), and a provisional
 * flag while games_together < provisionalMinGames.
 */
export function aggregateChemistry({ matches, ratings = {}, provisionalMinGames = PROVISIONAL_MIN_GAMES } = {}) {
  const pairs = new Map();
  const ratingOf = (modelId) => {
    const value = Number(ratings[modelId]);
    return Number.isFinite(value) ? value : 50;
  };

  for (const match of Array.isArray(matches) ? matches : []) {
    const teamA = [...(match?.team_a_models || [])].map(String);
    const teamB = [...(match?.team_b_models || [])].map(String);
    const draw = match?.draw === true || match?.winner_side == null;
    for (const [side, squad, opponents] of [['team_a', teamA, teamB], ['team_b', teamB, teamA]]) {
      const expected = expectedSquadWinRate(squad.map(ratingOf), opponents.map(ratingOf));
      const won = !draw && match.winner_side === side;
      for (const key of sameSquadPairs(squad)) {
        if (!pairs.has(key)) {
          pairs.set(key, { games_together: 0, wins: 0, draws: 0, losses: 0, expected_sum: 0 });
        }
        const stats = pairs.get(key);
        stats.games_together += 1;
        stats.expected_sum += expected;
        if (draw) stats.draws += 1;
        else if (won) stats.wins += 1;
        else stats.losses += 1;
      }
    }
  }

  const rows = [...pairs.entries()].map(([key, stats]) => {
    const winRate = (stats.wins + 0.5 * stats.draws) / stats.games_together;
    const expectedWinRate = stats.expected_sum / stats.games_together;
    return {
      models: key.split('|'),
      games_together: stats.games_together,
      wins: stats.wins,
      draws: stats.draws,
      losses: stats.losses,
      win_rate: round4(winRate),
      expected_win_rate: round4(expectedWinRate),
      rating_delta_vs_expected: round4(winRate - expectedWinRate),
      provisional: stats.games_together < provisionalMinGames,
    };
  });
  rows.sort((left, right) => (
    right.win_rate - left.win_rate
    || right.rating_delta_vs_expected - left.rating_delta_vs_expected
    || left.models[0].localeCompare(right.models[0])
    || left.models[1].localeCompare(right.models[1])
  ));
  return { pairs: rows };
}

/**
 * Validate a simulate_mixed_team_battle response against its frozen request:
 * mode, rosters, sizes, and — critically — that every fighter entry is
 * attributed to the roster model of its (side, slot). Throws on mismatch.
 */
export function validateMixedSimulation(simulation, match) {
  const expectedA = [...match.team_a_models].map(String);
  const expectedB = [...match.team_b_models].map(String);
  const sameIds = (actual, expected) => (
    Array.isArray(actual)
    && actual.length === expected.length
    && actual.every((value, index) => String(value) === expected[index])
  );
  if (
    simulation?.mode !== 'mixed_team'
    || simulation?.match_mode !== match.mode
    || !sameIds(simulation?.team_a_models, expectedA)
    || !sameIds(simulation?.team_b_models, expectedB)
    || Number(simulation?.team_size) !== expectedA.length
    || Number(simulation?.rounds) !== match.rounds
    || Number(simulation?.seed) !== match.seed
  ) {
    throw new Error('mixed battle response does not match its frozen request');
  }
  if (simulation.draw !== (simulation.winner_side == null)) {
    throw new Error('mixed battle response has an inconsistent winner/draw result');
  }
  if (!simulation.draw && !['team_a', 'team_b'].includes(simulation.winner_side)) {
    throw new Error('mixed battle response names an unknown winner side');
  }
  const fighters = simulation.fighters;
  if (!Array.isArray(fighters) || fighters.length !== expectedA.length + expectedB.length) {
    throw new Error('mixed battle response is missing per-fighter attribution');
  }
  const seen = new Set();
  for (const fighter of fighters) {
    const roster = fighter.side === 'team_a' ? expectedA : fighter.side === 'team_b' ? expectedB : null;
    const slot = Number(fighter.slot);
    const marker = `${fighter.side}:${slot}`;
    if (!roster || !Number.isSafeInteger(slot) || slot < 0 || slot >= roster.length || seen.has(marker)) {
      throw new Error('mixed battle response has an invalid fighter entry');
    }
    seen.add(marker);
    if (String(fighter.model_id) !== roster[slot]) {
      throw new Error(
        `fighter attribution mismatch: ${marker} is ${fighter.model_id}, expected ${roster[slot]}`,
      );
    }
    for (const field of ['eliminations', 'deaths']) {
      if (!Number.isSafeInteger(Number(fighter[field])) || Number(fighter[field]) < 0) {
        throw new Error(`fighter ${marker} has an invalid ${field} count`);
      }
    }
  }
  return simulation;
}

/** Compact per-match summary persisted in the chemistry artifact. */
export function summarizeMixedMatch(match, simulation) {
  return {
    match_id: match.match_id,
    seed: match.seed,
    mode: match.mode,
    team_a_models: [...match.team_a_models],
    team_b_models: [...match.team_b_models],
    winner_side: simulation.winner_side,
    draw: simulation.draw,
    team_a_objective: simulation.total_team_a_objective,
    team_b_objective: simulation.total_team_b_objective,
    team_a_score: simulation.total_team_a_score,
    team_b_score: simulation.total_team_b_score,
    fighters: simulation.fighters.map((fighter) => ({
      side: fighter.side,
      slot: fighter.slot,
      model_id: fighter.model_id,
      runtime: fighter.runtime,
      eliminations: fighter.eliminations,
      deaths: fighter.deaths,
      personal_score: fighter.personal_score,
      collaboration_score: fighter.collaboration_score,
    })),
  };
}

/**
 * Execute a full chemistry evaluation: build the schedule, run every mixed
 * battle through `apiJson` (injectable; defaults to the shared arena client
 * against `apiBase`), validate attribution, aggregate pair stats. Returns
 * { schedule, matches, aggregation }.
 */
export async function runChemistryEvaluation({
  apiBase,
  adminToken,
  models,
  k = DEFAULT_K,
  seed = DEFAULT_SEED,
  mode = DEFAULT_MODE,
  rounds = DEFAULT_ROUNDS,
  maxTicks = DEFAULT_MAX_TICKS,
  apiJson,
} = {}) {
  const roster = (Array.isArray(models) ? models : []).map((entry) => ({
    model_id: String(entry?.model_id || ''),
    rating: Number(entry?.rating),
    server_model_id: String(entry?.server_model_id || entry?.model_id || ''),
  })).filter((entry) => entry.model_id);
  const serverIdOf = new Map(roster.map((entry) => [entry.model_id, entry.server_model_id]));
  const callApi = apiJson || ((request) => arenaApiJson({ apiBase, adminToken, ...request }));
  const schedule = buildChemistrySchedule(roster.map((entry) => entry.model_id), { k, seed });
  const ratings = Object.fromEntries(roster.map((entry) => [entry.model_id, entry.rating]));

  const summaries = [];
  for (const scheduled of schedule.matches) {
    const match = { ...scheduled, mode, rounds };
    // The server validates fighters against its season-scoped registry ids;
    // aggregation below stays in stable league-id space.
    const serverMatch = {
      ...match,
      team_a_models: match.team_a_models.map((id) => serverIdOf.get(id) || id),
      team_b_models: match.team_b_models.map((id) => serverIdOf.get(id) || id),
    };
    const data = await callApi({
      method: 'POST',
      route: MIXED_BATTLE_ROUTE,
      timeoutMs: 180_000,
      body: {
        team_a_models: serverMatch.team_a_models,
        team_b_models: serverMatch.team_b_models,
        mode,
        rounds,
        seed: match.seed,
        max_ticks: maxTicks,
      },
    });
    const simulation = validateMixedSimulation(data?.simulation, serverMatch);
    summaries.push(summarizeMixedMatch(match, simulation));
  }

  return {
    schedule,
    matches: summaries,
    aggregation: aggregateChemistry({ matches: summaries, ratings }),
  };
}

/** Final artifact shape persisted under artifacts/arena/continuous/chemistry/. */
export function buildChemistryArtifact({
  generatedAt,
  leagueId = null,
  track = null,
  seasonId = null,
  models,
  k,
  seed,
  mode,
  rounds,
  maxTicks,
  evaluation,
}) {
  return {
    schema_version: CHEMISTRY_SCHEMA_VERSION,
    kind: CHEMISTRY_KIND,
    generated_at: generatedAt,
    league_id: leagueId,
    track,
    season_id: seasonId,
    k,
    seed,
    mode,
    rounds,
    max_ticks: maxTicks,
    models: models.map((entry) => ({
      model_id: entry.model_id,
      server_model_id: entry.server_model_id ?? entry.model_id,
      rating: entry.rating,
    })),
    schedule_complete: evaluation.schedule.complete,
    schedule: evaluation.schedule.matches,
    coverage: evaluation.schedule.coverage,
    matches: evaluation.matches,
    pairs: evaluation.aggregation.pairs,
    notes: 'Sample sizes are small by design; pairs with fewer than 3 games together are provisional.',
  };
}

/** Pick the chemistry roster: top `top` active models by rating, model_id as tie-break. */
export function selectChemistryModels(roster, top = DEFAULT_TOP_MODELS) {
  return (Array.isArray(roster) ? roster : [])
    .filter((entry) => entry && entry.model_id && (entry.status === undefined || entry.status === 'active'))
    .map((entry) => ({
      model_id: String(entry.model_id),
      rating: Number(entry.rating) || 0,
      wasm_sha256: entry?.artifact?.wasm_sha256 ?? null,
    }))
    .sort((left, right) => right.rating - left.rating || left.model_id.localeCompare(right.model_id))
    .slice(0, top);
}

/**
 * Resolve league model ids to the season-scoped ids the arena server
 * registers fighters under (`orw-<date>-…-<slug>`). The continuous league
 * evaluates through per-day seasons whose rosters bind provider_model +
 * wasm_sha256; the latest day season of the track is the current binding.
 * A model whose state artifact hash differs from the season binding is
 * rejected rather than silently measured against stale bytes. Models are
 * returned with `server_model_id` filled in; when no season directory exists
 * at all, league ids pass through unchanged (the caller may already be
 * passing store-native ids, e.g. via --models-file).
 */
export async function resolveServerModelIds({ artifactsRoot, leagueId, track, models, io }) {
  const seasonsDir = path.join(artifactsRoot, 'seasons');
  const fsx = io || await import('node:fs/promises');
  let names = [];
  try {
    names = await fsx.readdir(seasonsDir);
  } catch {
    return { models: models.map((entry) => ({ ...entry, server_model_id: entry.model_id })), season_id: null, day: null };
  }
  const prefix = `continuous-${leagueId}-${track}-day`;
  const days = names
    .filter((name) => name.startsWith(prefix))
    .map((name) => ({ name, day: Number.parseInt(name.slice(prefix.length), 10) }))
    .filter((entry) => Number.isSafeInteger(entry.day))
    .sort((a, b) => b.day - a.day);
  if (!days.length) {
    return { models: models.map((entry) => ({ ...entry, server_model_id: entry.model_id })), season_id: null, day: null };
  }
  for (const { name, day } of days) {
    let season = null;
    try {
      season = JSON.parse(await fsx.readFile(path.join(seasonsDir, name, 'season.json'), 'utf8'));
    } catch {
      continue;
    }
    const byProvider = new Map(
      (Array.isArray(season?.roster) ? season.roster : [])
        .filter((entry) => entry?.provider_model && entry?.model_id)
        .map((entry) => [String(entry.provider_model), entry]),
    );
    const resolved = [];
    let complete = true;
    for (const model of models) {
      const binding = byProvider.get(model.model_id);
      if (!binding) {
        complete = false;
        break;
      }
      if (model.wasm_sha256 && binding.wasm_sha256 && binding.wasm_sha256 !== model.wasm_sha256) {
        complete = false;
        break;
      }
      resolved.push({ ...model, server_model_id: String(binding.model_id) });
    }
    if (complete) return { models: resolved, season_id: name, day };
  }
  throw new Error(
    `no completed ${track} day season binds the current roster artifacts — run the daily evaluation first`,
  );
}

function parseArgs(argv) {
  const options = {
    stateDir: null,
    track: 'L2',
    k: DEFAULT_K,
    seed: DEFAULT_SEED,
    mode: DEFAULT_MODE,
    rounds: DEFAULT_ROUNDS,
    maxTicks: DEFAULT_MAX_TICKS,
    top: DEFAULT_TOP_MODELS,
    outDir: null,
    apiBase: (process.env.ARENA_API_BASE || 'http://127.0.0.1:8080').replace(/\/$/, ''),
    modelsFile: null,
    dryRun: false,
    help: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = () => {
      index += 1;
      if (index >= argv.length) throw new Error(`${arg} requires a value`);
      return argv[index];
    };
    if (arg === '--help' || arg === '-h') options.help = true;
    else if (arg === '--dry-run') options.dryRun = true;
    else if (arg === '--state-dir') options.stateDir = path.resolve(next());
    else if (arg === '--track') options.track = next();
    else if (arg === '--k') options.k = Number.parseInt(next(), 10);
    else if (arg === '--seed') options.seed = Number.parseInt(next(), 10);
    else if (arg === '--mode') options.mode = next();
    else if (arg === '--rounds') options.rounds = Number.parseInt(next(), 10);
    else if (arg === '--max-ticks') options.maxTicks = Number.parseInt(next(), 10);
    else if (arg === '--top') options.top = Number.parseInt(next(), 10);
    else if (arg === '--out-dir') options.outDir = path.resolve(next());
    else if (arg === '--api-base') options.apiBase = next().replace(/\/$/, '');
    else if (arg === '--models-file') options.modelsFile = path.resolve(next());
    else throw new Error(`unknown argument: ${arg}`);
  }
  return options;
}

function usage() {
  return `Usage: node scripts/arena/continuous/chemistry.mjs [options]

Runs one mixed-team chemistry evaluation round: a deterministic schedule of
mixed-squad battles covering every model pair as teammates, aggregated into
per-pair win rates vs rating-based expectations.

Options:
  --state-dir <dir>   continuous league state dir (default artifacts/arena/continuous)
  --track <id>        league track to draw the roster from (default L2)
  --models-file <f>   JSON [{model_id, rating}] roster override (skips state.json)
  --k <n>             teammate coverage target per pair (default ${DEFAULT_K})
  --seed <n>          schedule seed (default ${DEFAULT_SEED})
  --top <n>           roster size, top-N by rating (default ${DEFAULT_TOP_MODELS})
  --mode <mode>       battle mode (default ${DEFAULT_MODE})
  --rounds <n>        rounds per battle (default ${DEFAULT_ROUNDS})
  --max-ticks <n>     tick cap per round (default ${DEFAULT_MAX_TICKS})
  --out-dir <dir>     artifact dir (default <state-dir>/chemistry)
  --api-base <url>    arena API base (default env ARENA_API_BASE or http://127.0.0.1:8080)
  --dry-run           print the schedule plan without executing battles
`;
}

async function readAdminToken() {
  const direct = (process.env.ARENA_ADMIN_BEARER_TOKEN || '').trim();
  if (direct) return direct;
  const filePath = (process.env.ARENA_ADMIN_BEARER_TOKEN_FILE || '').trim();
  if (!filePath) return null;
  const value = (await fs.readFile(filePath, 'utf8')).trim();
  return value || null;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    process.stdout.write(usage());
    return;
  }
  const scriptDir = path.dirname(fileURLToPath(import.meta.url));
  const rootDir = path.resolve(scriptDir, '../../..');
  const stateDir = options.stateDir || path.join(rootDir, 'artifacts/arena/continuous');
  const outDir = options.outDir || path.join(stateDir, 'chemistry');

  let models;
  let leagueId = null;
  let track = null;
  if (options.modelsFile) {
    models = selectChemistryModels(JSON.parse(await fs.readFile(options.modelsFile, 'utf8')), options.top);
  } else {
    const state = JSON.parse(await fs.readFile(statePathFor(stateDir), 'utf8'));
    const slice = state?.tracks?.[options.track];
    if (!slice || !Array.isArray(slice.roster)) {
      throw new Error(`state.json has no roster for track ${options.track}`);
    }
    leagueId = state.league_id || null;
    track = options.track;
    models = selectChemistryModels(slice.roster, options.top);
  }
  if (models.length < 4) {
    throw new Error(`chemistry needs at least 4 models, found ${models.length}`);
  }

  // Translate league ids to the server's season-scoped fighter registry ids.
  const resolved = await resolveServerModelIds({
    artifactsRoot: path.join(rootDir, 'artifacts/arena'),
    leagueId,
    track,
    models,
  });
  models = resolved.models;

  if (options.dryRun) {
    const schedule = buildChemistrySchedule(models.map((entry) => entry.model_id), {
      k: options.k,
      seed: options.seed,
    });
    process.stdout.write(`${JSON.stringify({
      track,
      models,
      k: options.k,
      seed: options.seed,
      squad_size: schedule.squad_size,
      matches: schedule.matches.length,
      complete: schedule.complete,
      coverage: schedule.coverage,
      schedule: schedule.matches,
    }, null, 2)}\n`);
    return;
  }

  const adminToken = await readAdminToken();
  if (!adminToken) {
    throw new Error('ARENA_ADMIN_BEARER_TOKEN or ARENA_ADMIN_BEARER_TOKEN_FILE is required');
  }
  const generatedAt = new Date().toISOString();
  const evaluation = await runChemistryEvaluation({
    apiBase: options.apiBase,
    adminToken,
    models,
    k: options.k,
    seed: options.seed,
    mode: options.mode,
    rounds: options.rounds,
    maxTicks: options.maxTicks,
  });
  const artifact = buildChemistryArtifact({
    generatedAt,
    leagueId,
    track,
    seasonId: resolved.season_id,
    models,
    k: options.k,
    seed: options.seed,
    mode: options.mode,
    rounds: options.rounds,
    maxTicks: options.maxTicks,
    evaluation,
  });
  await fs.mkdir(outDir, { recursive: true });
  const target = path.join(outDir, `${generatedAt.slice(0, 10)}.json`);
  await atomicWriteJson(target, artifact);
  const topPairs = artifact.pairs.slice(0, 5)
    .map((pair) => `${pair.models.join(' + ')} wr=${pair.win_rate} n=${pair.games_together}${pair.provisional ? ' (provisional)' : ''}`);
  process.stdout.write(
    `[chemistry] ${artifact.matches.length} mixed battles, ${artifact.pairs.length} pairs -> ${target}\n`
    + topPairs.map((line) => `[chemistry]   ${line}\n`).join(''),
  );
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1]);
if (isMain) {
  main().catch((error) => {
    process.stderr.write(`chemistry failed: ${error?.message || error}\n`);
    process.exitCode = 1;
  });
}
