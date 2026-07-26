const { chromium, devices } = require('@playwright/test');
(async () => {
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ ...devices['iPhone 13'], browserName: 'chromium' });
  const page = await ctx.newPage();
  await page.goto('https://space.selfware.design/client.html?match_type=mobile_blitz', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await page.waitForTimeout(3000);
  await page.evaluate(() => { [...document.querySelectorAll('button, a, [role=button]')].find(e => /enter arena/i.test(e.textContent))?.click(); });
  await page.waitForFunction(() => window.__e2e?.hasLocalPlayer === true, null, { timeout: 60000 });
  for (let i = 0; i < 5; i++) {
    await page.waitForTimeout(3000);
    const s = await page.evaluate(() => {
      const hud = [...document.querySelectorAll('div,span')].filter(e => /\d+:\d{2}/.test(e.textContent) && e.children.length < 4 && e.textContent.length < 40).map(e => e.textContent.trim()).slice(0, 3);
      return { hud, e2e: window.__e2e.matchInfoSnapshot?.timeRemaining, state: window.__e2e.matchInfoSnapshot?.matchState, ready: window.__e2e.matchInfoReady };
    });
    console.log(`t=${(i+1)*3}s`, JSON.stringify(s));
  }
  await page.screenshot({ path: '/tmp/ingame2.png', timeout: 15000 }).catch(e => console.log('screenshot failed'));
  await browser.close();
})();
