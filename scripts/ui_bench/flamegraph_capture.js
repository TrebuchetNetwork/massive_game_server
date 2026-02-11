#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const { chromium } = require("playwright");

const META_FRAME_NAMES = new Set([
  "(root)",
  "(program)",
  "(idle)",
  "(garbage collector)"
]);

function parseArgs(argv) {
  const args = {
    url: "http://127.0.0.1:18080/client.html?profile=1&mode=stable",
    wsUrl: null,
    autoConnect: true,
    connectTimeoutMs: 90000,
    warmupMs: 5000,
    durationMs: 20000,
    sampleIntervalMs: 250,
    topFrames: 20,
    includeMetaFrames: false,
    headless: true,
    outPath: path.resolve(process.cwd(), "artifacts", "scale", "ui_flamegraph.cpuprofile"),
    summaryPath: null
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
    else if (arg === "--top-frames") args.topFrames = Number(argv[++i]);
    else if (arg === "--include-meta-frames") args.includeMetaFrames = true;
    else if (arg === "--headed") args.headless = false;
    else if (arg === "--out") args.outPath = path.resolve(process.cwd(), argv[++i]);
    else if (arg === "--summary-out") args.summaryPath = path.resolve(process.cwd(), argv[++i]);
    else if (arg === "--help") {
      printHelp();
      process.exit(0);
    }
  }

  args.connectTimeoutMs = Math.max(1000, Math.floor(Number(args.connectTimeoutMs) || 90000));
  args.warmupMs = Math.max(0, Math.floor(Number(args.warmupMs) || 0));
  args.durationMs = Math.max(1000, Math.floor(Number(args.durationMs) || 20000));
  args.sampleIntervalMs = Math.max(50, Math.floor(Number(args.sampleIntervalMs) || 250));
  args.topFrames = Math.max(1, Math.floor(Number(args.topFrames) || 20));
  if (!args.summaryPath) {
    args.summaryPath = args.outPath.endsWith(".cpuprofile")
      ? args.outPath.replace(/\.cpuprofile$/i, "_summary.json")
      : `${args.outPath}.summary.json`;
  }
  return args;
}

function printHelp() {
  console.log(`Flamegraph capture options:
  --url <url>                    Page URL (default: stable profile URL)
  --ws <ws_url>                  Override wsUrl input before connect
  --auto-connect                 Click Connect and wait for live state (default: true)
  --no-auto-connect              Do not auto-connect
  --connect-timeout-ms <ms>      Timeout waiting for live state (default: 90000)
  --warmup <seconds>             Warmup before CPU profiling (default: 5)
  --duration <seconds>           CPU profile duration (default: 20)
  --sample-interval-ms <ms>      Poll interval waiting for live state (default: 250)
  --top-frames <n>               Number of top frames to report (default: 20)
  --include-meta-frames          Include root/program/idle/GC frames in rankings
  --headed                       Run with visible browser
  --out <path>                   Output .cpuprofile path
  --summary-out <path>           Output summary JSON path
`);
}

function ensureProfileParam(urlValue) {
  const url = new URL(urlValue);
  if (!url.searchParams.has("profile") && !url.searchParams.has("perf")) {
    url.searchParams.set("profile", "1");
  }
  return url.toString();
}

function healthyStatus(status) {
  return status === "waiting" || status === "respawn" || status === "playing";
}

function shortUrl(rawUrl) {
  if (!rawUrl) return "";
  try {
    const parsed = new URL(rawUrl);
    const pathname = parsed.pathname || "";
    return pathname.length > 0 ? pathname : rawUrl;
  } catch (_) {
    return rawUrl;
  }
}

function toFixed(value, digits = 3) {
  if (!Number.isFinite(value)) return value;
  return Number(value.toFixed(digits));
}

