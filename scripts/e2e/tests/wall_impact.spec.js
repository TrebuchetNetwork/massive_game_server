const { test, expect } = require('@playwright/test');
const { spawn } = require('child_process');
const path = require('path');
const http = require('http');
const https = require('https');

let serverProcess;

function resolveBaseUrl() {
  return process.env.E2E_BASE_URL || 'http://127.0.0.1:8080';
}

function resolveWsUrl() {
  if (process.env.E2E_WS_URL) {
    return process.env.E2E_WS_URL;
  }
  const base = new URL(resolveBaseUrl());
  const wsProtocol = base.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${wsProtocol}//${base.host}/ws`;
}

function waitForHttpReady(url) {
  const timeoutMs = 180000;
  const start = Date.now();
  const client = url.startsWith('https') ? https : http;

  return new Promise((resolve, reject) => {
    const attempt = () => {
      const req = client.get(url, (res) => {
        if (res.statusCode === 200) {
          res.resume();
          resolve();
          return;
        }
        res.resume();
        retry();
      });
      req.on('error', retry);
    };

    const retry = () => {
      if (Date.now() - start > timeoutMs) {
        reject(new Error(`Timed out waiting for ${url}`));
        return;
      }
      setTimeout(attempt, 500);
    };

    attempt();
  });
}

async function startServer() {
  if (process.env.E2E_SERVER_SKIP === '1') return;

  const cwd = path.resolve(__dirname, '..', '..', '..');
  const cmd = process.env.E2E_SERVER_CMD || 'cargo';
  const args = process.env.E2E_SERVER_CMD
    ? process.env.E2E_SERVER_CMD.split(' ').slice(1)
    : ['run', '-p', 'massive_game_server_core', '--bin', 'massive_game_server_core'];

  const base = new URL(resolveBaseUrl());
  const childEnv = { ...process.env };
  if (!childEnv.MGS_PORT) {
    childEnv.MGS_PORT = base.port || (base.protocol === 'https:' ? '443' : '80');
  }
  if (!childEnv.MGS_HOST) {
    childEnv.MGS_HOST = '0.0.0.0';
  }
  if (!childEnv.MGS_TARGET_BOT_COUNT) {
    childEnv.MGS_TARGET_BOT_COUNT = '0';
  }

  serverProcess = spawn(cmd, args, {
    cwd,
    env: childEnv,
    stdio: ['ignore', 'pipe', 'pipe']
  });

  serverProcess.stdout.on('data', (data) => process.stdout.write(data.toString()));
  serverProcess.stderr.on('data', (data) => process.stderr.write(data.toString()));

  const baseUrl = resolveBaseUrl();
  const exitPromise = new Promise((_, reject) => {
    serverProcess.on('exit', (code) => reject(new Error(`Server exited early with code ${code}`)));
  });

  await Promise.race([waitForHttpReady(`${baseUrl}/client.html`), exitPromise]);
}

async function stopServer() {
  if (!serverProcess) return;
  serverProcess.kill('SIGINT');
}

test.beforeAll(async () => {
  await startServer();
});

test.afterAll(async () => {
  await stopServer();
});

test('shots generate wall impact events', async ({ page }) => {
  const pageErrors = [];
  page.on('pageerror', (err) => pageErrors.push(err.message || String(err)));

  await page.goto('/client.html', { waitUntil: 'domcontentloaded' });
  await page.waitForSelector('#connectButton', { state: 'attached' });

  const wsInput = page.locator('#wsUrl');
  if (await wsInput.count()) {
    await wsInput.fill(resolveWsUrl());
  }
  await page.click('#connectButton', { force: true });

  await page.waitForFunction(
    () => window.__e2e && window.__e2e.connectionStatus && window.__e2e.connectionStatus.statusKey === 'playing',
    null,
    { timeout: 60000 }
  );
  await page.waitForFunction(() => window.__e2e && window.__e2e.hasLocalPlayer === true, null, { timeout: 60000 });

  await page.evaluate(() => {
    if (!window.__e2e) window.__e2e = {};
    window.__e2e.wallImpactEventCount = 0;
    const fx = window.effectsManager;
    if (!fx || fx.__e2eWallImpactWrapped) return;
    const originalProcessGameEvent = fx.processGameEvent.bind(fx);
    fx.processGameEvent = function wrappedProcessGameEvent(event) {
      const eventType = Number(event && event.event_type);
      if (eventType === 4 || eventType === 0) {
        window.__e2e.wallImpactEventCount = (window.__e2e.wallImpactEventCount || 0) + 1;
      }
      return originalProcessGameEvent(event);
    };
    fx.__e2eWallImpactWrapped = true;
  });

  const canvas = page.locator('#pixiContainer canvas');
  await canvas.waitFor({ state: 'visible', timeout: 60000 });

  const aimPoint = await page.evaluate(() => {
    const app = window.app;
    const stage = app && app.stage;
    const gameScene = stage && stage.children && stage.children[0];
    const canvas = document.querySelector('#pixiContainer canvas');
    if (!app || !stage || !gameScene || !canvas || !window.PIXI) return null;

    const stack = [stage];
    let localSprite = null;
    while (stack.length) {
      const node = stack.pop();
      if (!node) continue;
      if (node.localIndicator && node.playerId) {
        localSprite = node;
        break;
      }
      const children = node.children || [];
      for (let i = children.length - 1; i >= 0; i -= 1) {
        stack.push(children[i]);
      }
    }
    if (!localSprite) return null;

    const x = Number(localSprite.x) || 0;
    const y = Number(localSprite.y) || 0;
    const worldMinX = -800;
    const worldMaxX = 800;
    const worldMinY = -600;
    const worldMaxY = 600;

    const borderTargets = [
      { distance: Math.abs(worldMinX - x), x: worldMinX + 4, y },
      { distance: Math.abs(worldMaxX - x), x: worldMaxX - 4, y },
      { distance: Math.abs(worldMinY - y), x, y: worldMinY + 4 },
      { distance: Math.abs(worldMaxY - y), x, y: worldMaxY - 4 },
    ];
    borderTargets.sort((a, b) => a.distance - b.distance);
    const target = borderTargets[0];

    const globalPoint = gameScene.toGlobal(new window.PIXI.Point(target.x, target.y));
    const rect = canvas.getBoundingClientRect();
    return {
      clientX: rect.left + globalPoint.x,
      clientY: rect.top + globalPoint.y,
    };
  });
  expect(aimPoint).toBeTruthy();

  const ammoBefore = await page
    .locator('#playerAmmo')
    .innerText()
    .then((value) => Number.parseInt(value, 10) || 0);

  await canvas.dispatchEvent('mousemove', { clientX: aimPoint.clientX, clientY: aimPoint.clientY });
  await canvas.dispatchEvent('mousedown', { button: 0, clientX: aimPoint.clientX, clientY: aimPoint.clientY });
  await page.waitForTimeout(2600);
  await canvas.dispatchEvent('mouseup', { button: 0, clientX: aimPoint.clientX, clientY: aimPoint.clientY });

  const ammoAfter = await page
    .locator('#playerAmmo')
    .innerText()
    .then((value) => Number.parseInt(value, 10) || 0);
  expect(ammoAfter).toBeLessThan(ammoBefore);

  await page.waitForFunction(
    () => (window.__e2e && Number(window.__e2e.wallImpactEventCount)) > 0,
    null,
    { timeout: 10000 }
  );

  const wallImpactEventCount = await page.evaluate(() => Number(window.__e2e?.wallImpactEventCount) || 0);
  expect(wallImpactEventCount).toBeGreaterThan(0);
  expect(pageErrors).toEqual([]);
});
