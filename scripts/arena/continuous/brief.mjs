// Continuous Model League — improvement brief builder.
//
// Every 48h each active roster model with submissions left gets a revision
// prompt built from its own recent battles. The brief is a compact (< 2KB)
// plain-text digest fed to the server's /api/arena/code/revise route as the
// stats_digest (server cap: 8KB), containing: a behavior fingerprint (action
// distribution), the worst 3 matchups by loss rate, runtime fault counts,
// per-mode weaknesses, and an instruction paragraph for the codegen model.
//
// Battle sampling mirrors build_model_pages.mjs: battle checkpoints are far
// too numerous to read blindly, so directories are listed, stat'ed, sorted by
// mtime descending, and only the newest ~200 records per model are read. It
// is READ-ONLY — nothing here writes into the page cache or the season dirs.
// Arena model ids are day-derived, so each day season's materialized
// generation checkpoints provide the arena-id → provider-id mapping. All IO
// is injectable so tests run on fixtures.

import { promises as fs } from 'node:fs';
import path from 'node:path';

export const BRIEF_MAX_BYTES = 2_048;
export const PER_MODEL_SAMPLE_LIMIT = 200;

// Same action set as build_model_pages.mjs (bot_tick_v2 telemetry).
const ACTIONS = Object.freeze(['idle', 'attack', 'defend', 'charge', 'support']);

const defaultIo = {
  readdir: (dir) => fs.readdir(dir),
  statMtimeMs: async (dir, name) => (await fs.stat(path.join(dir, name))).mtimeMs,
  readJson: async (file) => JSON.parse(await fs.readFile(file, 'utf8')),
};

const zeroCounts = () => ({ idle: 0, attack: 0, defend: 0, charge: 0, support: 0 });

function actionCountsOf(raw) {
  const counts = zeroCounts();
  for (const action of ACTIONS) counts[action] = Number(raw?.[action]) || 0;
  return counts;
}

/**
 * Compact record from one battle checkpoint file (same extraction pattern as
 * build_model_pages.mjs battleRecord, plus the runtime fault counters).
 */
function battleCheckpointRecord(mtimeMs, json) {
  const sim = json?.simulation;
  if (!sim || !json.model_a_id || !json.model_b_id) return null;
  return {
    m: mtimeMs,
    mode: String(sim.mode || json.mode || 'unknown'),
    a: json.model_a_id,
    b: json.model_b_id,
    winner: sim.winner_model_id || null,
    draw: sim.draw === true,
    ac: actionCountsOf(sim.team_a_action_counts),
    bc: actionCountsOf(sim.team_b_action_counts),
    faults: {
      trap: Number(sim.trap_count) || 0,
      fuel: Number(sim.fuel_error_count) || 0,
      fallback: Number(sim.fallback_count) || 0,
    },
  };
}

/**
 * List battle checkpoints as relative names, covering both the legacy flat
 * layout (`<task>.json`) and the sharded layout (`<2-hex>/<task>.json`).
 */
async function listBattleJsons(io, dir) {
  const names = [];
  for (const entry of await io.readdir(dir)) {
    if (entry.endsWith('.json')) {
      names.push(entry);
      continue;
    }
    if (!/^[0-9a-f]{2}$/.test(entry)) continue;
    let inner;
    try {
      inner = await io.readdir(path.join(dir, entry));
    } catch {
      continue; // Not a shard directory (or vanished).
    }
    for (const name of inner) {
      if (name.endsWith('.json')) names.push(`${entry}/${name}`);
    }
  }
  return names;
}

/**
 * Sample a roster model's newest battle records across the league's day
 * season directories (newest day first, newest files first within a day),
 * capped at perModelLimit. Returns normalized records:
 *   { mode, me, opponent, winner, draw, counts, faults, m }
 * with provider model ids on both sides. Battles not involving the model and
 * files that fail to parse are skipped.
 */