function mapToSortedRows(map, nodeById, totalUs, options = {}) {
  const rows = [];
  const includeMetaFrames = options.includeMetaFrames === true;
  const topFrames = options.topFrames || 20;
  const includeOnlyClient = options.includeOnlyClient === true;

  for (const [id, valueUs] of map.entries()) {
    if (!Number.isFinite(valueUs) || valueUs <= 0) continue;
    const node = nodeById.get(id);
    if (!node) continue;
    const callFrame = node.callFrame || {};
    const functionName = String(callFrame.functionName || "(anonymous)");
    const url = String(callFrame.url || "");

    if (!includeMetaFrames && META_FRAME_NAMES.has(functionName)) {
      continue;
    }
    if (includeOnlyClient && !url.includes("/client.html")) {
      continue;
    }

    const location = url
      ? `${shortUrl(url)}:${(Number(callFrame.lineNumber) || 0) + 1}`
      : "";
    const label = location ? `${functionName} (${location})` : functionName;
    const valueMs = valueUs / 1000;
    rows.push({
      id,
      label,
      functionName,
      url,
      line: (Number(callFrame.lineNumber) || 0) + 1,
      selfMs: toFixed(valueMs),
      selfPct: totalUs > 0 ? toFixed((valueUs / totalUs) * 100) : 0
    });
  }

  rows.sort((a, b) => b.selfMs - a.selfMs);
  return rows.slice(0, topFrames);
}

function mapToInclusiveRows(map, nodeById, totalUs, options = {}) {
  const rows = [];
  const includeMetaFrames = options.includeMetaFrames === true;
  const topFrames = options.topFrames || 20;
  const includeOnlyClient = options.includeOnlyClient === true;

  for (const [id, valueUs] of map.entries()) {
    if (!Number.isFinite(valueUs) || valueUs <= 0) continue;
    const node = nodeById.get(id);
    if (!node) continue;
    const callFrame = node.callFrame || {};
    const functionName = String(callFrame.functionName || "(anonymous)");
    const url = String(callFrame.url || "");

    if (!includeMetaFrames && META_FRAME_NAMES.has(functionName)) {
      continue;
    }
    if (includeOnlyClient && !url.includes("/client.html")) {
      continue;
    }

    const location = url
      ? `${shortUrl(url)}:${(Number(callFrame.lineNumber) || 0) + 1}`
      : "";
    const label = location ? `${functionName} (${location})` : functionName;
    const valueMs = valueUs / 1000;
    rows.push({
      id,
      label,
      functionName,
      url,
      line: (Number(callFrame.lineNumber) || 0) + 1,
      totalMs: toFixed(valueMs),
      totalPct: totalUs > 0 ? toFixed((valueUs / totalUs) * 100) : 0
    });
  }

  rows.sort((a, b) => b.totalMs - a.totalMs);
  return rows.slice(0, topFrames);
}

function analyzeProfile(profile, options) {
  const nodes = Array.isArray(profile?.nodes) ? profile.nodes : [];
  const samples = Array.isArray(profile?.samples) ? profile.samples : [];
  const timeDeltas = Array.isArray(profile?.timeDeltas) ? profile.timeDeltas : [];

  const nodeById = new Map();
  const parentById = new Map();
  for (const node of nodes) {
    nodeById.set(node.id, node);
    if (Array.isArray(node.children)) {
      for (const childId of node.children) {
        parentById.set(childId, node.id);
      }
    }
  }

  const sampleCount = samples.length;
  const totalSpan = Math.max(0, Number(profile?.endTime || 0) - Number(profile?.startTime || 0));
  const fallbackDeltaUs = sampleCount > 0 ? Math.max(1, totalSpan / sampleCount) : 1000;

  const selfUsByNode = new Map();
  const totalUsByNode = new Map();
  let totalSamplesUs = 0;

  for (let i = 0; i < sampleCount; i += 1) {
    const nodeId = Number(samples[i]) || 0;
    if (!nodeId) continue;
    const sampleUs = Number(timeDeltas[i]) > 0 ? Number(timeDeltas[i]) : fallbackDeltaUs;
    totalSamplesUs += sampleUs;
    selfUsByNode.set(nodeId, (selfUsByNode.get(nodeId) || 0) + sampleUs);

    let current = nodeId;
    let guard = 0;
    while (current && guard < 4096) {
      totalUsByNode.set(current, (totalUsByNode.get(current) || 0) + sampleUs);
      current = parentById.get(current) || 0;
      guard += 1;
    }
  }

  return {
    nodeCount: nodes.length,
    sampleCount,
    totalSampleMs: toFixed(totalSamplesUs / 1000),
    topSelf: mapToSortedRows(selfUsByNode, nodeById, totalSamplesUs, options),
    topTotal: mapToInclusiveRows(totalUsByNode, nodeById, totalSamplesUs, options),
    topSelfClient: mapToSortedRows(selfUsByNode, nodeById, totalSamplesUs, {
      ...options,
      includeOnlyClient: true
    }),
    topTotalClient: mapToInclusiveRows(totalUsByNode, nodeById, totalSamplesUs, {
      ...options,
      includeOnlyClient: true
    })
  };
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

async function waitForLiveState(page, timeoutMs, sampleIntervalMs) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
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
    await page.waitForTimeout(sampleIntervalMs);
  }
  throw new Error(`timed out after ${timeoutMs}ms waiting for live state`);
}

