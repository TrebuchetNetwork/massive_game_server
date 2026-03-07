const { test, expect } = require('@playwright/test');
const { registerServerLifecycle } = require('./helpers/serverLifecycle');
const { connectClient, getLocalPlayerPosition } = require('./helpers/gameClient');

registerServerLifecycle(test);

test.describe.configure({ timeout: 240000, retries: 1 });

test('live client keeps pickup and zone state coherent while moving', async ({ page }) => {
  const pageErrors = [];
  page.on('pageerror', (err) => pageErrors.push(err.message || String(err)));

  await connectClient(page, {
    name: 'PickupZoneE2E',
    query: '/client.html?disable_stun=1&match_type=quick',
    matchType: 'quick',
    timeout: 120000,
  });

  await page.waitForFunction(
    () =>
      window.__e2e &&
      Number(window.__e2e.pickupCount || 0) > 0 &&
      Number(window.__e2e.pickupSpriteCount || 0) > 0 &&
      Number(window.__e2e.zoneCount || 0) > 0 &&
      window.__e2e.nearestPickup,
    null,
    { timeout: 120000 }
  );

  const before = await page.evaluate(() => ({
    pickupCount: Number(window.__e2e?.pickupCount || 0),
    pickupSpriteCount: Number(window.__e2e?.pickupSpriteCount || 0),
    zoneCount: Number(window.__e2e?.zoneCount || 0),
    pickupTypeSummary: { ...(window.__e2e?.pickupTypeSummary || {}) },
    zoneTypeSummary: { ...(window.__e2e?.zoneTypeSummary || {}) },
    nearestPickup: window.__e2e?.nearestPickup || null,
  }));

  expect(before.pickupCount).toBeGreaterThan(0);
  expect(before.pickupSpriteCount).toBeGreaterThan(0);
  expect(before.pickupSpriteCount).toBeLessThanOrEqual(before.pickupCount);
  expect(before.zoneCount).toBeGreaterThan(0);
  expect(Object.keys(before.pickupTypeSummary).length).toBeGreaterThan(0);
  expect(Object.keys(before.zoneTypeSummary).length).toBeGreaterThan(0);
  expect(before.nearestPickup).toBeTruthy();

  const canvas = page.locator('#pixiContainer canvas');
  await canvas.waitFor({ state: 'visible', timeout: 60000 });
  const box = await canvas.boundingBox();
  expect(box).toBeTruthy();

  const beforePosition = await getLocalPlayerPosition(page);
  await page.mouse.move(box.x + box.width * 0.78, box.y + box.height * 0.48);
  await page.keyboard.down('KeyW');
  await page.waitForTimeout(900);
  await page.keyboard.up('KeyW');
  await page.waitForTimeout(600);

  const afterPosition = await getLocalPlayerPosition(page);
  const movedDistance = Math.hypot(
    afterPosition.x - beforePosition.x,
    afterPosition.y - beforePosition.y
  );
  expect(movedDistance).toBeGreaterThan(10);

  const after = await page.evaluate(() => ({
    pickupCount: Number(window.__e2e?.pickupCount || 0),
    pickupSpriteCount: Number(window.__e2e?.pickupSpriteCount || 0),
    zoneCount: Number(window.__e2e?.zoneCount || 0),
    pickupTypeSummary: { ...(window.__e2e?.pickupTypeSummary || {}) },
    zoneTypeSummary: { ...(window.__e2e?.zoneTypeSummary || {}) },
    nearestPickup: window.__e2e?.nearestPickup || null,
  }));

  expect(after.pickupCount).toBeGreaterThan(0);
  expect(after.pickupSpriteCount).toBeGreaterThan(0);
  expect(after.pickupSpriteCount).toBeLessThanOrEqual(after.pickupCount);
  expect(after.zoneCount).toBeGreaterThan(0);
  expect(Object.keys(after.pickupTypeSummary).length).toBeGreaterThan(0);
  expect(Object.keys(after.zoneTypeSummary).length).toBeGreaterThan(0);
  expect(after.nearestPickup).toBeTruthy();

  const nearestPickupDistanceChanged =
    Math.abs(Number(after.nearestPickup.distance || 0) - Number(before.nearestPickup.distance || 0)) > 1;
  expect(nearestPickupDistanceChanged || movedDistance > 15).toBeTruthy();
  expect(pageErrors).toEqual([]);
});
