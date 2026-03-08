const fs = require('fs');
const os = require('os');
const path = require('path');
const { test, expect } = require('@playwright/test');
const { startServer, stopServer } = require('./helpers/serverLifecycle');
const { connectClient } = require('./helpers/gameClient');

test.describe.configure({ timeout: 180000, retries: 1 });

test.afterEach(async () => {
  await stopServer();
});

test('server shutdown disconnects client cleanly and persists shutdown snapshot', async ({ page }) => {
  const shutdownStatePath = path.join(
    os.tmpdir(),
    `mgs_shutdown_state_${Date.now()}_${Math.random().toString(16).slice(2)}.json`
  );

  await startServer({
    baseUrl: 'http://127.0.0.1:19080',
    env: {
      MGS_HOST: '0.0.0.0',
      MGS_PORT: '19080',
      MGS_DISABLE_STUN: '1',
      MGS_TARGET_BOT_COUNT: '0',
      MGS_SHUTDOWN_STATE_PATH: shutdownStatePath,
    },
  });

  await connectClient(page, {
    name: 'ShutdownE2E',
    query: '/client.html?disable_stun=1&match_type=quick',
    baseUrl: 'http://127.0.0.1:19080',
    wsUrl: 'ws://127.0.0.1:19080/ws',
    matchType: 'quick',
    timeout: 90000,
  });

  await stopServer();

  await page.waitForFunction(
    () => {
      const statusKey = window.__e2e?.connectionStatus?.statusKey;
      return statusKey === 'error' || statusKey === 'connecting' || window.__e2e?.dataChannelOpen === false;
    },
    null,
    { timeout: 30000 }
  );

  await expect
    .poll(() => fs.existsSync(shutdownStatePath), { timeout: 10000 })
    .toBe(true);

  const snapshot = JSON.parse(fs.readFileSync(shutdownStatePath, 'utf8'));
  expect(Number(snapshot.frame || 0)).toBeGreaterThanOrEqual(0);
  expect(snapshot.match_summary).toBeTruthy();
  expect(snapshot.population).toBeTruthy();
  expect(Number(snapshot.population.total_players || 0)).toBeGreaterThanOrEqual(1);
  expect(Array.isArray(snapshot.players)).toBe(true);
  expect(snapshot.players.length).toBeGreaterThanOrEqual(1);
  expect(snapshot.entities).toBeTruthy();
  expect(typeof snapshot.entities.projectiles_total).toBe('number');
});
