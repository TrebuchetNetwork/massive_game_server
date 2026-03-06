const { test, expect } = require('@playwright/test');
const { registerServerLifecycle, resolveWsUrl } = require('./helpers/serverLifecycle');

registerServerLifecycle(test);

test('local player cannot move through world boundary walls', async ({ page }) => {
  const pageErrors = [];
  page.on('pageerror', (err) => pageErrors.push(err.message || String(err)));

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
    const worldBounds = window.__e2e?.worldBounds;
    const worldMinX = Number(worldBounds?.minX ?? -800);
    const worldMaxX = Number(worldBounds?.maxX ?? 800);
    const worldMinY = Number(worldBounds?.minY ?? -600);
    const worldMaxY = Number(worldBounds?.maxY ?? 600);
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

  const bounds = await page.evaluate(() => window.__e2e?.worldBounds || null);
  const worldMinX = Number(bounds?.minX ?? -800);
  const worldMaxX = Number(bounds?.maxX ?? 800);
  const worldMinY = Number(bounds?.minY ?? -600);
  const worldMaxY = Number(bounds?.maxY ?? 600);
  const boundaryEpsilon = 1.5;
  expect(after.x).toBeGreaterThanOrEqual(worldMinX - boundaryEpsilon);
  expect(after.x).toBeLessThanOrEqual(worldMaxX + boundaryEpsilon);
  expect(after.y).toBeGreaterThanOrEqual(worldMinY - boundaryEpsilon);
  expect(after.y).toBeLessThanOrEqual(worldMaxY + boundaryEpsilon);

  const displacement = Math.hypot(after.x - before.x, after.y - before.y);
  expect(displacement).toBeGreaterThan(20);
  expect(pageErrors).toEqual([]);
});
