const { test, expect } = require('@playwright/test');
const { registerServerLifecycle } = require('./helpers/serverLifecycle');
const turnFallbackPort = 30000 + Math.floor(Math.random() * 10000);
const turnFallbackBaseUrl = `http://127.0.0.1:${turnFallbackPort}`;
const turnFallbackWsUrl = `ws://127.0.0.1:${turnFallbackPort}/ws`;

registerServerLifecycle(test, {
  baseUrl: turnFallbackBaseUrl,
  env: {
    MGS_TARGET_BOT_COUNT: '0',
    MGS_TURN_URLS: 'turn:127.0.0.1:3478?transport=udp',
    MGS_TURN_USERNAME: 'turn-user',
    MGS_TURN_CREDENTIAL: 'turn-password',
    MGS_TURN_CREDENTIAL_TYPE: 'password',
  },
});

test.describe.configure({ timeout: 180000, retries: 1 });

test('client receives TURN-backed ICE config from signaling server', async ({ page }) => {
  await page.goto(`${turnFallbackBaseUrl}/client.html?disable_stun=1&match_type=quick`, {
    waitUntil: 'domcontentloaded',
  });
  const iceSnapshot = await page.evaluate((wsUrl) => {
    return new Promise((resolve, reject) => {
      const timeoutId = setTimeout(() => reject(new Error('timed out waiting for ice_servers')), 10000);
      const socket = new WebSocket(`${wsUrl}?username=turn-fallback`);
      socket.addEventListener('message', (event) => {
        try {
          const payload = JSON.parse(String(event.data || ''));
          if (payload.event !== 'ice_servers' || !Array.isArray(payload.ice_servers)) {
            return;
          }
          const turnCount = payload.ice_servers.filter((server) =>
            Array.isArray(server?.urls)
              ? server.urls.some((url) => String(url).startsWith('turn:'))
              : String(server?.urls || '').startsWith('turn:')
          ).length;
          clearTimeout(timeoutId);
          socket.close();
          resolve({
            serverIceServerCount: payload.ice_servers.length,
            serverTurnCount: turnCount,
          });
        } catch (error) {
          reject(error);
        }
      });
      socket.addEventListener('error', () => reject(new Error('websocket failed before ice config')));
    });
  }, turnFallbackWsUrl);

  expect(iceSnapshot.serverIceServerCount).toBeGreaterThan(0);
  expect(iceSnapshot.serverTurnCount).toBeGreaterThan(0);
});
