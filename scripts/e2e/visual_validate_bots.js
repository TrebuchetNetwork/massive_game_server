#!/usr/bin/env node

const fs = require("fs");
const path = require("path");
const { chromium } = require(path.resolve(__dirname, "node_modules", "playwright"));

function parseArgs(argv) {
  const args = {
    url: "http://127.0.0.1:18080/client.html",
    ws: "ws://127.0.0.1:18080/ws",
    outDir: path.resolve(process.cwd(), "artifacts", "ui_validation"),
    waitTimeoutMs: 45000,
    trackDurationMs: 18000,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    const next = argv[i + 1];
    if (token === "--url" && next) {
      args.url = next;
      i += 1;
    } else if (token === "--ws" && next) {
      args.ws = next;
      i += 1;
    } else if (token === "--out" && next) {
      args.outDir = path.resolve(next);
      i += 1;
    } else if (token === "--wait-timeout-ms" && next) {
      args.waitTimeoutMs = Number.parseInt(next, 10);
      i += 1;
    } else if (token === "--track-duration-ms" && next) {
      args.trackDurationMs = Number.parseInt(next, 10);
      i += 1;
    } else if (token === "--help") {
      console.log(`
Usage: node scripts/e2e/visual_validate_bots.js [options]
  --url <http-url>               Client URL (default: http://127.0.0.1:18080/client.html)
  --ws <ws-url>                  WebSocket URL (default: ws://127.0.0.1:18080/ws)
  --out <dir>                    Output directory (default: artifacts/ui_validation)
  --wait-timeout-ms <ms>         Connect timeout (default: 45000)
  --track-duration-ms <ms>       Bot tracking window (default: 18000)
`);
      process.exit(0);
    }
  }

  return args;
}

function nowSlug() {
  const d = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  return (
    `${d.getUTCFullYear()}${pad(d.getUTCMonth() + 1)}${pad(d.getUTCDate())}` +
    `_${pad(d.getUTCHours())}${pad(d.getUTCMinutes())}${pad(d.getUTCSeconds())}`
  );
}

async function waitForConnectedState(page, timeoutMs) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const state = await page.evaluate(() => ({
      statusKey: window.__e2e?.connectionStatus?.statusKey ?? null,
      detailText: window.__e2e?.connectionStatus?.detailText ?? "",
      playerCount: Number.parseInt(document.getElementById("playerCount")?.textContent ?? "0", 10) || 0,
      matchInfoReady: Boolean(window.__e2e?.matchInfoReady),
      renderFrames: Number(window.__e2e?.renderFrames ?? 0),
    }));
    if ((state.statusKey === "waiting" || state.statusKey === "playing" || state.statusKey === "respawn") && state.playerCount > 1) {
      return state;
    }
    await page.waitForTimeout(250);
  }
  throw new Error(`Timed out waiting for connected game state after ${timeoutMs}ms`);
}

async function readUiValidation(page) {
  return page.evaluate(() => {
    const checkVisible = (id) => {
      const el = document.getElementById(id);
      if (!el) return false;
      const style = window.getComputedStyle(el);
      const rect = el.getBoundingClientRect();
      const hiddenByClass = el.classList.contains("hidden");
      return !hiddenByClass && style.display !== "none" && style.visibility !== "hidden" && rect.width > 0 && rect.height > 0;
    };
    const exists = (id) => Boolean(document.getElementById(id));
    return {
      connectionStatusVisible: checkVisible("connectionStatus"),
      playerCountVisible: checkVisible("playerCount"),
      matchInfoVisible: checkVisible("matchInfo"),
      minimapVisible: checkVisible("minimapContainer"),
      killFeedVisible: checkVisible("killFeed"),
      chatVisible: checkVisible("chatDisplay"),
      controlsPanelExists: exists("controlsPanel"),
      canvasCount: document.querySelectorAll("#pixiContainer canvas").length,
    };
  });
}

async function showScoreboardAndFindBot(page) {
  await page.evaluate(() => window.toggleScoreboard?.(true));
  await page.waitForTimeout(500);
  return page.evaluate(() => {
    const rows = Array.from(document.querySelectorAll("#scoreboard table tbody tr"));
    const parsed = rows.map((row) => Array.from(row.querySelectorAll("td")).map((td) => (td.textContent || "").trim()));

    // FFA rows: rank, username, score, kills, deaths
    // Team rows: username, score, kills, deaths
    const normalized = parsed.map((cells) => {
      if (cells.length >= 5) {
        return {
          username: cells[1],
          score: Number.parseInt(cells[2], 10) || 0,
          kills: Number.parseInt(cells[3], 10) || 0,
          deaths: Number.parseInt(cells[4], 10) || 0,
        };
      }
      if (cells.length >= 4) {
        return {
          username: cells[0],
          score: Number.parseInt(cells[1], 10) || 0,
          kills: Number.parseInt(cells[2], 10) || 0,
          deaths: Number.parseInt(cells[3], 10) || 0,
        };
      }
      return null;
    }).filter(Boolean);

    const bot = normalized.find((p) => typeof p.username === "string" && p.username.startsWith("Bot "));
    return { rows: normalized, bot: bot || null };
  });
}

