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

test('firing produces visible projectile updates', async ({ page }) => {
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
    () => window.__e2e && window.__e2e.matchInfoReady === true,
    null,
    { timeout: 60000 }
  );
  await page.waitForFunction(
    () => window.__e2e && window.__e2e.hasLocalPlayer === true,
    null,
    { timeout: 60000 }
  );
  await page.waitForFunction(
    () =>
      window.__e2e &&
      window.__e2e.connectionStatus &&
      window.__e2e.connectionStatus.statusKey === 'playing',
    null,
    { timeout: 60000 }
  );

  const canvas = page.locator('#pixiContainer canvas');
  await canvas.waitFor({ state: 'visible', timeout: 60000 });
  const box = await canvas.boundingBox();
  expect(box).toBeTruthy();

  const ammoBefore = await page
    .locator('#playerAmmo')
    .innerText()
    .then((value) => Number.parseInt(value, 10) || 0);
  const aimX = box.x + box.width * 0.75;
  const aimY = box.y + box.height * 0.5;
  await canvas.dispatchEvent('mousemove', { clientX: aimX, clientY: aimY });
  await canvas.dispatchEvent('mousedown', { button: 0, clientX: aimX, clientY: aimY });
  await page.waitForTimeout(1800);
  await canvas.dispatchEvent('mouseup', { button: 0, clientX: aimX, clientY: aimY });

  const ammoAfter = await page
    .locator('#playerAmmo')
    .innerText()
    .then((value) => Number.parseInt(value, 10) || 0);

  const observation = await page.evaluate(async () => {
    const startedAt = performance.now();
    let maxProjectileCount = 0;
    let maxVisibleProjectileCount = 0;
    while (performance.now() - startedAt < 6000) {
      const e2e = window.__e2e || {};
      const projectileCount = Number(e2e.projectileCount) || 0;
      const visibleProjectileCount = Number(e2e.visibleProjectileCount) || 0;
      if (projectileCount > maxProjectileCount) maxProjectileCount = projectileCount;
      if (visibleProjectileCount > maxVisibleProjectileCount) {
        maxVisibleProjectileCount = visibleProjectileCount;
      }
      if (maxProjectileCount > 0 && maxVisibleProjectileCount > 0) break;
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    return { maxProjectileCount, maxVisibleProjectileCount };
  });

  expect(ammoAfter).toBeLessThan(ammoBefore);
  expect(observation.maxProjectileCount).toBeGreaterThan(0);
  expect(observation.maxVisibleProjectileCount).toBeGreaterThan(0);
  expect(pageErrors).toEqual([]);
});
