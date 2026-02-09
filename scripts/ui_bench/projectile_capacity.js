#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const { chromium } = require("playwright");

function parseArgs(argv) {
  const args = {
    url: "http://127.0.0.1:18080/client.html?mode=bench",
    wsUrl: null,
    autoConnect: false,
    durationMs: 8000,
    warmupMs: 1500,
    settleMs: 600,
    sampleIntervalMs: 200,
    baselineCount: 0,
    startCount: 200,
    maxCount: 3000,
    stepCount: 200,
    stopOnFail: true,
    refine: true,
    refineGranularity: 50,
    minFps: 0,
    minFpsRatio: 0.7,
    maxLongTasks: -1,
    maxLongTaskAvgMs: -1,
    maxHeapGrowthMb: -1,
    headless: true,
    outPath: path.resolve(process.cwd(), "artifacts", "ui_bench", "projectile_capacity.json")
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--url") args.url = argv[++i];
    else if (arg === "--ws") args.wsUrl = argv[++i];
    else if (arg === "--auto-connect") args.autoConnect = true;
    else if (arg === "--no-auto-connect") args.autoConnect = false;
    else if (arg === "--duration") args.durationMs = Number(argv[++i]) * 1000;
    else if (arg === "--warmup") args.warmupMs = Number(argv[++i]) * 1000;
    else if (arg === "--settle-ms") args.settleMs = Number(argv[++i]);
    else if (arg === "--sample-interval-ms") args.sampleIntervalMs = Number(argv[++i]);
    else if (arg === "--baseline") args.baselineCount = Number(argv[++i]);
    else if (arg === "--start") args.startCount = Number(argv[++i]);
    else if (arg === "--max") args.maxCount = Number(argv[++i]);
    else if (arg === "--step") args.stepCount = Number(argv[++i]);
    else if (arg === "--stop-on-fail") args.stopOnFail = true;
    else if (arg === "--no-stop-on-fail") args.stopOnFail = false;
    else if (arg === "--refine") args.refine = true;
    else if (arg === "--no-refine") args.refine = false;
    else if (arg === "--refine-granularity") args.refineGranularity = Number(argv[++i]);
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
  args.stepCount = Math.max(1, Math.floor(args.stepCount));
  args.maxCount = Math.max(0, Math.floor(args.maxCount));
  args.startCount = Math.max(0, Math.floor(args.startCount));
  args.baselineCount = Math.max(0, Math.floor(args.baselineCount));
  args.refineGranularity = Math.max(1, Math.floor(args.refineGranularity));

  return args;
}

function printHelp() {
  console.log(`Projectile capacity benchmark options:
  --url <url>                    Page URL (default: client.html?mode=bench)
  --ws <ws_url>                  Set wsUrl input before connect
  --auto-connect                 Click Connect button (default: false)
  --duration <seconds>           Measure duration per stage (default: 8)
  --warmup <seconds>             Warmup delay after load (default: 1.5)
  --settle-ms <ms>               Delay after count change (default: 600)
  --sample-interval-ms <ms>      Snapshot interval for trend sampling (default: 200)
  --baseline <count>             Baseline synthetic projectile count (default: 0)
  --start <count>                Sweep start count (default: 200)
  --max <count>                  Sweep max count (default: 3000)
  --step <count>                 Sweep step count (default: 200)
  --stop-on-fail                 Stop after first failing stage (default: true)
  --no-stop-on-fail              Continue sweep after failures
  --refine                       Binary refine pass/fail boundary (default: true)
  --no-refine                    Disable binary refinement
  --refine-granularity <count>   Stop refining when range <= this count (default: 50)
  --min-fps <fps>                Absolute FPS floor (default: 0)
  --min-fps-ratio <0-1>          Min ratio vs baseline FPS (default: 0.7)
  --max-long-tasks <count>       Long-task ceiling (-1 disables, default: -1)
  --max-long-task-avg-ms <ms>    Long-task avg ceiling (-1 disables, default: -1)
  --max-heap-growth-mb <mb>      Heap growth ceiling (-1 disables, default: -1)
  --headed                       Show browser UI
  --out <path>                   Output JSON path
  --help                         Show help
`);
}

function ensureBenchMode(urlValue) {
  const url = new URL(urlValue);
  if (!url.searchParams.has("mode") && !url.searchParams.has("bench")) {
    url.searchParams.set("mode", "bench");
  }
  return url.toString();
}

function toFixedNumber(value, digits = 2) {
  if (!Number.isFinite(value)) return value;
  return Number(value.toFixed(digits));
}

async function setWsUrl(page, wsUrl) {
  if (!wsUrl) return false;
  return page.evaluate((targetWs) => {
    const wsInput = document.getElementById("wsUrl");
    if (!wsInput) return false;
    wsInput.value = targetWs;
    return true;
  }, wsUrl);
}

async function clickConnect(page) {
  const connectButton = page.locator("#connectButton");
  if (!(await connectButton.count())) return false;
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

async function setSyntheticCount(page, count) {
  const appliedCount = await page.evaluate((nextCount) => {
    if (!window.__e2e || typeof window.__e2e.setSyntheticProjectileCount !== "function") {
      return -1;
    }
    return Number(window.__e2e.setSyntheticProjectileCount(nextCount));
  }, count);

  if (!Number.isFinite(appliedCount) || appliedCount < 0) {
    throw new Error("Client does not expose __e2e.setSyntheticProjectileCount");
  }

  return Math.floor(appliedCount);
}

function evaluateStagePass(stage, thresholds) {
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

  return {
    passed: failures.length === 0,
    failures
  };
}

async function measureStage(page, args, requestedCount, baselineFps) {
  const appliedCount = await setSyntheticCount(page, requestedCount);
  if (args.settleMs > 0) {
    await page.waitForTimeout(args.settleMs);
  }

  const heapStart = await page.evaluate(() => {
    return performance.memory ? performance.memory.usedJSHeapSize : null;
  });

  await page.evaluate(() => {
    if (!window.__projectileBench) {
      window.__projectileBench = {
        running: false,
        frames: 0,
        longTasks: 0,
        longTaskTotalMs: 0,
        longTaskMaxMs: 0,
        startTime: 0,
        endTime: 0
      };
    }
    window.__projectileBench.running = true;
    window.__projectileBench.frames = 0;
    window.__projectileBench.longTasks = 0;
    window.__projectileBench.longTaskTotalMs = 0;
    window.__projectileBench.longTaskMaxMs = 0;
    window.__projectileBench.startTime = performance.now();
    window.__projectileBench.endTime = 0;
  });

  const trendSamples = [];
  const trendTarget = Math.max(1, Math.ceil(args.durationMs / args.sampleIntervalMs));
  for (let i = 0; i < trendTarget; i += 1) {
    await page.waitForTimeout(args.sampleIntervalMs);
    const sample = await page.evaluate(() => {
      const now = performance.now();
      return {
        atMs: Number(now.toFixed(1)),
        projectileCount: Number(window.__e2e?.projectileCount ?? 0),
        visibleProjectileCount: Number(window.__e2e?.visibleProjectileCount ?? 0),
        projectileRenderCap: Number(window.__e2e?.projectileRenderCap ?? 0),
        smoothedFrameMs: Number(window.__e2e?.smoothedFrameMs ?? 0),
        statusKey: window.__e2e?.connectionStatus?.statusKey ?? null
      };
    });
    trendSamples.push(sample);
  }

  const heapEnd = await page.evaluate(() => {
    return performance.memory ? performance.memory.usedJSHeapSize : null;
  });

  const snapshot = await page.evaluate(() => {
    const bench = window.__projectileBench || {};
    bench.running = false;
    bench.endTime = performance.now();

    return {
      frames: Number(bench.frames ?? 0),
      longTasks: Number(bench.longTasks ?? 0),
      longTaskTotalMs: Number(bench.longTaskTotalMs ?? 0),
      longTaskMaxMs: Number(bench.longTaskMaxMs ?? 0),
      startTime: Number(bench.startTime ?? 0),
      endTime: Number(bench.endTime ?? 0),
      projectileCount: Number(window.__e2e?.projectileCount ?? 0),
      visibleProjectileCount: Number(window.__e2e?.visibleProjectileCount ?? 0),
      projectileRenderCap: Number(window.__e2e?.projectileRenderCap ?? 0),
      syntheticProjectileCount: Number(window.__e2e?.syntheticProjectileCount ?? 0),
      statusKey: window.__e2e?.connectionStatus?.statusKey ?? null
    };
  });

  const durationSec =
    snapshot.endTime > snapshot.startTime ? (snapshot.endTime - snapshot.startTime) / 1000 : 0;
  const fps = durationSec > 0 ? snapshot.frames / durationSec : 0;
  const longTaskAvgMs =
    snapshot.longTasks > 0 ? snapshot.longTaskTotalMs / snapshot.longTasks : 0;
  const heapGrowthMb =
    heapStart != null && heapEnd != null ? (heapEnd - heapStart) / 1024 / 1024 : null;

  const fpsFloor = Math.max(args.minFps, baselineFps > 0 ? baselineFps * args.minFpsRatio : 0);
  const thresholds = {
    fpsFloor,
    maxLongTasks: args.maxLongTasks,
    maxLongTaskAvgMs: args.maxLongTaskAvgMs,
    maxHeapGrowthMb: args.maxHeapGrowthMb
  };
  const trendMaxMapCount = trendSamples.reduce(
    (max, sample) => Math.max(max, Number(sample.projectileCount || 0)),
    0
  );
  const trendMaxVisibleCount = trendSamples.reduce(
    (max, sample) => Math.max(max, Number(sample.visibleProjectileCount || 0)),
    0
  );
  const trendMaxRenderCap = trendSamples.reduce(
    (max, sample) => Math.max(max, Number(sample.projectileRenderCap || 0)),
    0
  );

  const stage = {
    requestedCount,
    appliedCount,
    runtime: {
      durationSec: toFixedNumber(durationSec, 2),
      fps: toFixedNumber(fps, 2),
      frames: snapshot.frames,
      longTasks: snapshot.longTasks,
      longTaskTotalMs: toFixedNumber(snapshot.longTaskTotalMs, 2),
      longTaskMaxMs: toFixedNumber(snapshot.longTaskMaxMs, 2),
      longTaskAvgMs: toFixedNumber(longTaskAvgMs, 2),
      heapStartBytes: heapStart,
      heapEndBytes: heapEnd,
      heapGrowthMb: heapGrowthMb == null ? null : toFixedNumber(heapGrowthMb, 2)
    },
    projectile: {
      mapCount: snapshot.projectileCount,
      visibleCount: snapshot.visibleProjectileCount,
      renderCap: snapshot.projectileRenderCap,
      syntheticCount: snapshot.syntheticProjectileCount,
      trendMaxMapCount,
      trendMaxVisibleCount,
      trendMaxRenderCap
    },
    statusKey: snapshot.statusKey,
    thresholds,
    trendSamples,
    passed: true,
    failures: []
  };

  const gate = evaluateStagePass(stage, thresholds);
  stage.passed = gate.passed;
  stage.failures = gate.failures;

  return stage;
}

function uniqueSortedCounts(values) {
  return Array.from(new Set(values.map((v) => Math.max(0, Math.floor(v))))).sort((a, b) => a - b);
}

async function main() {
  const rawArgs = parseArgs(process.argv.slice(2));
  const args = {
    ...rawArgs,
    url: ensureBenchMode(rawArgs.url)
  };

  const startedAt = Date.now();

  const browser = await chromium.launch({ headless: args.headless });
  const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  const page = await context.newPage();

  await page.addInitScript(() => {
    window.__projectileBench = {
      running: false,
      frames: 0,
      longTasks: 0,
      longTaskTotalMs: 0,
      longTaskMaxMs: 0,
      startTime: 0,
      endTime: 0
    };

    const tick = () => {
      const bench = window.__projectileBench;
      if (bench && bench.running) {
        bench.frames += 1;
      }
      window.requestAnimationFrame(tick);
    };
    window.requestAnimationFrame(tick);

    if ("PerformanceObserver" in window) {
      try {
        const observer = new PerformanceObserver((list) => {
          const bench = window.__projectileBench;
          if (!bench || !bench.running) return;
          for (const entry of list.getEntries()) {
            bench.longTasks += 1;
            const duration = Number(entry.duration || 0);
            bench.longTaskTotalMs += duration;
            if (duration > bench.longTaskMaxMs) {
              bench.longTaskMaxMs = duration;
            }
          }
        });
        observer.observe({ entryTypes: ["longtask"] });
      } catch (_) {
        // Long task API may be unavailable in some environments.
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
    }

    await page.waitForFunction(() => window.__e2e && typeof window.__e2e.setSyntheticProjectileCount === "function", {
      timeout: 30000
    });

    if (args.warmupMs > 0) {
      await page.waitForTimeout(args.warmupMs);
    }

    const baselineStage = await measureStage(page, args, args.baselineCount, 0);
    stageMap.set(baselineStage.requestedCount, baselineStage);
    const baselineFps = baselineStage.runtime.fps;

    const sweepCounts = [];
    for (let count = args.startCount; count <= args.maxCount; count += args.stepCount) {
      sweepCounts.push(count);
    }

    let bestPassingStage = baselineStage.passed ? baselineStage : null;
    let firstFailingStage = null;

    for (const requestedCount of uniqueSortedCounts(sweepCounts)) {
      if (requestedCount === args.baselineCount) continue;
      const stage = await measureStage(page, args, requestedCount, baselineFps);
      stageMap.set(stage.requestedCount, stage);

      if (stage.passed) {
        if (!bestPassingStage || stage.requestedCount > bestPassingStage.requestedCount) {
          bestPassingStage = stage;
        }
      } else if (!firstFailingStage || stage.requestedCount < firstFailingStage.requestedCount) {
        firstFailingStage = stage;
        if (args.stopOnFail) {
          break;
        }
      }
    }

    if (
      args.refine &&
      firstFailingStage &&
      bestPassingStage &&
      firstFailingStage.requestedCount - bestPassingStage.requestedCount > args.refineGranularity
    ) {
      let low = bestPassingStage.requestedCount;
      let high = firstFailingStage.requestedCount - 1;

      while (high - low > args.refineGranularity) {
        const mid = Math.floor((low + high) / 2);
        if (stageMap.has(mid)) {
          const existing = stageMap.get(mid);
          if (existing.passed) {
            low = mid;
            bestPassingStage = existing;
          } else {
            high = mid - 1;
            firstFailingStage = existing;
          }
          continue;
        }

        const stage = await measureStage(page, args, mid, baselineFps);
        stageMap.set(mid, stage);

        if (stage.passed) {
          low = mid;
          bestPassingStage = stage;
        } else {
          high = mid - 1;
          firstFailingStage = stage;
        }
      }
    }

    await page.evaluate(() => {
      if (window.__e2e && typeof window.__e2e.clearSyntheticProjectiles === "function") {
        window.__e2e.clearSyntheticProjectiles();
      }
    });

    const stages = Array.from(stageMap.values()).sort((a, b) => a.requestedCount - b.requestedCount);
    const maxObservedProjectileCount = stages.reduce(
      (max, stage) =>
        Math.max(max, stage.projectile.mapCount, stage.projectile.trendMaxMapCount || 0),
      0
    );
    const maxObservedVisibleProjectileCount = stages.reduce(
      (max, stage) =>
        Math.max(max, stage.projectile.visibleCount, stage.projectile.trendMaxVisibleCount || 0),
      0
    );
    const maxObservedRenderCap = stages.reduce(
      (max, stage) =>
        Math.max(max, stage.projectile.renderCap, stage.projectile.trendMaxRenderCap || 0),
      0
    );

    const result = {
      url: args.url,
      wsUrl: args.wsUrl,
      autoConnect: args.autoConnect,
      options: {
        durationSec: toFixedNumber(args.durationMs / 1000, 2),
        warmupSec: toFixedNumber(args.warmupMs / 1000, 2),
        settleMs: args.settleMs,
        sampleIntervalMs: args.sampleIntervalMs,
        baselineCount: args.baselineCount,
        startCount: args.startCount,
        maxCount: args.maxCount,
        stepCount: args.stepCount,
        stopOnFail: args.stopOnFail,
        refine: args.refine,
        refineGranularity: args.refineGranularity,
        minFps: args.minFps,
        minFpsRatio: args.minFpsRatio,
        maxLongTasks: args.maxLongTasks,
        maxLongTaskAvgMs: args.maxLongTaskAvgMs,
        maxHeapGrowthMb: args.maxHeapGrowthMb
      },
      baseline: {
        requestedCount: baselineStage.requestedCount,
        fps: baselineStage.runtime.fps,
        longTasks: baselineStage.runtime.longTasks,
        longTaskAvgMs: baselineStage.runtime.longTaskAvgMs,
        heapGrowthMb: baselineStage.runtime.heapGrowthMb
      },
      summary: {
        maxSustainableRequestedCount: bestPassingStage ? bestPassingStage.requestedCount : null,
        maxSustainableVisibleProjectileCount: bestPassingStage
          ? bestPassingStage.projectile.visibleCount
          : null,
        maxSustainableMapProjectileCount: bestPassingStage
          ? bestPassingStage.projectile.mapCount
          : null,
        firstFailingRequestedCount: firstFailingStage ? firstFailingStage.requestedCount : null,
        maxObservedProjectileCount,
        maxObservedVisibleProjectileCount,
        maxObservedRenderCap
      },
      stages,
      passed: Boolean(bestPassingStage),
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
