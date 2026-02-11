#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const { chromium, devices } = require("playwright");

const PROFILE_PRESETS = {
  desktop: {
    viewport: { width: 1440, height: 900 },
    userAgent:
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36",
    hasTouch: false,
    isMobile: false
  },
  iphone13: devices["iPhone 13"],
  iphone12: devices["iPhone 12"],
  pixel7: devices["Pixel 7"],
  galaxyS9: devices["Galaxy S9+"],
  ipadMini: devices["iPad Mini"],
};

function parseArgs(argv) {
  const args = {
    url: "http://127.0.0.1:18080/client.html?mode=mass&worker_cull=1&mobile=1&auto_connect=1&auto_reconnect=1",
    wsUrl: null,
    profiles: ["desktop", "iphone13", "pixel7"],
    durationMs: 10000,
    connectTimeoutMs: 15000,
    reconnectTimeoutMs: 15000,
    sampleIntervalMs: 250,
    minFps: 55,
    maxConnectMs: 10000,
    maxReconnectMs: 15000,
    skipReconnectCheck: false,
    headless: true,
    outPath: path.resolve(process.cwd(), "artifacts", "scale", "mobile_matrix.json")
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--url") args.url = argv[++i];
    else if (arg === "--ws") args.wsUrl = argv[++i];
    else if (arg === "--profiles") {
      args.profiles = String(argv[++i] || "")
        .split(",")
        .map((value) => value.trim())
        .filter((value) => value.length > 0);
    } else if (arg === "--duration") args.durationMs = Number(argv[++i]) * 1000;
    else if (arg === "--connect-timeout-ms") args.connectTimeoutMs = Number(argv[++i]);
    else if (arg === "--reconnect-timeout-ms") args.reconnectTimeoutMs = Number(argv[++i]);
    else if (arg === "--sample-interval-ms") args.sampleIntervalMs = Number(argv[++i]);
    else if (arg === "--min-fps") args.minFps = Number(argv[++i]);
    else if (arg === "--max-connect-ms") args.maxConnectMs = Number(argv[++i]);
    else if (arg === "--max-reconnect-ms") args.maxReconnectMs = Number(argv[++i]);
    else if (arg === "--skip-reconnect-check") args.skipReconnectCheck = true;
    else if (arg === "--headed") args.headless = false;
    else if (arg === "--out") args.outPath = path.resolve(process.cwd(), argv[++i]);
    else if (arg === "--help") {
      printHelp();
      process.exit(0);
    }
  }

  args.durationMs = Math.max(1000, Math.floor(Number(args.durationMs) || 10000));
  args.connectTimeoutMs = Math.max(1000, Math.floor(Number(args.connectTimeoutMs) || 15000));
  args.reconnectTimeoutMs = Math.max(1000, Math.floor(Number(args.reconnectTimeoutMs) || 15000));
  args.sampleIntervalMs = Math.max(50, Math.floor(Number(args.sampleIntervalMs) || 250));
  args.minFps = Math.max(0, Number(args.minFps) || 0);
  args.maxConnectMs = Math.max(100, Math.floor(Number(args.maxConnectMs) || 10000));
  args.maxReconnectMs = Math.max(100, Math.floor(Number(args.maxReconnectMs) || 15000));
  if (!Array.isArray(args.profiles) || args.profiles.length === 0) {
    args.profiles = ["desktop", "iphone13", "pixel7"];
  }
  return args;
}

