const { chromium, devices } = require('@playwright/test');
(async () => {
  const results = [];
  const check = (name, ok, detail = '') => {
    results.push({ name, ok });
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? ' — ' + detail : ''}`);
  };
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ ...devices['iPhone 13'], browserName: 'chromium' });
  const page = await ctx.newPage();
  const consoleErrors = [];
  page.on('pageerror', (e) => consoleErrors.push(String(e).slice(0, 200)));

  await page.goto('https://space.selfware.design/client.html?match_type=mobile_blitz', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await page.waitForTimeout(5000);
  const state0 = await page.evaluate(() => ({
    btnVisible: !!document.getElementById('connectButton')?.offsetParent,
    autoConnect: window.__e2e?.autoConnectOnLoad,
    dc: window.__e2e?.dataChannelOpen,
    lp: window.__e2e?.hasLocalPlayer,
    status: document.getElementById('connectionStatusDetail')?.textContent,
  }));
  console.log('state after 5s:', JSON.stringify(state0));
  check('auto-connect kicks in from match_type link', state0.autoConnect === true || state0.dc === true, JSON.stringify(state0));

  let spawned = false;
  try {
    await page.waitForFunction(() => window.__e2e?.hasLocalPlayer === true, null, { timeout: 45000 });
    spawned = true;
  } catch (_) {}
  check('player spawns into match', spawned, spawned ? '' : JSON.stringify(await page.evaluate(() => ({ dc: window.__e2e?.dataChannelOpen, status: document.getElementById('connectionStatusDetail')?.textContent }))));

  if (spawned) {
    const snap = async () => page.evaluate(() => ({
      walls: window.__e2e.wallCount, players: window.__e2e.playerCount,
      me: window.__e2e.localPlayerSnapshot, match: window.__e2e.matchInfoSnapshot,
      frames: window.__e2e.renderFrames, device: window.__e2e.deviceClassification,
    }));
    const s0 = await snap();
    check('walls render', s0.walls > 0, `walls=${s0.walls}`);
    check('bots present', s0.players > 1, `players=${s0.players}`);
    check('mobile profile', s0.device !== 'desktop', s0.device);
    const before = s0.me;
    await page.keyboard.down('w');
    await page.waitForTimeout(2500);
    await page.keyboard.up('w');
    const s1 = await snap();
    const moved = Math.hypot(s1.me.x - before.x, s1.me.y - before.y);
    check('player moves', moved > 20, `${moved.toFixed(0)}px`);
    await page.evaluate(() => window.__e2e.forcePrimaryFire(true));
    await page.waitForTimeout(5000);
    await page.evaluate(() => window.__e2e.forcePrimaryFire(false));
    const s2 = await snap();
    check('player shoots', s2.me.ammo < s1.me.ammo, `ammo ${s1.me.ammo} -> ${s2.me.ammo}`);
    check('render loop alive', s2.frames > s0.frames);
    const t0 = s0.match?.timeRemaining, t1 = s2.match?.timeRemaining;
    check('match timer counts down', typeof t0 === 'number' && t1 < t0, `${t0?.toFixed(1)} -> ${t1?.toFixed(1)}`);
    await page.screenshot({ path: '/tmp/user-journey-game.png', timeout: 10000 }).catch(() => {});
  }
  check('no fatal page errors', consoleErrors.length === 0, consoleErrors.slice(0, 2).join(' ; '));
  console.log(`\n${results.filter(r => r.ok).length}/${results.length} passed`);
  await browser.close();
})();
