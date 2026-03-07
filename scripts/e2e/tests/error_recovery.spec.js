const { test, expect } = require('@playwright/test');
const { startServer, stopServer } = require('./helpers/serverLifecycle');
const { connectClient } = require('./helpers/gameClient');

test.describe.configure({ timeout: 240000, retries: 1 });

test.beforeAll(async () => {
  await startServer();
});

test.afterAll(async () => {
  await stopServer();
});

test('client clears stale entities and reopens transport after server restart', async ({ page }) => {
  const pageErrors = [];
  page.on('pageerror', (err) => pageErrors.push(err.message || String(err)));

  await connectClient(page, {
    name: 'RecoveryE2E',
    query:
      '/client.html?auto_reconnect=1&auto_reconnect_base_ms=300&auto_reconnect_max_ms=1200&auto_reconnect_max=20&disable_stun=1&match_type=quick',
    matchType: 'quick',
    timeout: 90000,
  });

  await page.waitForFunction(
    () =>
      window.__e2e?.dataChannelOpen === true &&
      Number(window.__e2e?.lastStateUpdate || 0) > 0,
    null,
    { timeout: 30000 }
  );

  const before = await page.evaluate(() => ({
    lastStateUpdate: Number(window.__e2e?.lastStateUpdate) || 0,
    playerCount: Number(window.__e2e?.playerCount || 0),
    playerSpriteCount: Number(window.__e2e?.playerSpriteCount || 0),
  }));
  expect(before.lastStateUpdate).toBeGreaterThan(0);
  expect(before.playerSpriteCount).toBeLessThanOrEqual(before.playerCount);

  await stopServer();

  await page.waitForFunction(
    () => {
      const key = window.__e2e?.connectionStatus?.statusKey;
      return key === 'error' || key === 'connecting' || key === 'negotiating';
    },
    null,
    { timeout: 30000 }
  );

  await startServer();
  const reconnectMarker = await page.evaluate(() => performance.now());
  await page.evaluate(() => window.__e2e?.forceReconnectNow?.());

  await page.waitForFunction(
    (marker) =>
      window.__e2e?.dataChannelOpen === true &&
      Number(window.__e2e?.lastStateUpdate || 0) > Number(marker || 0),
    reconnectMarker,
    { timeout: 90000 }
  );

  const after = await page.evaluate(() => ({
    lastStateUpdate: Number(window.__e2e?.lastStateUpdate) || 0,
    dataChannelOpen: !!window.__e2e?.dataChannelOpen,
    statusKey: window.__e2e?.connectionStatus?.statusKey || null,
    matchInfoReady: !!window.__e2e?.matchInfoReady,
    hasLocalPlayer: !!window.__e2e?.hasLocalPlayer,
    playerCount: Number(window.__e2e?.playerCount || 0),
    playerSpriteCount: Number(window.__e2e?.playerSpriteCount || 0),
    localPlayerSpriteReady: !!window.__e2e?.localPlayerSpriteReady,
  }));

  expect(after.lastStateUpdate).toBeGreaterThan(before.lastStateUpdate);
  expect(after.dataChannelOpen).toBeTruthy();
  expect(['connecting', 'negotiating', 'waiting', 'playing', 'respawn']).toContain(
    after.statusKey
  );
  expect(after.playerSpriteCount).toBeLessThanOrEqual(after.playerCount);
  expect(after.localPlayerSpriteReady && !after.hasLocalPlayer).toBeFalsy();
  expect(pageErrors).toEqual([]);
});
