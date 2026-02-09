#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const { chromium } = require('@playwright/test');

const baseUrl = process.env.E2E_BASE_URL || 'http://127.0.0.1:8080';
const wsUrl = process.env.E2E_WS_URL || `${baseUrl.replace(/^http/, 'ws')}/ws`;
const runId = new Date().toISOString().replace(/[:.]/g, '-');
const outputDir = path.resolve(__dirname, '..', '..', 'artifacts', 'ui_audit', runId);

function ensureDir(dirPath) {
  fs.mkdirSync(dirPath, { recursive: true });
}

async function captureScenario(page, scenarioId, actionFn) {
  const result = {
    id: scenarioId,
    screenshot: `${scenarioId}.png`,
    metrics: {},
    checks: [],
    issues: []
  };

  await actionFn(page, result);
  await page.screenshot({ path: path.join(outputDir, result.screenshot), fullPage: true });
  return result;
}

async function collectCoreMetrics(page) {
  return page.evaluate(() => {
    const panel = document.getElementById('controlsPanel');
    const connectBtn = document.getElementById('connectButton');
    const title = panel ? panel.querySelector('h1') : null;
    const settingsMenu = document.getElementById('settingsMenu');
    const scoreboard = document.getElementById('scoreboard');
    const mobileControls = document.getElementById('mobileControls');
    const hudMenuToggle = document.getElementById('hudMenuToggle');
    const statusTitle = document.getElementById('connectionStatusTitle');
    const statusDetail = document.getElementById('connectionStatusDetail');
    const panelStyle = panel ? getComputedStyle(panel) : null;
    const btnStyle = connectBtn ? getComputedStyle(connectBtn) : null;
    const titleStyle = title ? getComputedStyle(title) : null;
    const rect = (el) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { x: r.x, y: r.y, width: r.width, height: r.height };
    };

    return {
      panelRect: rect(panel),
      connectRect: rect(connectBtn),
      titleRect: rect(title),
      panelVisible: !!panel && !panel.classList.contains('is-hidden'),
      settingsVisible: !!settingsMenu && !settingsMenu.classList.contains('hidden'),
      scoreboardVisible: !!scoreboard && !scoreboard.classList.contains('hidden'),
      mobileControlsVisible: !!mobileControls && !mobileControls.classList.contains('hidden'),
      hudMenuToggleVisible: !!hudMenuToggle && !hudMenuToggle.classList.contains('hidden'),
      panelBg: panelStyle ? panelStyle.backgroundColor : null,
      connectBg: btnStyle ? btnStyle.backgroundColor : null,
      connectColor: btnStyle ? btnStyle.color : null,
      connectPadding: btnStyle ? btnStyle.padding : null,
      connectRadius: btnStyle ? btnStyle.borderRadius : null,
      connectWeight: btnStyle ? btnStyle.fontWeight : null,
      titleColor: titleStyle ? titleStyle.color : null,
      titleSize: titleStyle ? titleStyle.fontSize : null,
      titleWeight: titleStyle ? titleStyle.fontWeight : null,
      connectionTitle: statusTitle ? statusTitle.textContent : null,
      connectionDetail: statusDetail ? statusDetail.textContent : null,
      e2eState: window.__e2e || null
    };
  });
}

async function ensureControlsPanelVisible(page) {
  await page.waitForSelector('#controlsPanel', { state: 'attached' });
  const isHidden = await page.evaluate(() => {
    const panel = document.getElementById('controlsPanel');
    return !!panel && panel.classList.contains('is-hidden');
  });
  if (isHidden) {
    await page.click('#hudMenuToggle');
    await page.waitForFunction(() => {
      const panel = document.getElementById('controlsPanel');
      return !!panel && !panel.classList.contains('is-hidden');
    });
  }
}

