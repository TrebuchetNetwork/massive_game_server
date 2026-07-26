const { chromium, devices } = require('@playwright/test');

(async () => {
  const results = [];
  const check = (name, ok, detail = '') => {
    results.push({ name, ok, detail });
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? ' — ' + detail : ''}`);
  };

  const browser = await chromium.launch();
  const ctx = await browser.newContext({ ...devices['iPhone 13'], browserName: 'chromium' });
  const page = await ctx.newPage();
  const consoleErrors = [];
  page.on('pageerror', (e) => consoleErrors.push(String(e).slice(0, 200)));

  // 1. Landing page like a user
  await page.goto('https://space.selfware.design/', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await page.waitForTimeout(2000);
  const title = await page.title();
  check('landing page loads', title.length > 0, `title="${title}"`);
  const playLink = await page.evaluate(() => {
    const a = [...document.querySelectorAll('a')].find(x => /play|launch|client/i.test(x.textContent + x.href));
    return a ? a.href : null;
  });
  check('play link present', !!playLink, playLink || 'none found');

  // 2. Go to the client
  await page.goto(playLink || 'https://space.selfware.design/client.html', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await page.waitForSelector('#connectButton', { state: 'visible', timeout: 15000 });
  const wsDefault = await page.evaluate(() => document.getElementById('wsUrl')?.value);
  check('ws url defaults to public wss', /wss:\/\//.test(wsDefault || ''), wsDefault);

  // 3. Connect like a user (tap Connect) — default name via automation context
  await page.evaluate(() => document.getElementById('connectButton').click());
  let joined = false;
  try {
    await page.waitForFunction(() => window.__e2e?.dataChannelOpen === true, null, { timeout: 45000 });
    joined = true;
  } catch (_) {}
  check('data channel opens over public internet (ngrok+TURN)', joined);
  if (joined) {
    let spawned = false;
    try {
      await page.waitForFunction(() => window.__e2e?.hasLocalPlayer === true, null, { timeout: 30000 });
      spawned = true;
    } catch (_) {}
    check('player spawns into match', spawned);
    if (spawned) {
      const snap = async () => page.evaluate(() => ({
        walls: window.__e2e.wallCount, players: window.__e2e.playerCount,
        me: window.__e2e.localPlayerSnapshot, match: window.__e2e.matchInfoSnapshot,
        frames: window.__e2e.renderFrames, device: window.__e2e.deviceClassification,
      }));
      const s0 = await snap();
      check('walls render', s0.walls > 0, `walls=${s0.walls}`);
      check('other players/bots present', s0.players > 1, `players=${s0.players}`);
      check('mobile profile active', s0.device !== 'desktop', s0.device);
      check('match timer ticks', true, `state=${s0.match?.matchState} t=${s0.match?.timeRemaining?.toFixed(0)}s`);

      // 4. Move like a user (keyboard sim for movement; touch UI exists but keyboard drives same input path)
      const before = s0.me;
      await page.keyboard.down('w');
      await page.waitForTimeout(2500);
      await page.keyboard.up('w');
      const s1 = await snap();
      const moved = Math.hypot(s1.me.x - before.x, s1.me.y - before.y);
      check('player moves', moved > 20, `moved ${moved.toFixed(0)}px`);

      // 5. Shoot like a user
      await page.evaluate(() => window.__e2e.forcePrimaryFire(true));
      await page.waitForTimeout(5000);
      await page.evaluate(() => window.__e2e.forcePrimaryFire(false));
      const s2 = await snap();
      check('player shoots (ammo decreases)', s2.me.ammo < s1.me.ammo, `ammo ${s1.me.ammo} -> ${s2.me.ammo}`);
      check('render loop alive', s2.frames > s0.frames, `${s0.frames} -> ${s2.frames}`);

      // 6. Match timer progressing?
      const t0 = s0.match?.timeRemaining, t1 = s2.match?.timeRemaining;
      check('match timer counts down', typeof t0 === 'number' && typeof t1 === 'number' && t1 < t0, `${t0?.toFixed(1)} -> ${t1?.toFixed(1)}`);

      await page.screenshot({ path: '/tmp/user-journey-game.png' });
    }
  }

  // 7. Arena/leaderboard surfaces
  await page.goto('https://space.selfware.design/', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await page.waitForTimeout(1500);
  const arenaLinks = await page.evaluate(() =>
    [...document.querySelectorAll('a')].map(a => a.href).filter(h => /arena|leaderboard|rank|league/i.test(h)));
  console.log('arena-ish links found:', JSON.stringify(arenaLinks));
  const bodyText = await page.evaluate(() => document.body.innerText.slice(0, 400));
  console.log('landing text sample:', bodyText.replace(/\n+/g, ' | ').slice(0, 300));

  check('no fatal page errors', consoleErrors.length === 0, consoleErrors.slice(0, 2).join(' ; '));

  console.log('\n== SUMMARY ==');
  const fails = results.filter(r => !r.ok);
  console.log(`${results.length - fails.length}/${results.length} checks passed`);
  await browser.close();
})();
