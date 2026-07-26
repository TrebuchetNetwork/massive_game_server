const { chromium, devices } = require('@playwright/test');
(async () => {
  const results = [];
  const check = (name, ok, detail = '') => { results.push(ok); console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? ' — ' + detail : ''}`); };
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ ...devices['iPhone 13'], browserName: 'chromium' });
  const page = await ctx.newPage();
  await page.goto('https://space.selfware.design/client.html?match_type=mobile_blitz', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await page.waitForTimeout(3000);
  await page.evaluate(() => { [...document.querySelectorAll('button, a, [role=button]')].find(e => /enter arena/i.test(e.textContent))?.click(); });
  await page.waitForFunction(() => window.__e2e?.hasLocalPlayer === true, null, { timeout: 60000 });

  const snap = async () => page.evaluate(() => ({
    me: window.__e2e.localPlayerSnapshot,
    mc: !!document.getElementById('mobileControls') && !document.getElementById('mobileControls').classList.contains('hidden'),
    fireBtn: (() => { const b = document.getElementById('mobileFire'); if (!b) return null; const r = b.getBoundingClientRect(); return [r.x + r.width/2, r.y + r.height/2, r.width, r.height]; })(),
    match: window.__e2e.matchInfoSnapshot,
  }));
  const s0 = await snap();
  console.log('mobile controls visible:', s0.mc, 'fire button at:', JSON.stringify(s0.fireBtn));
  check('mobile controls shown on phone', s0.mc);
  check('fire button present', !!s0.fireBtn);
  if (s0.fireBtn) {
    // touch and hold Fire like a user
    await page.touchscreen.tap(s0.fireBtn[0], s0.fireBtn[1]);
    // also hold via dispatched touchstart for sustained fire
    await page.evaluate(([x, y]) => {
      const b = document.getElementById('mobileFire');
      const t = new Touch({ identifier: 1, target: b, clientX: x, clientY: y });
      b.dispatchEvent(new TouchEvent('touchstart', { touches: [t], changedTouches: [t], bubbles: true, cancelable: true }));
    }, [s0.fireBtn[0], s0.fireBtn[1]]);
    await page.waitForTimeout(5000);
    const s1 = await snap();
    check('player shoots via fire button (ammo drops)', s1.me.ammo < s0.me.ammo, `ammo ${s0.me.ammo} -> ${s1.me.ammo}`);
  }
  // match timer
  const t0 = s0.match?.timeRemaining;
  await page.waitForTimeout(3000);
  const t1 = (await snap()).match?.timeRemaining;
  check('match timer counts down', typeof t0 === 'number' && t1 < t0, `${t0?.toFixed(1)} -> ${t1?.toFixed(1)}`);
  console.log(`\n${results.filter(Boolean).length}/${results.length} passed`);
  await browser.close();
})();
