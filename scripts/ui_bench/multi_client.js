#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const { chromium } = require("playwright");

function parseArgs(argv) {
  const args = {
    url: "http://localhost:8080/client.html",
    wsUrl: null,
    clients: 24,
    connectConcurrency: 6,
    durationMs: 45000,
    spawnDelayMs: 120,
    connectTimeoutMs: 30000,
    navTimeoutMs: 60000,
    clickTimeoutMs: 10000,
    stateReadTimeoutMs: 5000,
    sampleIntervalMs: 2000,
    minConnectedRatio: 0.9,
    maxErrorClients: 2,
    maxTotalMs: 0,
    waveBatchSize: 24,
    waveBatchDelayMs: 500,
    joinStageUrl: null,
    resetJoinStages: false,
    headless: true,
    outPath: path.resolve(process.cwd(), "artifacts", "scale", "multi_client.json"),
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--url") args.url = argv[++i];
    else if (arg === "--ws") args.wsUrl = argv[++i];
    else if (arg === "--clients") args.clients = Number(argv[++i]);
    else if (arg === "--connect-concurrency") args.connectConcurrency = Number(argv[++i]);
    else if (arg === "--duration") args.durationMs = Number(argv[++i]) * 1000;
    else if (arg === "--spawn-delay-ms") args.spawnDelayMs = Number(argv[++i]);
    else if (arg === "--connect-timeout-ms") args.connectTimeoutMs = Number(argv[++i]);
    else if (arg === "--nav-timeout-ms") args.navTimeoutMs = Number(argv[++i]);
    else if (arg === "--click-timeout-ms") args.clickTimeoutMs = Number(argv[++i]);
    else if (arg === "--state-read-timeout-ms") args.stateReadTimeoutMs = Number(argv[++i]);
    else if (arg === "--sample-interval-ms") args.sampleIntervalMs = Number(argv[++i]);
    else if (arg === "--min-connected-ratio") args.minConnectedRatio = Number(argv[++i]);
    else if (arg === "--max-error-clients") args.maxErrorClients = Number(argv[++i]);
    else if (arg === "--max-total-ms") args.maxTotalMs = Number(argv[++i]);
    else if (arg === "--wave-batch-size") args.waveBatchSize = Number(argv[++i]);
    else if (arg === "--wave-batch-delay-ms") args.waveBatchDelayMs = Number(argv[++i]);
    else if (arg === "--join-stage-url") args.joinStageUrl = argv[++i];
    else if (arg === "--reset-join-stages") args.resetJoinStages = true;
    else if (arg === "--headed") args.headless = false;
    else if (arg === "--out") args.outPath = path.resolve(process.cwd(), argv[++i]);
    else if (arg === "--help") {
      printHelp();
      process.exit(0);
    }
  }

  return args;
}

function printHelp() {
  console.log(`Multi-client scale runner options:
  --url <url>                  Page URL (default: localhost client.html)
  --ws <ws_url>                Override wsUrl input value
  --clients <count>            Number of browser clients (default: 24)
  --connect-concurrency <n>    Max concurrent connect attempts (default: 6)
  --duration <seconds>         Sampling duration after connect phase (default: 45)
  --spawn-delay-ms <ms>        Delay between launching clients (default: 120)
  --connect-timeout-ms <ms>    Per-client connect timeout (default: 30000)
  --nav-timeout-ms <ms>        Page navigation timeout (default: 60000)
  --click-timeout-ms <ms>      Connect click timeout (default: 10000)
  --state-read-timeout-ms <ms> Timeout for per-page state reads (default: 5000)
  --sample-interval-ms <ms>    Sampling interval during run (default: 2000)
  --min-connected-ratio <0-1>  Minimum required connected ratio (default: 0.9)
  --max-error-clients <count>  Max clients allowed in error state (default: 2)
  --max-total-ms <ms>          Hard timeout for full benchmark (default: auto)
  --wave-batch-size <count>    Optional launch wave size (default: 24, 0 disables)
  --wave-batch-delay-ms <ms>   Delay between launch waves (default: 500)
  --join-stage-url <url>       Optional server join-stage report endpoint
  --reset-join-stages          Reset join-stage metrics before launch (requires endpoint)
  --headed                     Show browser UI
  --out <path>                 Output JSON path (default: artifacts/scale/multi_client.json)
  --help                       Show help
`);
}

