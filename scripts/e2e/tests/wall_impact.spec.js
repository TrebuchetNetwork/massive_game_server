const { test, expect } = require('@playwright/test');
const { registerServerLifecycle, resolveWsUrl } = require('./helpers/serverLifecycle');

async function connectClient(page) {
  await page.addInitScript(() => {
    try {
      localStorage.setItem('mgs_player_name', 'E2EPlayer');
    } catch (_) {}
  });
  await page.goto('/client.html?disable_stun=1&match_type=quick', { waitUntil: 'domcontentloaded' });
  await page.waitForSelector('#connectButton', { state: 'attached' });
  const matchTypeSelect = page.locator('#matchTypeSelect');
  if (await matchTypeSelect.count()) {
    await matchTypeSelect.selectOption('quick');
  }

  const wsInput = page.locator('#wsUrl');
  if (await wsInput.count()) {
    await wsInput.fill(resolveWsUrl());
  }

  let lastError = null;
  for (let attempt = 0; attempt < 2; attempt++) {
    await page.click('#connectButton', { force: true });
    try {
      await page.waitForFunction(
        () => window.__e2e && window.__e2e.connectionStatus && window.__e2e.connectionStatus.statusKey === 'playing',
        null,
        { timeout: 120000 }
      );
      await page.waitForFunction(
        () => window.__e2e && window.__e2e.hasLocalPlayer === true,
        null,
        { timeout: 120000 }
      );
      return;
    } catch (error) {
      lastError = error;
      await page.waitForTimeout(2000);
    }
  }

  throw lastError || new Error('Unable to establish local player state in connectClient');
}

registerServerLifecycle(test);

test.describe.configure({ retries: 1, timeout: 420000 });

test('shots generate wall impact events', async ({ page }) => {
  const pageErrors = [];
  page.on('pageerror', (err) => pageErrors.push(err.message || String(err)));

  await connectClient(page);

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
    const worldBounds = window.__e2e?.worldBounds;
    const worldMinX = Number(worldBounds?.minX ?? -800);
    const worldMaxX = Number(worldBounds?.maxX ?? 800);
    const worldMinY = Number(worldBounds?.minY ?? -600);
    const worldMaxY = Number(worldBounds?.maxY ?? 600);

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
