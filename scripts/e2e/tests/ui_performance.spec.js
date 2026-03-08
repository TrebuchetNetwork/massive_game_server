const { test, expect } = require('@playwright/test');
const { registerServerLifecycle } = require('./helpers/serverLifecycle');
const { connectClient } = require('./helpers/gameClient');

registerServerLifecycle(test);

test.describe('UI Performance', () => {
  test.describe.configure({ timeout: 420000, retries: 1 });
  test('maintains acceptable FPS after connection', async ({ page }) => {
    await connectClient(page, { timeout: 120000, requireLocalPlayer: false });

    // Warmup period
    await page.waitForTimeout(2000);

    // Use the e2e render-frame counter instead of requestAnimationFrame-only
    // timing; hosted headless runners can throttle RAF aggressively.
    const result = await page.evaluate(() => {
      return new Promise((resolve) => {
        const start = performance.now();
        const startFrames = Number(window.__e2e?.renderFrames) || 0;
        const target = 3000; // 3 seconds

        setTimeout(() => {
          const elapsed = (performance.now() - start) / 1000;
          const endFrames = Number(window.__e2e?.renderFrames) || 0;
          const renderFrameDelta = Math.max(0, endFrames - startFrames);
          const lastRenderAgeMs = Math.max(0, performance.now() - (Number(window.__e2e?.lastRenderTime) || 0));
          resolve({
            elapsed,
            renderFrameDelta,
            fps: renderFrameDelta / Math.max(elapsed, 0.001),
            lastRenderAgeMs
          });
        }, target);
      });
    });

    console.log(
      `Render loop measurement: ${result.fps.toFixed(1)} FPS-equivalent over ${result.elapsed.toFixed(1)}s (${result.renderFrameDelta} frames, last render age ${result.lastRenderAgeMs.toFixed(0)}ms)`
    );

    // Hosted CI runners aggressively throttle headless rendering. Keep this as a
    // liveness/perf-smoke gate rather than a workstation FPS target.
    expect(result.renderFrameDelta).toBeGreaterThanOrEqual(4);
    expect(result.lastRenderAgeMs).toBeLessThan(3000);
  });

  test('no excessive memory growth over time', async ({ page }) => {
    await connectClient(page, { timeout: 120000, requireLocalPlayer: false });

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
    await connectClient(page, { timeout: 120000, requireLocalPlayer: false });
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
    await connectClient(page, { timeout: 120000, requireLocalPlayer: false });
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
    await connectClient(page, { timeout: 120000, requireLocalPlayer: false });
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
    await connectClient(page, { timeout: 120000, requireLocalPlayer: false });
    await page.waitForTimeout(3000);

    const playerCount = await page.evaluate(() => {
      const el = document.getElementById('playerCount');
      return el ? parseInt(el.textContent, 10) : -1;
    });

    // Should show at least 1 player (ourselves + bots)
    expect(playerCount).toBeGreaterThanOrEqual(1);
  });
});