function isHealthyStatus(status) {
  return status === "waiting" || status === "respawn" || status === "playing";
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

const CONNECT_LATENCY_WAVES = [
  { key: "wave_1_24", label: "1-24", startClientId: 1, endClientId: 24 },
  { key: "wave_25_48", label: "25-48", startClientId: 25, endClientId: 48 },
  { key: "wave_49_72", label: "49-72", startClientId: 49, endClientId: 72 },
  { key: "wave_73_plus", label: "73+", startClientId: 73, endClientId: null },
];

const JOIN_TIMING_FIELDS = [
  { key: "signalingOpenMs", label: "signaling-open" },
  { key: "offerCreatedMs", label: "offer-created" },
  { key: "localDescriptionMs", label: "local-description" },
  { key: "answerReceivedMs", label: "answer-received" },
  { key: "remoteDescriptionMs", label: "remote-description" },
  { key: "firstIceCandidateMs", label: "first-ice-candidate" },
  { key: "dataChannelOpenMs", label: "datachannel-open" },
  { key: "firstPacketMs", label: "first-packet" },
  { key: "firstStateMs", label: "first-state" },
  { key: "firstRenderMs", label: "first-render" },
  { key: "totalMs", label: "total" },
];

function percentileFromSorted(values, percentile) {
  if (!values.length) return 0;
  if (values.length === 1) return values[0];
  const clamped = Math.max(0, Math.min(1, percentile));
  const index = (values.length - 1) * clamped;
  const lower = Math.floor(index);
  const upper = Math.ceil(index);
  if (lower === upper) return values[lower];
  const weight = index - lower;
  return values[lower] + (values[upper] - values[lower]) * weight;
}

function summarizeNumericDurations(rawDurations) {
  const durations = rawDurations
    .filter((value) => Number.isFinite(value) && value >= 0)
    .sort((a, b) => a - b);
  if (!durations.length) {
    return {
      count: 0,
      minMs: 0,
      avgMs: 0,
      maxMs: 0,
      p50Ms: 0,
      p90Ms: 0,
      p95Ms: 0,
      p99Ms: 0,
    };
  }

  const avgMs =
    durations.reduce((sum, value) => sum + value, 0) / durations.length;

  return {
    count: durations.length,
    minMs: Number(durations[0].toFixed(2)),
    avgMs: Number(avgMs.toFixed(2)),
    maxMs: Number(durations[durations.length - 1].toFixed(2)),
    p50Ms: Number(percentileFromSorted(durations, 0.5).toFixed(2)),
    p90Ms: Number(percentileFromSorted(durations, 0.9).toFixed(2)),
    p95Ms: Number(percentileFromSorted(durations, 0.95).toFixed(2)),
    p99Ms: Number(percentileFromSorted(durations, 0.99).toFixed(2)),
  };
}

function summarizeConnectLatency(connectLatencyEvents) {
  const summary = summarizeNumericDurations(
    connectLatencyEvents.map((event) => event.durationMs)
  );

  const slowestClients = connectLatencyEvents
    .slice()
    .sort((a, b) => b.durationMs - a.durationMs)
    .slice(0, 5)
    .map((event) => ({
      clientId: event.clientId,
      durationMs: Number(event.durationMs.toFixed(2)),
      statusKey: event.statusKey || "unknown",
    }));

  return {
    ...summary,
    slowestClients,
  };
}

function requestedSlotsForWave(clientsRequested, wave) {
  const start = wave.startClientId;
  const endInclusive =
    Number.isFinite(wave.endClientId) ? wave.endClientId : clientsRequested;
  if (clientsRequested < start) return 0;
  const clampedEnd = Math.min(clientsRequested, endInclusive);
  return Math.max(0, clampedEnd - start + 1);
}

function summarizeConnectLatencyByWave(connectLatencyEvents, clientsRequested) {
  const summaryByWave = {};
  for (const wave of CONNECT_LATENCY_WAVES) {
    const waveEvents = connectLatencyEvents.filter((event) => {
      if (event.clientId < wave.startClientId) return false;
      if (!Number.isFinite(wave.endClientId)) return true;
      return event.clientId <= wave.endClientId;
    });
    const waveSummary = summarizeConnectLatency(waveEvents);
    summaryByWave[wave.key] = {
      label: wave.label,
      startClientId: wave.startClientId,
      endClientId: Number.isFinite(wave.endClientId) ? wave.endClientId : null,
      requestedSlots: requestedSlotsForWave(clientsRequested, wave),
      ...waveSummary,
    };
  }
  return summaryByWave;
}

function summarizeJoinTiming(connectLatencyEvents) {
  const summary = {};
  for (const field of JOIN_TIMING_FIELDS) {
    const durations = connectLatencyEvents
      .map((event) => Number(event?.joinTimingSummary?.[field.key]))
      .filter((value) => Number.isFinite(value) && value >= 0);
    summary[field.key] = {
      label: field.label,
      ...summarizeNumericDurations(durations),
    };
  }
  return summary;
}

function summarizeJoinTimingByWave(connectLatencyEvents, clientsRequested) {
  const summaryByWave = {};
  for (const wave of CONNECT_LATENCY_WAVES) {
    const waveEvents = connectLatencyEvents.filter((event) => {
      if (event.clientId < wave.startClientId) return false;
      if (!Number.isFinite(wave.endClientId)) return true;
      return event.clientId <= wave.endClientId;
    });
    summaryByWave[wave.key] = {
      label: wave.label,
      startClientId: wave.startClientId,
      endClientId: Number.isFinite(wave.endClientId) ? wave.endClientId : null,
      requestedSlots: requestedSlotsForWave(clientsRequested, wave),
      metrics: summarizeJoinTiming(waveEvents),
    };
  }
  return summaryByWave;
}

function withTimeout(promise, timeoutMs, message) {
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    return promise;
  }
  let timer = null;
  const timeoutPromise = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(message)), timeoutMs);
  });

  return Promise.race([promise, timeoutPromise]).finally(() => {
    if (timer) clearTimeout(timer);
  });
}

