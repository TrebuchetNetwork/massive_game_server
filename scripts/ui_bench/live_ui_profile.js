#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const { chromium } = require("playwright");
const { buildLaunchOptions, urlRequestsWebGpu } = require("./launch_options");

function parseArgs(argv) {
  const args = {
    url: "http://localhost:8080/client.html?profile=1",
    durationMs: 30000,
    warmupMs: 5000,
    sampleIntervalMs: 1000,
    connectTimeoutMs: 60000,
    topPhases: 10,
    fpsThreshold: 0,
    maxLongTasks: -1,
    maxHeapGrowthMb: -1,
    autoConnect: true,
    wsUrl: null,
    headless: true,
    headlessExplicit: false,
    outPath: path.resolve(process.cwd(), "artifacts", "ui_profile.json")
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--url") args.url = argv[++i];
    else if (arg === "--duration") args.durationMs = Number(argv[++i]) * 1000;
    else if (arg === "--warmup") args.warmupMs = Number(argv[++i]) * 1000;
    else if (arg === "--sample-interval-ms") args.sampleIntervalMs = Number(argv[++i]);
    else if (arg === "--connect-timeout-ms") args.connectTimeoutMs = Number(argv[++i]);
    else if (arg === "--top-phases") args.topPhases = Number(argv[++i]);
    else if (arg === "--fps-threshold") args.fpsThreshold = Number(argv[++i]);
    else if (arg === "--max-long-tasks") args.maxLongTasks = Number(argv[++i]);
    else if (arg === "--max-heap-growth-mb") args.maxHeapGrowthMb = Number(argv[++i]);
    else if (arg === "--ws") args.wsUrl = argv[++i];
    else if (arg === "--out") args.outPath = path.resolve(process.cwd(), argv[++i]);
    else if (arg === "--headed") {
      args.headless = false;
      args.headlessExplicit = true;
    }
    else if (arg === "--headless") {
      args.headless = true;
      args.headlessExplicit = true;
    }
    else if (arg === "--no-auto-connect") args.autoConnect = false;
    else if (arg === "--help") {
      printHelp();
      process.exit(0);
    }
  }

  return args;
}

function printHelp() {
  console.log(`Live UI profile options:
  --url <url>                 Page URL (default: localhost client with profile=1)
  --duration <seconds>        Profile duration (default: 30)
  --warmup <seconds>          Warmup duration before profiling (default: 5)
  --sample-interval-ms <ms>   Sample interval (default: 1000)
  --connect-timeout-ms <ms>   Timeout waiting for live state (default: 60000)
  --top-phases <count>        Number of ranked phases in output (default: 10)
  --fps-threshold <fps>       Optional FPS gate (0 disables)
  --max-long-tasks <count>    Optional long task gate (-1 disables)
  --max-heap-growth-mb <mb>   Optional heap growth gate (-1 disables)
  --ws <ws_url>               Set wsUrl input before connect
  --headed                    Show browser UI
  --headless                  Force headless mode
  --no-auto-connect           Do not click connect button
  --out <path>                Output JSON path (default: artifacts/ui_profile.json)
  --help                      Show help
`);
}

function ensureProfileParam(urlValue) {
  const url = new URL(urlValue);
  if (url.searchParams.get("profile") !== "1" && url.searchParams.get("perf") !== "1") {
    url.searchParams.set("profile", "1");
  }
  return url.toString();
}

function healthyStatus(statusKey) {
  return statusKey === "waiting" || statusKey === "respawn" || statusKey === "playing";
}

function toFixedNumber(value, digits = 2) {
  if (!Number.isFinite(value)) return value;
  return Number(value.toFixed(digits));
}

async function waitForLiveState(page, timeoutMs) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    const state = await page.evaluate(() => {
      const status = window.__e2e?.connectionStatus?.statusKey ?? null;
      const detailText = window.__e2e?.connectionStatus?.detailText ?? "";
      return {
        status,
        detailText,
        matchInfoReady: Boolean(window.__e2e?.matchInfoReady),
        lastStateUpdate: Number(window.__e2e?.lastStateUpdate ?? 0),
        hasLocalPlayer: Boolean(window.__e2e?.hasLocalPlayer)
      };
    });

    if (healthyStatus(state.status) && state.matchInfoReady && state.lastStateUpdate > 0) {
      return state;
    }

    if (state.status === "error") {
      throw new Error(`client entered error state: ${state.detailText || "unknown"}`);
    }

    await page.waitForTimeout(250);
  }
  throw new Error(`timed out after ${timeoutMs}ms waiting for connected live state`);
}

