const path = require('path');
const { test, expect } = require('@playwright/test');
const { registerServerLifecycle } = require('./helpers/serverLifecycle');
const { connectClient } = require('./helpers/gameClient');

const customMapPath = path.resolve(__dirname, '..', 'fixtures', 'custom_map_smoke.json');

registerServerLifecycle(test, {
  env: {
    MGS_TARGET_BOT_COUNT: '0',
    MGS_MAP_PATH: customMapPath,
  },
});

test.describe.configure({ timeout: 120000, retries: 1 });

test('client sees walls from a custom map fixture', async ({ page }) => {
  await connectClient(page, {
    name: 'CustomMapE2E',
    query: '/client.html?disable_stun=1&match_type=quick',
    matchType: 'quick',
    timeout: 90000,
  });

  await page.waitForFunction(() => Number(window.__e2e?.wallCount || 0) >= 2, null, { timeout: 30000 });
  const wallCount = await page.evaluate(() => Number(window.__e2e?.wallCount || 0));
  expect(wallCount).toBe(2);
});