async function readClientState(page, args, clientId) {
  return withTimeout(
    page.evaluate(() => {
      const statusKey = window.__e2e?.connectionStatus?.statusKey ?? null;
      const detailText = window.__e2e?.connectionStatus?.detailText ?? "";
      const playerCountRaw = document.getElementById("playerCount")?.textContent ?? "0";
      const playerCount = Number.parseInt(playerCountRaw, 10);
      const joinTiming = window.__e2e?.joinTiming ?? null;
      const joinTimingSummary =
        joinTiming && joinTiming.summary && typeof joinTiming.summary === "object"
          ? joinTiming.summary
          : null;
      return {
        statusKey,
        detailText,
        playerCount: Number.isFinite(playerCount) ? playerCount : 0,
        matchInfoReady: Boolean(window.__e2e?.matchInfoReady),
        renderFrames: Number(window.__e2e?.renderFrames ?? 0),
        joinTimingSummary,
      };
    }),
    args.stateReadTimeoutMs,
    `client ${clientId} timed out reading client state`
  );
}

async function waitForConnectedState(page, timeoutMs, args, clientId) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    let state;
    try {
      state = await readClientState(page, args, clientId);
    } catch (_) {
      await page.waitForTimeout(200);
      continue;
    }
    if (isHealthyStatus(state.statusKey)) {
      return state;
    }
    if (state.statusKey === "error") {
      throw new Error(`client entered error state: ${state.detailText || "unknown error"}`);
    }
    await page.waitForTimeout(200);
  }
  throw new Error(`timed out after ${timeoutMs}ms waiting for healthy state`);
}

async function connectClient(page, args, clientId) {
  await withTimeout(
    page.goto(args.url, { waitUntil: "domcontentloaded", timeout: args.navTimeoutMs }),
    args.navTimeoutMs + 1000,
    `client ${clientId} timed out navigating to page`
  );
  if (args.wsUrl) {
    await page.evaluate((wsUrl) => {
      const wsInput = document.getElementById("wsUrl");
      if (wsInput) {
        wsInput.value = wsUrl;
      }
    }, args.wsUrl);
  }
  const connectButton = page.locator("#connectButton");
  if (!(await connectButton.count())) {
    throw new Error("connect button not found");
  }
  try {
    await withTimeout(
      connectButton.click({ timeout: args.clickTimeoutMs }),
      args.clickTimeoutMs + 1000,
      `client ${clientId} timed out clicking connect`
    );
  } catch (clickErr) {
    const message = String(clickErr?.message || clickErr);
    if (!/timed out|timeout/i.test(message)) {
      throw clickErr;
    }
    const clickedViaDom = await page.evaluate(() => {
      const btn = document.getElementById("connectButton");
      if (!btn) return false;
      btn.click();
      return true;
    });
    if (!clickedViaDom) {
      throw clickErr;
    }
  }
  return waitForConnectedState(page, args.connectTimeoutMs, args, clientId);
}

