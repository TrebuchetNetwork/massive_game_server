const { test, expect } = require('@playwright/test');
const { registerServerLifecycle } = require('./helpers/serverLifecycle');
const { connectClient, waitForPlaying } = require('./helpers/gameClient');

test.use({
  viewport: { width: 390, height: 844 },
  isMobile: true,
  hasTouch: true,
});

registerServerLifecycle(test);

test.describe.configure({ timeout: 120000, retries: 1 });

test('mobile layout exposes touch controls, data saver mode, and touch-fire state', async ({ page }) => {
  await page.addInitScript(() => {
    try {
      localStorage.setItem('dataSaverMode', 'true');
    } catch (_) {}
  });
  const pageErrors = [];
  page.on('pageerror', (err) => pageErrors.push(err.message || String(err)));

  await connectClient(page, {
    name: 'MobileTouchE2E',
    query: '/client.html?mobile=1&disable_stun=1&match_type=mobile_blitz',
    matchType: 'mobile_blitz',
    timeout: 90000,
  });

  await expect(page.locator('#mobileControls')).toBeVisible();
  await page.waitForFunction(
    () => window.__e2e?.mobileDynamicsEnabled === true && window.__e2e?.bodyMobileMode === true,
    null,
    { timeout: 30000 }
  );

  await expect(page.locator('#mobileFire')).toBeVisible();
  await expect(page.locator('#mobileReload')).toBeVisible();
  await expect(page.locator('#mobileAbilityDash')).toBeVisible();
  await expect(page.locator('#mobileAbilityDodge')).toBeVisible();

  await waitForPlaying(page, 30000);
  await expect
    .poll(async () => page.evaluate(() => !!window.__e2e?.dataSaverMode), { timeout: 10000 })
    .toBe(true);

  await page.evaluate(() => {
    const button = document.getElementById('mobileFire');
    if (!button || typeof Touch !== 'function' || typeof TouchEvent !== 'function') {
      return;
    }
    const touch = new Touch({
      identifier: 1,
      target: button,
      clientX: 24,
      clientY: 24,
      pageX: 24,
      pageY: 24,
      radiusX: 2,
      radiusY: 2,
      rotationAngle: 0,
      force: 1,
    });
    button.dispatchEvent(new TouchEvent('touchstart', {
      bubbles: true,
      cancelable: true,
      touches: [touch],
      targetTouches: [touch],
      changedTouches: [touch],
    }));
  });
  await page.waitForFunction(() => window.__e2e?.mobileFireTouchActive === true, null, { timeout: 5000 });
  await page.evaluate(() => {
    const button = document.getElementById('mobileFire');
    if (!button || typeof Touch !== 'function' || typeof TouchEvent !== 'function') {
      return;
    }
    const touch = new Touch({
      identifier: 1,
      target: button,
      clientX: 24,
      clientY: 24,
      pageX: 24,
      pageY: 24,
      radiusX: 2,
      radiusY: 2,
      rotationAngle: 0,
      force: 0,
    });
    button.dispatchEvent(new TouchEvent('touchend', {
      bubbles: true,
      cancelable: true,
      touches: [],
      targetTouches: [],
      changedTouches: [touch],
    }));
  });
  await page.waitForFunction(() => window.__e2e?.mobileFireTouchActive === false, null, { timeout: 5000 });


  await page.setViewportSize({ width: 844, height: 390 });
  await page.waitForTimeout(400);
  await expect(page.locator('#mobileControls')).toBeVisible();

  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(400);
  await expect(page.locator('#mobileControls')).toBeVisible();

  const mobileSnapshot = await page.evaluate(() => ({
    mobileDynamicsEnabled: !!window.__e2e?.mobileDynamicsEnabled,
    bodyMobileMode: !!window.__e2e?.bodyMobileMode,
    dataSaverMode: !!window.__e2e?.dataSaverMode,
    selectedMatchType: window.__e2e?.selectedMatchType || '',
    statusKey: window.__e2e?.connectionStatus?.statusKey || null,
  }));

  expect(mobileSnapshot.mobileDynamicsEnabled).toBeTruthy();
  expect(mobileSnapshot.bodyMobileMode).toBeTruthy();
  expect(mobileSnapshot.dataSaverMode).toBeTruthy();
  expect(mobileSnapshot.selectedMatchType).toBe('mobile_blitz');
  expect(['waiting', 'playing', 'respawn']).toContain(mobileSnapshot.statusKey);
  expect(pageErrors).toEqual([]);
});
