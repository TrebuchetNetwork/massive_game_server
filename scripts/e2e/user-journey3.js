const { chromium, devices } = require('@playwright/test');
(async () => {
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ ...devices['iPhone 13'], browserName: 'chromium' });
  const page = await ctx.newPage();
  await page.goto('https://space.selfware.design/client.html?match_type=mobile_blitz', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await page.waitForTimeout(4000);
  const probe = await page.evaluate(() => {
    const el = (id) => { const e = document.getElementById(id); if (!e) return null;
      const r = e.getBoundingClientRect(); const cs = getComputedStyle(e);
      return { visible: r.width > 0 && r.height > 0 && cs.display !== 'none' && cs.visibility !== 'hidden', rect: [r.x|0, r.y|0, r.width|0, r.height|0], text: (e.textContent||'').trim().slice(0,40) }; };
    return { hudMenuToggle: el('hudMenuToggle'), controlsPanel: el('controlsPanel'), controls: el('controls') };
  });
  console.log('probe:', JSON.stringify(probe, null, 1));
  const hud = await page.$('#hudMenuToggle');
  if (hud) {
    await hud.click().catch(e => console.log('hud click failed:', String(e).slice(0,120)));
    await page.waitForTimeout(1000);
    const after = await page.evaluate(() => ({
      btnVisible: !!document.getElementById('connectButton')?.offsetParent,
      panelHidden: document.getElementById('controlsPanel')?.classList.contains('is-hidden'),
    }));
    console.log('after hud toggle:', JSON.stringify(after));
  }
  await page.screenshot({ path: '/tmp/mobile-landing.png' });
  await browser.close();
})();
