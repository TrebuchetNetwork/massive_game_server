const { test, expect } = require('@playwright/test');
const { connectClient, getLocalPlayerPosition } = require('./helpers/gameClient');
const { sendMovement } = require('./helpers/multiplayerClient');

test.describe.configure({ timeout: 120000, retries: 1 });
test.skip(process.env.E2E_SERVER_SKIP !== '1', 'kind smoke runs only under the dedicated k8s gate');

test('@smoke kind cluster supports data-channel gameplay movement', async ({ page }) => {
  await connectClient(page, {
    name: 'KindSmoke',
    query: '/client.html?disable_stun=1&match_type=quick',
    matchType: 'quick',
    timeout: 90000,
  });

  const before = await getLocalPlayerPosition(page);
  await sendMovement(page, 'KeyD', 900);

  await page.waitForFunction(
    ({ startX, startY }) => {
      const snapshot = window.__e2e?.localPlayerSnapshot;
      if (!snapshot) return false;
      const nextX = Number(snapshot.x || 0);
      const nextY = Number(snapshot.y || 0);
      return Math.abs(nextX - startX) > 8 || Math.abs(nextY - startY) > 8;
    },
    { startX: before.x, startY: before.y },
    { timeout: 15000 }
  );

  const after = await getLocalPlayerPosition(page);
  expect(Math.abs(after.x - before.x) > 8 || Math.abs(after.y - before.y) > 8).toBeTruthy();
  expect(await page.evaluate(() => Boolean(window.__e2e?.dataChannelOpen))).toBe(true);
});
