const { test, expect } = require('@playwright/test');
const { registerServerLifecycle, resolveWsUrl } = require('./helpers/serverLifecycle');

registerServerLifecycle(test);

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
      window.__e2e.matchInfoReady === true &&
      window.__e2e.hasLocalPlayer === true,
    null,
    { timeout: 60000 }
  );

  const before = await snapshotSpriteState(page);
  expect(before.localPlayerCount).toBe(1);
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
      window.__e2e.matchInfoReady === true &&
      window.__e2e.hasLocalPlayer === true &&
      window.__e2e.dataChannelOpen === true,
    null,
    { timeout: 90000 }
  );

  const after = await snapshotSpriteState(page);
  expect(after.duplicateIds).toEqual([]);

  await expect(page.locator('#myPlayerIdSpan')).not.toHaveText('N/A');
  const playerCountText = await page.locator('#playerCount').innerText();
  expect(Number.parseInt(playerCountText, 10) || 0).toBeGreaterThan(0);

  const status = await page.evaluate(() => window.__e2e?.connectionStatus?.statusKey || null);
  expect(['waiting', 'playing', 'respawn']).toContain(status);
});
