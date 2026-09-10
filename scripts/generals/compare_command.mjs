#!/usr/bin/env node
// Does having a general actually help?
//
// Reads the match summary history and compares matches where exactly one side
// was commanded against the uncommanded baseline. Because both teams field the
// same ten fighter models, a single-sided commanded match is a controlled A/B:
// same map, same roster, same conditions, one difference. The commanding side
// alternates session to session, so a spawn-side advantage cancels out — and
// the baseline below measures that bias directly so we can tell the two apart.
//
// Usage: node compare_command.mjs [--json OUT] [--since-days N]

import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, '../..');
const SUMMARY_FILES = [
  path.join(REPO_ROOT, 'data/live_replay/matches/match_summaries.jsonl.1'),
  path.join(REPO_ROOT, 'data/live_replay/matches/match_summaries.jsonl'),
];

const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const SINCE_DAYS = Number(opt('since-days', 0));
const JSON_OUT = opt('json', path.join(REPO_ROOT, 'static_client/media/generals_report.json'));

async function loadSummaries() {
  const rows = [];
  for (const file of SUMMARY_FILES) {
    let text;
    try { text = await readFile(file, 'utf8'); } catch (_) { continue; }
    for (const line of text.split('\n')) {
      if (!line.trim()) continue;
      try { rows.push(JSON.parse(line)); } catch (_) { /* skip partial line */ }
    }
  }
  const cutoff = SINCE_DAYS > 0 ? Date.now() - SINCE_DAYS * 86400000 : 0;
  return rows
    .filter((r) => (r.generated_at_ms || 0) >= cutoff)
    .filter((r) => r.game_mode === 'TeamDeathmatch' || r.game_mode === 'CaptureTheFlag')
    // The gauntlet is structurally lopsided by design (10 model allies vs a
    // larger generic wave), so it cannot sit in the same pool as an even
    // exhibition match without swamping the side-bias baseline.
    .filter((r) => !r.coop_gauntlet);
}

const mean = (xs) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : 0);

// Two-sided exact binomial test against p=0.5. Small n, so exact beats normal.
function binomialTwoSided(wins, n) {
  if (!n) return 1;
  const logFact = (k) => { let s = 0; for (let i = 2; i <= k; i++) s += Math.log(i); return s; };
  const pmf = (k) => Math.exp(logFact(n) - logFact(k) - logFact(n - k) - n * Math.log(2));
  const observed = pmf(wins);
  let p = 0;
  for (let k = 0; k <= n; k++) {
    const v = pmf(k);
    if (v <= observed + 1e-12) p += v;
  }
  return Math.min(1, p);
}

function scoreFor(row, teamId) {
  const players = row.players || [];
  return players.filter((p) => p.team_id === teamId).reduce((a, p) => a + (p.score || 0), 0);
}

