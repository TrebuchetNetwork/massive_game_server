const { test, expect } = require('@playwright/test');
const { registerServerLifecycle, resolveWsUrl } = require('./helpers/serverLifecycle');

registerServerLifecycle(test);

test('client receives state and renders frames', async ({ page }) => {
  await page.goto('/client.html', { waitUntil: 'domcontentloaded' });
  await page.waitForSelector('#connectButton', { state: 'attached' });
  const wsInput = page.locator('#wsUrl');
  if (await wsInput.count()) {
    await wsInput.fill(resolveWsUrl());
  }
  await page.click('#connectButton', { force: true });

  await page.waitForFunction(() => window.__e2e && window.__e2e.matchInfoReady === true, null, { timeout: 60000 });
  await page.waitForFunction(() => window.__e2e && window.__e2e.lastStateUpdate > 0, null, { timeout: 60000 });

  const startFrames = await page.evaluate(() => window.__e2e.renderFrames);
  await page.waitForTimeout(1000);
  const endFrames = await page.evaluate(() => window.__e2e.renderFrames);

  expect(endFrames).toBeGreaterThan(startFrames);
  const hasLocalPlayer = await page.evaluate(() => window.__e2e.hasLocalPlayer);
  expect(hasLocalPlayer).toBeTruthy();
});
