const { test, expect } = require('@playwright/test');
const { registerServerLifecycle, resolveWsUrl } = require('./helpers/serverLifecycle');

registerServerLifecycle(test);

test('client receives state and renders frames', async ({ page }) => {
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

  await page.waitForFunction(() => window.__e2e && window.__e2e.matchInfoReady === true, null, { timeout: 60000 });
  await page.waitForFunction(() => window.__e2e && window.__e2e.lastStateUpdate > 0, null, { timeout: 60000 });

  const dataChannelOpen = await page.evaluate(() => Boolean(window.__e2e && window.__e2e.dataChannelOpen));
  expect(dataChannelOpen).toBeTruthy();
  const status = await page.evaluate(() => window.__e2e && window.__e2e.connectionStatus);
  expect(status).toBeTruthy();
  expect(['waiting', 'playing', 'respawn']).toContain(status.statusKey);
});