function analyse(rows) {
  const single = [];   // exactly one side commanded: the controlled test
  const both = [];     // both sides commanded: a showcase, not a test
  const none = [];     // baseline

  for (const row of rows) {
    const generals = Array.isArray(row.generals) ? row.generals.filter((g) => g && g.orders > 0) : [];
    if (generals.length === 1) single.push({ row, general: generals[0] });
    else if (generals.length > 1) both.push(row);
    else none.push(row);
  }

  // Baseline side bias: how often does team 1 win when nobody commands?
  const decidedNone = none.filter((r) => r.winning_team === 1 || r.winning_team === 2);
  const team1Wins = decidedNone.filter((r) => r.winning_team === 1).length;

  // The test: did the commanded side win?
  const decidedSingle = single.filter(({ row }) => row.winning_team === 1 || row.winning_team === 2);
  const commandedWins = decidedSingle.filter(({ row, general }) => row.winning_team === general.team_id).length;
  const draws = single.length - decidedSingle.length;

  const marginForCommanded = decidedSingle.map(({ row, general }) => {
    const other = general.team_id === 1 ? 2 : 1;
    return scoreFor(row, general.team_id) - scoreFor(row, other);
  });

  const perModel = new Map();
  for (const { row, general } of decidedSingle) {
    const key = general.model_id;
    const rec = perModel.get(key) || { model: key, name: general.model_name, played: 0, won: 0, orders: 0 };
    rec.played += 1;
    if (row.winning_team === general.team_id) rec.won += 1;
    rec.orders += general.orders || 0;
    perModel.set(key, rec);
  }

  return {
    generated_at: new Date().toISOString(),
    window_days: SINCE_DAYS || null,
    matches_total: rows.length,
    commanded_one_side: {
      matches: single.length,
      decided: decidedSingle.length,
      draws,
      commanded_side_wins: commandedWins,
      commanded_side_win_rate: decidedSingle.length ? commandedWins / decidedSingle.length : null,
      p_value_vs_coin_flip: decidedSingle.length ? binomialTwoSided(commandedWins, decidedSingle.length) : null,
      mean_score_margin_for_commanded: marginForCommanded.length ? mean(marginForCommanded) : null,
      mean_kills: single.length ? mean(single.map(({ row }) => row.total_kills || 0)) : null,
      mean_kills_per_minute: single.length ? mean(single.map(({ row }) => row.kills_per_minute || 0)) : null,
    },
    commanded_both_sides: {
      matches: both.length,
      mean_kills: both.length ? mean(both.map((r) => r.total_kills || 0)) : null,
      mean_kills_per_minute: both.length ? mean(both.map((r) => r.kills_per_minute || 0)) : null,
    },
    uncommanded: {
      matches: none.length,
      decided: decidedNone.length,
      team1_win_rate: decidedNone.length ? team1Wins / decidedNone.length : null,
      side_bias_p_value: decidedNone.length ? binomialTwoSided(team1Wins, decidedNone.length) : null,
      mean_kills: none.length ? mean(none.map((r) => r.total_kills || 0)) : null,
      mean_kills_per_minute: none.length ? mean(none.map((r) => r.kills_per_minute || 0)) : null,
    },
    by_model: [...perModel.values()].sort((a, b) => b.played - a.played),
  };
}

function pct(x) { return x === null || x === undefined ? 'n/a' : `${(x * 100).toFixed(1)}%`; }
function num(x, d = 1) { return x === null || x === undefined ? 'n/a' : x.toFixed(d); }

function render(report) {
  const c = report.commanded_one_side;
  const u = report.uncommanded;
  const lines = [];
  lines.push(`Generals: commanded vs uncommanded  (${report.matches_total} team matches${report.window_days ? `, last ${report.window_days}d` : ''})`);
  lines.push('');
  lines.push(`Controlled test — exactly one side commanded: ${c.matches} matches (${c.decided} decided, ${c.draws} drawn)`);
  if (c.decided) {
    lines.push(`  commanded side won ${c.commanded_side_wins}/${c.decided} = ${pct(c.commanded_side_win_rate)}  (p=${num(c.p_value_vs_coin_flip, 3)} vs a coin flip)`);
    lines.push(`  mean score margin for the commanded side: ${num(c.mean_score_margin_for_commanded, 2)}`);
  } else {
    lines.push('  not enough decided matches yet');
  }
  lines.push(`  tempo: ${num(c.mean_kills)} kills, ${num(c.mean_kills_per_minute, 2)}/min`);
  lines.push('');
  lines.push(`Baseline — nobody commanding: ${u.matches} matches (${u.decided} decided)`);
  lines.push(`  team 1 win rate ${pct(u.team1_win_rate)} (p=${num(u.side_bias_p_value, 3)}) — this is the spawn-side bias to subtract`);
  lines.push(`  tempo: ${num(u.mean_kills)} kills, ${num(u.mean_kills_per_minute, 2)}/min`);
  if (report.commanded_both_sides.matches) {
    const b = report.commanded_both_sides;
    lines.push('');
    lines.push(`Both sides commanded (showcase, not a test): ${b.matches} matches, ${num(b.mean_kills)} kills, ${num(b.mean_kills_per_minute, 2)}/min`);
  }
  if (report.by_model.length) {
    lines.push('');
    lines.push('By commanding model:');
    for (const m of report.by_model) {
      lines.push(`  ${m.name || m.model}: ${m.won}/${m.played} won, ${m.orders} orders`);
    }
  }
  return lines.join('\n');
}

const rows = await loadSummaries();
const report = analyse(rows);
console.log(render(report));
await writeFile(JSON_OUT, JSON.stringify(report, null, 2));
console.log(`\nreport written to ${path.relative(REPO_ROOT, JSON_OUT)}`);
