#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const { chromium } = require("playwright");

function parseArgs(argv) {
  const args = {
    url: "http://localhost:8080/client.html",
    durationMs: 30000,
    warmupMs: 5000,
    fpsThreshold: 55,
    maxLongTasks: 20,
    maxHeapGrowthMb: 150,
    headless: true,
    autoConnect: true,
    wsUrl: null,
    outPath: path.resolve(process.cwd(), "artifacts", "ui_bench.json")
  };

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--url") args.url = argv[++i];
    else if (arg === "--duration") args.durationMs = Number(argv[++i]) * 1000;
    else if (arg === "--warmup") args.warmupMs = Number(argv[++i]) * 1000;
    else if (arg === "--fps-threshold") args.fpsThreshold = Number(argv[++i]);
    else if (arg === "--max-long-tasks") args.maxLongTasks = Number(argv[++i]);
    else if (arg === "--max-heap-growth-mb") args.maxHeapGrowthMb = Number(argv[++i]);
    else if (arg === "--headed") args.headless = false;
    else if (arg === "--no-auto-connect") args.autoConnect = false;
    else if (arg === "--ws") args.wsUrl = argv[++i];
    else if (arg === "--out") args.outPath = path.resolve(process.cwd(), argv[++i]);
    else if (arg === "--help") {
      printHelp();
      process.exit(0);
    }
  }

  return args;
}

function printHelp() {
  console.log(`UI Bench Options:
  --url <url>                 Page URL (default: localhost client.html)
  --duration <seconds>        Benchmark duration (default: 30)
  --warmup <seconds>          Warmup duration (default: 5)
  --fps-threshold <fps>       Fail if FPS below (default: 55)
  --max-long-tasks <count>    Fail if long tasks exceed (default: 20)
  --max-heap-growth-mb <mb>   Fail if heap growth exceeds (default: 150)
  --headed                    Show browser UI
  --no-auto-connect           Do not click Connect button
  --ws <ws_url>               Set wsUrl input before connect
  --out <path>                Output JSON path (default: artifacts/ui_bench.json)
  --help                      Show help
`);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));

  const browser = await chromium.launch({ headless: args.headless });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();

  await page.addInitScript(() => {
    window.__uiBench = {
      frames: 0,
      longTasks: 0,
      startTime: 0,
      endTime: 0,
      running: false
    };

    let rafId = null;
    const tick = () => {
      if (window.__uiBench.running) {
        window.__uiBench.frames += 1;
      }
      rafId = window.requestAnimationFrame(tick);
    };
    rafId = window.requestAnimationFrame(tick);

    if ("PerformanceObserver" in window) {
      try {
        const obs = new PerformanceObserver((list) => {
          for (const entry of list.getEntries()) {
            if (window.__uiBench.running) {
              window.__uiBench.longTasks += 1;
            }
          }
        });
        obs.observe({ entryTypes: ["longtask"] });
      } catch (_) {
        // Ignore if longtask not supported
      }
    }
  });

  const start = Date.now();
  await page.goto(args.url, { waitUntil: "domcontentloaded", timeout: 60000 });

  if (args.wsUrl) {
    const wsInput = page.locator("#wsUrl");
    if (await wsInput.count()) {
      await wsInput.fill(args.wsUrl);
    }
  }

  if (args.autoConnect) {
    const connectButton = page.locator("#connectButton");
    if (await connectButton.count()) {
      await connectButton.click();
    }
  }

  if (args.warmupMs > 0) {
    await page.waitForTimeout(args.warmupMs);
  }

  await page.evaluate(() => {
    window.__uiBench.frames = 0;
    window.__uiBench.longTasks = 0;
    window.__uiBench.running = true;
    window.__uiBench.startTime = performance.now();
  });

  const heapStart = await page.evaluate(() => {
    return performance.memory ? performance.memory.usedJSHeapSize : null;
  });

  await page.waitForTimeout(args.durationMs);

  const heapEnd = await page.evaluate(() => {
    return performance.memory ? performance.memory.usedJSHeapSize : null;
  });

  const metrics = await page.evaluate(() => {
    window.__uiBench.running = false;
    window.__uiBench.endTime = performance.now();
    return {
      frames: window.__uiBench.frames,
      longTasks: window.__uiBench.longTasks,
      startTime: window.__uiBench.startTime,
      endTime: window.__uiBench.endTime
    };
  });

  const durationSec = (metrics.endTime - metrics.startTime) / 1000;
  const fps = durationSec > 0 ? metrics.frames / durationSec : 0;
  const heapGrowthMb =
    heapStart != null && heapEnd != null
      ? (heapEnd - heapStart) / 1024 / 1024
      : null;

  const result = {
    url: args.url,
    durationSec: Number(durationSec.toFixed(2)),
    fps: Number(fps.toFixed(2)),
    longTasks: metrics.longTasks,
    heapStartBytes: heapStart,
    heapEndBytes: heapEnd,
    heapGrowthMb: heapGrowthMb == null ? null : Number(heapGrowthMb.toFixed(2)),
    thresholds: {
      fps: args.fpsThreshold,
      longTasks: args.maxLongTasks,
      heapGrowthMb: args.maxHeapGrowthMb
    },
    passed: true,
    startedAt: new Date(start).toISOString(),
    finishedAt: new Date().toISOString()
  };

  const failures = [];
  if (!Number.isNaN(args.fpsThreshold) && fps < args.fpsThreshold) {
    failures.push(`FPS ${fps.toFixed(2)} < ${args.fpsThreshold}`);
  }
  if (!Number.isNaN(args.maxLongTasks) && metrics.longTasks > args.maxLongTasks) {
    failures.push(`Long tasks ${metrics.longTasks} > ${args.maxLongTasks}`);
  }
  if (
    heapGrowthMb != null &&
    !Number.isNaN(args.maxHeapGrowthMb) &&
    heapGrowthMb > args.maxHeapGrowthMb
  ) {
    failures.push(`Heap growth ${heapGrowthMb.toFixed(2)}MB > ${args.maxHeapGrowthMb}MB`);
  }

  if (failures.length > 0) {
    result.passed = false;
    result.failures = failures;
  }

  const outDir = path.dirname(args.outPath);
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(args.outPath, JSON.stringify(result, null, 2));

  console.log(JSON.stringify(result, null, 2));

  await browser.close();

  if (!result.passed) {
    process.exit(2);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
