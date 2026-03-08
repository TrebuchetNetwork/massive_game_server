const { test, expect } = require('@playwright/test');
const { registerServerLifecycle } = require('./helpers/serverLifecycle');
const { connectClient } = require('./helpers/gameClient');

registerServerLifecycle(test);

async function waitForPlaying(page, timeout = 60000) {
  const allowCrossBrowserLiveState = process.env.PLAYWRIGHT_CROSS_BROWSER === '1';
  await page.waitForFunction(
    (allowLiveState) => {
      const e2e = window.__e2e;
      if (!e2e || !e2e.connectionStatus) return false;
      const key = e2e.connectionStatus.statusKey;
      if (key === 'playing') {
        return true;
      }
      if (!allowLiveState) {
        return false;
      }
      return ['waiting', 'respawn'].includes(key) || Number(e2e.lastStateUpdate || 0) > 0;
    },
    allowCrossBrowserLiveState,
    { timeout }
  );
}

async function getLocalPlayerPosition(page) {
  const position = await page.evaluate(() => {
    const snapshot = window.__e2e && window.__e2e.localPlayerSnapshot;
    if (snapshot) {
      return {
        x: Number(snapshot.x) || 0,
        y: Number(snapshot.y) || 0,
      };
    }

    const stage = window.app && window.app.stage;
    if (!stage) return null;

    const stack = [stage];
    while (stack.length) {
      const node = stack.pop();
      if (!node) continue;
      if (node.localIndicator && node.playerId) {
        return { x: Number(node.x) || 0, y: Number(node.y) || 0 };
      }
      const children = node.children || [];
      for (let i = children.length - 1; i >= 0; i -= 1) {
        stack.push(children[i]);
      }
    }
    return null;
  });
  expect(position).toBeTruthy();
  return position;
}

async function readAmmo(page) {
  return page.locator('#playerAmmo').innerText().then((value) => Number.parseInt(value, 10) || 0);
}

async function isElementHiddenByClass(page, selector) {
  return page.locator(selector).evaluate((el) => el.classList.contains('hidden'));
}

test('human gameplay journey remains stable and responsive', async ({ page }) => {
  const pageErrors = [];
  page.on('pageerror', (err) => pageErrors.push(err.message || String(err)));

  await page.addInitScript(() => {
    try {
      localStorage.setItem('mgs_player_name', 'E2EPlayer');
    } catch (_) {}
  });
  await connectClient(page, {
    name: 'E2EPlayer',
    query: '/client.html?auto_reconnect=1&disable_stun=1&match_type=quick',
    matchType: 'quick',
    requireLocalPlayer: process.env.PLAYWRIGHT_CROSS_BROWSER !== '1',
    timeout: 60000,
  });
  await waitForPlaying(page);

  const autoReconnectEnabled = await page.evaluate(
    () => !!(window.__e2e && window.__e2e.autoReconnectEnabled)
  );
  expect(autoReconnectEnabled).toBeTruthy();

  const canvas = page.locator('#pixiContainer canvas');
  await canvas.waitFor({ state: 'visible', timeout: 60000 });
  const box = await canvas.boundingBox();
  expect(box).toBeTruthy();

  const posBefore = await getLocalPlayerPosition(page);
  const ammoBefore = await readAmmo(page);

  const aimX = box.x + box.width * 0.76;
  const aimY = box.y + box.height * 0.52;
  await page.mouse.move(aimX, aimY);

  // Simulate directional movement + ability usage.
  await page.keyboard.down('KeyW');
  await page.waitForTimeout(1200);
  await page.keyboard.press('KeyQ'); // dash
  await page.waitForTimeout(220);
  await page.keyboard.press('KeyE'); // dodge
  await page.waitForTimeout(320);
  await page.keyboard.up('KeyW');
  await page.keyboard.down('KeyA');
  await page.waitForTimeout(600);
  await page.keyboard.up('KeyA');
  await page.waitForTimeout(300);

  const posAfterMove = await getLocalPlayerPosition(page);
  const movementDistance = Math.hypot(
    posAfterMove.x - posBefore.x,
    posAfterMove.y - posBefore.y
  );
  expect(movementDistance).toBeGreaterThan(8);

  // Simulate weapon flow: swap, shoot burst, reload, melee.
  await page.keyboard.press('Digit2');
  await page.waitForTimeout(80);
  await page.keyboard.press('Digit1');
  await page.waitForTimeout(120);

  await page.mouse.move(aimX, aimY);
  await page.mouse.down({ button: 'left' });
  await page.waitForTimeout(1400);
  await page.mouse.up({ button: 'left' });

  const projectileObservation = await page.evaluate(async () => {
    const startedAt = performance.now();
    let maxProjectileCount = 0;
    let maxVisibleProjectileCount = 0;
    while (performance.now() - startedAt < 5000) {
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

  await page.keyboard.press('KeyR');
  await page.waitForTimeout(200);
  await page.keyboard.press('KeyV');

  const ammoAfter = await readAmmo(page);
  const weaponFlowObserved =
    projectileObservation.maxProjectileCount > 0 ||
    projectileObservation.maxVisibleProjectileCount > 0 ||
    ammoAfter <= ammoBefore;
  expect(weaponFlowObserved).toBeTruthy();

  // UI interactions a player commonly performs.
  await page.keyboard.down('Tab');
  await page.waitForTimeout(220);
  expect(await isElementHiddenByClass(page, '#scoreboard')).toBeFalsy();
  await page.keyboard.up('Tab');
  await page.waitForTimeout(220);
  expect(await isElementHiddenByClass(page, '#scoreboard')).toBeTruthy();

  await page.keyboard.press('Escape');
  await page.waitForTimeout(180);
  expect(await isElementHiddenByClass(page, '#settingsMenu')).toBeFalsy();
  await page.keyboard.press('Escape');
  await page.waitForTimeout(180);
  expect(await isElementHiddenByClass(page, '#settingsMenu')).toBeTruthy();

  expect(pageErrors).toEqual([]);
});
