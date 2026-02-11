#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const { chromium } = require("playwright");

function parseArgs(argv) {
  const args = {
    url: "http://127.0.0.1:18080/client.html?mode=mass&worker_cull=1&auto_connect=1&auto_reconnect=1",
    wsUrl: null,
    autoConnect: true,
    connectTimeoutMs: 60000,
    warmupMs: 3000,
    cycles: 8,
    mode: "mixed",
    settleMs: 1200,
    recoveryTimeoutMs: 15000,
    sampleIntervalMs: 200,
    minRecoveryRatio: 0.9,
    maxMedianRecoveryMs: 6000,
    headless: true,
    outPath: path.resolve(process.cwd(), "artifacts", "scale", "reconnect_chaos.json")
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--url") args.url = argv[++i];
    else if (arg === "--ws") args.wsUrl = argv[++i];
    else if (arg === "--auto-connect") args.autoConnect = true;
    else if (arg === "--no-auto-connect") args.autoConnect = false;
    else if (arg === "--connect-timeout-ms") args.connectTimeoutMs = Number(argv[++i]);
    else if (arg === "--warmup") args.warmupMs = Number(argv[++i]) * 1000;
    else if (arg === "--cycles") args.cycles = Number(argv[++i]);
    else if (arg === "--mode") args.mode = String(argv[++i] || "mixed").toLowerCase();
    else if (arg === "--settle-ms") args.settleMs = Number(argv[++i]);
    else if (arg === "--recovery-timeout-ms") args.recoveryTimeoutMs = Number(argv[++i]);
    else if (arg === "--sample-interval-ms") args.sampleIntervalMs = Number(argv[++i]);
    else if (arg === "--min-recovery-ratio") args.minRecoveryRatio = Number(argv[++i]);
    else if (arg === "--max-median-recovery-ms") args.maxMedianRecoveryMs = Number(argv[++i]);
    else if (arg === "--headed") args.headless = false;
    else if (arg === "--out") args.outPath = path.resolve(process.cwd(), argv[++i]);
    else if (arg === "--help") {
      printHelp();
      process.exit(0);
    }
  }

  args.connectTimeoutMs = Math.max(1000, Math.floor(Number(args.connectTimeoutMs) || 60000));
  args.warmupMs = Math.max(0, Math.floor(Number(args.warmupMs) || 0));
  args.cycles = Math.max(1, Math.min(200, Math.floor(Number(args.cycles) || 1)));
  args.mode = ["data", "signaling", "mixed"].includes(args.mode) ? args.mode : "mixed";
  args.settleMs = Math.max(0, Math.min(30000, Math.floor(Number(args.settleMs) || 0)));
  args.recoveryTimeoutMs = Math.max(1000, Math.min(120000, Math.floor(Number(args.recoveryTimeoutMs) || 15000)));
  args.sampleIntervalMs = Math.max(50, Math.min(5000, Math.floor(Number(args.sampleIntervalMs) || 200)));
  args.minRecoveryRatio = Math.max(0, Math.min(1, Number(args.minRecoveryRatio) || 0));
  args.maxMedianRecoveryMs = Math.max(100, Math.min(60000, Math.floor(Number(args.maxMedianRecoveryMs) || 6000)));
  return args;
}

function printHelp() {
  console.log(`Reconnect chaos options:
  --url <url>                       Page URL (default includes auto_connect/auto_reconnect)
  --ws <ws_url>                     Override wsUrl input before connect
  --auto-connect                    Click Connect and wait for live state (default: true)
  --no-auto-connect                 Do not auto-connect
  --connect-timeout-ms <ms>         Timeout waiting for initial live state
  --warmup <seconds>                Warmup before first chaos cycle (default: 3)
  --cycles <n>                      Number of disconnect/recover cycles (default: 8)
  --mode <data|signaling|mixed>     Disconnect type pattern (default: mixed)
  --settle-ms <ms>                  Delay between cycles (default: 1200)
  --recovery-timeout-ms <ms>        Max time allowed per recovery (default: 15000)
  --sample-interval-ms <ms>         Poll interval while waiting recovery (default: 200)
  --min-recovery-ratio <0-1>        Pass threshold for successful recoveries (default: 0.9)
  --max-median-recovery-ms <ms>     Pass threshold for median recovery time (default: 6000)
  --headed                          Run with visible browser
  --out <path>                      Output JSON path
`);
}

