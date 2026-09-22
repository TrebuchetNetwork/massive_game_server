// Intervention Lift benchmark — the citable answer to "does feedback actually
// make models better?", computed from the continuous league's 4-track state.
//
// Pure module: no IO. Used by build_model_pages.mjs to render
// static_client/models/benchmark.html + benchmark.json.
//
// Design (multi-track amendment + field-effect control):
//
//   Every model fields the SAME v1 artifact in every track; tracks diverge
//   only by intervention policy (L0 never revised, L1 compile-fix only,
//   L2 two feedback revisions, L3 weekly feedback). A model's cross-track
//   rating delta (L2 − L0, L3 − L0) mixes two effects:
//
//     (a) intervention lift   — revisions actually improved the fighter
//     (b) field composition   — different opponents across tracks (divisions
//                               are derived per track, so the same rating can
//                               mean different things against different fields)
//
//   The control group separates them: models whose artifact is still v1 in
//   EVERY track never received gameplay feedback, so their cross-track deltas
//   measure pure field composition. The published net lift is
//   median(revised deltas) − median(control deltas).

import { TRACKS } from './continuous/league.mjs';

// Aggregates computed from fewer than this many models are marked provisional.
export const MIN_GROUP_N = 5;

export const METHODOLOGY = {
  summary: 'Cross-track rating deltas against each model\'s own L0 zero-shot baseline, with a never-revised control group subtracting field-composition effects.',
  lift: 'Lift is the per-model rating delta between an intervention track and the same model\'s L0 zero-shot baseline (same v1 artifact, same season length). Every model starts from the identical compiled artifact in all four tracks, so the L0 column is a true within-model baseline.',
  control: 'Cross-track deltas also reflect field composition: divisions are derived per track, so opponents differ across tracks. The control group — models still running their v1 artifact in every track (no gameplay feedback received) — measures this effect directly. Net lift = median(revised-group delta) − median(control-group delta).',
  confounds: [
    'Field composition is controlled only at the median; individual rows still mix both effects.',
    'Small samples: groups below n=5 are marked provisional.',
    'Selection effects: revisions are earned by surviving models; retired models appear at their final rating (🪦), which biases the revised group both ways.',
    'Ratings are relative to each track\'s current field and drift over time; the league day differs slightly per track.',
  ],
};

/** Median of a finite numeric array; null when empty. */
export function median(values) {
  const xs = (Array.isArray(values) ? values : []).filter((v) => Number.isFinite(v));
  if (!xs.length) return null;
  xs.sort((a, b) => a - b);
  const mid = Math.floor(xs.length / 2);
  return xs.length % 2 ? xs[mid] : (xs[mid - 1] + xs[mid]) / 2;
}

const round2 = (v) => (Number.isFinite(v) ? Math.round(v * 100) / 100 : null);

/**
 * Per-model benchmark rows from a validated v2 league state. Each track cell
 * prefers the active roster entry and falls back to the retired ledger's
 * final rating (flagged `retired`) — excluding retired models would silently
 * drop every model that aged out of any track (a survivor-only table) and
 * would leave the never-revised control group nearly empty, since field
 * composition is exactly what retires models differentially across tracks.
 * The remaining selection effects are listed in the methodology.
 *
 * Rows are sorted by L3 lift desc (then L2 lift, then L0 rating); models
 * without an L0 baseline carry null lifts and sort last.
 */
export function benchmarkRows(state) {
  const byModel = new Map(); // model_id -> { slug, mascot, cells: {track -> {entry, retired}} }
  for (const trackId of TRACKS) {
    const slice = state.tracks[trackId] || {};
    for (const [list, retired] of [[slice.roster || [], false], [slice.retired || [], true]]) {
      for (const e of list) {
        const key = String(e.model_id);
        if (!byModel.has(key)) byModel.set(key, { model_id: key, slug: e.slug, mascot: e.mascot, cells: {} });
        const cells = byModel.get(key).cells;
        // Active roster wins over a stale retired record (re-recruited stint).
        const existing = cells[trackId];
        if (existing && (!existing.retired || retired)) continue;
        cells[trackId] = { entry: e, retired };
      }
    }
  }

  const rows = [...byModel.values()].map((row) => {
    const cell = (t) => row.cells[t] || null;
    const rating = (t) => (cell(t) ? round2(cell(t).entry.rating) : null);
    const version = (t) => cell(t)?.entry?.artifact?.version ?? null;
    const l0 = rating('L0');
    const l2 = rating('L2');
    const l3 = rating('L3');
    const versions = TRACKS.map((t) => version(t)).filter((v) => v !== null);
    const revised = versions.some((v) => v >= 2);
    const fieldControl = versions.length > 0 && versions.every((v) => v === 1);
    return {
      slug: row.slug,
      model_id: row.model_id,
      mascot: row.mascot,
      l0,
      l1: rating('L1'),
      l2,
      l3,
      l2_version: version('L2'),
      l3_version: version('L3'),
      retired_in: TRACKS.filter((t) => cell(t)?.retired),
      l2_lift: l0 !== null && l2 !== null ? round2(l2 - l0) : null,
      l3_lift: l0 !== null && l3 !== null ? round2(l3 - l0) : null,
      revised,
      field_control: fieldControl,
    };
  });

  const liftKey = (r) => (r.l3_lift ?? r.l2_lift ?? null);
  return rows.sort((a, b) => {
    const la = liftKey(a);
    const lb = liftKey(b);
    if (la !== null && lb !== null) return lb - la || (b.l2_lift ?? -Infinity) - (a.l2_lift ?? -Infinity);
    if (la !== null) return -1;
    if (lb !== null) return 1;
    return (b.l0 ?? -1) - (a.l0 ?? -1) || String(a.slug).localeCompare(String(b.slug));
  });
}

function groupAggregate(rows, liftField) {
  const revisedLifts = rows.filter((r) => r.revised && r[liftField] !== null).map((r) => r[liftField]);
  const controlLifts = rows.filter((r) => r.field_control && r[liftField] !== null).map((r) => r[liftField]);
  const revisedMedian = median(revisedLifts);
  const controlMedian = median(controlLifts);
  return {
    revised_n: revisedLifts.length,
    revised_median: round2(revisedMedian),
    control_n: controlLifts.length,
    control_median: round2(controlMedian),
    net: revisedMedian !== null && controlMedian !== null ? round2(revisedMedian - controlMedian) : null,
    provisional: revisedLifts.length < MIN_GROUP_N || controlLifts.length < MIN_GROUP_N,
  };
}

/** Full benchmark: rows + group aggregates for L2 and L3 lifts. */
export function computeBenchmark(state) {
  const rows = benchmarkRows(state);
  return {
    league_id: state.league_id,
    day_index: Math.max(...TRACKS.map((t) => state.tracks[t]?.day_index ?? 0)),
    rows,
    aggregates: {
      l2: groupAggregate(rows, 'l2_lift'),
      l3: groupAggregate(rows, 'l3_lift'),
      models_total: rows.length,
      revised_total: rows.filter((r) => r.revised).length,
      control_total: rows.filter((r) => r.field_control).length,
    },
  };
}

/** Machine-readable citation payload emitted as models/benchmark.json. */
export function benchmarkJson(benchmark, generatedAt) {
  return {
    schema_version: 1,
    benchmark: 'intervention-lift',
    league_id: benchmark.league_id,
    day_index: benchmark.day_index,
    generated_at: generatedAt,
    min_group_n: MIN_GROUP_N,
    methodology: METHODOLOGY,
    aggregates: benchmark.aggregates,
    rows: benchmark.rows,
  };
}
