const { test, expect } = require('@playwright/test');
const { registerServerLifecycle, resolveWsUrl } = require('./helpers/serverLifecycle');

registerServerLifecycle(test);

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
  await page.waitForFunction(
    () => {
      const e2e = window.__e2e || {};
      return (Number(e2e.projectileCount) || 0) > 0 || (Number(e2e.visibleProjectileCount) || 0) > 0;
    },
    null,
    { timeout: 4000 }
  );
  await page.waitForTimeout(1200);
  await canvas.dispatchEvent('mouseup', { button: 0, clientX: aimX, clientY: aimY });

  const ammoAfter = await page
    .locator('#playerAmmo')
    .innerText()
    .then((value) => Number.parseInt(value, 10) || 0);

  const observation = await page.evaluate(() => {
    const e2e = window.__e2e || {};
    return {
      projectileCount: Number(e2e.projectileCount) || 0,
      visibleProjectileCount: Number(e2e.visibleProjectileCount) || 0
    };
  });

  expect(ammoAfter).toBeLessThan(ammoBefore);
  expect(observation.projectileCount > 0 || observation.visibleProjectileCount > 0).toBeTruthy();
  expect(pageErrors).toEqual([]);
});