function calculateMaxTotalMs(args) {
  if (Number.isFinite(args.maxTotalMs) && args.maxTotalMs > 0) {
    return args.maxTotalMs;
  }
  const effectiveConcurrency = Math.max(1, Math.min(args.clients, Math.floor(args.connectConcurrency)));
  const connectWaves = Math.ceil(args.clients / effectiveConcurrency);
  const perWaveBudgetMs =
    args.navTimeoutMs +
    args.connectTimeoutMs +
    args.clickTimeoutMs +
    args.stateReadTimeoutMs;
  return (
    connectWaves * perWaveBudgetMs * 2 +
    args.clients * Math.max(0, args.spawnDelayMs) +
    args.durationMs +
    30000
  );
}

function resolveLaunchOptions(args) {
  const launchOptions = { headless: args.headless };
  const configuredExecutablePath = process.env.MGS_PLAYWRIGHT_EXECUTABLE_PATH;
  if (configuredExecutablePath) {
    launchOptions.executablePath = configuredExecutablePath;
    return launchOptions;
  }

  const defaultExecutablePath = chromium.executablePath();
  if (!defaultExecutablePath || fs.existsSync(defaultExecutablePath)) {
    return launchOptions;
  }

  // Playwright can occasionally resolve x64 cache paths on Apple Silicon hosts.
  if (process.platform === "darwin" && defaultExecutablePath.includes("-x64/")) {
    const arm64ExecutablePath = defaultExecutablePath.replace("-x64/", "-arm64/");
    if (arm64ExecutablePath !== defaultExecutablePath && fs.existsSync(arm64ExecutablePath)) {
      launchOptions.executablePath = arm64ExecutablePath;
      console.warn(
        `[multi] default chromium path missing; falling back to arm64 binary: ${arm64ExecutablePath}`
      );
    }
  }

  return launchOptions;
}

