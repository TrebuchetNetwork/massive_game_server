#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const { chromium } = require("playwright");

function parseArgs(argv) {
  const args = {
    url: "http://127.0.0.1:18080/client.html?profile=1",
    wsUrl: null,
    autoConnect: true,
    connectTimeoutMs: 90000,
    warmupMs: 3000,
    settleMs: 700,
    durationMs: 12000,
    sampleIntervalMs: 500,
    startIntensity: 4,
    maxIntensity: 28,
    stepIntensity: 4,
    refine: true,
    refineGranularity: 1,
    fxIntervalMs: 120,
    syntheticProjectiles: 1400,
    minFps: 0,
    minFpsRatio: 0.55,
    maxLongTasks: -1,
    maxLongTaskAvgMs: -1,
    maxHeapGrowthMb: -1,
    headless: true,
    outPath: path.resolve(process.cwd(), "artifacts", "ui_bench", "full_fx_battle.json")
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--url") args.url = argv[++i];
    else if (arg === "--ws") args.wsUrl = argv[++i];
    else if (arg === "--auto-connect") args.autoConnect = true;
    else if (arg === "--no-auto-connect") args.autoConnect = false;
    else if (arg === "--connect-timeout-ms") args.connectTimeoutMs = Number(argv[++i]);
    else if (arg === "--warmup") args.warmupMs = Number(argv[++i]) * 1000;
    else if (arg === "--settle-ms") args.settleMs = Number(argv[++i]);
    else if (arg === "--duration") args.durationMs = Number(argv[++i]) * 1000;
    else if (arg === "--sample-interval-ms") args.sampleIntervalMs = Number(argv[++i]);
    else if (arg === "--start-intensity") args.startIntensity = Number(argv[++i]);
    else if (arg === "--max-intensity") args.maxIntensity = Number(argv[++i]);
    else if (arg === "--step-intensity") args.stepIntensity = Number(argv[++i]);
    else if (arg === "--refine") args.refine = true;
    else if (arg === "--no-refine") args.refine = false;
    else if (arg === "--refine-granularity") args.refineGranularity = Number(argv[++i]);
    else if (arg === "--fx-interval-ms") args.fxIntervalMs = Number(argv[++i]);
    else if (arg === "--synthetic-projectiles") args.syntheticProjectiles = Number(argv[++i]);
    else if (arg === "--min-fps") args.minFps = Number(argv[++i]);
    else if (arg === "--min-fps-ratio") args.minFpsRatio = Number(argv[++i]);
    else if (arg === "--max-long-tasks") args.maxLongTasks = Number(argv[++i]);
    else if (arg === "--max-long-task-avg-ms") args.maxLongTaskAvgMs = Number(argv[++i]);
    else if (arg === "--max-heap-growth-mb") args.maxHeapGrowthMb = Number(argv[++i]);
    else if (arg === "--headed") args.headless = false;
    else if (arg === "--out") args.outPath = path.resolve(process.cwd(), argv[++i]);
    else if (arg === "--help") {
      printHelp();
      process.exit(0);
    }
  }

  args.durationMs = Math.max(1000, args.durationMs);
  args.warmupMs = Math.max(0, args.warmupMs);
  args.settleMs = Math.max(0, args.settleMs);
  args.sampleIntervalMs = Math.max(100, args.sampleIntervalMs);
  args.startIntensity = Math.max(1, Math.floor(args.startIntensity));
  args.maxIntensity = Math.max(args.startIntensity, Math.floor(args.maxIntensity));
  args.stepIntensity = Math.max(1, Math.floor(args.stepIntensity));
  args.refineGranularity = Math.max(1, Math.floor(args.refineGranularity));
  args.fxIntervalMs = Math.max(25, Math.floor(args.fxIntervalMs));
  args.syntheticProjectiles = Math.max(0, Math.floor(args.syntheticProjectiles));

  return args;
}