function printHelp() {
  console.log(`Mobile/browser matrix options:
  --url <url>                    Page URL
  --ws <ws_url>                  Override wsUrl input before connect
  --profiles <csv>               Profile keys (desktop,iphone13,pixel7,ipadMini,...)
  --duration <seconds>           In-match sample duration per profile (default: 10)
  --connect-timeout-ms <ms>      Wait limit for initial connect (default: 15000)
  --reconnect-timeout-ms <ms>    Wait limit for reconnect recovery (default: 15000)
  --sample-interval-ms <ms>      Poll interval while waiting states (default: 250)
  --min-fps <fps>                Pass threshold per profile (default: 55)
  --max-connect-ms <ms>          Pass threshold for initial connect (default: 10000)
  --max-reconnect-ms <ms>        Pass threshold for reconnect (default: 15000)
  --skip-reconnect-check         Disable reconnect injection test
  --headed                       Run with visible browser
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

async function getState(page) {
  return page.evaluate(() => ({
    atMs: Number(performance.now().toFixed(1)),
    status: window.__e2e?.connectionStatus?.statusKey ?? null,
    detail: window.__e2e?.connectionStatus?.detailText ?? "",
    playerCount: Number(window.__e2e?.playerCount ?? 0),
    visiblePlayerCount: Number(window.__e2e?.visiblePlayerCount ?? 0),
    smoothedFrameMs: Number(window.__e2e?.smoothedFrameMs ?? 0),
    mobileModeClass: document.body.classList.contains("mobile-mode"),
    autoReconnectEnabled: Boolean(window.__e2e?.autoReconnectEnabled),
    iceServerCount: Number(window.__e2e?.iceServerCount ?? 0),
    lastStateUpdate: Number(window.__e2e?.lastStateUpdate ?? 0)
  }));
}

async function waitForLiveState(page, timeoutMs, sampleIntervalMs) {
  const startedAt = Date.now();
  let lastState = null;
  while (Date.now() - startedAt < timeoutMs) {
    lastState = await getState(page);
    if (healthyStatus(lastState.status) && lastState.lastStateUpdate > 0) {
      return {
        ok: true,
        elapsedMs: Date.now() - startedAt,
        state: lastState
      };
    }
    await page.waitForTimeout(sampleIntervalMs);
  }
  return {
    ok: false,
    elapsedMs: Date.now() - startedAt,
    state: lastState
  };
}

async function sampleFps(page, durationMs) {
  await page.evaluate(() => {
    window.__matrixProbe = {
      running: true,
      frames: 0,
      start: performance.now(),
      end: 0
    };
    const tick = () => {
      if (window.__matrixProbe?.running) {
        window.__matrixProbe.frames += 1;
      }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });

  await page.waitForTimeout(durationMs);

  return page.evaluate(() => {
    if (!window.__matrixProbe) {
      return { fps: 0, frames: 0, durationSec: 0 };
    }
    window.__matrixProbe.running = false;
    window.__matrixProbe.end = performance.now();
    const elapsedMs = Math.max(1, window.__matrixProbe.end - window.__matrixProbe.start);
    const durationSec = elapsedMs / 1000;
    const fps = window.__matrixProbe.frames / durationSec;
    return {
      fps: Number(fps.toFixed(2)),
      frames: Number(window.__matrixProbe.frames || 0),
      durationSec: Number(durationSec.toFixed(2))
    };
  });
}

async function runProfile(browser, args, profileName) {
  const preset = PROFILE_PRESETS[profileName];
  if (!preset) {
    return {
      profile: profileName,
      passed: false,
      failures: [`Unknown profile "${profileName}"`]
    };
  }

  const context = await browser.newContext(preset);
  const page = await context.newPage();
  const failures = [];

  try {
    await page.goto(args.url, { waitUntil: "domcontentloaded", timeout: 60000 });
    if (args.wsUrl) {
      await setWsUrl(page, args.wsUrl);
    }
    await clickConnect(page);

    const initialLive = await waitForLiveState(page, args.connectTimeoutMs, args.sampleIntervalMs);
    if (!initialLive.ok) {
      failures.push(`Initial connect timed out after ${initialLive.elapsedMs}ms`);
    } else if (initialLive.elapsedMs > args.maxConnectMs) {
      failures.push(`Initial connect ${initialLive.elapsedMs}ms > max ${args.maxConnectMs}ms`);
    }

    let reconnectResult = null;
    if (!args.skipReconnectCheck) {
      const chaos = await page.evaluate(() => {
        if (typeof window.__e2e?.forceCloseDataChannel === "function") {
          return { triggered: !!window.__e2e.forceCloseDataChannel(), mode: "data" };
        }
        return { triggered: false, mode: "none" };
      });
      if (!chaos.triggered) {
        failures.push("Reconnect chaos trigger unavailable");
      } else {
        reconnectResult = await waitForLiveState(page, args.reconnectTimeoutMs, args.sampleIntervalMs);
        if (!reconnectResult.ok) {
          failures.push(`Reconnect timed out after ${reconnectResult.elapsedMs}ms`);
        } else if (reconnectResult.elapsedMs > args.maxReconnectMs) {
          failures.push(`Reconnect ${reconnectResult.elapsedMs}ms > max ${args.maxReconnectMs}ms`);
        }
      }
    }

    const fpsSample = await sampleFps(page, args.durationMs);
    if (fpsSample.fps < args.minFps) {
      failures.push(`FPS ${fpsSample.fps} < min ${args.minFps}`);
    }

    const finalState = await getState(page);
    const result = {
      profile: profileName,
      emulation: {
        viewport: preset.viewport || null,
        isMobile: Boolean(preset.isMobile),
        hasTouch: Boolean(preset.hasTouch)
      },
      initialConnectMs: initialLive.elapsedMs,
      reconnectMs: reconnectResult ? reconnectResult.elapsedMs : null,
      fps: fpsSample.fps,
      frames: fpsSample.frames,
      durationSec: fpsSample.durationSec,
      finalState,
      passed: failures.length === 0,
      failures
    };
    return result;
  } finally {
    await context.close();
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const startedAt = Date.now();
  const browser = await chromium.launch({ headless: args.headless });

  try {
    const profileResults = [];
    for (const profileName of args.profiles) {
      const result = await runProfile(browser, args, profileName);
      profileResults.push(result);
      console.log(
        `[mobile-matrix] ${profileName} passed=${result.passed} connect=${result.initialConnectMs ?? "n/a"}ms reconnect=${result.reconnectMs ?? "n/a"}ms fps=${result.fps ?? "n/a"}`
      );
    }

    const failures = [];
    const passedProfiles = profileResults.filter((result) => result.passed).length;
    if (passedProfiles < profileResults.length) {
      failures.push(`${profileResults.length - passedProfiles} profile(s) failed`);
    }

    const result = {
      url: args.url,
      wsUrl: args.wsUrl,
      profiles: args.profiles,
      thresholds: {
        minFps: args.minFps,
        maxConnectMs: args.maxConnectMs,
        maxReconnectMs: args.maxReconnectMs
      },
      summary: {
        totalProfiles: profileResults.length,
        passedProfiles,
        failedProfiles: profileResults.length - passedProfiles
      },
      profileResults,
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