export async function sampleModelBattles({
  seasonsDirectory,
  leagueId,
  dayIndex,
  modelId,
  perModelLimit = PER_MODEL_SAMPLE_LIMIT,
  io = defaultIo,
}) {
  const records = [];
  for (let day = dayIndex - 1; day >= 0 && records.length < perModelLimit; day -= 1) {
    const seasonDirectory = path.join(
      seasonsDirectory,
      `continuous-${leagueId}-day${day}`,
    );
    let generationNames;
    try {
      generationNames = (await io.readdir(path.join(seasonDirectory, 'generations')))
        .filter((name) => name.endsWith('.json'));
    } catch {
      continue; // No materialized generation layout for that day.
    }
    const providerByArenaId = new Map();
    for (const name of generationNames) {
      try {
        const checkpoint = await io.readJson(path.join(seasonDirectory, 'generations', name));
        if (checkpoint?.model_id && checkpoint?.provider_model) {
          providerByArenaId.set(checkpoint.model_id, checkpoint.provider_model);
        }
      } catch {
        // Skip an unreadable checkpoint; the id map is best-effort.
      }
    }

    const battlesDirectory = path.join(seasonDirectory, 'battles');
    let names;
    try {
      names = await listBattleJsons(io, battlesDirectory);
    } catch {
      continue; // No battles for that day.
    }
    const listed = [];
    for (const name of names) {
      try {
        listed.push([name, await io.statMtimeMs(battlesDirectory, name)]);
      } catch {
        // File vanished between listing and stat.
      }
    }
    listed.sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]));

    for (const [name, mtime] of listed) {
      if (records.length >= perModelLimit) break;
      let json;
      try {
        json = await io.readJson(path.join(battlesDirectory, name));
      } catch {
        continue;
      }
      const record = battleCheckpointRecord(mtime, json);
      if (!record) continue;
      const providerA = providerByArenaId.get(record.a);
      const providerB = providerByArenaId.get(record.b);
      if (providerA !== modelId && providerB !== modelId) continue;
      const mine = providerA === modelId;
      records.push({
        mode: record.mode,
        me: modelId,
        opponent: (mine ? providerB : providerA) ?? null,
        winner: record.winner ? providerByArenaId.get(record.winner) ?? record.winner : null,
        draw: record.draw,
        counts: mine ? record.ac : record.bc,
        faults: record.faults,
        m: record.m,
      });
    }
  }
  return records;
}

const pct = (fraction) => `${Math.round(fraction * 100)}%`;

function truncateToBytes(text, maxBytes) {
  if (Buffer.byteLength(text, 'utf8') <= maxBytes) return text;
  const suffix = ' …';
  let out = text;
  while (out && Buffer.byteLength(out + suffix, 'utf8') > maxBytes) {
    out = out.slice(0, -8);
  }
  return out + suffix;
}

/**
 * Build the improvement brief for one roster model from its sampled battle
 * records (see sampleModelBattles). Always returns at most maxBytes bytes.
 */