function printHelp() {
  console.log(`Full FX battle stress options:
  --url <url>                    Page URL (default: client profile URL)
  --ws <ws_url>                  Override wsUrl input before connect
  --auto-connect                 Click Connect and wait for live state (default: true)
  --no-auto-connect              Do not auto-connect
  --connect-timeout-ms <ms>      Timeout waiting for live state (default: 90000)
  --warmup <seconds>             Warmup after connect (default: 3)
  --settle-ms <ms>               Delay after stage setup (default: 700)
  --duration <seconds>           Stage sample duration (default: 12)
  --sample-interval-ms <ms>      Sample interval (default: 500)
  --start-intensity <n>          FX stress start intensity (default: 4)
  --max-intensity <n>            FX stress max intensity (default: 28)
  --step-intensity <n>           FX stress step (default: 4)
  --refine                       Binary-refine pass/fail boundary (default: true)
  --no-refine                    Disable refinement
  --refine-granularity <n>       Stop refine when range <= n (default: 1)
  --fx-interval-ms <ms>          FX burst interval while stressed (default: 120)
  --synthetic-projectiles <n>    Synthetic projectile pressure while stressed (default: 1400)
  --min-fps <fps>                Absolute FPS floor (default: 0)
  --min-fps-ratio <0-1>          Min ratio vs baseline FPS (default: 0.55)
  --max-long-tasks <n>           Max long tasks per stage (-1 disables)
  --max-long-task-avg-ms <ms>    Max long-task avg duration (-1 disables)
  --max-heap-growth-mb <mb>      Max heap growth per stage (-1 disables)
  --headed                       Run with visible browser
  --out <path>                   Output JSON path
`);
}

function ensureProfileUrl(urlValue) {
  const url = new URL(urlValue);
  if (!url.searchParams.has("profile") && !url.searchParams.has("perf")) {
    url.searchParams.set("profile", "1");
  }
  return url.toString();
}

function toFixed(value, digits = 2) {
  if (!Number.isFinite(value)) return value;
  return Number(value.toFixed(digits));
}

function healthyStatus(status) {
  return status === "waiting" || status === "respawn" || status === "playing";
}

async function waitForLiveState(page, timeoutMs) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const state = await page.evaluate(() => ({
      status: window.__e2e?.connectionStatus?.statusKey ?? null,
      detail: window.__e2e?.connectionStatus?.detailText ?? "",
      matchInfoReady: Boolean(window.__e2e?.matchInfoReady),
      lastStateUpdate: Number(window.__e2e?.lastStateUpdate ?? 0),
      hasLocalPlayer: Boolean(window.__e2e?.hasLocalPlayer)
    }));

    if (healthyStatus(state.status) && state.matchInfoReady && state.lastStateUpdate > 0) {
      return state;
    }
    if (state.status === "error") {
      throw new Error(`client entered error state: ${state.detail || "unknown"}`);
    }
    await page.waitForTimeout(250);
  }
  throw new Error(`timed out after ${timeoutMs}ms waiting for live state`);
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

function evaluatePass(stage, thresholds) {
  const failures = [];

  if (thresholds.fpsFloor > 0 && stage.runtime.fps < thresholds.fpsFloor) {
    failures.push(`FPS ${stage.runtime.fps.toFixed(2)} < ${thresholds.fpsFloor.toFixed(2)}`);
  }
  if (thresholds.maxLongTasks >= 0 && stage.runtime.longTasks > thresholds.maxLongTasks) {
    failures.push(`Long tasks ${stage.runtime.longTasks} > ${thresholds.maxLongTasks}`);
  }
  if (
    thresholds.maxLongTaskAvgMs >= 0 &&
    stage.runtime.longTaskAvgMs > thresholds.maxLongTaskAvgMs
  ) {
    failures.push(
      `Long task avg ${stage.runtime.longTaskAvgMs.toFixed(2)}ms > ${thresholds.maxLongTaskAvgMs.toFixed(2)}ms`
    );
  }
  if (
    stage.runtime.heapGrowthMb != null &&
    thresholds.maxHeapGrowthMb >= 0 &&
    stage.runtime.heapGrowthMb > thresholds.maxHeapGrowthMb
  ) {
    failures.push(
      `Heap growth ${stage.runtime.heapGrowthMb.toFixed(2)}MB > ${thresholds.maxHeapGrowthMb.toFixed(2)}MB`
    );
  }
  if (stage.intensity > 0 && stage.fx.deltaBursts <= 0) {
    failures.push("No synthetic FX bursts recorded");
  }
  if (stage.intensity > 0 && stage.fx.trendMaxActiveEffectCount <= 0) {
    failures.push("No active FX objects observed");
  }

  return {
    passed: failures.length === 0,
    failures
  };
}