async function captureSnapshot(page) {
  return page.evaluate(() => {
    const e2e = window.__e2e || null;
    const perfTopPhases = Array.isArray(e2e?.perfReport?.rankedPhases)
      ? e2e.perfReport.rankedPhases.slice(0, 12)
      : [];
    return {
      status: e2e?.connectionStatus?.statusKey ?? null,
      playerCount: Number(e2e?.playerCount ?? 0),
      visiblePlayerCount: Number(e2e?.visiblePlayerCount ?? 0),
      projectileCount: Number(e2e?.projectileCount ?? 0),
      visibleProjectileCount: Number(e2e?.visibleProjectileCount ?? 0),
      activeEffectCount: Number(e2e?.activeEffectCount ?? 0),
      smoothedFrameMs: Number(e2e?.smoothedFrameMs ?? 0),
      workerCullReady: Boolean(e2e?.workerCullReady),
      workerCullKernel: e2e?.workerCullKernel ?? null,
      workerCullAvgComputeMs: Number(e2e?.workerCullAvgComputeMs ?? 0),
      workerCullDropped: Number(e2e?.workerCullDropped ?? 0),
      webgpuProjectileLayerReady: Boolean(e2e?.webgpuProjectileLayerReady),
      webgpuProjectileInstances: Number(e2e?.webgpuProjectileInstances ?? 0),
      webgpuPlayerLayerReady: Boolean(e2e?.webgpuPlayerLayerReady),
      webgpuPlayerInstances: Number(e2e?.webgpuPlayerInstances ?? 0),
      perfReportTopPhases: perfTopPhases
    };
  });
}

async function main() {
  const rawArgs = parseArgs(process.argv.slice(2));
  const args = {
    ...rawArgs,
    url: ensureProfileParam(rawArgs.url)
  };

  const browser = await chromium.launch({ headless: args.headless });
  const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  const page = await context.newPage();

  try {
    await page.goto(args.url, { waitUntil: "domcontentloaded", timeout: 60000 });
    if (args.wsUrl) {
      await setWsUrl(page, args.wsUrl);
    }
    if (args.autoConnect) {
      await clickConnect(page);
      await waitForLiveState(page, args.connectTimeoutMs, args.sampleIntervalMs);
    }
    if (args.warmupMs > 0) {
      await page.waitForTimeout(args.warmupMs);
    }

    const cdp = await context.newCDPSession(page);
    await cdp.send("Profiler.enable");
    await cdp.send("Profiler.start");

    await page.waitForTimeout(args.durationMs);

    const stopResult = await cdp.send("Profiler.stop");
    const profile = stopResult?.profile || null;
    if (!profile) {
      throw new Error("Profiler.stop returned no profile");
    }

    const snapshot = await captureSnapshot(page);
    const analysis = analyzeProfile(profile, {
      topFrames: args.topFrames,
      includeMetaFrames: args.includeMetaFrames
    });

    const summary = {
      capturedAt: new Date().toISOString(),
      url: args.url,
      wsUrl: args.wsUrl,
      durationSec: toFixed(args.durationMs / 1000, 2),
      profilePath: args.outPath,
      profileStats: {
        nodeCount: analysis.nodeCount,
        sampleCount: analysis.sampleCount,
        totalSampleMs: analysis.totalSampleMs
      },
      snapshot,
      topSelf: analysis.topSelf,
      topTotal: analysis.topTotal,
      topSelfClient: analysis.topSelfClient,
      topTotalClient: analysis.topTotalClient
    };

    fs.mkdirSync(path.dirname(args.outPath), { recursive: true });
    fs.mkdirSync(path.dirname(args.summaryPath), { recursive: true });
    fs.writeFileSync(args.outPath, JSON.stringify(profile));
    fs.writeFileSync(args.summaryPath, JSON.stringify(summary, null, 2));

    console.log(JSON.stringify(summary, null, 2));
  } finally {
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error?.stack || error?.message || String(error));
  process.exitCode = 1;
});