function validateCoreMetrics(result, opts = {}) {
  const m = result.metrics;
  const checks = [];
  const issues = [];

  if (opts.expectPanelHidden === true) {
    checks.push({
      check: 'controls_panel_hidden',
      pass: m.panelVisible === false,
      value: m.panelVisible
    });
    checks.push({
      check: 'hud_menu_toggle_visible',
      pass: m.hudMenuToggleVisible === true,
      value: m.hudMenuToggleVisible
    });
  } else {
    checks.push({
      check: 'controls_panel_present',
      pass: !!m.panelRect && m.panelRect.width > 180 && m.panelRect.height > 120,
      value: m.panelRect
    });
    checks.push({
      check: 'connect_button_styled',
      pass: !!m.connectBg && m.connectBg !== 'rgb(239, 239, 239)' && (m.connectRect?.width || 0) > 140,
      value: { bg: m.connectBg, rect: m.connectRect }
    });
    checks.push({
      check: 'title_styled',
      pass: !!m.titleColor && m.titleColor !== 'rgb(0, 0, 0)' && m.titleWeight !== '400',
      value: { color: m.titleColor, weight: m.titleWeight, size: m.titleSize }
    });
  }

  if (opts.expectSettingsVisible === true) {
    checks.push({
      check: 'settings_visible',
      pass: m.settingsVisible === true,
      value: m.settingsVisible
    });
  }
  if (opts.expectSettingsVisible === false) {
    checks.push({
      check: 'settings_hidden',
      pass: m.settingsVisible === false,
      value: m.settingsVisible
    });
  }
  if (opts.expectScoreboardVisible === true) {
    checks.push({
      check: 'scoreboard_visible',
      pass: m.scoreboardVisible === true,
      value: m.scoreboardVisible
    });
  }
  if (opts.expectMobileVisible === true) {
    checks.push({
      check: 'mobile_controls_visible',
      pass: m.mobileControlsVisible === true,
      value: m.mobileControlsVisible
    });
  }
  if (opts.expectConnected === true) {
    checks.push({
      check: 'connected_has_local_player',
      pass: !!m.e2eState && !!m.e2eState.hasLocalPlayer,
      value: m.e2eState
    });
  }

  for (const c of checks) {
    if (!c.pass) {
      issues.push(`${c.check} failed`);
    }
  }
  result.checks = checks;
  result.issues = issues;
}