async function configureStage(page, stageConfig) {
  if (stageConfig.intensity <= 0) {
    return page.evaluate(() => {
      const e2e = window.__e2e || null;
      if (!e2e) return false;
      if (typeof e2e.stopFxStress === "function") {
        e2e.stopFxStress(false);
      }
      if (typeof e2e.applyFullFxMode === "function") {
        e2e.applyFullFxMode();
      }
      return true;
    });
  }

  return page.evaluate((cfg) => {
    const e2e = window.__e2e || null;
    if (!e2e) return false;
    if (typeof e2e.applyFullFxMode === "function") {
      e2e.applyFullFxMode();
    }
    if (typeof e2e.startFxStress !== "function") return false;
    return Boolean(
      e2e.startFxStress({
        intensity: cfg.intensity,
        intervalMs: cfg.fxIntervalMs,
        syntheticProjectiles: cfg.syntheticProjectiles,
        includeScreenFx: true
      })
    );
  }, stageConfig);
}

async function stopFxStress(page, clearProjectiles = false) {
  return page.evaluate((shouldClear) => {
    const e2e = window.__e2e || null;
    if (!e2e || typeof e2e.stopFxStress !== "function") return false;
    return Boolean(e2e.stopFxStress(shouldClear));
  }, clearProjectiles);
}

async function snapshotFxCounters(page) {
  return page.evaluate(() => ({
    syntheticFxBursts: Number(window.__e2e?.syntheticFxBursts ?? 0),
    syntheticFxEvents: Number(window.__e2e?.syntheticFxEvents ?? 0)
  }));
}