async function readBotStats(page, botName) {
  return page.evaluate((name) => {
    const rows = Array.from(document.querySelectorAll("#scoreboard table tbody tr"));
    const parsed = rows
      .map((row) => Array.from(row.querySelectorAll("td")).map((td) => (td.textContent || "").trim()))
      .map((cells) => {
        if (cells.length >= 5) {
          return { username: cells[1], score: cells[2], kills: cells[3], deaths: cells[4] };
        }
        if (cells.length >= 4) {
          return { username: cells[0], score: cells[1], kills: cells[2], deaths: cells[3] };
        }
        return null;
      })
      .filter(Boolean);
    const match = parsed.find((p) => p.username === name);
    if (!match) return null;
    return {
      username: match.username,
      score: Number.parseInt(match.score, 10) || 0,
      kills: Number.parseInt(match.kills, 10) || 0,
      deaths: Number.parseInt(match.deaths, 10) || 0,
      capturedAt: new Date().toISOString(),
    };
  }, botName);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const runId = nowSlug();
  const outDir = path.resolve(args.outDir, runId);
  fs.mkdirSync(outDir, { recursive: true });

  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  const page = await context.newPage();
  page.setDefaultTimeout(30000);

  const report = {
    runId,
    startedAt: new Date().toISOString(),
    url: args.url,
    ws: args.ws,
    screenshots: {},
    uiValidation: null,
    initialState: null,
    botTracking: {
      selectedBot: null,
      samples: [],
      changed: false,
    },
    passed: false,
    failures: [],
  };

  try {
    await page.goto(args.url, { waitUntil: "domcontentloaded", timeout: 45000 });
    await page.evaluate((wsUrl) => {
      const wsInput = document.getElementById("wsUrl");
      if (wsInput) wsInput.value = wsUrl;
    }, args.ws);

    await page.locator("#connectButton").click({ timeout: 15000 });
    report.initialState = await waitForConnectedState(page, args.waitTimeoutMs);

    const shotConnected = path.join(outDir, "01_connected.png");
    await page.screenshot({ path: shotConnected, fullPage: false });
    report.screenshots.connected = shotConnected;

    report.uiValidation = await readUiValidation(page);
    const uiChecks = report.uiValidation;
    if (!uiChecks.connectionStatusVisible) report.failures.push("connectionStatus not visible");
    if (!uiChecks.playerCountVisible) report.failures.push("playerCount not visible");
    if (!uiChecks.matchInfoVisible) report.failures.push("matchInfo not visible");
    if (!uiChecks.minimapVisible) report.failures.push("minimapContainer not visible");
    if (!uiChecks.killFeedVisible) report.failures.push("killFeed not visible");
    if (!uiChecks.chatVisible) report.failures.push("chatDisplay not visible");
    if (uiChecks.canvasCount < 1) report.failures.push("PIXI canvas not mounted");

    const scoreData = await showScoreboardAndFindBot(page);
    if (!scoreData.bot) {
      report.failures.push("No bot row found in scoreboard");
    } else {
      report.botTracking.selectedBot = scoreData.bot.username;
    }

    const shotScoreboard = path.join(outDir, "02_scoreboard_bots.png");
    await page.screenshot({ path: shotScoreboard, fullPage: false });
    report.screenshots.scoreboard = shotScoreboard;

    if (report.botTracking.selectedBot) {
      const botName = report.botTracking.selectedBot;

      // Enter overview to keep bot activity visible.
      await page.keyboard.down("Tab");
      await page.waitForTimeout(900);
      const shotOverview = path.join(outDir, "03_overview_track_start.png");
      await page.screenshot({ path: shotOverview, fullPage: false });
      report.screenshots.overviewStart = shotOverview;
      await page.keyboard.up("Tab");

      const t0 = Date.now();
      while (Date.now() - t0 < args.trackDurationMs) {
        await page.evaluate(() => window.toggleScoreboard?.(true));
        const sample = await readBotStats(page, botName);
        if (sample) {
          report.botTracking.samples.push(sample);
        }
        await page.waitForTimeout(3000);
      }

      const shotEnd = path.join(outDir, "04_overview_track_end.png");
      await page.keyboard.down("Tab");
      await page.waitForTimeout(800);
      await page.screenshot({ path: shotEnd, fullPage: false });
      await page.keyboard.up("Tab");
      report.screenshots.overviewEnd = shotEnd;

      if (report.botTracking.samples.length >= 2) {
        const first = report.botTracking.samples[0];
        const last = report.botTracking.samples[report.botTracking.samples.length - 1];
        report.botTracking.changed =
          first.score !== last.score || first.kills !== last.kills || first.deaths !== last.deaths;
      }
    }

    report.passed = report.failures.length === 0;
  } catch (err) {
    report.failures.push(String(err && err.stack ? err.stack : err));
    report.passed = false;
  } finally {
    report.finishedAt = new Date().toISOString();
    const reportPath = path.join(outDir, "report.json");
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
    await context.close();
    await browser.close();

    console.log(JSON.stringify({ outDir, reportPath, passed: report.passed, failures: report.failures }, null, 2));
    process.exit(report.passed ? 0 : 1);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});