function shouldReplaceJoinTimingSummary(currentSummary, candidateSummary) {
  if (!candidateSummary || typeof candidateSummary !== "object") return false;
  if (!currentSummary || typeof currentSummary !== "object") return true;

  const currentFirstRender = Number(currentSummary.firstRenderMs);
  const candidateFirstRender = Number(candidateSummary.firstRenderMs);
  if (!Number.isFinite(currentFirstRender) && Number.isFinite(candidateFirstRender)) {
    return true;
  }

  const currentFirstState = Number(currentSummary.firstStateMs);
  const candidateFirstState = Number(candidateSummary.firstStateMs);
  if (!Number.isFinite(currentFirstState) && Number.isFinite(candidateFirstState)) {
    return true;
  }

  const currentTotal = Number(currentSummary.totalMs);
  const candidateTotal = Number(candidateSummary.totalMs);
  if (!Number.isFinite(currentTotal) && Number.isFinite(candidateTotal)) {
    return true;
  }
  if (Number.isFinite(candidateTotal) && Number.isFinite(currentTotal) && candidateTotal >= currentTotal) {
    return true;
  }
  return false;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const startedAt = new Date();
  const effectiveConcurrency = Math.max(1, Math.min(args.clients, Math.floor(args.connectConcurrency)));
  const waveBatchSize = Number.isFinite(args.waveBatchSize)
    ? Math.max(0, Math.floor(args.waveBatchSize))
    : 0;
  const waveBatchDelayMs = Number.isFinite(args.waveBatchDelayMs)
    ? Math.max(0, Math.floor(args.waveBatchDelayMs))
    : 0;
  const maxTotalMs = calculateMaxTotalMs({ ...args, connectConcurrency: effectiveConcurrency });

  const browser = await chromium.launch(resolveLaunchOptions(args));
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  context.setDefaultTimeout(Math.max(args.connectTimeoutMs, args.clickTimeoutMs, 10000));

  const clients = [];
  const launchFailures = [];
  const connectLatencyEvents = [];
  let connectedAtLeastOnce = 0;
  const startedMs = Date.now();
  let timedOutDuringLaunch = false;
  let timedOutDuringSampling = false;

  if (args.joinStageUrl && args.resetJoinStages) {
    const resetUrl = args.joinStageUrl.endsWith("/reset")
      ? args.joinStageUrl
      : `${args.joinStageUrl.replace(/\/$/, "")}/reset`;
    try {
      await fetch(resetUrl, { method: "POST" });
      console.log(`[multi] reset join-stage metrics via ${resetUrl}`);
    } catch (err) {
      console.warn(`[multi] failed to reset join-stage metrics: ${String(err?.message || err)}`);
    }
  }

  console.log(
    `[multi] start clients=${args.clients} concurrency=${effectiveConcurrency} waveBatchSize=${waveBatchSize} waveBatchDelayMs=${waveBatchDelayMs} durationMs=${args.durationMs} timeoutMs=${maxTotalMs}`
  );

  try {
    const launchClient = async (index) => {
      const id = index + 1;
      if (Date.now() - startedMs > maxTotalMs) {
        timedOutDuringLaunch = true;
        return;
      }

      const page = await withTimeout(
        context.newPage(),
        args.navTimeoutMs + 1000,
        `client ${id} timed out creating page`
      );
      const client = {
        id,
        page,
        connectedAtLeastOnce: false,
        connectDurationMs: null,
        joinTimingSummary: null,
        lastState: null,
        stateSamples: 0,
        playerCountSamples: 0,
        playerCountTotal: 0,
      };
      clients[id - 1] = client;

      const clientStarted = Date.now();
      try {
        const initialState = await connectClient(page, args, id);
        const connectDurationMs = Date.now() - clientStarted;
        client.connectedAtLeastOnce = true;
        client.connectDurationMs = connectDurationMs;
        client.lastState = initialState;
        client.joinTimingSummary = initialState.joinTimingSummary || null;
        connectLatencyEvents.push({
          clientId: client.id,
          durationMs: connectDurationMs,
          statusKey: initialState.statusKey,
          joinTimingSummary: client.joinTimingSummary,
        });
        connectedAtLeastOnce += 1;
        console.log(
          `[multi] client ${id}/${args.clients} connected in ${connectDurationMs}ms (${initialState.statusKey || "unknown"})`
        );
      } catch (err) {
        const errorMsg = String(err.message || err);
        launchFailures.push({
          clientId: client.id,
          error: errorMsg,
          elapsedMs: Date.now() - clientStarted,
        });
        console.log(`[multi] client ${id}/${args.clients} failed: ${errorMsg}`);
      }
    };

    const inFlight = new Set();
    for (let i = 0; i < args.clients; i += 1) {
      if (timedOutDuringLaunch) {
        break;
      }
      const task = launchClient(i).finally(() => {
        inFlight.delete(task);
      });
      inFlight.add(task);

      if (inFlight.size >= effectiveConcurrency) {
        await Promise.race(inFlight);
      }
      if (args.spawnDelayMs > 0) {
        await sleep(args.spawnDelayMs);
      }
      if (waveBatchSize > 0 && (i + 1) < args.clients && (i + 1) % waveBatchSize === 0) {
        if (inFlight.size > 0) {
          await Promise.all(Array.from(inFlight));
        }
        if (waveBatchDelayMs > 0) {
          await sleep(waveBatchDelayMs);
        }
      }
    }
    if (inFlight.size > 0) {
      await Promise.all(Array.from(inFlight));
    }
    if (timedOutDuringLaunch) {
      console.warn(`[multi] launch window exceeded maxTotalMs=${maxTotalMs}; continuing with partial client set`);
    }

    const sampleWindowStart = Date.now();
    const sampleWindowEnd = sampleWindowStart + args.durationMs;
    // Enforce the global timeout during sampling only if launch already timed out.
    // Otherwise slow-but-successful launches can produce false sampling timeouts.
    const enforceGlobalTimeoutDuringSampling = timedOutDuringLaunch;
    const errorClientsObserved = new Set();

    while (Date.now() < sampleWindowEnd) {
      if (enforceGlobalTimeoutDuringSampling && Date.now() - startedMs > maxTotalMs) {
        timedOutDuringSampling = true;
        console.warn(
          `[multi] sampling window exceeded maxTotalMs=${maxTotalMs}; finishing with collected samples`
        );
        break;
      }
      for (const client of clients) {
        if (!client) continue;
        if (client.page.isClosed()) {
          errorClientsObserved.add(client.id);
          continue;
        }
        try {
          const state = await readClientState(client.page, args, client.id);
          client.lastState = state;
          if (shouldReplaceJoinTimingSummary(client.joinTimingSummary, state.joinTimingSummary)) {
            client.joinTimingSummary = state.joinTimingSummary;
          }
          client.stateSamples += 1;
          client.playerCountSamples += 1;
          client.playerCountTotal += state.playerCount;
          if (isHealthyStatus(state.statusKey)) {
            client.connectedAtLeastOnce = true;
          } else if (state.statusKey === "error") {
            errorClientsObserved.add(client.id);
          }
        } catch (_) {
          errorClientsObserved.add(client.id);
        }
      }

      const connectedNow = clients.filter((client) => client && client.lastState && isHealthyStatus(client.lastState.statusKey)).length;
      console.log(
        `[multi] sample connected=${connectedNow}/${args.clients} errors=${errorClientsObserved.size}`
      );
      await sleep(args.sampleIntervalMs);
    }

    const finalHealthyClients = clients.filter((client) => {
      return client.lastState && isHealthyStatus(client.lastState.statusKey);
    }).length;
    const connectedClients = clients.filter((client) => client.connectedAtLeastOnce).length;
    const connectedRatio = args.clients > 0 ? connectedClients / args.clients : 0;

    const averagePlayerCountPerClient = clients.map((client) => {
      if (client.playerCountSamples === 0) return 0;
      return client.playerCountTotal / client.playerCountSamples;
    });
    const avgPlayerCount =
      averagePlayerCountPerClient.length > 0
        ? averagePlayerCountPerClient.reduce((sum, value) => sum + value, 0) /
          averagePlayerCountPerClient.length
        : 0;

    const failures = [];
    if (connectedRatio < args.minConnectedRatio) {
      failures.push(
        `Connected ratio ${connectedRatio.toFixed(3)} < min ${args.minConnectedRatio.toFixed(3)}`
      );
    }
    if (errorClientsObserved.size > args.maxErrorClients) {
      failures.push(
        `Error clients ${errorClientsObserved.size} > max ${args.maxErrorClients}`
      );
    }
    if (timedOutDuringLaunch) {
      failures.push(`Timed out during launch after ${maxTotalMs}ms`);
    }
    if (timedOutDuringSampling) {
      failures.push(`Timed out during sampling after ${maxTotalMs}ms`);
    }
    const connectLatencyEventsWithJoinTiming = connectLatencyEvents.map((event) => {
      const client = clients[event.clientId - 1];
      return {
        ...event,
        joinTimingSummary: client?.joinTimingSummary || event.joinTimingSummary || null,
      };
    });
    const connectLatencyMs = summarizeConnectLatency(connectLatencyEventsWithJoinTiming);
    const connectLatencyByWave = summarizeConnectLatencyByWave(
      connectLatencyEventsWithJoinTiming,
      args.clients
    );
    const joinTimingMs = summarizeJoinTiming(connectLatencyEventsWithJoinTiming);
    const joinTimingByWave = summarizeJoinTimingByWave(
      connectLatencyEventsWithJoinTiming,
      args.clients
    );

    const result = {
      url: args.url,
      wsUrl: args.wsUrl,
      clientsRequested: args.clients,
      clientsLaunched: clients.length,
      clientsConnectedAtLeastOnce: connectedClients,
      clientsHealthyFinal: finalHealthyClients,
      connectedRatio: Number(connectedRatio.toFixed(4)),
      averagePlayerCountPerClient: Number(avgPlayerCount.toFixed(2)),
      connectLatencyMs,
      connectLatencyByWave,
      joinTimingMs,
      joinTimingByWave,
      launchFailures,
      errorClientIds: Array.from(errorClientsObserved).sort((a, b) => a - b),
      thresholds: {
        minConnectedRatio: args.minConnectedRatio,
        maxErrorClients: args.maxErrorClients,
      },
      launchPolicy: {
        connectConcurrency: effectiveConcurrency,
        spawnDelayMs: args.spawnDelayMs,
        waveBatchSize,
        waveBatchDelayMs,
      },
      timedOutDuringLaunch,
      timedOutDuringSampling,
      passed: failures.length === 0,
      failures,
      startedAt: startedAt.toISOString(),
      finishedAt: new Date().toISOString(),
      durationMs: Date.now() - startedMs,
    };

    if (args.joinStageUrl) {
      try {
        const response = await fetch(args.joinStageUrl);
        if (response.ok) {
          result.serverJoinStages = await response.json();
        } else {
          result.serverJoinStagesError = `HTTP ${response.status}`;
        }
      } catch (err) {
        result.serverJoinStagesError = String(err?.message || err);
      }
    }

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
