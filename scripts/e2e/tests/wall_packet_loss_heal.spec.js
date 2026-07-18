const { test, expect } = require('@playwright/test');
const { devices } = require('@playwright/test');
const { registerServerLifecycle, resolveWsUrl, resolveBaseUrl } = require('./helpers/serverLifecycle');

registerServerLifecycle(test);

const LOSS_RATE = 0.25;
const BLACKOUT_RATE = 1.0;
const HEAL_TIMEOUT_MS = 20_000; // must stay well below the 30s wall respawn delay
const APPROACH_DIST_PX = 350;

/**
 * Init script that simulates inbound (server -> client) packet loss by
 * wrapping RTCDataChannel message delivery with a seeded PRNG drop.
 * window.__lossControl.rate is adjustable at runtime (1.0 = full blackout).
 */
function packetLossInitScript({ lossRate, seed }) {
  let s = seed >>> 0;
  const rand = () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
  window.__lossControl = { rate: lossRate };
  const origCreate = RTCPeerConnection.prototype.createDataChannel;
  RTCPeerConnection.prototype.createDataChannel = function (label, opts) {
    const dc = origCreate.call(this, label, opts);
    const desc = Object.getOwnPropertyDescriptor(
      Object.getPrototypeOf(dc),
      'onmessage',
    );
    let handler = null;
    Object.defineProperty(dc, 'onmessage', {
      configurable: true,
      get() {
        return handler;
      },
      set(fn) {
        handler = fn;
        desc.set.call(dc, (ev) => {
          if (rand() >= window.__lossControl.rate) fn.call(dc, ev);
        });
      },
    });
    return dc;
  };
}

async function joinPlayer(page) {
  await page.goto('/client.html');
  await page.waitForSelector('#wsUrl', { state: 'visible' });
  await page.fill('#wsUrl', resolveWsUrl());
  await page.evaluate(() => document.getElementById('connectButton').click());
  await page.waitForFunction(
    () => window.__e2e && window.__e2e.dataChannelOpen === true && window.__e2e.hasLocalPlayer === true,
    null,
    { timeout: 90_000 },
  );
}

async function joinSpectator(page) {
  await page.goto('/client.html?spectator=1');
  await page.waitForSelector('#wsUrl', { state: 'visible' });
  await page.fill('#wsUrl', resolveWsUrl());
  await page.evaluate(() => document.getElementById('connectButton').click());
  await page.waitForFunction(
    () => window.__e2e && window.__e2e.dataChannelOpen === true,
    null,
    { timeout: 90_000 },
  );
}

const readWalls = (page) => page.evaluate(() => window.__e2e.wallsSnapshot || []);
const readPlayer = (page) => page.evaluate(() => window.__e2e.localPlayerSnapshot);
const setLossRate = (page, rate) => page.evaluate((r) => { window.__lossControl.rate = r; }, rate);
const waitForWalls = (page) =>
  page.waitForFunction(() => window.__e2e && window.__e2e.wallCount > 0, null, { timeout: 45_000 });

