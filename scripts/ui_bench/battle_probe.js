#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const { chromium } = require("playwright");
const { buildLaunchOptions, urlRequestsWebGpu } = require("./launch_options");

function parseArgs(argv) {
  const args = {
    url: "http://127.0.0.1:18080/client.html?mode=stable",
    wsUrl: null,
    autoConnect: true,
    connectTimeoutMs: 90000,
    warmupMs: 5000,
    durationMs: 30000,
    sampleIntervalMs: 500,
    fpsThreshold: 0,
    headless: true,
    headlessExplicit: false,
    outPath: path.resolve(process.cwd(), "artifacts", "scale", "battle_probe.json")
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--url") args.url = argv[++i];
    else if (arg === "--ws") args.wsUrl = argv[++i];
    else if (arg === "--auto-connect") args.autoConnect = true;
    else if (arg === "--no-auto-connect") args.autoConnect = false;
    else if (arg === "--connect-timeout-ms") args.connectTimeoutMs = Number(argv[++i]);
    else if (arg === "--warmup") args.warmupMs = Number(argv[++i]) * 1000;
    else if (arg === "--duration") args.durationMs = Number(argv[++i]) * 1000;
    else if (arg === "--sample-interval-ms") args.sampleIntervalMs = Number(argv[++i]);
    else if (arg === "--fps-threshold") args.fpsThreshold = Number(argv[++i]);
    else if (arg === "--headed") {
      args.headless = false;
      args.headlessExplicit = true;
    }
    else if (arg === "--headless") {
      args.headless = true;
      args.headlessExplicit = true;
    }
    else if (arg === "--out") args.outPath = path.resolve(process.cwd(), argv[++i]);
    else if (arg === "--help") {
      printHelp();
      process.exit(0);
    }
  }

  args.durationMs = Math.max(1000, Math.floor(args.durationMs));
  args.warmupMs = Math.max(0, Math.floor(args.warmupMs));
  args.sampleIntervalMs = Math.max(100, Math.floor(args.sampleIntervalMs));
  args.connectTimeoutMs = Math.max(1000, Math.floor(args.connectTimeoutMs));
  args.fpsThreshold = Math.max(0, Number(args.fpsThreshold) || 0);

  return args;
}

function printHelp() {
  console.log(`Battle probe options:
  --url <url>                    Page URL (default: stable mode)
  --ws <ws_url>                  Override wsUrl input before connect
  --auto-connect                 Click Connect and wait for live state (default: true)
  --no-auto-connect              Do not auto-connect
  --connect-timeout-ms <ms>      Timeout waiting for live state (default: 90000)
  --warmup <seconds>             Warmup after connect (default: 5)
  --duration <seconds>           Sample duration (default: 30)
  --sample-interval-ms <ms>      Sample interval (default: 500)
  --fps-threshold <fps>          Optional FPS gate (default: 0 disabled)
  --headed                       Run with visible browser
  --headless                     Force headless mode
  --out <path>                   Output JSON path
`);
}

function toFixed(value, digits = 2) {
  if (!Number.isFinite(value)) return value;
  return Number(value.toFixed(digits));
}

