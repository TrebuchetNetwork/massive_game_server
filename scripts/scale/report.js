#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");

function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--steps") args.steps = argv[++i];
    else if (arg === "--ui") args.ui = argv[++i];
    else if (arg === "--multi") args.multi = argv[++i];
    else if (arg === "--out") args.out = argv[++i];
    else if (arg === "--md") args.md = argv[++i];
    else if (arg === "--help") {
      printHelp();
      process.exit(0);
    }
  }
  return args;
}

function printHelp() {
  console.log(`Usage: node report.js --steps <steps.tsv> --ui <ui.json> --multi <multi.json> --out <summary.json> --md <summary.md>`);
}

function readJsonIfExists(filePath) {
  if (!filePath || !fs.existsSync(filePath)) return null;
  try {
    return JSON.parse(fs.readFileSync(filePath, "utf8"));
  } catch (err) {
    return { parseError: String(err.message || err) };
  }
}

function parseSteps(filePath) {
  if (!filePath || !fs.existsSync(filePath)) return [];
  const content = fs.readFileSync(filePath, "utf8");
  const lines = content.split(/\r?\n/).filter(Boolean);
  if (lines.length <= 1) return [];
  const steps = [];
  for (let i = 1; i < lines.length; i += 1) {
    const cols = lines[i].split("\t");
    if (cols.length < 3) continue;
    const [name, status, logPath] = cols;
    steps.push({
      name,
      status,
      logPath,
      passed: status === "PASS",
    });
  }
  return steps;
}

function parseStressMetrics(logPath, label) {
  if (!logPath || !fs.existsSync(logPath)) return null;
  const content = fs.readFileSync(logPath, "utf8");
  const re = new RegExp(
    `\\[stress:${label}\\]\\s+samples=(\\d+)\\s+avg_ms=([\\d.]+)\\s+p95_ms=([\\d.]+)\\s+max_ms=([\\d.]+)`
  );
  const match = content.match(re);
  if (!match) return null;
  return {
    samples: Number(match[1]),
    avgMs: Number(match[2]),
    p95Ms: Number(match[3]),
    maxMs: Number(match[4]),
  };
}

function findStep(steps, name) {
  return steps.find((step) => step.name === name) || null;
}

function toMarkdown(summary) {
  const lines = [];
  lines.push("# Scale Summary");
  lines.push("");
  lines.push(`- Generated: ${summary.generatedAt}`);
  lines.push(`- Overall: ${summary.passed ? "PASS" : "FAIL"}`);
  lines.push("");
  lines.push("## Steps");
  lines.push("");
  lines.push("| Step | Status | Log |");
  lines.push("| --- | --- | --- |");
  for (const step of summary.steps) {
    lines.push(`| ${step.name} | ${step.status} | ${step.logPath} |`);
  }
  lines.push("");
  lines.push("## Metrics");
  lines.push("");

  if (summary.metrics.stressBaseline) {
    const m = summary.metrics.stressBaseline;
    lines.push(`- Baseline Stress: samples=${m.samples}, avg=${m.avgMs}ms, p95=${m.p95Ms}ms, max=${m.maxMs}ms`);
  } else {
    lines.push("- Baseline Stress: unavailable");
  }

  if (summary.metrics.stressBots) {
    const m = summary.metrics.stressBots;
    lines.push(`- Bot Stress: samples=${m.samples}, avg=${m.avgMs}ms, p95=${m.p95Ms}ms, max=${m.maxMs}ms`);
  } else {
    lines.push("- Bot Stress: unavailable");
  }

  if (summary.metrics.uiBench) {
    const m = summary.metrics.uiBench;
    lines.push(`- UI Bench: fps=${m.fps}, longTasks=${m.longTasks}, heapGrowthMb=${m.heapGrowthMb}, passed=${m.passed}`);
  } else {
    lines.push("- UI Bench: unavailable");
  }

  if (summary.metrics.multiClient) {
    const m = summary.metrics.multiClient;
    lines.push(
      `- Multi-Client: connectedRatio=${m.connectedRatio}, healthyFinal=${m.clientsHealthyFinal}/${m.clientsRequested}, passed=${m.passed}`
    );
  } else {
    lines.push("- Multi-Client: unavailable");
  }

  return `${lines.join("\n")}\n`;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.steps || !args.out || !args.md) {
    printHelp();
    process.exit(1);
  }

  const steps = parseSteps(args.steps);
  const uiBench = readJsonIfExists(args.ui);
  const multiClient = readJsonIfExists(args.multi);

  const stressBaselineStep = findStep(steps, "Backend stress baseline");
  const stressBotsStep = findStep(steps, "Backend stress bots");
  const stressBaseline = stressBaselineStep ? parseStressMetrics(stressBaselineStep.logPath, "baseline") : null;
  const stressBots = stressBotsStep ? parseStressMetrics(stressBotsStep.logPath, "bots") : null;

  const passed = steps.length > 0 && steps.every((step) => step.passed);

  const summary = {
    generatedAt: new Date().toISOString(),
    passed,
    steps,
    metrics: {
      stressBaseline,
      stressBots,
      uiBench,
      multiClient,
    },
  };

  const outPath = path.resolve(args.out);
  const mdPath = path.resolve(args.md);
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.mkdirSync(path.dirname(mdPath), { recursive: true });
  fs.writeFileSync(outPath, JSON.stringify(summary, null, 2));
  fs.writeFileSync(mdPath, toMarkdown(summary));

  console.log(
    JSON.stringify(
      {
        passed: summary.passed,
        steps: summary.steps.length,
        out: outPath,
        markdown: mdPath,
      },
      null,
      2
    )
  );
}

main();