async function measureStage(page, args, stageConfig, baselineFps) {
  const configured = await configureStage(page, stageConfig);
  if (!configured) {
    throw new Error("Failed to configure stage: __e2e full FX helpers missing");
  }

  if (args.settleMs > 0) {
    await page.waitForTimeout(args.settleMs);
  }

  const fxStart = await snapshotFxCounters(page);
  const heapStart = await page.evaluate(() => {
    return performance.memory ? performance.memory.usedJSHeapSize : null;
  });

  await page.evaluate(() => {
    window.__fullFxBench.running = true;
    window.__fullFxBench.frames = 0;
    window.__fullFxBench.longTasks = 0;
    window.__fullFxBench.longTaskTotalMs = 0;
    window.__fullFxBench.longTaskMaxMs = 0;
    window.__fullFxBench.startTime = performance.now();
    window.__fullFxBench.endTime = 0;
  });

  const sampleTarget = Math.max(1, Math.ceil(args.durationMs / args.sampleIntervalMs));
  const samples = [];

  for (let i = 0; i < sampleTarget; i += 1) {
    await page.waitForTimeout(args.sampleIntervalMs);
    const sample = await page.evaluate(() => {
      const now = performance.now();
      const playerCountRaw = document.getElementById("playerCount")?.textContent ?? "0";
      const playerCount = Number.parseInt(playerCountRaw, 10);
      return {
        atMs: Number(now.toFixed(1)),
        statusKey: window.__e2e?.connectionStatus?.statusKey ?? null,
        playerCount: Number.isFinite(playerCount) ? playerCount : 0,
        smoothedFrameMs: Number(window.__e2e?.smoothedFrameMs ?? 0),
        projectileCount: Number(window.__e2e?.projectileCount ?? 0),
        visibleProjectileCount: Number(window.__e2e?.visibleProjectileCount ?? 0),
        projectileRenderCap: Number(window.__e2e?.projectileRenderCap ?? 0),
        activeEffectCount: Number(window.__e2e?.activeEffectCount ?? 0),
        syntheticFxBursts: Number(window.__e2e?.syntheticFxBursts ?? 0),
        syntheticFxEvents: Number(window.__e2e?.syntheticFxEvents ?? 0),
        fxStressActive: Boolean(window.__e2e?.fxStressActive)
      };
    });
    samples.push(sample);
  }

  const heapEnd = await page.evaluate(() => {
    return performance.memory ? performance.memory.usedJSHeapSize : null;
  });

  const snapshot = await page.evaluate(() => {
    window.__fullFxBench.running = false;
    window.__fullFxBench.endTime = performance.now();

    return {
      frames: Number(window.__fullFxBench.frames ?? 0),
      longTasks: Number(window.__fullFxBench.longTasks ?? 0),
      longTaskTotalMs: Number(window.__fullFxBench.longTaskTotalMs ?? 0),
      longTaskMaxMs: Number(window.__fullFxBench.longTaskMaxMs ?? 0),
      startTime: Number(window.__fullFxBench.startTime ?? 0),
      endTime: Number(window.__fullFxBench.endTime ?? 0),
      statusKey: window.__e2e?.connectionStatus?.statusKey ?? null,
      playerCount: Number.parseInt(document.getElementById("playerCount")?.textContent ?? "0", 10) || 0,
      projectileCount: Number(window.__e2e?.projectileCount ?? 0),
      visibleProjectileCount: Number(window.__e2e?.visibleProjectileCount ?? 0),
      projectileRenderCap: Number(window.__e2e?.projectileRenderCap ?? 0),
      activeEffectCount: Number(window.__e2e?.activeEffectCount ?? 0),
      syntheticFxBursts: Number(window.__e2e?.syntheticFxBursts ?? 0),
      syntheticFxEvents: Number(window.__e2e?.syntheticFxEvents ?? 0),
      fxStressActive: Boolean(window.__e2e?.fxStressActive),
      fullFxMode: Boolean(window.__e2e?.fullFxMode)
    };
  });

  if (stageConfig.intensity > 0) {
    await stopFxStress(page, false);
  }

  const fxEnd = {
    syntheticFxBursts: snapshot.syntheticFxBursts,
    syntheticFxEvents: snapshot.syntheticFxEvents
  };

  const durationSec =
    snapshot.endTime > snapshot.startTime ? (snapshot.endTime - snapshot.startTime) / 1000 : 0;
  const fps = durationSec > 0 ? snapshot.frames / durationSec : 0;
  const longTaskAvgMs =
    snapshot.longTasks > 0 ? snapshot.longTaskTotalMs / snapshot.longTasks : 0;
  const heapGrowthMb =
    heapStart != null && heapEnd != null ? (heapEnd - heapStart) / 1024 / 1024 : null;

  const fpsFloor =
    stageConfig.intensity > 0
      ? Math.max(args.minFps, baselineFps > 0 ? baselineFps * args.minFpsRatio : 0)
      : Math.max(args.minFps, 0);

  const thresholds = {
    fpsFloor,
    maxLongTasks: args.maxLongTasks,
    maxLongTaskAvgMs: args.maxLongTaskAvgMs,
    maxHeapGrowthMb: args.maxHeapGrowthMb
  };

  const trendMaxPlayerCount = samples.reduce((max, s) => Math.max(max, s.playerCount || 0), 0);
  const trendMaxProjectileCount = samples.reduce((max, s) => Math.max(max, s.projectileCount || 0), 0);
  const trendMaxVisibleProjectileCount = samples.reduce(
    (max, s) => Math.max(max, s.visibleProjectileCount || 0),
    0
  );
  const trendMaxActiveEffectCount = samples.reduce(
    (max, s) => Math.max(max, s.activeEffectCount || 0),
    0
  );

  const stage = {
    intensity: stageConfig.intensity,
    runtime: {
      durationSec: toFixed(durationSec, 2),
      fps: toFixed(fps, 2),
      frames: snapshot.frames,
      longTasks: snapshot.longTasks,
      longTaskTotalMs: toFixed(snapshot.longTaskTotalMs, 2),
      longTaskMaxMs: toFixed(snapshot.longTaskMaxMs, 2),
      longTaskAvgMs: toFixed(longTaskAvgMs, 2),
      heapStartBytes: heapStart,
      heapEndBytes: heapEnd,
      heapGrowthMb: heapGrowthMb == null ? null : toFixed(heapGrowthMb, 2)
    },
    state: {
      statusKey: snapshot.statusKey,
      playerCount: snapshot.playerCount,
      trendMaxPlayerCount,
      fullFxMode: snapshot.fullFxMode,
      fxStressActive: snapshot.fxStressActive
    },
    projectile: {
      mapCount: snapshot.projectileCount,
      visibleCount: snapshot.visibleProjectileCount,
      renderCap: snapshot.projectileRenderCap,
      trendMaxMapCount: trendMaxProjectileCount,
      trendMaxVisibleCount: trendMaxVisibleProjectileCount
    },
    fx: {
      activeEffectCount: snapshot.activeEffectCount,
      trendMaxActiveEffectCount,
      syntheticFxBurstsStart: fxStart.syntheticFxBursts,
      syntheticFxBurstsEnd: fxEnd.syntheticFxBursts,
      syntheticFxEventsStart: fxStart.syntheticFxEvents,
      syntheticFxEventsEnd: fxEnd.syntheticFxEvents,
      deltaBursts: fxEnd.syntheticFxBursts - fxStart.syntheticFxBursts,
      deltaEvents: fxEnd.syntheticFxEvents - fxStart.syntheticFxEvents
    },
    thresholds,
    samples,
    passed: true,
    failures: []
  };

  const gate = evaluatePass(stage, thresholds);
  stage.passed = gate.passed;
  stage.failures = gate.failures;

  return stage;
}