function toFixed(value, digits = 2) {
  if (!Number.isFinite(value)) return value;
  return Number(value.toFixed(digits));
}

function healthyStatus(status) {
  return status === "waiting" || status === "respawn" || status === "playing";
}

function percentile(values, p) {
  if (!Array.isArray(values) || values.length === 0) return 0;
  const sorted = values.slice().sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.max(0, Math.floor((sorted.length - 1) * p)));
  return sorted[index];
}

async function setWsUrl(page, wsUrl) {
  if (!wsUrl) return false;
  return page.evaluate((nextWs) => {
    const el = document.getElementById("wsUrl");
    if (!el) return false;
    el.value = nextWs;
    return true;
  }, wsUrl);
}

async function clickConnect(page) {
  const button = page.locator("#connectButton");
  if (!(await button.count())) return false;
  try {
    await button.click({ timeout: 3000 });
    return true;
  } catch (_) {
    return page.evaluate(() => {
      const btn = document.getElementById("connectButton");
      if (!btn) return false;
      btn.click();
      return true;
    });
  }
}

async function getConnectionSnapshot(page) {
  return page.evaluate(() => ({
    atMs: Number(performance.now().toFixed(1)),
    status: window.__e2e?.connectionStatus?.statusKey ?? null,
    detail: window.__e2e?.connectionStatus?.detailText ?? "",
    playerCount: Number(window.__e2e?.playerCount ?? 0),
    lastStateUpdate: Number(window.__e2e?.lastStateUpdate ?? 0),
    hasLocalPlayer: Boolean(window.__e2e?.hasLocalPlayer),
    autoReconnectEnabled: Boolean(window.__e2e?.autoReconnectEnabled),
    activeSignalingUrl: window.__e2e?.activeSignalingUrl ?? null
  }));
}

async function waitForLiveState(page, timeoutMs, sampleIntervalMs) {
  const startedAt = Date.now();
  let lastSnapshot = null;
  while (Date.now() - startedAt < timeoutMs) {
    lastSnapshot = await getConnectionSnapshot(page);
    if (healthyStatus(lastSnapshot.status) && lastSnapshot.lastStateUpdate > 0) {
      return {
        ok: true,
        recoveryMs: Date.now() - startedAt,
        snapshot: lastSnapshot
      };
    }
    await page.waitForTimeout(sampleIntervalMs);
  }
  return {
    ok: false,
    recoveryMs: Date.now() - startedAt,
    snapshot: lastSnapshot
  };
}

async function injectChaos(page, mode, cycleIndex) {
  return page.evaluate(
    ({ selectedMode, index }) => {
      const modeToUse = selectedMode === "mixed" ? (index % 2 === 0 ? "data" : "signaling") : selectedMode;
      let triggered = false;
      if (modeToUse === "data" && typeof window.__e2e?.forceCloseDataChannel === "function") {
        triggered = !!window.__e2e.forceCloseDataChannel();
      } else if (modeToUse === "signaling" && typeof window.__e2e?.forceCloseSignaling === "function") {
        triggered = !!window.__e2e.forceCloseSignaling();
      }
      return { mode: modeToUse, triggered };
    },
    { selectedMode: mode, index: cycleIndex }
  );
}

