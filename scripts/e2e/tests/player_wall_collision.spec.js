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

test('local player cannot move through world boundary walls', async ({ page }) => {
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

  const canvas = page.locator('#pixiContainer canvas');
  await canvas.waitFor({ state: 'visible', timeout: 60000 });

  const setup = await page.evaluate(() => {
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
    const boundaries = [
      { axis: 'x', direction: -1, targetX: worldMinX + 4, targetY: y, distance: Math.abs(x - worldMinX) },
      { axis: 'x', direction: 1, targetX: worldMaxX - 4, targetY: y, distance: Math.abs(worldMaxX - x) },
      { axis: 'y', direction: -1, targetX: x, targetY: worldMinY + 4, distance: Math.abs(y - worldMinY) },
      { axis: 'y', direction: 1, targetX: x, targetY: worldMaxY - 4, distance: Math.abs(worldMaxY - y) },
    ];
    boundaries.sort((a, b) => a.distance - b.distance);
    const chosen = boundaries[0];

    const globalPoint = gameScene.toGlobal(new window.PIXI.Point(chosen.targetX, chosen.targetY));
    const rect = canvas.getBoundingClientRect();
    return {
      axis: chosen.axis,
      direction: chosen.direction,
      targetClientX: rect.left + globalPoint.x,
      targetClientY: rect.top + globalPoint.y,
      distance: chosen.distance,
    };
  });
  expect(setup).toBeTruthy();

  const moveDurationMs = Math.max(2800, Math.min(7000, Math.round((setup.distance / 150) * 1000 + 1400)));
  await canvas.dispatchEvent('mousemove', { clientX: setup.targetClientX, clientY: setup.targetClientY });

  const before = await page.evaluate(() => {
    const stage = window.app && window.app.stage;
    const walk = (node) => {
      if (!node) return null;
      if (node.localIndicator && node.playerId) return node;
      const children = node.children || [];
      for (let i = 0; i < children.length; i += 1) {
        const found = walk(children[i]);
        if (found) return found;
      }
      return null;
    };
    const local = walk(stage);
    return { x: Number(local?.x) || 0, y: Number(local?.y) || 0 };
  });

  await page.keyboard.down('w');
  await page.waitForTimeout(moveDurationMs);
  await page.keyboard.up('w');

  const after = await page.evaluate(() => {
    const stage = window.app && window.app.stage;
    const walk = (node) => {
      if (!node) return null;
      if (node.localIndicator && node.playerId) return node;
      const children = node.children || [];
      for (let i = 0; i < children.length; i += 1) {
        const found = walk(children[i]);
        if (found) return found;
      }
      return null;
    };
    const local = walk(stage);
    return { x: Number(local?.x) || 0, y: Number(local?.y) || 0 };
  });

  const worldMinX = -800;
  const worldMaxX = 800;
  const worldMinY = -600;
  const worldMaxY = 600;
  const boundaryEpsilon = 1.5;
  expect(after.x).toBeGreaterThanOrEqual(worldMinX - boundaryEpsilon);
  expect(after.x).toBeLessThanOrEqual(worldMaxX + boundaryEpsilon);
  expect(after.y).toBeGreaterThanOrEqual(worldMinY - boundaryEpsilon);
  expect(after.y).toBeLessThanOrEqual(worldMaxY + boundaryEpsilon);

  const displacement = Math.hypot(after.x - before.x, after.y - before.y);
  expect(displacement).toBeGreaterThan(20);
  expect(pageErrors).toEqual([]);
});
