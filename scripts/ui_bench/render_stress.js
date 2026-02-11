#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const { chromium } = require("playwright");

function parseArgs(argv) {
  const args = {
    url: "http://127.0.0.1:18080/client.html?profile=1&mode=bench",
    wsUrl: null,
    autoConnect: true,
    connectTimeoutMs: 90000,
    warmupMs: 3000,
    settleMs: 700,
    durationMs: 10000,
    sampleIntervalMs: 500,
    startObjects: 400,
    maxObjects: 2800,
    stepObjects: 400,
    refine: true,
    refineGranularity: 100,
    fxIntervalMs: 120,
    intensityScale: 0.015,
    minIntensity: 2,
    maxIntensity: 40,
    minFps: 0,
    minFpsRatio: 0.6,
    maxLongTasks: -1,
    maxLongTaskAvgMs: -1,
    maxHeapGrowthMb: -1,
    maxSmoothedFrameMs: 34,
    headless: true,
    outPath: path.resolve(process.cwd(), "artifacts", "ui_bench", "render_stress.json")
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
    else if (arg === "--start-objects") args.startObjects = Number(argv[++i]);
    else if (arg === "--max-objects") args.maxObjects = Number(argv[++i]);
    else if (arg === "--step-objects") args.stepObjects = Number(argv[++i]);
    else if (arg === "--refine") args.refine = true;
    else if (arg === "--no-refine") args.refine = false;
    else if (arg === "--refine-granularity") args.refineGranularity = Number(argv[++i]);
    else if (arg === "--fx-interval-ms") args.fxIntervalMs = Number(argv[++i]);
    else if (arg === "--intensity-scale") args.intensityScale = Number(argv[++i]);
    else if (arg === "--min-intensity") args.minIntensity = Number(argv[++i]);
    else if (arg === "--max-intensity") args.maxIntensity = Number(argv[++i]);
    else if (arg === "--min-fps") args.minFps = Number(argv[++i]);
    else if (arg === "--min-fps-ratio") args.minFpsRatio = Number(argv[++i]);
    else if (arg === "--max-long-tasks") args.maxLongTasks = Number(argv[++i]);
    else if (arg === "--max-long-task-avg-ms") args.maxLongTaskAvgMs = Number(argv[++i]);
    else if (arg === "--max-heap-growth-mb") args.maxHeapGrowthMb = Number(argv[++i]);
    else if (arg === "--max-smoothed-frame-ms") args.maxSmoothedFrameMs = Number(argv[++i]);
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
  args.startObjects = Math.max(1, Math.floor(args.startObjects));
  args.maxObjects = Math.max(args.startObjects, Math.floor(args.maxObjects));
  args.stepObjects = Math.max(1, Math.floor(args.stepObjects));
  args.refineGranularity = Math.max(1, Math.floor(args.refineGranularity));
  args.fxIntervalMs = Math.max(25, Math.floor(args.fxIntervalMs));
  args.intensityScale = Math.max(0.0001, Math.min(1, Number(args.intensityScale) || 0.015));
  args.minIntensity = Math.max(1, Math.floor(args.minIntensity));
  args.maxIntensity = Math.max(args.minIntensity, Math.floor(args.maxIntensity));
  args.minFps = Math.max(0, Number(args.minFps) || 0);
  args.minFpsRatio = Math.max(0, Math.min(1, Number(args.minFpsRatio) || 0));
  args.maxSmoothedFrameMs = Number(args.maxSmoothedFrameMs);

  return args;
}

function printHelp() {
  console.log(`Real-frame render stress options:
  --url <url>                    Page URL (default: client profile bench URL)
  --ws <ws_url>                  Override wsUrl input before connect
  --auto-connect                 Click Connect and wait for live state (default: true)
  --no-auto-connect              Do not auto-connect
  --connect-timeout-ms <ms>      Timeout waiting for live state (default: 90000)
  --warmup <seconds>             Warmup after connect (default: 3)
  --settle-ms <ms>               Delay after stage setup (default: 700)
  --duration <seconds>           Stage sample duration (default: 10)
  --sample-interval-ms <ms>      Sample interval (default: 500)
  --start-objects <n>            Synthetic object sweep start (default: 400)
  --max-objects <n>              Synthetic object sweep max (default: 2800)
  --step-objects <n>             Synthetic object step (default: 400)
  --refine                       Binary-refine pass/fail boundary (default: true)
  --no-refine                    Disable refinement
  --refine-granularity <n>       Stop refine when range <= n (default: 100)
  --fx-interval-ms <ms>          Synthetic FX pulse interval (default: 120)
  --intensity-scale <n>          FX intensity = objects * scale (default: 0.015)
  --min-intensity <n>            Lower clamp for FX intensity (default: 2)
  --max-intensity <n>            Upper clamp for FX intensity (default: 40)
  --min-fps <fps>                Absolute FPS floor (default: 0)
  --min-fps-ratio <0-1>          Min ratio vs baseline FPS (default: 0.6)
  --max-long-tasks <n>           Max long tasks per stage (-1 disables)
  --max-long-task-avg-ms <ms>    Max long-task avg duration (-1 disables)
  --max-heap-growth-mb <mb>      Max heap growth per stage (-1 disables)
  --max-smoothed-frame-ms <ms>   Max average smoothed frame ms (-1 disables, default: 34)
  --headed                       Run with visible browser
  --out <path>                   Output JSON path
`);
}

function ensureProfileBenchUrl(urlValue) {
  const url = new URL(urlValue);
  if (!url.searchParams.has("profile") && !url.searchParams.has("perf")) {
    url.searchParams.set("profile", "1");
  }
  if (!url.searchParams.has("mode") && !url.searchParams.has("bench")) {
    url.searchParams.set("mode", "bench");
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

function deriveIntensity(objects, args) {
  if (!Number.isFinite(objects) || objects <= 0) {
    return 0;
  }
  const scaled = Math.round(objects * args.intensityScale);
  return Math.max(args.minIntensity, Math.min(args.maxIntensity, scaled));
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
  if (
    thresholds.maxSmoothedFrameMs >= 0 &&
    stage.runtime.avgSmoothedFrameMs > thresholds.maxSmoothedFrameMs
  ) {
    failures.push(
      `Avg smoothed frame ${stage.runtime.avgSmoothedFrameMs.toFixed(2)}ms > ${thresholds.maxSmoothedFrameMs.toFixed(2)}ms`
    );
  }
  if (stage.objects > 0 && stage.fx.deltaBursts <= 0) {
    failures.push("No synthetic FX bursts recorded");
  }
  if (stage.objects > 0 && stage.render.trendMaxVisibleProjectiles <= 0) {
    failures.push("No visible projectiles observed");
  }
  if (stage.objects > 0 && stage.render.trendMaxActiveEffects <= 0) {
    failures.push("No active effects observed");
  }

  return {
    passed: failures.length === 0,
    failures
  };
}

async function configureStage(page, stageConfig) {
  if (stageConfig.objects <= 0) {
    return page.evaluate(() => {
      const e2e = window.__e2e || null;
      if (!e2e) return false;
      if (typeof e2e.stopFxStress === "function") {
        e2e.stopFxStress(true);
      }
      if (typeof e2e.clearSyntheticProjectiles === "function") {
        e2e.clearSyntheticProjectiles();
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
        syntheticProjectiles: cfg.objects,
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
    throw new Error("Failed to configure stage: __e2e stress helpers missing");
  }

  if (args.settleMs > 0) {
    await page.waitForTimeout(args.settleMs);
  }

  const fxStart = await snapshotFxCounters(page);
  const heapStart = await page.evaluate(() => {
    return performance.memory ? performance.memory.usedJSHeapSize : null;
  });

  await page.evaluate(() => {
    window.__renderStressBench.running = true;
    window.__renderStressBench.frames = 0;
    window.__renderStressBench.longTasks = 0;
    window.__renderStressBench.longTaskTotalMs = 0;
    window.__renderStressBench.longTaskMaxMs = 0;
    window.__renderStressBench.startTime = performance.now();
    window.__renderStressBench.endTime = 0;
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
    window.__renderStressBench.running = false;
    window.__renderStressBench.endTime = performance.now();

    return {
      frames: Number(window.__renderStressBench.frames ?? 0),
      longTasks: Number(window.__renderStressBench.longTasks ?? 0),
      longTaskTotalMs: Number(window.__renderStressBench.longTaskTotalMs ?? 0),
      longTaskMaxMs: Number(window.__renderStressBench.longTaskMaxMs ?? 0),
      startTime: Number(window.__renderStressBench.startTime ?? 0),
      endTime: Number(window.__renderStressBench.endTime ?? 0),
      statusKey: window.__e2e?.connectionStatus?.statusKey ?? null,
      playerCount: Number.parseInt(document.getElementById("playerCount")?.textContent ?? "0", 10) || 0,
      projectileCount: Number(window.__e2e?.projectileCount ?? 0),
      visibleProjectileCount: Number(window.__e2e?.visibleProjectileCount ?? 0),
      projectileRenderCap: Number(window.__e2e?.projectileRenderCap ?? 0),
      activeEffectCount: Number(window.__e2e?.activeEffectCount ?? 0),
      syntheticFxBursts: Number(window.__e2e?.syntheticFxBursts ?? 0),
      syntheticFxEvents: Number(window.__e2e?.syntheticFxEvents ?? 0),
      fxStressActive: Boolean(window.__e2e?.fxStressActive)
    };
  });

  if (stageConfig.objects > 0) {
    await stopFxStress(page, false);
  }

  const fxEnd = {
    syntheticFxBursts: snapshot.syntheticFxBursts,
    syntheticFxEvents: snapshot.syntheticFxEvents
  };

  const durationSec =
    snapshot.endTime > snapshot.startTime ? (snapshot.endTime - snapshot.startTime) / 1000 : 0;
  const fps = durationSec > 0 ? snapshot.frames / durationSec : 0;
  const longTaskAvgMs = snapshot.longTasks > 0 ? snapshot.longTaskTotalMs / snapshot.longTasks : 0;
  const heapGrowthMb =
    heapStart != null && heapEnd != null ? (heapEnd - heapStart) / 1024 / 1024 : null;

  const avgSmoothedFrameMs =
    samples.length > 0
      ? samples.reduce((sum, sample) => sum + Number(sample.smoothedFrameMs || 0), 0) / samples.length
      : 0;
  const maxSmoothedFrameMsObserved = samples.reduce(
    (max, sample) => Math.max(max, Number(sample.smoothedFrameMs || 0)),
    0
  );

  const fpsFloor =
    stageConfig.objects > 0
      ? Math.max(args.minFps, baselineFps > 0 ? baselineFps * args.minFpsRatio : 0)
      : Math.max(args.minFps, 0);

  const thresholds = {
    fpsFloor,
    maxLongTasks: args.maxLongTasks,
    maxLongTaskAvgMs: args.maxLongTaskAvgMs,
    maxHeapGrowthMb: args.maxHeapGrowthMb,
    maxSmoothedFrameMs: Number.isFinite(args.maxSmoothedFrameMs) ? args.maxSmoothedFrameMs : -1
  };

  const trendMaxPlayerCount = samples.reduce((max, s) => Math.max(max, s.playerCount || 0), 0);
  const trendMaxMapProjectiles = samples.reduce((max, s) => Math.max(max, s.projectileCount || 0), 0);
  const trendMaxVisibleProjectiles = samples.reduce(
    (max, s) => Math.max(max, s.visibleProjectileCount || 0),
    0
  );
  const trendMaxActiveEffects = samples.reduce((max, s) => Math.max(max, s.activeEffectCount || 0), 0);

  const stage = {
    objects: stageConfig.objects,
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
      heapGrowthMb: heapGrowthMb == null ? null : toFixed(heapGrowthMb, 2),
      avgSmoothedFrameMs: toFixed(avgSmoothedFrameMs, 2),
      maxSmoothedFrameMsObserved: toFixed(maxSmoothedFrameMsObserved, 2)
    },
    state: {
      statusKey: snapshot.statusKey,
      playerCount: snapshot.playerCount,
      trendMaxPlayerCount
    },
    render: {
      mapProjectiles: snapshot.projectileCount,
      visibleProjectiles: snapshot.visibleProjectileCount,
      projectileRenderCap: snapshot.projectileRenderCap,
      activeEffectCount: snapshot.activeEffectCount,
      trendMaxMapProjectiles,
      trendMaxVisibleProjectiles,
      trendMaxActiveEffects
    },
    fx: {
      syntheticFxBurstsStart: fxStart.syntheticFxBursts,
      syntheticFxBurstsEnd: fxEnd.syntheticFxBursts,
      syntheticFxEventsStart: fxStart.syntheticFxEvents,
      syntheticFxEventsEnd: fxEnd.syntheticFxEvents,
      deltaBursts: fxEnd.syntheticFxBursts - fxStart.syntheticFxBursts,
      deltaEvents: fxEnd.syntheticFxEvents - fxStart.syntheticFxEvents,
      fxStressActive: snapshot.fxStressActive
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

function stageObjects(args) {
  const values = [];
  for (let objects = args.startObjects; objects <= args.maxObjects; objects += args.stepObjects) {
    values.push(objects);
  }
  return Array.from(new Set(values)).sort((a, b) => a - b);
}

async function main() {
  const rawArgs = parseArgs(process.argv.slice(2));
  const args = {
    ...rawArgs,
    url: ensureProfileBenchUrl(rawArgs.url)
  };

  const startedAt = Date.now();
  const browser = await chromium.launch({ headless: args.headless });
  const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  const page = await context.newPage();

  await page.addInitScript(() => {
    window.__renderStressBench = {
      running: false,
      frames: 0,
      longTasks: 0,
      longTaskTotalMs: 0,
      longTaskMaxMs: 0,
      startTime: 0,
      endTime: 0
    };

    const tick = () => {
      if (window.__renderStressBench?.running) {
        window.__renderStressBench.frames += 1;
      }
      window.requestAnimationFrame(tick);
    };
    window.requestAnimationFrame(tick);

    if ("PerformanceObserver" in window) {
      try {
        const observer = new PerformanceObserver((list) => {
          if (!window.__renderStressBench?.running) return;
          for (const entry of list.getEntries()) {
            window.__renderStressBench.longTasks += 1;
            const duration = Number(entry.duration || 0);
            window.__renderStressBench.longTaskTotalMs += duration;
            if (duration > window.__renderStressBench.longTaskMaxMs) {
              window.__renderStressBench.longTaskMaxMs = duration;
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
        typeof window.__e2e.stopFxStress === "function" &&
        typeof window.__e2e.clearSyntheticProjectiles === "function"
      );
    }, { timeout: 30000 });

    if (args.warmupMs > 0) {
      await page.waitForTimeout(args.warmupMs);
    }

    const baselineStage = await measureStage(
      page,
      args,
      {
        objects: 0,
        intensity: 0,
        fxIntervalMs: args.fxIntervalMs
      },
      0
    );
    stageMap.set(0, baselineStage);

    const baselineFps = baselineStage.runtime.fps;
    let bestPassing = baselineStage;
    let firstFailing = null;

    for (const objects of stageObjects(args)) {
      const stage = await measureStage(
        page,
        args,
        {
          objects,
          intensity: deriveIntensity(objects, args),
          fxIntervalMs: args.fxIntervalMs
        },
        baselineFps
      );
      stageMap.set(objects, stage);

      if (stage.passed) {
        if (!bestPassing || objects > bestPassing.objects) {
          bestPassing = stage;
        }
      } else if (!firstFailing || objects < firstFailing.objects) {
        firstFailing = stage;
        break;
      }
    }

    if (
      args.refine &&
      firstFailing &&
      bestPassing &&
      firstFailing.objects - bestPassing.objects > args.refineGranularity
    ) {
      let low = bestPassing.objects;
      let high = firstFailing.objects - 1;

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
            objects: mid,
            intensity: deriveIntensity(mid, args),
            fxIntervalMs: args.fxIntervalMs
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

    const stages = Array.from(stageMap.values()).sort((a, b) => a.objects - b.objects);
    const stressStages = stages.filter((stage) => stage.objects > 0);
    const maxObservedVisibleProjectiles = stages.reduce(
      (max, stage) => Math.max(max, stage.render.trendMaxVisibleProjectiles || 0),
      0
    );
    const maxObservedMapProjectiles = stages.reduce(
      (max, stage) => Math.max(max, stage.render.trendMaxMapProjectiles || 0),
      0
    );
    const maxObservedActiveEffects = stages.reduce(
      (max, stage) => Math.max(max, stage.render.trendMaxActiveEffects || 0),
      0
    );
    const minStressFps = stressStages.reduce(
      (min, stage) => Math.min(min, stage.runtime.fps),
      Number.POSITIVE_INFINITY
    );

    const result = {
      url: args.url,
      wsUrl: args.wsUrl,
      autoConnect: args.autoConnect,
      options: {
        durationSec: toFixed(args.durationMs / 1000, 2),
        warmupSec: toFixed(args.warmupMs / 1000, 2),
        settleMs: args.settleMs,
        sampleIntervalMs: args.sampleIntervalMs,
        startObjects: args.startObjects,
        maxObjects: args.maxObjects,
        stepObjects: args.stepObjects,
        refine: args.refine,
        refineGranularity: args.refineGranularity,
        fxIntervalMs: args.fxIntervalMs,
        intensityScale: args.intensityScale,
        minIntensity: args.minIntensity,
        maxIntensity: args.maxIntensity,
        minFps: args.minFps,
        minFpsRatio: args.minFpsRatio,
        maxLongTasks: args.maxLongTasks,
        maxLongTaskAvgMs: args.maxLongTaskAvgMs,
        maxHeapGrowthMb: args.maxHeapGrowthMb,
        maxSmoothedFrameMs: args.maxSmoothedFrameMs
      },
      baseline: {
        fps: baselineStage.runtime.fps,
        longTasks: baselineStage.runtime.longTasks,
        longTaskAvgMs: baselineStage.runtime.longTaskAvgMs,
        heapGrowthMb: baselineStage.runtime.heapGrowthMb,
        avgSmoothedFrameMs: baselineStage.runtime.avgSmoothedFrameMs
      },
      summary: {
        maxSustainableObjects: bestPassing ? bestPassing.objects : null,
        firstFailingObjects: firstFailing ? firstFailing.objects : null,
        maxObservedVisibleProjectiles,
        maxObservedMapProjectiles,
        maxObservedActiveEffects,
        minStressFps: Number.isFinite(minStressFps) ? toFixed(minStressFps, 2) : null
      },
      stages,
      passed: Boolean(bestPassing && bestPassing.objects >= args.startObjects),
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
