#!/usr/bin/env node
// Aggregate per-mode excitement from the append-only match summary history
// (data/live_replay/matches/match_summaries.jsonl). Dynamic-transition
// matches are attributed phase by phase; fixed-mode matches count as one
// phase. Prints a ranking and writes static_client/media/excitement.json.
//
// Usage: node excitement_report.mjs [--history FILE] [--out FILE]
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, '../..');

const args = process.argv.slice(2);
let historyPath = path.join(REPO_ROOT, 'data/live_replay/matches/match_summaries.jsonl');
let outPath = path.join(REPO_ROOT, 'static_client/media/excitement.json');
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--history') historyPath = path.resolve(args[++i]);
  else if (args[i] === '--out') outPath = path.resolve(args[++i]);
  else throw new Error(`unknown arg: ${args[i]}`);
}

async function readLines(file) {
  try {
    const text = await readFile(file, 'utf8');
    return text.split('\n').filter(Boolean);
  } catch {
    return [];
  }
}

const lines = [
  ...(await readLines(`${historyPath}.1`)),
  ...(await readLines(historyPath)),
];
if (!lines.length) {
  console.error(`no match history at ${historyPath} yet — play some matches first`);
  process.exit(0);
}

const modes = new Map(); // mode -> aggregate
function bucket(mode) {
  let entry = modes.get(mode);
  if (!entry) {
    entry = { mode, phases: 0, seconds: 0, kills: 0, marginSum: 0, marginCount: 0 };
    modes.set(mode, entry);
  }
  return entry;
}

let matches = 0;
for (const line of lines) {
  let summary;
  try {
    summary = JSON.parse(line);
  } catch {
    continue;
  }
  matches++;
  const phases = Array.isArray(summary.phases) && summary.phases.length
    ? summary.phases
    : [{
        game_mode: summary.game_mode,
        duration_secs: summary.match_duration,
        kills: summary.total_kills ?? 0,
      }];
  for (const phase of phases) {
    const entry = bucket(phase.game_mode || 'Unknown');
    entry.phases++;
    entry.seconds += Math.max(0, Number(phase.duration_secs) || 0);
    entry.kills += Math.max(0, Number(phase.kills) || 0);
  }
  // Margin belongs to the match's final competitive standing.
  const finalEntry = bucket(summary.game_mode || 'Unknown');
  if (Number.isFinite(summary.final_score_margin)) {
    finalEntry.marginSum += Math.abs(summary.final_score_margin);
    finalEntry.marginCount++;
  }
}

const report = [...modes.values()]
  .filter((entry) => entry.seconds > 30)
  .map((entry) => {
    const killsPerMinute = entry.kills / (entry.seconds / 60);
    const avgMargin = entry.marginCount ? entry.marginSum / entry.marginCount : null;
    // Tempo, damped by blowout margins: a mode that ends 200-0 is less
    // exciting than the same tempo ending 90-80.
    const closeness = avgMargin === null ? 1 : 1 / (1 + avgMargin / 80);
    return {
      mode: entry.mode,
      phases: entry.phases,
      minutes: +(entry.seconds / 60).toFixed(1),
      kills: entry.kills,
      kills_per_minute: +killsPerMinute.toFixed(2),
      avg_final_margin: avgMargin === null ? null : +avgMargin.toFixed(1),
      excitement: +(killsPerMinute * closeness).toFixed(2),
    };
  })
  .sort((a, b) => b.excitement - a.excitement);

console.log(`matches analyzed: ${matches}`);
for (const row of report) {
  console.log(
    `${row.mode.padEnd(16)} excitement ${String(row.excitement).padStart(6)}  ` +
    `${String(row.kills_per_minute).padStart(5)} kills/min over ${row.minutes} min ` +
    `(${row.phases} phases${row.avg_final_margin === null ? '' : `, avg margin ${row.avg_final_margin}`})`,
  );
}

await mkdir(path.dirname(outPath), { recursive: true });
await writeFile(
  outPath,
  JSON.stringify({ generated_at: new Date().toISOString(), matches, modes: report }, null, 2),
);
console.log(`wrote ${outPath}`);
