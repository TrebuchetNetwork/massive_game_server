const { chromium, devices } = require('@playwright/test');
(async () => {
  const results = [];
  const check = (name, ok, detail = '') => { results.push(ok); console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? ' — ' + detail : ''}`); };
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ ...devices['iPhone 13'], browserName: 'chromium' });
  const page = await ctx.newPage();
  const errs = [];
  page.on('pageerror', (e) => errs.push(String(e).slice(0, 150)));
  await page.goto('https://space.selfware.design/client.html?match_type=mobile_blitz', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await page.waitForTimeout(3000);

  // Find and tap ENTER ARENA like a user
  const enterBtn = await page.evaluate(() => {
    const els = [...document.querySelectorAll('button, a, [role=button]')];
    const b = els.find(e => /enter arena/i.test(e.textContent));
    return b ? (b.id || b.className.slice(0, 60)) : null;
  });
  console.log('enter button:', enterBtn);
  check('ENTER ARENA button found', !!enterBtn);
  if (enterBtn) {
    await page.evaluate(() => {
      const els = [...document.querySelectorAll('button, a, [role=button]')];
      els.find(e => /enter arena/i.test(e.textContent))?.click();
    });
    let spawned = false;
    try {
      await page.waitForFunction(() => window.__e2e?.hasLocalPlayer === true, null, { timeout: 60000 });
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
      check('player moves', Math.hypot(s1.me.x - before.x, s1.me.y - before.y) > 20, `${Math.hypot(s1.me.x - before.x, s1.me.y - before.y).toFixed(0)}px`);
      await page.evaluate(() => window.__e2e.forcePrimaryFire(true));
      await page.waitForTimeout(5000);
      await page.evaluate(() => window.__e2e.forcePrimaryFire(false));
      const s2 = await snap();
      check('player shoots', s2.me.ammo < s1.me.ammo, `ammo ${s1.me.ammo} -> ${s2.me.ammo}`);
      const t0 = s0.match?.timeRemaining, t1 = s2.match?.timeRemaining;
      check('match timer counts down', typeof t0 === 'number' && t1 < t0, `${t0?.toFixed(1)} -> ${t1?.toFixed(1)}`);
      check('render loop alive', s2.frames > s0.frames, `${s0.frames} -> ${s2.frames}`);
      await page.screenshot({ path: '/tmp/ingame.png', timeout: 10000 }).catch(() => {});
    }
  }
  check('no fatal page errors', errs.length === 0, errs.slice(0, 2).join(';'));
  console.log(`\n${results.filter(Boolean).length}/${results.length} passed`);
  await browser.close();
})();