test('wall state self-heals after total packet-loss blackout', async ({ browser }) => {
  // Oracle: spectator sees the whole map with no loss = ground truth.
  const oracleCtx = await browser.newContext({ ...devices['Desktop Chrome'], baseURL: resolveBaseUrl() });
  const oracle = await oracleCtx.newPage();

  // Lossy: mobile client with controllable seeded inbound loss.
  const lossyCtx = await browser.newContext({ ...devices['iPhone 13'], browserName: 'chromium', baseURL: resolveBaseUrl() });
  await lossyCtx.addInitScript(packetLossInitScript, { lossRate: LOSS_RATE, seed: 4242 });
  const lossy = await lossyCtx.newPage();

  try {
    await joinSpectator(oracle);
    await joinPlayer(lossy);
    await waitForWalls(oracle);
    await waitForWalls(lossy); // resync must overcome loss for the initial set

    // Pick the nearest destructible wall the oracle can also see.
    const player = await readPlayer(lossy);
    expect(player).toBeTruthy();
    const [oracleWalls, lossyWalls] = [await readWalls(oracle), await readWalls(lossy)];
    const oracleIds = new Set(oracleWalls.map((w) => w.id));
    const candidates = lossyWalls
      .filter((w) => w.destructible && w.health > 0 && oracleIds.has(w.id))
      .map((w) => ({
        ...w,
        cx: w.x + w.width / 2,
        cy: w.y + w.height / 2,
        dist: Math.hypot(w.x + w.width / 2 - player.x, w.y + w.height / 2 - player.y),
      }))
      .sort((a, b) => a.dist - b.dist);
    expect(candidates.length).toBeGreaterThan(0);
    const target = candidates[0];

    // Approach the wall until within firing range.
    const approachDeadline = Date.now() + 30_000;
    while (Date.now() < approachDeadline) {
      const p = await readPlayer(lossy);
      if (!p || !p.alive) { await lossy.waitForTimeout(400); continue; }
      const dx = target.cx - p.x;
      const dy = target.cy - p.y;
      const dist = Math.hypot(dx, dy);
      if (dist <= APPROACH_DIST_PX) break;
      await lossy.evaluate((r) => window.__e2e.setAimRotation(r), Math.atan2(dy, dx));
      await lossy.keyboard.down('w');
      await lossy.waitForTimeout(120);
    }
    await lossy.keyboard.up('w');

    // Fire at the wall until the oracle confirms damage (aim verified).
    const oracleWallHealth = async () => {
      const walls = await readWalls(oracle);
      const w = walls.find((x) => x.id === target.id);
      return w ? w.health : null;
    };

    const aimAndFire = async () => {
      const p = await readPlayer(lossy);
      if (!p) return;
      const rot = Math.atan2(target.cy - p.y, target.cx - p.x);
      await lossy.evaluate(
        ([r]) => {
          window.__e2e.setAimRotation(r);
          window.__e2e.forcePrimaryFire(true);
        },
        [rot],
      );
    };

    await aimAndFire();
    const damageDeadline = Date.now() + 45_000;
    let oracleHealth = await oracleWallHealth();
    expect(oracleHealth).toBeGreaterThan(0);
    while (oracleHealth > 30 && Date.now() < damageDeadline) {
      await aimAndFire(); // re-aim in case we drifted / respawned
      await lossy.keyboard.press('r'); // keep the magazine topped up
      await lossy.waitForTimeout(700);
      oracleHealth = await oracleWallHealth();
    }
    expect(
      oracleHealth,
      `could not damage wall ${target.id} (oracle health still ${oracleHealth})`,
    ).toBeLessThanOrEqual(30);

    // Blackout: drop EVERY inbound message on the lossy client. The destroy
    // notification that follows is guaranteed to be lost.
    await setLossRate(lossy, BLACKOUT_RATE);
    const destroyDeadline = Date.now() + 20_000;
    while (oracleHealth > 0 && Date.now() < destroyDeadline) {
      await aimAndFire();
      await lossy.keyboard.press('r');
      await lossy.waitForTimeout(400);
      oracleHealth = await oracleWallHealth();
    }
    await lossy.evaluate(() => window.__e2e.forcePrimaryFire(false));
    await setLossRate(lossy, LOSS_RATE);
    expect(oracleHealth, `wall ${target.id} was not destroyed during blackout`).toBe(0);

    // The wall is destroyed server-side and the lossy client provably missed
    // the one-shot notification. Without the server resync it would keep a
    // phantom wall forever; with it, state heals within a resync window.
    const healDeadline = Date.now() + HEAL_TIMEOUT_MS;
    let lossyHealth = null;
    while (Date.now() < healDeadline) {
      const walls = await readWalls(lossy);
      const w = walls.find((x) => x.id === target.id);
      lossyHealth = w ? w.health : 0; // wall dropped from the map also counts as gone
      if (lossyHealth === 0) break;
      await lossy.waitForTimeout(500);
    }
    expect(
      lossyHealth,
      `phantom wall ${target.id} never healed on lossy client (health stuck at ${lossyHealth})`,
    ).toBe(0);
  } finally {
    await oracleCtx.close();
    await lossyCtx.close();
  }
});