export function buildBrief({ model, records, maxBytes = BRIEF_MAX_BYTES }) {
  const sample = Array.isArray(records) ? records : [];
  const counts = zeroCounts();
  const faults = { trap: 0, fuel: 0, fallback: 0 };
  const rivals = new Map();
  const modes = new Map();
  let wins = 0;
  let losses = 0;
  let draws = 0;
  for (const record of sample) {
    for (const action of ACTIONS) counts[action] += Number(record.counts?.[action]) || 0;
    faults.trap += Number(record.faults?.trap) || 0;
    faults.fuel += Number(record.faults?.fuel) || 0;
    faults.fallback += Number(record.faults?.fallback) || 0;
    const outcome = record.draw ? 'd' : record.winner === record.me ? 'w' : record.winner ? 'l' : 'd';
    if (outcome === 'w') wins += 1;
    else if (outcome === 'l') losses += 1;
    else draws += 1;
    const mode = record.mode || 'unknown';
    if (!modes.has(mode)) modes.set(mode, { w: 0, l: 0, d: 0 });
    modes.get(mode)[outcome] += 1;
    if (record.opponent) {
      if (!rivals.has(record.opponent)) rivals.set(record.opponent, { w: 0, l: 0, d: 0 });
      rivals.get(record.opponent)[outcome] += 1;
    }
  }

  const actionTotal = ACTIONS.reduce((sum, action) => sum + counts[action], 0);
  const share = (action) => (actionTotal > 0 ? counts[action] / actionTotal : 0);
  const aggression = share('attack') + share('charge');

  const recordOf = (entry) => ({ ...entry, games: entry.w + entry.l + entry.d });
  const lossRate = (entry) => (entry.games > 0 ? entry.l / entry.games : 0);
  const byWeakness = (left, right) => lossRate(right) - lossRate(left)
    || right.games - left.games;
  const modeSummaries = [...modes.entries()]
    .map(([mode, entry]) => ({ mode, ...recordOf(entry) }))
    .sort((a, b) => byWeakness(a, b) || a.mode.localeCompare(b.mode));
  const rivalSummaries = [...rivals.entries()]
    .map(([opponent, entry]) => ({ opponent, ...recordOf(entry) }))
    .sort((a, b) => byWeakness(a, b) || a.opponent.localeCompare(b.opponent));
  const worst = rivalSummaries.slice(0, 3);

  const lines = [];
  lines.push(
    `Arena fighter improvement brief — ${model.model_id} `
    + `(artifact v${model.artifact?.version ?? 1}, rating ${model.rating}, `
    + `league record ${model.wins}-${model.losses}-${model.draws}).`,
  );
  if (sample.length === 0) {
    lines.push('No sampled battles yet; make a robust all-round improvement.');
  } else {
    lines.push(`Sampled ${sample.length} recent battles: ${wins}W-${losses}L-${draws}D.`);
    lines.push(
      'Behavior fingerprint: '
      + ACTIONS.map((action) => `${action} ${pct(share(action))}`).join(', ')
      + ` (aggression ${aggression.toFixed(2)}).`,
    );
    lines.push(
      `Per-mode record: ${modeSummaries.map((summary) => (
        `${summary.mode} ${summary.w}-${summary.l}-${summary.d} (loses ${pct(lossRate(summary))})`
      )).join(', ')}.`,
    );
    lines.push(
      worst.length > 0
        ? `Worst matchups: ${worst.map((summary) => (
          `vs ${summary.opponent} ${summary.w}-${summary.l}-${summary.d} (loses ${pct(lossRate(summary))})`
        )).join('; ')}.`
        : 'Worst matchups: none on record.',
    );
    lines.push(
      `Runtime faults in sample: traps ${faults.trap}, `
      + `fuel errors ${faults.fuel}, fallbacks ${faults.fallback}.`,
    );
  }

  const focuses = [];
  const weakest = modeSummaries[0];
  if (weakest && weakest.games > 0 && lossRate(weakest) >= 0.5) {
    focuses.push(
      `your weakest mode is ${weakest.mode} (loses ${pct(lossRate(weakest))}); `
      + 'rework objective play, positioning, and target selection for it',
    );
  }
  if (worst.length > 0 && lossRate(worst[0]) >= 0.5) {
    focuses.push(
      `you lose most games against ${worst.map((summary) => summary.opponent).join(', ')}; `
      + 'counter their pressure without abandoning your strengths',
    );
  }
  const faultTotal = faults.trap + faults.fuel + faults.fallback;
  if (faultTotal > 0) {
    focuses.push(`eliminate the ${faultTotal} runtime traps/fuel errors/fallbacks in the sample`);
  }
  if (focuses.length === 0) {
    focuses.push('make a conservative robustness improvement rather than a risky rewrite');
  }
  lines.push(
    'Instructions: revise the fighter bot, keeping the bot_tick_v2 ABI and the '
    + `source-size limit. ${focuses.join('; ')}. Preserve what already works.`,
  );
  return truncateToBytes(lines.join('\n'), maxBytes);
}
