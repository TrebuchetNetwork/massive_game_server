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
