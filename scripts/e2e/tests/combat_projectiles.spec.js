const { test, expect } = require('@playwright/test');
const { registerServerLifecycle, resolveWsUrl } = require('./helpers/serverLifecycle');

registerServerLifecycle(test);
test.skip(process.env.E2E_DEEP_PROJECTILE !== '1', 'requires the dedicated projectile harness lane');

test('firing consumes ammo and publishes projectile state', async ({ page }) => {
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
  await page.waitForFunction(
    () => {
      const ammoEl = document.getElementById('playerAmmo');
      return (Number.parseInt(ammoEl?.textContent || '0', 10) || 0) > 0;
    },
    null,
    { timeout: 15000 }
  );

  const ammoBefore = await page
    .locator('#playerAmmo')
    .innerText()
    .then((value) => Number.parseInt(value, 10) || 0);
  const projectileCountBefore = await page.evaluate(() => {
    const e2e = window.__e2e || {};
    return {
      projectileCount: Number(e2e.projectileCount) || 0,
      visibleProjectileCount: Number(e2e.visibleProjectileCount) || 0,
    };
  });
  let observedFire = false;
  for (let attempt = 0; attempt < 3 && !observedFire; attempt += 1) {
    await page.evaluate(() => window.__e2e?.forcePrimaryFire?.(true));
    await page.waitForTimeout(250);
    await page.evaluate(() => window.__e2e?.forcePrimaryFire?.(false));
    try {
      await page.waitForFunction(
        ({ previousAmmo, previousProjectileCount, previousVisibleProjectileCount }) => {
          const e2e = window.__e2e || {};
          const ammoEl = document.getElementById('playerAmmo');
          const ammoNow = Number.parseInt(ammoEl?.textContent || '0', 10) || 0;
          return (
            ammoNow < previousAmmo ||
            (Number(e2e.projectileCount) || 0) > previousProjectileCount ||
            (Number(e2e.visibleProjectileCount) || 0) > previousVisibleProjectileCount
          );
        },
        {
          previousAmmo: ammoBefore,
          previousProjectileCount: projectileCountBefore.projectileCount,
          previousVisibleProjectileCount: projectileCountBefore.visibleProjectileCount,
        },
        { timeout: 4000 }
      );
      observedFire = true;
    } catch (error) {
      if (attempt === 2) throw error;
      await page.waitForTimeout(300);
    }
  }
  await page.waitForTimeout(400);
  await page.evaluate(() => window.__e2e?.forcePrimaryFire?.(false));

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

  expect(
    observation.projectileCount > projectileCountBefore.projectileCount ||
      observation.visibleProjectileCount > projectileCountBefore.visibleProjectileCount ||
      ammoAfter < ammoBefore
  ).toBeTruthy();
  expect(pageErrors).toEqual([]);
});