function healthyStatus(status) {
  return status === "waiting" || status === "respawn" || status === "playing";
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

function summaryFromSamples(samples, fps) {
  const sampleCount = samples.length;
  const samplesWithProjectiles = samples.filter((s) => (s.visibleProjectileCount || 0) > 0).length;
  const samplesWithEffects = samples.filter((s) => (s.activeEffectCount || 0) > 0).length;

  return {
    maxPlayerCount: samples.reduce((m, s) => Math.max(m, s.playerCount || 0), 0),
    maxVisiblePlayerCount: samples.reduce((m, s) => Math.max(m, s.visiblePlayerCount || 0), 0),
    maxProjectileCount: samples.reduce((m, s) => Math.max(m, s.projectileCount || 0), 0),
    maxVisibleProjectileCount: samples.reduce((m, s) => Math.max(m, s.visibleProjectileCount || 0), 0),
    maxActiveEffectCount: samples.reduce((m, s) => Math.max(m, s.activeEffectCount || 0), 0),
    maxSmoothedFrameMs: samples.reduce((m, s) => Math.max(m, s.smoothedFrameMs || 0), 0),
    samplesWithProjectiles,
    samplesWithEffects,
    sampleCount,
    projectileActiveRatio: sampleCount > 0 ? toFixed(samplesWithProjectiles / sampleCount, 3) : 0,
    effectsActiveRatio: sampleCount > 0 ? toFixed(samplesWithEffects / sampleCount, 3) : 0,
    meets60Fps: fps >= 60
  };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const startedAt = Date.now();
  const webgpuRequested = urlRequestsWebGpu(args.url);

  // WebGPU instance layers are typically disabled in headless Chromium.
  // For battle validation, default to headed mode unless explicitly overridden.
  if (webgpuRequested && !args.headlessExplicit) {
    args.headless = false;
  }

  const launchOptions = buildLaunchOptions({ headless: args.headless, url: args.url });
  const browser = await chromium.launch(launchOptions);
  const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  const page = await context.newPage();

  await page.addInitScript(() => {
    window.__battleProbe = {
      running: false,
      frames: 0,
      longTasks: 0,
      longTaskTotalMs: 0,
      longTaskMaxMs: 0,
      start: 0,
      end: 0
    };

    const tick = () => {
      if (window.__battleProbe?.running) {
        window.__battleProbe.frames += 1;
      }
      window.requestAnimationFrame(tick);
    };
    window.requestAnimationFrame(tick);

    if ("PerformanceObserver" in window) {
      try {
        const observer = new PerformanceObserver((list) => {
          if (!window.__battleProbe?.running) return;
          for (const entry of list.getEntries()) {
            const duration = Number(entry.duration || 0);
            window.__battleProbe.longTasks += 1;
            window.__battleProbe.longTaskTotalMs += duration;
            if (duration > window.__battleProbe.longTaskMaxMs) {
              window.__battleProbe.longTaskMaxMs = duration;
            }
          }
        });
        observer.observe({ entryTypes: ["longtask"] });
      } catch (_) {
        // Long task API may be unavailable.
      }
    }
  });

  try {
    await page.goto(args.url, { waitUntil: "domcontentloaded", timeout: 60000 });
    if (args.wsUrl) await setWsUrl(page, args.wsUrl);
    if (args.autoConnect) {
      await clickConnect(page);
      await waitForLiveState(page, args.connectTimeoutMs);
    }

    if (args.warmupMs > 0) {
      await page.waitForTimeout(args.warmupMs);
    }

    await page.evaluate(() => {
      window.__battleProbe.running = true;
      window.__battleProbe.frames = 0;
      window.__battleProbe.longTasks = 0;
      window.__battleProbe.longTaskTotalMs = 0;
      window.__battleProbe.longTaskMaxMs = 0;
      window.__battleProbe.start = performance.now();
      window.__battleProbe.end = 0;
    });

    const sampleTarget = Math.max(1, Math.ceil(args.durationMs / args.sampleIntervalMs));
    const samples = [];
    for (let i = 0; i < sampleTarget; i += 1) {
      await page.waitForTimeout(args.sampleIntervalMs);
      const sample = await page.evaluate(() => ({
        atMs: Number(performance.now().toFixed(1)),
        status: window.__e2e?.connectionStatus?.statusKey ?? null,
        playerCount: Number(window.__e2e?.playerCount ?? 0),
        visiblePlayerCount: Number(window.__e2e?.visiblePlayerCount ?? 0),
        projectileCount: Number(window.__e2e?.projectileCount ?? 0),
        visibleProjectileCount: Number(window.__e2e?.visibleProjectileCount ?? 0),
        activeEffectCount: Number(window.__e2e?.activeEffectCount ?? 0),
        smoothedFrameMs: Number(window.__e2e?.smoothedFrameMs ?? 0),
        ultraPerformanceMode: Boolean(window.__e2e?.ultraPerformanceMode),
        effectsProfile: window.__e2e?.effectsProfile ?? null,
        webgpuProjectileLayerReady: Boolean(window.__e2e?.webgpuProjectileLayerReady),
        webgpuProjectileInstances: Number(window.__e2e?.webgpuProjectileInstances ?? 0),
        webgpuPlayerLayerReady: Boolean(window.__e2e?.webgpuPlayerLayerReady),
        webgpuPlayerInstances: Number(window.__e2e?.webgpuPlayerInstances ?? 0)
      }));
      samples.push(sample);
    }

    const final = await page.evaluate(() => {
      window.__battleProbe.running = false;
      window.__battleProbe.end = performance.now();
      return {
        frames: Number(window.__battleProbe.frames ?? 0),
        longTasks: Number(window.__battleProbe.longTasks ?? 0),
        longTaskTotalMs: Number(window.__battleProbe.longTaskTotalMs ?? 0),
        longTaskMaxMs: Number(window.__battleProbe.longTaskMaxMs ?? 0),
        start: Number(window.__battleProbe.start ?? 0),
        end: Number(window.__battleProbe.end ?? 0),
        status: window.__e2e?.connectionStatus?.statusKey ?? null,
        playerCount: Number(window.__e2e?.playerCount ?? 0),
        visiblePlayerCount: Number(window.__e2e?.visiblePlayerCount ?? 0),
        projectileCount: Number(window.__e2e?.projectileCount ?? 0),
        visibleProjectileCount: Number(window.__e2e?.visibleProjectileCount ?? 0),
        activeEffectCount: Number(window.__e2e?.activeEffectCount ?? 0),
        smoothedFrameMs: Number(window.__e2e?.smoothedFrameMs ?? 0),
        ultraPerformanceMode: Boolean(window.__e2e?.ultraPerformanceMode),
        effectsProfile: window.__e2e?.effectsProfile ?? null,
        webgpuProjectileLayerReady: Boolean(window.__e2e?.webgpuProjectileLayerReady),
        webgpuProjectileInstances: Number(window.__e2e?.webgpuProjectileInstances ?? 0),
        webgpuPlayerLayerReady: Boolean(window.__e2e?.webgpuPlayerLayerReady),
        webgpuPlayerInstances: Number(window.__e2e?.webgpuPlayerInstances ?? 0)
      };
    });

    const durationSec = final.end > final.start ? (final.end - final.start) / 1000 : 0;
    const fps = durationSec > 0 ? final.frames / durationSec : 0;
    const longTaskAvgMs = final.longTasks > 0 ? final.longTaskTotalMs / final.longTasks : 0;
    const summary = summaryFromSamples(samples, fps);

    const result = {
      url: args.url,
      wsUrl: args.wsUrl,
      launch: {
        headless: args.headless,
        webgpuRequested: launchOptions.webgpuRequested
      },
      durationSec: toFixed(durationSec, 2),
      runtime: {
        fps: toFixed(fps, 2),
        frames: final.frames,
        longTasks: final.longTasks,
        longTaskTotalMs: toFixed(final.longTaskTotalMs, 2),
        longTaskMaxMs: toFixed(final.longTaskMaxMs, 2),
        longTaskAvgMs: toFixed(longTaskAvgMs, 2)
      },
      final,
      summary,
      samples,
      startedAt: new Date(startedAt).toISOString(),
      finishedAt: new Date().toISOString()
    };

    fs.mkdirSync(path.dirname(args.outPath), { recursive: true });
    fs.writeFileSync(args.outPath, JSON.stringify(result, null, 2));
    console.log(JSON.stringify(result, null, 2));

    if (args.fpsThreshold > 0 && result.runtime.fps < args.fpsThreshold) {
      process.exit(2);
    }
  } finally {
    await browser.close();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