async function setWsUrl(page, wsUrl) {
  return page.evaluate((nextWsUrl) => {
    const wsInput = document.getElementById("wsUrl");
    if (!wsInput) return false;
    wsInput.value = nextWsUrl;
    return true;
  }, wsUrl);
}

async function clickConnect(page) {
  const connectButton = page.locator("#connectButton");
  if (!(await connectButton.count())) {
    return false;
  }
  try {
    await connectButton.click({ timeout: 3000 });
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

function summarizeSamples(samples) {
  if (!Array.isArray(samples) || samples.length === 0) {
    return {
      sampleCount: 0,
      maxPlayerCountSeen: 0,
      statusTransitions: [],
      stalePerfSamples: 0
    };
  }

  let maxPlayerCountSeen = 0;
  let stalePerfSamples = 0;
  const statusTransitions = [];
  let lastStatus = null;

  for (const sample of samples) {
    if (Number.isFinite(sample.playerCount) && sample.playerCount > maxPlayerCountSeen) {
      maxPlayerCountSeen = sample.playerCount;
    }
    if (!sample.hasFreshPerfReport) {
      stalePerfSamples += 1;
    }
    if (sample.status !== lastStatus) {
      statusTransitions.push({ atMs: sample.atMs, status: sample.status });
      lastStatus = sample.status;
    }
  }

  return {
    sampleCount: samples.length,
    maxPlayerCountSeen,
    statusTransitions,
    stalePerfSamples
  };
}

async function main() {
  const rawArgs = parseArgs(process.argv.slice(2));
  const args = {
    ...rawArgs,
    url: ensureProfileParam(rawArgs.url)
  };
  const webgpuRequested = urlRequestsWebGpu(args.url);
  if (webgpuRequested && !args.headlessExplicit) {
    args.headless = false;
  }

  const startWallTime = Date.now();

  const launchOptions = buildLaunchOptions({ headless: args.headless, url: args.url });
  const browser = await chromium.launch(launchOptions);
  const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  const page = await context.newPage();

  await page.addInitScript(() => {
    window.__uiLiveProfile = {
      frames: 0,
      longTasks: 0,
      longTaskTotalMs: 0,
      longTaskMaxMs: 0,
      running: false,
      startTime: 0,
      endTime: 0
    };

    const tick = () => {
      if (window.__uiLiveProfile.running) {
        window.__uiLiveProfile.frames += 1;
      }
      window.requestAnimationFrame(tick);
    };
    window.requestAnimationFrame(tick);

    if ("PerformanceObserver" in window) {
      try {
        const observer = new PerformanceObserver((list) => {
          for (const entry of list.getEntries()) {
            if (!window.__uiLiveProfile.running) continue;
            window.__uiLiveProfile.longTasks += 1;
            const duration = Number(entry.duration || 0);
            window.__uiLiveProfile.longTaskTotalMs += duration;
            if (duration > window.__uiLiveProfile.longTaskMaxMs) {
              window.__uiLiveProfile.longTaskMaxMs = duration;
            }
          }
        });
        observer.observe({ entryTypes: ["longtask"] });
      } catch (_) {
        // Long task API may be unavailable in some environments.
      }
    }
  });

  let result = null;

  try {
    await page.goto(args.url, { waitUntil: "domcontentloaded", timeout: 60000 });

    if (args.wsUrl) {
      await setWsUrl(page, args.wsUrl);
    }

    if (args.autoConnect) {
      await clickConnect(page);
    }

    const liveState = await waitForLiveState(page, args.connectTimeoutMs);

    if (args.warmupMs > 0) {
      await page.waitForTimeout(args.warmupMs);
    }

    const heapStart = await page.evaluate(() => {
      return performance.memory ? performance.memory.usedJSHeapSize : null;
    });

    await page.evaluate(() => {
      if (window.__e2e?.setPerfProfiling) {
        window.__e2e.setPerfProfiling(true);
      }
      if (window.__e2e?.resetPerfStats) {
        window.__e2e.resetPerfStats();
      }
      window.__uiLiveProfile.frames = 0;
      window.__uiLiveProfile.longTasks = 0;
      window.__uiLiveProfile.longTaskTotalMs = 0;
      window.__uiLiveProfile.longTaskMaxMs = 0;
      window.__uiLiveProfile.startTime = performance.now();
      window.__uiLiveProfile.endTime = 0;
      window.__uiLiveProfile.running = true;
    });

    const sampleCount = Math.max(1, Math.ceil(args.durationMs / Math.max(100, args.sampleIntervalMs)));
    const samples = [];

    for (let i = 0; i < sampleCount; i += 1) {
      await page.waitForTimeout(Math.max(100, args.sampleIntervalMs));
      const sample = await page.evaluate(() => {
        const perfReport = window.__e2e?.perfReport ?? null;
        const perfGeneratedAt = Number(window.__e2e?.perfReportGeneratedAt ?? 0);
        const now = performance.now();
        const topPhase = Array.isArray(perfReport?.rankedPhases) ? perfReport.rankedPhases[0] : null;
        const playerCountRaw = document.getElementById("playerCount")?.textContent ?? "0";
        const playerCount = Number.parseInt(playerCountRaw, 10);

        return {
          atMs: Number(now.toFixed(1)),
          status: window.__e2e?.connectionStatus?.statusKey ?? null,
          playerCount: Number.isFinite(playerCount) ? playerCount : 0,
          hasFreshPerfReport: perfGeneratedAt > 0 && now - perfGeneratedAt < 5000,
          topPhase: topPhase
            ? {
                name: topPhase.name,
                avgMs: topPhase.avgMs,
                maxMs: topPhase.maxMs,
                dutyCyclePct: topPhase.dutyCyclePct,
                sharePct: topPhase.sharePct
              }
            : null
        };
      });

      samples.push(sample);
    }

    const heapEnd = await page.evaluate(() => {
      return performance.memory ? performance.memory.usedJSHeapSize : null;
    });

    const finalSnapshot = await page.evaluate(() => {
      window.__uiLiveProfile.running = false;
      window.__uiLiveProfile.endTime = performance.now();

      const e2e = window.__e2e || null;
      return {
        ui: {
          frames: Number(window.__uiLiveProfile.frames ?? 0),
          longTasks: Number(window.__uiLiveProfile.longTasks ?? 0),
          longTaskTotalMs: Number(window.__uiLiveProfile.longTaskTotalMs ?? 0),
          longTaskMaxMs: Number(window.__uiLiveProfile.longTaskMaxMs ?? 0),
          startTime: Number(window.__uiLiveProfile.startTime ?? 0),
          endTime: Number(window.__uiLiveProfile.endTime ?? 0)
        },
        e2e: e2e
          ? {
              statusKey: e2e.connectionStatus?.statusKey ?? null,
              detailText: e2e.connectionStatus?.detailText ?? "",
              perfProfilingEnabled: Boolean(e2e.perfProfilingEnabled),
              ultraPerformanceMode: Boolean(e2e.ultraPerformanceMode),
              matchInfoReady: Boolean(e2e.matchInfoReady),
              lastStateUpdate: Number(e2e.lastStateUpdate ?? 0),
              hasLocalPlayer: Boolean(e2e.hasLocalPlayer),
              perfReportGeneratedAt: Number(e2e.perfReportGeneratedAt ?? 0),
              perfReport: e2e.perfReport ?? null
            }
          : null
      };
    });

    const durationSec =
      finalSnapshot.ui.endTime > finalSnapshot.ui.startTime
        ? (finalSnapshot.ui.endTime - finalSnapshot.ui.startTime) / 1000
        : 0;
    const fps = durationSec > 0 ? finalSnapshot.ui.frames / durationSec : 0;
    const heapGrowthMb =
      heapStart != null && heapEnd != null
        ? (heapEnd - heapStart) / 1024 / 1024
        : null;
    const longTaskAvgMs =
      finalSnapshot.ui.longTasks > 0
        ? finalSnapshot.ui.longTaskTotalMs / finalSnapshot.ui.longTasks
        : 0;

    const profileReport = finalSnapshot.e2e?.perfReport || null;
    const rankedPhases = Array.isArray(profileReport?.rankedPhases)
      ? profileReport.rankedPhases
      : [];
    const bottlenecks = rankedPhases.slice(0, Math.max(1, args.topPhases)).map((phase) => ({
      phase: phase.name,
      avgMs: phase.avgMs,
      maxMs: phase.maxMs,
      totalMs: phase.totalMs,
      calls: phase.calls,
      callsPerSec: phase.callsPerSec,
      msPerSec: phase.msPerSec,
      dutyCyclePct: phase.dutyCyclePct,
      sharePct: phase.sharePct
    }));

    const failures = [];
    if (args.fpsThreshold > 0 && fps < args.fpsThreshold) {
      failures.push(`FPS ${fps.toFixed(2)} < ${args.fpsThreshold}`);
    }
    if (args.maxLongTasks >= 0 && finalSnapshot.ui.longTasks > args.maxLongTasks) {
      failures.push(`Long tasks ${finalSnapshot.ui.longTasks} > ${args.maxLongTasks}`);
    }
    if (
      heapGrowthMb != null &&
      args.maxHeapGrowthMb >= 0 &&
      heapGrowthMb > args.maxHeapGrowthMb
    ) {
      failures.push(`Heap growth ${heapGrowthMb.toFixed(2)}MB > ${args.maxHeapGrowthMb}MB`);
    }
    if (!profileReport) {
      failures.push("No perf report exported from client (__e2e.perfReport missing)");
    }
    if (bottlenecks.length === 0) {
      failures.push("No profiled phases captured");
    }

    result = {
      url: args.url,
      wsUrl: args.wsUrl,
      launch: {
        headless: args.headless,
        webgpuRequested: launchOptions.webgpuRequested
      },
      durationSec: toFixedNumber(durationSec, 2),
      runtime: {
        fps: toFixedNumber(fps, 2),
        frames: finalSnapshot.ui.frames,
        longTasks: finalSnapshot.ui.longTasks,
        longTaskTotalMs: toFixedNumber(finalSnapshot.ui.longTaskTotalMs, 2),
        longTaskMaxMs: toFixedNumber(finalSnapshot.ui.longTaskMaxMs, 2),
        longTaskAvgMs: toFixedNumber(longTaskAvgMs, 2),
        heapStartBytes: heapStart,
        heapEndBytes: heapEnd,
        heapGrowthMb: heapGrowthMb == null ? null : toFixedNumber(heapGrowthMb, 2)
      },
      connection: {
        initial: liveState,
        finalStatusKey: finalSnapshot.e2e?.statusKey ?? null,
        finalStatusDetail: finalSnapshot.e2e?.detailText ?? "",
        hasLocalPlayer: Boolean(finalSnapshot.e2e?.hasLocalPlayer),
        matchInfoReady: Boolean(finalSnapshot.e2e?.matchInfoReady)
      },
      profile: {
        perfProfilingEnabled: Boolean(finalSnapshot.e2e?.perfProfilingEnabled),
        ultraPerformanceMode: Boolean(finalSnapshot.e2e?.ultraPerformanceMode),
        phaseCount: Number(profileReport?.phaseCount ?? 0),
        elapsedMs: profileReport?.elapsedMs ?? null,
        elapsedSec: profileReport?.elapsedSec ?? null,
        totalPhaseMs: profileReport?.totalPhaseMs ?? null,
        instrumentedDutyCyclePct: profileReport?.instrumentedDutyCyclePct ?? null,
        topBottlenecks: bottlenecks
      },
      samplesSummary: summarizeSamples(samples),
      samples,
      thresholds: {
        fpsThreshold: args.fpsThreshold,
        maxLongTasks: args.maxLongTasks,
        maxHeapGrowthMb: args.maxHeapGrowthMb
      },
      passed: failures.length === 0,
      failures,
      startedAt: new Date(startWallTime).toISOString(),
      finishedAt: new Date().toISOString()
    };

    const outDir = path.dirname(args.outPath);
    fs.mkdirSync(outDir, { recursive: true });
    fs.writeFileSync(args.outPath, JSON.stringify(result, null, 2));

    console.log(JSON.stringify(result, null, 2));

    if (!result.passed) {
      process.exitCode = 2;
    }
  } finally {
    await browser.close();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
