const { test, expect } = require('@playwright/test');
const { registerServerLifecycle } = require('./helpers/serverLifecycle');
const { connectClient } = require('./helpers/gameClient');

registerServerLifecycle(test, {
  env: {
    MGS_TARGET_BOT_COUNT: '0',
  },
});

test.describe.configure({ timeout: 180000, retries: 1 });

test('two browser clients join concurrently and both keep receiving state', async ({ browser }) => {
  const contextA = await browser.newContext();
  const contextB = await browser.newContext();
  const pageA = await contextA.newPage();
  const pageB = await contextB.newPage();

  try {
    await connectClient(pageA, {
      name: 'MultiA',
      query: '/client.html?disable_stun=1&match_type=quick',
      matchType: 'quick',
      timeout: 90000,
    });
    await connectClient(pageB, {
      name: 'MultiB',
      query: '/client.html?disable_stun=1&match_type=quick',
      matchType: 'quick',
      timeout: 90000,
    });

    const beforeA = await pageA.evaluate(() => Number(window.__e2e?.lastStateUpdate || 0));
    const beforeB = await pageB.evaluate(() => Number(window.__e2e?.lastStateUpdate || 0));

    await pageA.waitForFunction(
      (before) => Number(window.__e2e?.lastStateUpdate || 0) > Number(before || 0),
      beforeA,
      { timeout: 30000 }
    );
    await pageB.waitForFunction(
      (before) => Number(window.__e2e?.lastStateUpdate || 0) > Number(before || 0),
      beforeB,
      { timeout: 30000 }
    );

    const [snapshotA, snapshotB] = await Promise.all([
      pageA.evaluate(() => ({
        playerCount: Number(window.__e2e?.playerCount || 0),
        spriteCount: Number(window.__e2e?.playerSpriteCount || 0),
        hasLocalPlayer: !!window.__e2e?.hasLocalPlayer,
        lastStateUpdate: Number(window.__e2e?.lastStateUpdate || 0),
      })),
      pageB.evaluate(() => ({
        playerCount: Number(window.__e2e?.playerCount || 0),
        spriteCount: Number(window.__e2e?.playerSpriteCount || 0),
        hasLocalPlayer: !!window.__e2e?.hasLocalPlayer,
        lastStateUpdate: Number(window.__e2e?.lastStateUpdate || 0),
      })),
    ]);

    expect(snapshotA.playerCount).toBeGreaterThanOrEqual(1);
    expect(snapshotB.playerCount).toBeGreaterThanOrEqual(1);
    expect(snapshotA.spriteCount).toBeGreaterThanOrEqual(1);
    expect(snapshotB.spriteCount).toBeGreaterThanOrEqual(1);
    expect(snapshotA.lastStateUpdate).toBeGreaterThan(beforeA);
    expect(snapshotB.lastStateUpdate).toBeGreaterThan(beforeB);
    expect(snapshotA.hasLocalPlayer).toBe(true);
    expect(snapshotB.hasLocalPlayer).toBe(true);
  } finally {
    await contextA.close();
    await contextB.close();
  }
});
