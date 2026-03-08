const { test, expect } = require('@playwright/test');
const { getServerMetrics, registerServerLifecycle } = require('./helpers/serverLifecycle');
const {
  closeAllClients,
  createConnectedClients,
  sendMovement,
  waitForPlayerVisibility,
} = require('./helpers/multiplayerClient');

registerServerLifecycle(test, {
  env: {
    MGS_METRICS_ENABLED: '1',
    MGS_METRICS_BIND_ADDR: '127.0.0.1:19190',
    MGS_TARGET_BOT_COUNT: '0',
    MGS_JOIN_RATE_LIMIT_PER_SEC: '0',
    MGS_IP_RATE_LIMIT_PER_SEC: '0',
  },
});

test.describe.configure({ timeout: 240000, retries: 1 });

test('@critical five browser clients join concurrently and keep receiving live state', async ({ browser }) => {
  const clients = await createConnectedClients(browser, 5, {
    query: '/client.html?disable_stun=1&match_type=quick',
    matchType: 'quick',
    timeout: 90000,
    nameFactory: (index) => `Scaled${index + 1}`,
  });

  try {
    await Promise.all(clients.map(({ page }) => waitForPlayerVisibility(page, 1, 60000)));

    const { response: metricsResponse, metrics } = await getServerMetrics('http://127.0.0.1:19190/metrics');
    expect(metricsResponse.ok).toBe(true);
    expect(Number(metrics.game_players_connected || 0)).toBeGreaterThanOrEqual(5);
    expect(Number(metrics.game_ws_connections_active || 0)).toBeGreaterThanOrEqual(5);

    const beforeMovement = await Promise.all(
      clients.map(({ page }) =>
        page.evaluate(() => ({
          playerCount: Number(window.__e2e?.playerCount || 0),
          lastStateUpdate: Number(window.__e2e?.lastStateUpdate || 0),
          hasLocalPlayer: !!window.__e2e?.hasLocalPlayer,
        }))
      )
    );

    beforeMovement.forEach((snapshot) => {
      expect(snapshot.playerCount).toBeGreaterThanOrEqual(1);
      expect(snapshot.hasLocalPlayer).toBe(true);
    });

    await sendMovement(clients[0].page, 'KeyW', 750);
    await sendMovement(clients[1].page, 'KeyD', 750);

    await Promise.all(
      clients.map(({ page }, index) =>
        page.waitForFunction(
          (before) => Number(window.__e2e?.lastStateUpdate || 0) > Number(before || 0),
          beforeMovement[index].lastStateUpdate,
          { timeout: 30000 }
        )
      )
    );

    const afterMovement = await Promise.all(
      clients.map(({ page }) =>
        page.evaluate(() => ({
          playerCount: Number(window.__e2e?.playerCount || 0),
          visiblePlayerCount: Number(window.__e2e?.visiblePlayerCount || 0),
          connectionStatus: window.__e2e?.connectionStatus?.statusKey || null,
        }))
      )
    );

    afterMovement.forEach((snapshot) => {
      expect(snapshot.playerCount).toBeGreaterThanOrEqual(1);
      expect(snapshot.visiblePlayerCount).toBeGreaterThanOrEqual(1);
      expect(['waiting', 'playing', 'respawn']).toContain(snapshot.connectionStatus);
    });
  } finally {
    await closeAllClients(clients);
  }
});
