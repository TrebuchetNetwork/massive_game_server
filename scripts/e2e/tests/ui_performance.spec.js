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

async function connectClient(page) {
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
    () => window.__e2e && window.__e2e.lastStateUpdate > 0,
    null,
    { timeout: 60000 }
  );
}

test.beforeAll(async () => {
  await startServer();
});

test.afterAll(async () => {
  await stopServer();
});

test.describe('UI Performance', () => {
  test('maintains acceptable FPS after connection', async ({ page }) => {
    await connectClient(page);

    // Warmup period
    await page.waitForTimeout(2000);

    // Measure FPS over 3 seconds using requestAnimationFrame
    const result = await page.evaluate(() => {
      return new Promise((resolve) => {
        let frames = 0;
        const start = performance.now();
        const target = 3000; // 3 seconds

        function tick() {
          frames++;
          if (performance.now() - start >= target) {
            const elapsed = (performance.now() - start) / 1000;
            resolve({ frames, elapsed, fps: frames / elapsed });
          } else {
            requestAnimationFrame(tick);
          }
        }
        requestAnimationFrame(tick);
      });
    });

    console.log(`FPS measurement: ${result.fps.toFixed(1)} FPS over ${result.elapsed.toFixed(1)}s (${result.frames} frames)`);

    // Client should maintain at least 30 FPS in headless mode
    expect(result.fps).toBeGreaterThan(30);
  });

  test('no excessive memory growth over time', async ({ page }) => {
    await connectClient(page);

    // Wait for initial state to settle
    await page.waitForTimeout(3000);

    // Take initial heap snapshot
    const heapStart = await page.evaluate(() => {
      if (performance.memory) {
        return performance.memory.usedJSHeapSize;
      }
      return null;
    });

    // Run for 10 seconds
    await page.waitForTimeout(10000);

    // Take final heap snapshot
    const heapEnd = await page.evaluate(() => {
      if (performance.memory) {
        return performance.memory.usedJSHeapSize;
      }
      return null;
    });

    if (heapStart !== null && heapEnd !== null) {
      const growthMB = (heapEnd - heapStart) / (1024 * 1024);
      console.log(`Heap: ${(heapStart / 1024 / 1024).toFixed(1)}MB -> ${(heapEnd / 1024 / 1024).toFixed(1)}MB (growth: ${growthMB.toFixed(1)}MB)`);

      // Heap should not grow more than 50MB in 10 seconds
      expect(growthMB).toBeLessThan(50);
    } else {
      console.log('performance.memory not available (non-Chromium), skipping heap check');
    }
  });

  test('render loop processes state updates without stalling', async ({ page }) => {
    await connectClient(page);
    await page.waitForTimeout(2000);

    // Check that state updates keep flowing
    const stateUpdates = await page.evaluate(() => {
      return new Promise((resolve) => {
        const updates = [];
        const start = Date.now();

        const check = setInterval(() => {
          updates.push({
            time: Date.now() - start,
            lastUpdate: window.__e2e?.lastStateUpdate || 0,
            renderFrames: window.__e2e?.renderFrames || 0
          });

          if (updates.length >= 10) {
            clearInterval(check);
            resolve(updates);
          }
        }, 500);
      });
    });

    // Verify state updates are flowing (not stalled)
    const lastUpdateValues = stateUpdates.map(u => u.lastUpdate);
    const uniqueUpdates = new Set(lastUpdateValues);
    console.log(`State updates over 5s: ${uniqueUpdates.size} unique values from ${lastUpdateValues.length} samples`);

    // Should have at least 3 different lastStateUpdate values over 5 seconds
    expect(uniqueUpdates.size).toBeGreaterThanOrEqual(3);

    // Render frames should increase monotonically
    for (let i = 1; i < stateUpdates.length; i++) {
      expect(stateUpdates[i].renderFrames).toBeGreaterThanOrEqual(stateUpdates[i - 1].renderFrames);
    }
  });

  test('client handles rapid input without frame drops', async ({ page }) => {
    await connectClient(page);
    await page.waitForTimeout(2000);

    // Get baseline FPS
    const baseline = await page.evaluate(() => {
      return new Promise((resolve) => {
        let frames = 0;
        const start = performance.now();
        function tick() {
          frames++;
          if (performance.now() - start >= 2000) {
            resolve({ fps: frames / ((performance.now() - start) / 1000) });
          } else {
            requestAnimationFrame(tick);
          }
        }
        requestAnimationFrame(tick);
      });
    });

    // Simulate rapid keyboard input (WASD + mouse movement)
    for (let i = 0; i < 30; i++) {
      await page.keyboard.down('KeyW');
      await page.mouse.move(640 + Math.sin(i * 0.5) * 200, 360 + Math.cos(i * 0.5) * 200);
      await page.waitForTimeout(33); // ~30Hz input rate
      await page.keyboard.up('KeyW');
      await page.keyboard.down('KeyD');
      await page.waitForTimeout(33);
      await page.keyboard.up('KeyD');
    }

    // Measure FPS after input burst
    const afterInput = await page.evaluate(() => {
      return new Promise((resolve) => {
        let frames = 0;
        const start = performance.now();
        function tick() {
          frames++;
          if (performance.now() - start >= 2000) {
            resolve({ fps: frames / ((performance.now() - start) / 1000) });
          } else {
            requestAnimationFrame(tick);
          }
        }
        requestAnimationFrame(tick);
      });
    });

    console.log(`FPS: baseline=${baseline.fps.toFixed(1)}, after input=${afterInput.fps.toFixed(1)}`);

    // FPS after input should not drop below 50% of baseline
    if (baseline.fps > 10) {
      expect(afterInput.fps).toBeGreaterThan(baseline.fps * 0.5);
    }
  });

  test('canvas element exists and is rendering', async ({ page }) => {
    await connectClient(page);
    await page.waitForTimeout(2000);

    // Verify canvas exists
    const canvasInfo = await page.evaluate(() => {
      const canvas = document.querySelector('canvas');
      if (!canvas) return null;
      return {
        width: canvas.width,
        height: canvas.height,
        hasContext: !!canvas.getContext('webgl2') || !!canvas.getContext('webgl')
      };
    });

    expect(canvasInfo).not.toBeNull();
    expect(canvasInfo.width).toBeGreaterThan(0);
    expect(canvasInfo.height).toBeGreaterThan(0);
  });

  test('player count displays correctly', async ({ page }) => {
    await connectClient(page);
    await page.waitForTimeout(3000);

    const playerCount = await page.evaluate(() => {
      const el = document.getElementById('playerCount');
      return el ? parseInt(el.textContent, 10) : -1;
    });

    // Should show at least 1 player (ourselves + bots)
    expect(playerCount).toBeGreaterThanOrEqual(1);
  });
});
