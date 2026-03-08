const { test, expect } = require('@playwright/test');
const { registerServerLifecycle, resolveWsUrl } = require('./helpers/serverLifecycle');

registerServerLifecycle(test);

function isCrossBrowserMatrix() {
  return process.env.PLAYWRIGHT_CROSS_BROWSER === '1';
}

async function snapshotSpriteState(page) {
  return page.evaluate(() => {
    const stage = window.app && window.app.stage;
    if (!stage) {
      return {
        uniqueCount: 0,
        totalCount: 0,
        duplicateIds: [],
        localPlayerCount: 0,
        lastStateUpdate: 0,
      };
    }

    const playerIds = [];
    let localPlayerCount = 0;
    const stack = [stage];
    while (stack.length) {
      const node = stack.pop();
      if (!node) continue;
      if (node.playerId) {
        playerIds.push(String(node.playerId));
        if (node.localIndicator) {
          localPlayerCount += 1;
        }
      }
      const children = node.children || [];
      for (let i = children.length - 1; i >= 0; i -= 1) {
        stack.push(children[i]);
      }
    }

    const counts = new Map();
    for (const id of playerIds) {
      counts.set(id, (counts.get(id) || 0) + 1);
    }
    const duplicateIds = Array.from(counts.entries())
      .filter(([, count]) => count > 1)
      .map(([id]) => id);

    return {
      uniqueCount: counts.size,
      totalCount: playerIds.length,
      duplicateIds,
      localPlayerCount,
      lastStateUpdate: window.__e2e?.lastStateUpdate || 0,
    };
  });
}

test('disconnect and reconnect clears stale state without ghost entities', async ({ page }) => {
  const pageErrors = [];
  page.on('pageerror', (err) => pageErrors.push(err.message || String(err)));

  await page.addInitScript(() => {
    try {
      localStorage.setItem('mgs_player_name', 'ReconnectE2E');
    } catch (_) {}
  });

  await page.goto('/client.html?auto_reconnect=1&disable_stun=1&match_type=quick', {
    waitUntil: 'domcontentloaded',
  });
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

  await page.waitForFunction(
    () =>
      window.__e2e &&
      window.__e2e.dataChannelOpen === true &&
      (window.__e2e.matchInfoReady === true ||
        ['waiting', 'playing', 'respawn'].includes(window.__e2e.connectionStatus?.statusKey || '') ||
        Number(window.__e2e.lastStateUpdate || 0) > 0),
    null,
    { timeout: 60000 }
  );

  const before = await snapshotSpriteState(page);
  if (!isCrossBrowserMatrix()) {
    expect(before.localPlayerCount).toBe(1);
  }
  expect(before.duplicateIds).toEqual([]);

  await page.evaluate(() => {
    if (typeof window.__e2e?.forceCloseDataChannel === 'function') {
      window.__e2e.forceCloseDataChannel();
    } else if (typeof window.__e2e?.forceResetConnection === 'function') {
      window.__e2e.forceResetConnection();
    } else {
      throw new Error('Reconnect E2E hook missing');
    }
  });

  await page.waitForFunction(
    () =>
      window.__e2e &&
      window.__e2e.connectionStatus &&
      ['connecting', 'negotiating'].includes(window.__e2e.connectionStatus.statusKey),
    null,
    { timeout: 30000 }
  );

  await page.waitForFunction(
    () =>
      window.__e2e &&
      window.__e2e.dataChannelOpen === true &&
      Number(window.__e2e.lastStateUpdate || 0) > 0,
    null,
    { timeout: 90000 }
  );

  await page.waitForTimeout(1000);

  const after = await snapshotSpriteState(page);
  expect(after.duplicateIds).toEqual([]);
  const afterE2e = await page.evaluate(() => ({
    dataChannelOpen: !!window.__e2e?.dataChannelOpen,
    lastStateUpdate: Number(window.__e2e?.lastStateUpdate || 0),
    status: window.__e2e?.connectionStatus?.statusKey || null,
    playerCount: Number(window.__e2e?.playerCount || 0),
    playerSpriteCount: Number(window.__e2e?.playerSpriteCount || 0),
    hasLocalPlayer: !!window.__e2e?.hasLocalPlayer,
    localPlayerSpriteReady: !!window.__e2e?.localPlayerSpriteReady,
  }));
  expect(afterE2e.dataChannelOpen).toBeTruthy();
  expect(afterE2e.lastStateUpdate).toBeGreaterThan(before.lastStateUpdate);
  expect(afterE2e.playerSpriteCount).toBeLessThanOrEqual(afterE2e.playerCount);
  expect(afterE2e.localPlayerSpriteReady && !afterE2e.hasLocalPlayer).toBeFalsy();
  expect(['connecting', 'negotiating', 'waiting', 'playing', 'respawn']).toContain(afterE2e.status);
  if (!isCrossBrowserMatrix()) {
    expect(pageErrors).toEqual([]);
  }
});