function stageCounts(args) {
  const counts = [];
  for (let intensity = args.startIntensity; intensity <= args.maxIntensity; intensity += args.stepIntensity) {
    counts.push(intensity);
  }
  return Array.from(new Set(counts)).sort((a, b) => a - b);
}

async function main() {
  const rawArgs = parseArgs(process.argv.slice(2));
  const args = {
    ...rawArgs,
    url: ensureProfileUrl(rawArgs.url)
  };

  const startedAt = Date.now();
  const browser = await chromium.launch({ headless: args.headless });
  const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  const page = await context.newPage();

  await page.addInitScript(() => {
    window.__fullFxBench = {
      running: false,
      frames: 0,
      longTasks: 0,
      longTaskTotalMs: 0,
      longTaskMaxMs: 0,
      startTime: 0,
      endTime: 0
    };

    const tick = () => {
      if (window.__fullFxBench?.running) {
        window.__fullFxBench.frames += 1;
      }
      window.requestAnimationFrame(tick);
    };
    window.requestAnimationFrame(tick);

    if ("PerformanceObserver" in window) {
      try {
        const observer = new PerformanceObserver((list) => {
          if (!window.__fullFxBench?.running) return;
          for (const entry of list.getEntries()) {
            window.__fullFxBench.longTasks += 1;
            const duration = Number(entry.duration || 0);
            window.__fullFxBench.longTaskTotalMs += duration;
            if (duration > window.__fullFxBench.longTaskMaxMs) {
              window.__fullFxBench.longTaskMaxMs = duration;
            }
          }
        });
        observer.observe({ entryTypes: ["longtask"] });
      } catch (_) {
        // Long task API may be unavailable in this environment.
      }
    }
  });

  const stageMap = new Map();

  try {
    await page.goto(args.url, { waitUntil: "domcontentloaded", timeout: 60000 });

    if (args.wsUrl) {
      await setWsUrl(page, args.wsUrl);
    }
    if (args.autoConnect) {
      await clickConnect(page);
      await waitForLiveState(page, args.connectTimeoutMs);
    }

    await page.waitForFunction(() => {
      return (
        window.__e2e &&
        typeof window.__e2e.applyFullFxMode === "function" &&
        typeof window.__e2e.startFxStress === "function" &&
        typeof window.__e2e.stopFxStress === "function"
      );
    }, { timeout: 30000 });

    if (args.warmupMs > 0) {
      await page.waitForTimeout(args.warmupMs);
    }

    const baselineStage = await measureStage(
      page,
      args,
      {
        intensity: 0,
        fxIntervalMs: args.fxIntervalMs,
        syntheticProjectiles: 0
      },
      0
    );
    stageMap.set(0, baselineStage);

    const baselineFps = baselineStage.runtime.fps;
    let bestPassing = baselineStage;
    let firstFailing = null;

    for (const intensity of stageCounts(args)) {
      const stage = await measureStage(
        page,
        args,
        {
          intensity,
          fxIntervalMs: args.fxIntervalMs,
          syntheticProjectiles: args.syntheticProjectiles
        },
        baselineFps
      );

      stageMap.set(intensity, stage);
      if (stage.passed) {
        if (!bestPassing || intensity > bestPassing.intensity) {
          bestPassing = stage;
        }
      } else if (!firstFailing || intensity < firstFailing.intensity) {
        firstFailing = stage;
        break;
      }
    }

    if (
      args.refine &&
      firstFailing &&
      bestPassing &&
      firstFailing.intensity - bestPassing.intensity > args.refineGranularity
    ) {
      let low = bestPassing.intensity;
      let high = firstFailing.intensity - 1;

      while (high - low > args.refineGranularity) {
        const mid = Math.floor((low + high) / 2);
        if (stageMap.has(mid)) {
          const existing = stageMap.get(mid);
          if (existing.passed) {
            low = mid;
            bestPassing = existing;
          } else {
            high = mid - 1;
            firstFailing = existing;
          }
          continue;
        }

        const stage = await measureStage(
          page,
          args,
          {
            intensity: mid,
            fxIntervalMs: args.fxIntervalMs,
            syntheticProjectiles: args.syntheticProjectiles
          },
          baselineFps
        );

        stageMap.set(mid, stage);
        if (stage.passed) {
          low = mid;
          bestPassing = stage;
        } else {
          high = mid - 1;
          firstFailing = stage;
        }
      }
    }

    await stopFxStress(page, true);

    const stages = Array.from(stageMap.values()).sort((a, b) => a.intensity - b.intensity);
    const maxObservedActiveEffects = stages.reduce(
      (max, stage) => Math.max(max, stage.fx.trendMaxActiveEffectCount || 0),
      0
    );
    const maxObservedProjectiles = stages.reduce(
      (max, stage) => Math.max(max, stage.projectile.trendMaxMapCount || 0),
      0
    );
    const maxObservedVisibleProjectiles = stages.reduce(
      (max, stage) => Math.max(max, stage.projectile.trendMaxVisibleCount || 0),
      0
    );
    const minBattleFps = stages
      .filter((stage) => stage.intensity > 0)
      .reduce((min, stage) => Math.min(min, stage.runtime.fps), Number.POSITIVE_INFINITY);

    const result = {
      url: args.url,
      wsUrl: args.wsUrl,
      autoConnect: args.autoConnect,
      options: {
        durationSec: toFixed(args.durationMs / 1000, 2),
        warmupSec: toFixed(args.warmupMs / 1000, 2),
        settleMs: args.settleMs,
        sampleIntervalMs: args.sampleIntervalMs,
        startIntensity: args.startIntensity,
        maxIntensity: args.maxIntensity,
        stepIntensity: args.stepIntensity,
        refine: args.refine,
        refineGranularity: args.refineGranularity,
        fxIntervalMs: args.fxIntervalMs,
        syntheticProjectiles: args.syntheticProjectiles,
        minFps: args.minFps,
        minFpsRatio: args.minFpsRatio,
        maxLongTasks: args.maxLongTasks,
        maxLongTaskAvgMs: args.maxLongTaskAvgMs,
        maxHeapGrowthMb: args.maxHeapGrowthMb
      },
      baseline: {
        fps: baselineStage.runtime.fps,
        longTasks: baselineStage.runtime.longTasks,
        longTaskAvgMs: baselineStage.runtime.longTaskAvgMs,
        heapGrowthMb: baselineStage.runtime.heapGrowthMb,
        playerCount: baselineStage.state.trendMaxPlayerCount
      },
      summary: {
        maxSustainableIntensity: bestPassing ? bestPassing.intensity : null,
        firstFailingIntensity: firstFailing ? firstFailing.intensity : null,
        maxObservedActiveEffects,
        maxObservedProjectiles,
        maxObservedVisibleProjectiles,
        minBattleFps: Number.isFinite(minBattleFps) ? toFixed(minBattleFps, 2) : null
      },
      stages,
      passed: Boolean(bestPassing && bestPassing.intensity > 0),
      startedAt: new Date(startedAt).toISOString(),
      finishedAt: new Date().toISOString()
    };

    fs.mkdirSync(path.dirname(args.outPath), { recursive: true });
    fs.writeFileSync(args.outPath, JSON.stringify(result, null, 2));
    console.log(JSON.stringify(result, null, 2));

    await browser.close();

    if (!result.passed) {
      process.exit(2);
    }
  } catch (err) {
    await browser.close();
    throw err;
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
