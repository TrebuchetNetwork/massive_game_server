const { test, expect } = require('@playwright/test');
const { registerServerLifecycle, resolveWsUrl } = require('./helpers/serverLifecycle');

registerServerLifecycle(test);

test('connects and receives match state', async ({ page }) => {
  await page.addInitScript(() => {
    try {
      localStorage.setItem('mgs_player_name', 'E2EPlayer');
    } catch (_) {}
  });
  const response = await page.goto('/client.html', { waitUntil: 'domcontentloaded' });
  if (!response || !response.ok()) {
    throw new Error(`Failed to load /client.html. Status: ${response ? response.status() : 'no response'}`);
  }
  await page.waitForSelector('#connectButton', { state: 'attached' });
  const wsInput = page.locator('#wsUrl');
  if (await wsInput.count()) {
    await wsInput.fill(resolveWsUrl());
  }
  await page.click('#connectButton', { force: true });

  await page.waitForFunction(() => window.__e2e && window.__e2e.matchInfoReady === true, null, { timeout: 60000 });

  const status = await page.evaluate(() => window.__e2e && window.__e2e.connectionStatus);
  expect(status).toBeTruthy();
  expect(['waiting', 'playing', 'respawn']).toContain(status.statusKey);
});
