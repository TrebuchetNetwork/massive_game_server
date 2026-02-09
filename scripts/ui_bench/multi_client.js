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
      return {
        statusKey,
        detailText,
        playerCount: Number.isFinite(playerCount) ? playerCount : 0,
        matchInfoReady: Boolean(window.__e2e?.matchInfoReady),
        renderFrames: Number(window.__e2e?.renderFrames ?? 0),
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

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const startedAt = new Date();
  const effectiveConcurrency = Math.max(1, Math.min(args.clients, Math.floor(args.connectConcurrency)));
  const maxTotalMs = calculateMaxTotalMs({ ...args, connectConcurrency: effectiveConcurrency });

  const browser = await chromium.launch({ headless: args.headless });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  context.setDefaultTimeout(Math.max(args.connectTimeoutMs, args.clickTimeoutMs, 10000));

  const clients = [];
  const launchFailures = [];
  let connectedAtLeastOnce = 0;
  const startedMs = Date.now();

  console.log(
    `[multi] start clients=${args.clients} concurrency=${effectiveConcurrency} durationMs=${args.durationMs} timeoutMs=${maxTotalMs}`
  );

  try {
    const launchClient = async (index) => {
      const id = index + 1;
      if (Date.now() - startedMs > maxTotalMs) {
        throw new Error(`benchmark timed out after ${maxTotalMs}ms during launch`);
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
        lastState: null,
        stateSamples: 0,
        playerCountSamples: 0,
        playerCountTotal: 0,
      };
      clients[id - 1] = client;

      const clientStarted = Date.now();
      try {
        const initialState = await connectClient(page, args, id);
        client.connectedAtLeastOnce = true;
        client.lastState = initialState;
        connectedAtLeastOnce += 1;
        console.log(
          `[multi] client ${id}/${args.clients} connected in ${Date.now() - clientStarted}ms (${initialState.statusKey || "unknown"})`
        );
      } catch (err) {
        const errorMsg = String(err.message || err);
        launchFailures.push({ clientId: client.id, error: errorMsg });
        console.log(`[multi] client ${id}/${args.clients} failed: ${errorMsg}`);
      }
    };

    const inFlight = new Set();
    for (let i = 0; i < args.clients; i += 1) {
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
    }
    if (inFlight.size > 0) {
      await Promise.all(Array.from(inFlight));
    }

    const sampleWindowStart = Date.now();
    const sampleWindowEnd = sampleWindowStart + args.durationMs;
    const errorClientsObserved = new Set();

    while (Date.now() < sampleWindowEnd) {
      if (Date.now() - startedMs > maxTotalMs) {
        throw new Error(`benchmark timed out after ${maxTotalMs}ms during sampling`);
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

    const result = {
      url: args.url,
      wsUrl: args.wsUrl,
      clientsRequested: args.clients,
      clientsLaunched: clients.length,
      clientsConnectedAtLeastOnce: connectedClients,
      clientsHealthyFinal: finalHealthyClients,
      connectedRatio: Number(connectedRatio.toFixed(4)),
      averagePlayerCountPerClient: Number(avgPlayerCount.toFixed(2)),
      launchFailures,
      errorClientIds: Array.from(errorClientsObserved).sort((a, b) => a - b),
      thresholds: {
        minConnectedRatio: args.minConnectedRatio,
        maxErrorClients: args.maxErrorClients,
      },
      passed: failures.length === 0,
      failures,
      startedAt: startedAt.toISOString(),
      finishedAt: new Date().toISOString(),
      durationMs: Date.now() - startedMs,
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