async function runAudit() {
  ensureDir(outputDir);
  const browser = await chromium.launch({ headless: true });
  const report = {
    runId,
    baseUrl,
    wsUrl,
    generatedAt: new Date().toISOString(),
    scenarios: [],
    summary: {
      totalScenarios: 0,
      totalChecks: 0,
      failedChecks: 0,
      pass: true
    }
  };

  try {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });

    report.scenarios.push(await captureScenario(page, 'menu_idle', async (p, result) => {
      await p.goto(`${baseUrl}/client.html`, { waitUntil: 'networkidle' });
      await ensureControlsPanelVisible(p);
      await p.waitForSelector('#connectButton', { state: 'visible' });
      result.metrics = await collectCoreMetrics(p);
      validateCoreMetrics(result, { expectSettingsVisible: false });
    }));

    report.scenarios.push(await captureScenario(page, 'menu_settings', async (p, result) => {
      await p.click('#settingsButton');
      await p.waitForFunction(() => {
        const menu = document.getElementById('settingsMenu');
        return !!menu && !menu.classList.contains('hidden');
      });
      result.metrics = await collectCoreMetrics(p);
      validateCoreMetrics(result, { expectSettingsVisible: true });
      await p.click('#cancelSettingsButton');
      await p.waitForFunction(() => {
        const menu = document.getElementById('settingsMenu');
        return !!menu && menu.classList.contains('hidden');
      });
    }));

    report.scenarios.push(await captureScenario(page, 'menu_focus_mode', async (p, result) => {
      await p.click('#focusModeButton');
      result.metrics = await collectCoreMetrics(p);
      validateCoreMetrics(result, { expectSettingsVisible: false });
      await p.click('#focusModeButton');
    }));

    report.scenarios.push(await captureScenario(page, 'menu_hud_hidden', async (p, result) => {
      await p.click('#minimizeHudButton');
      await p.waitForFunction(() => {
        const panel = document.getElementById('controlsPanel');
        const toggle = document.getElementById('hudMenuToggle');
        return !!panel && panel.classList.contains('is-hidden') && !!toggle && !toggle.classList.contains('hidden');
      });
      result.metrics = await collectCoreMetrics(p);
      validateCoreMetrics(result, { expectSettingsVisible: false, expectPanelHidden: true });
      await p.click('#hudMenuToggle');
      await p.waitForFunction(() => {
        const panel = document.getElementById('controlsPanel');
        return !!panel && !panel.classList.contains('is-hidden');
      });
    }));

    report.scenarios.push(await captureScenario(page, 'menu_mobile_idle', async (_p, result) => {
      const mobilePage = await browser.newPage({ viewport: { width: 390, height: 844 } });
      try {
        await mobilePage.goto(`${baseUrl}/client.html?mobile=1`, { waitUntil: 'networkidle' });
        await ensureControlsPanelVisible(mobilePage);
        await mobilePage.waitForSelector('#connectButton', { state: 'visible' });
        result.metrics = await collectCoreMetrics(mobilePage);
        validateCoreMetrics(result, { expectMobileVisible: true });
      } finally {
        await mobilePage.screenshot({ path: path.join(outputDir, result.screenshot), fullPage: true });
        await mobilePage.close();
      }
    }));

    report.scenarios.push(await captureScenario(page, 'connected_hud', async (p, result) => {
      await p.goto(`${baseUrl}/client.html`, { waitUntil: 'networkidle' });
      await ensureControlsPanelVisible(p);
      await p.fill('#wsUrl', wsUrl);
      await p.click('#connectButton');
      await p.waitForFunction(() => window.__e2e && window.__e2e.matchInfoReady === true, null, { timeout: 60000 });
      await p.waitForFunction(() => window.__e2e && window.__e2e.lastStateUpdate > 0, null, { timeout: 60000 });
      await ensureControlsPanelVisible(p);
      result.metrics = await collectCoreMetrics(p);
      validateCoreMetrics(result, { expectConnected: true });
    }));

    report.scenarios.push(await captureScenario(page, 'connected_scoreboard', async (p, result) => {
      await p.keyboard.down('Tab');
      await p.waitForFunction(() => {
        const s = document.getElementById('scoreboard');
        return !!s && !s.classList.contains('hidden');
      });
      result.metrics = await collectCoreMetrics(p);
      validateCoreMetrics(result, { expectConnected: true, expectScoreboardVisible: true });
      await p.keyboard.up('Tab');
    }));

    report.scenarios.push(await captureScenario(page, 'connected_settings', async (p, result) => {
      await ensureControlsPanelVisible(p);
      await p.click('#settingsButton');
      await p.waitForFunction(() => {
        const menu = document.getElementById('settingsMenu');
        return !!menu && !menu.classList.contains('hidden');
      });
      result.metrics = await collectCoreMetrics(p);
      validateCoreMetrics(result, { expectConnected: true, expectSettingsVisible: true });
      await p.click('#cancelSettingsButton');
      await p.waitForFunction(() => {
        const menu = document.getElementById('settingsMenu');
        return !!menu && menu.classList.contains('hidden');
      });
    }));
  } finally {
    await browser.close();
  }

  report.summary.totalScenarios = report.scenarios.length;
  report.summary.totalChecks = report.scenarios.reduce((acc, s) => acc + s.checks.length, 0);
  report.summary.failedChecks = report.scenarios.reduce((acc, s) => acc + s.issues.length, 0);
  report.summary.pass = report.summary.failedChecks === 0;

  const reportPath = path.join(outputDir, 'report.json');
  fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
  process.stdout.write(`${report.summary.pass ? 'PASS' : 'FAIL'} ${reportPath}\n`);
  if (!report.summary.pass) {
    process.exitCode = 1;
  }
}

runAudit().catch((err) => {
  process.stderr.write(`UI audit failed: ${err.stack || err.message}\n`);
  process.exit(1);
});
