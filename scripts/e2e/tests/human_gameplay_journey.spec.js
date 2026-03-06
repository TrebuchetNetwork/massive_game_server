const { test, expect } = require('@playwright/test');
const { registerServerLifecycle, resolveWsUrl } = require('./helpers/serverLifecycle');

registerServerLifecycle(test);

async function waitForPlaying(page, timeout = 60000) {
  await page.waitForFunction(
    () =>
      window.__e2e &&
      window.__e2e.connectionStatus &&
      window.__e2e.connectionStatus.statusKey === 'playing',
    null,
    { timeout }
  );
}

async function dismissUsernameModalIfVisible(page) {
  const modalVisible = await page.evaluate(() => {
    const modal = document.getElementById('usernameModal');
    return !!modal && !modal.classList.contains('hidden');
  });
  if (!modalVisible) return;

  const quickStartButton = page.locator('#usernameSkipButton');
  if (await quickStartButton.count()) {
    await quickStartButton.click({ force: true });
    return;
  }
  const saveButton = page.locator('#usernameSaveButton');
  if (await saveButton.count()) {
    await saveButton.click({ force: true });
  }
}

async function getLocalSpritePosition(page) {
  const position = await page.evaluate(() => {
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
  await page.goto('/client.html?auto_reconnect=1&disable_stun=1&match_type=quick', { waitUntil: 'domcontentloaded' });
  await dismissUsernameModalIfVisible(page);

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
    () => window.__e2e && window.__e2e.matchInfoReady === true,
    null,
    { timeout: 60000 }
  );
  await page.waitForFunction(
    () => window.__e2e && window.__e2e.hasLocalPlayer === true,
    null,
    { timeout: 60000 }
  );
  await waitForPlaying(page);

  const autoReconnectEnabled = await page.evaluate(
    () => !!(window.__e2e && window.__e2e.autoReconnectEnabled)
  );
  expect(autoReconnectEnabled).toBeTruthy();

  const canvas = page.locator('#pixiContainer canvas');
  await canvas.waitFor({ state: 'visible', timeout: 60000 });
  const box = await canvas.boundingBox();
  expect(box).toBeTruthy();

  const posBefore = await getLocalSpritePosition(page);
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
  await page.waitForTimeout(450);
  await page.keyboard.up('KeyA');

  const posAfterMove = await getLocalSpritePosition(page);
  const movementDistance = Math.hypot(
    posAfterMove.x - posBefore.x,
    posAfterMove.y - posBefore.y
  );
  expect(movementDistance).toBeGreaterThan(35);

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
  expect(ammoAfter).toBeLessThan(ammoBefore);
  expect(
    projectileObservation.maxProjectileCount > 0 ||
      projectileObservation.maxVisibleProjectileCount > 0 ||
      (ammoBefore - ammoAfter) >= 1
  ).toBeTruthy();

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
