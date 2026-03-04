const fs = require("fs");
const path = require("path");
const { test, expect } = require("@playwright/test");
const { registerServerLifecycle, resolveWsUrl } = require("./helpers/serverLifecycle");

const KEY_MODULE_FRAGMENTS = [
  "/client_logic/UIManager.js",
  "/client_logic/InputManager.js",
  "/client_logic/ConnectionManager.js",
  "/client_logic/WorldRenderer.js",
];

function mergeCoveredRanges(ranges) {
  if (!ranges.length) return [];
  ranges.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const merged = [ranges[0]];
  for (let i = 1; i < ranges.length; i += 1) {
    const [start, end] = ranges[i];
    const last = merged[merged.length - 1];
    if (start <= last[1]) {
      last[1] = Math.max(last[1], end);
    } else {
      merged.push([start, end]);
    }
  }
  return merged;
}

function computeEntryCoverage(entry) {
  const textLength = entry?.text ? entry.text.length : 0;
  let inferredTotalBytes = 0;
  const usedRanges = [];
  for (const fn of entry.functions || []) {
    for (const range of fn.ranges || []) {
      inferredTotalBytes = Math.max(inferredTotalBytes, range.endOffset || 0);
      if (range.count > 0) {
        usedRanges.push([range.startOffset, range.endOffset]);
      }
    }
  }
  const totalBytes = Math.max(textLength, inferredTotalBytes);
  if (!totalBytes) return { usedBytes: 0, totalBytes: 0, usedPct: 0 };

  const merged = mergeCoveredRanges(usedRanges);
  const usedBytes = merged.reduce((sum, [start, end]) => sum + Math.max(0, end - start), 0);
  const usedPct = (usedBytes / totalBytes) * 100;
  return { usedBytes, totalBytes, usedPct };
}

async function connectClient(page) {
  await page.goto("/client.html", { waitUntil: "domcontentloaded" });
  await page.waitForSelector("#connectButton", { state: "attached" });
  const wsInput = page.locator("#wsUrl");
  if (await wsInput.count()) {
    await wsInput.fill(resolveWsUrl());
  }
  await page.click("#connectButton", { force: true });
  await page.waitForFunction(() => window.__e2e && window.__e2e.matchInfoReady === true, null, {
    timeout: 120000,
  });
  await page.waitForFunction(() => window.__e2e && window.__e2e.lastStateUpdate > 0, null, {
    timeout: 120000,
  });
  await page.waitForFunction(
    () =>
      window.__e2e &&
      window.__e2e.connectionStatus &&
      ["playing", "waiting", "respawn"].includes(window.__e2e.connectionStatus.statusKey),
    null,
    { timeout: 120000 }
  );
}

registerServerLifecycle(test);

test.describe.configure({ timeout: 420000, retries: 1 });

test("UI coverage and visual states stay healthy", async ({ page }, testInfo) => {
  const pageErrors = [];
  page.on("pageerror", (err) => pageErrors.push(err.message || String(err)));

  await page.coverage.startJSCoverage({ reportAnonymousScripts: false, resetOnNavigation: false });

  await page.goto("/client.html", { waitUntil: "domcontentloaded" });
  await page.waitForSelector("#connectButton", { state: "visible" });

  const menuShot = testInfo.outputPath("menu-idle.png");
  await page.screenshot({ path: menuShot, fullPage: true });

  await page.click("#settingsButton");
  await page.waitForFunction(() => {
    const settings = document.getElementById("settingsMenu");
    return settings && !settings.classList.contains("hidden");
  });
  const settingsShot = testInfo.outputPath("menu-settings.png");
  await page.screenshot({ path: settingsShot, fullPage: true });
  await page.click("#cancelSettingsButton");

  await page.click("#minimizeHudButton");
  await page.waitForFunction(() => {
    const panel = document.getElementById("controlsPanel");
    const toggle = document.getElementById("hudMenuToggle");
    return panel && panel.classList.contains("is-hidden") && toggle && !toggle.classList.contains("hidden");
  });
  await page.click("#hudMenuToggle");
  await page.waitForFunction(() => {
    const panel = document.getElementById("controlsPanel");
    return panel && !panel.classList.contains("is-hidden");
  });

  const wsInput = page.locator("#wsUrl");
  if (await wsInput.count()) {
    await wsInput.fill(resolveWsUrl());
  }
  await page.click("#connectButton", { force: true });

  await page.waitForFunction(() => window.__e2e && window.__e2e.matchInfoReady === true, null, {
    timeout: 120000,
  });
  await page.waitForFunction(() => window.__e2e && window.__e2e.lastStateUpdate > 0, null, {
    timeout: 120000,
  });
  await page.waitForFunction(() => window.__e2e && window.__e2e.hasLocalPlayer === true, null, {
    timeout: 120000,
  });

  const hudShot = testInfo.outputPath("connected-hud.png");
  await page.screenshot({ path: hudShot, fullPage: true });

  await page.keyboard.down("Tab");
  await page.waitForFunction(() => {
    const scoreboard = document.getElementById("scoreboard");
    return scoreboard && !scoreboard.classList.contains("hidden");
  });
  const scoreboardShot = testInfo.outputPath("connected-scoreboard.png");
  await page.screenshot({ path: scoreboardShot, fullPage: true });
  await page.keyboard.up("Tab");

  const entries = await page.coverage.stopJSCoverage();

  const keyMetrics = {};
  let totalUsed = 0;
  let totalBytes = 0;
  for (const fragment of KEY_MODULE_FRAGMENTS) {
    const matching = entries.filter((entry) => (entry.url || "").includes(fragment));
    if (!matching.length) continue;
    const combined = matching.reduce(
      (acc, entry) => {
        const metrics = computeEntryCoverage(entry);
        acc.usedBytes += metrics.usedBytes;
        acc.totalBytes += metrics.totalBytes;
        return acc;
      },
      { usedBytes: 0, totalBytes: 0 }
    );
    combined.usedPct =
      combined.totalBytes > 0 ? (combined.usedBytes / combined.totalBytes) * 100 : 0;
    keyMetrics[fragment] = combined;
    totalUsed += combined.usedBytes;
    totalBytes += combined.totalBytes;
  }

  const aggregatePct = totalBytes > 0 ? (totalUsed / totalBytes) * 100 : 0;
  console.log(`UI key-module aggregate coverage: ${aggregatePct.toFixed(2)}%`);
  for (const [name, metrics] of Object.entries(keyMetrics)) {
    console.log(
      `${name}: ${metrics.usedPct.toFixed(2)}% (${metrics.usedBytes}/${metrics.totalBytes} bytes)`
    );
  }

  expect(Object.keys(keyMetrics).length).toBeGreaterThanOrEqual(3);
  expect(aggregatePct).toBeGreaterThan(8);

  for (const fragment of ["/client_logic/UIManager.js", "/client_logic/InputManager.js"]) {
    if (keyMetrics[fragment]) {
      expect(keyMetrics[fragment].usedPct).toBeGreaterThan(3);
    }
  }

  [menuShot, settingsShot, hudShot, scoreboardShot].forEach((filePath) => {
    expect(fs.existsSync(filePath)).toBe(true);
    const stat = fs.statSync(filePath);
    expect(stat.size).toBeGreaterThan(10_000);
    testInfo.attach(path.basename(filePath), { path: filePath, contentType: "image/png" });
  });

  expect(pageErrors).toEqual([]);
});