async function forceReconnectNow(page) {
  return page.evaluate(() => {
    if (typeof window.__e2e?.forceReconnectNow === "function") {
      return !!window.__e2e.forceReconnectNow();
    }
    return false;
  });
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const startedAt = Date.now();
  const browser = await chromium.launch({ headless: args.headless });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();

  const cycleResults = [];
  const failures = [];

  try {
    await page.goto(args.url, { waitUntil: "domcontentloaded", timeout: 60000 });
    if (args.wsUrl) await setWsUrl(page, args.wsUrl);
    if (args.autoConnect) {
      await clickConnect(page);
    }

    const initial = await waitForLiveState(page, args.connectTimeoutMs, args.sampleIntervalMs);
    if (!initial.ok) {
      throw new Error(`timed out waiting for initial live state (${args.connectTimeoutMs}ms)`);
    }

    if (args.warmupMs > 0) {
      await page.waitForTimeout(args.warmupMs);
    }

    for (let i = 0; i < args.cycles; i += 1) {
      const cycleNo = i + 1;
      let before = await getConnectionSnapshot(page);
      if (!healthyStatus(before.status)) {
        const preForce = await forceReconnectNow(page);
        if (preForce) {
          const preRecovered = await waitForLiveState(page, args.recoveryTimeoutMs, args.sampleIntervalMs);
          if (preRecovered.ok && preRecovered.snapshot) {
            before = preRecovered.snapshot;
          }
        }
      }

      let chaos = await injectChaos(page, args.mode, i);
      if (!chaos.triggered) {
        const forceRetry = await forceReconnectNow(page);
        if (forceRetry) {
          const rearmed = await waitForLiveState(page, args.recoveryTimeoutMs, args.sampleIntervalMs);
          if (rearmed.ok) {
            if (args.settleMs > 0) {
              await page.waitForTimeout(Math.min(args.settleMs, 1200));
            }
            chaos = await injectChaos(page, args.mode, i);
          }
        }
      }

      if (!chaos.triggered) {
        cycleResults.push({
          cycle: cycleNo,
          mode: chaos.mode,
          triggered: false,
          recovered: false,
          recoveryMs: null,
          before,
          after: await getConnectionSnapshot(page),
          error: "chaos trigger did not execute"
        });
        continue;
      }

      const recovered = await waitForLiveState(page, args.recoveryTimeoutMs, args.sampleIntervalMs);
      const after = recovered.snapshot || (await getConnectionSnapshot(page));
      cycleResults.push({
        cycle: cycleNo,
        mode: chaos.mode,
        triggered: true,
        recovered: recovered.ok,
        recoveryMs: recovered.ok ? recovered.recoveryMs : null,
        timeoutMs: recovered.ok ? null : recovered.recoveryMs,
        before,
        after
      });

      if (args.settleMs > 0) {
        await page.waitForTimeout(args.settleMs);
      }
    }

    const successfulRecoveries = cycleResults.filter((cycle) => cycle.recovered && Number.isFinite(cycle.recoveryMs));
    const recoveryTimes = successfulRecoveries.map((cycle) => cycle.recoveryMs);
    const recoveryRatio = args.cycles > 0 ? successfulRecoveries.length / args.cycles : 0;
    const medianRecoveryMs = recoveryTimes.length > 0 ? percentile(recoveryTimes, 0.5) : 0;
    const p95RecoveryMs = recoveryTimes.length > 0 ? percentile(recoveryTimes, 0.95) : 0;

    if (recoveryRatio < args.minRecoveryRatio) {
      failures.push(
        `Recovery ratio ${recoveryRatio.toFixed(3)} < min ${args.minRecoveryRatio.toFixed(3)}`
      );
    }
    if (medianRecoveryMs > args.maxMedianRecoveryMs) {
      failures.push(
        `Median recovery ${medianRecoveryMs}ms > max ${args.maxMedianRecoveryMs}ms`
      );
    }
    if (cycleResults.some((cycle) => !cycle.triggered)) {
      failures.push("One or more chaos cycles did not trigger");
    }

    const finalSnapshot = await getConnectionSnapshot(page);
    const result = {
      url: args.url,
      wsUrl: args.wsUrl,
      cycles: args.cycles,
      mode: args.mode,
      thresholds: {
        minRecoveryRatio: args.minRecoveryRatio,
        maxMedianRecoveryMs: args.maxMedianRecoveryMs
      },
      summary: {
        successfulRecoveries: successfulRecoveries.length,
        recoveryRatio: toFixed(recoveryRatio, 3),
        medianRecoveryMs: toFixed(medianRecoveryMs, 1),
        p95RecoveryMs: toFixed(p95RecoveryMs, 1),
        maxRecoveryMs: recoveryTimes.length ? Math.max(...recoveryTimes) : 0
      },
      finalSnapshot,
      cycleResults,
      passed: failures.length === 0,
      failures,
      startedAt: new Date(startedAt).toISOString(),
      finishedAt: new Date().toISOString()
    };

    fs.mkdirSync(path.dirname(args.outPath), { recursive: true });
    fs.writeFileSync(args.outPath, JSON.stringify(result, null, 2));
    console.log(JSON.stringify(result, null, 2));

    if (!result.passed) {
      process.exit(2);
    }
  } finally {
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
