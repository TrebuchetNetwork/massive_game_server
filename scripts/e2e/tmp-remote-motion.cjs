const { chromium } = require('@playwright/test');
(async () => {
  const browser = await chromium.launch();
  const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
  await page.goto('https://space.selfware.design/client.html?match_type=mobile_blitz', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await page.waitForTimeout(2500);
  const skip = page.locator('#usernameSkipButton');
  if (await skip.isVisible().catch(() => false)) await skip.click();
  await page.waitForSelector('#connectButton', { state: 'visible', timeout: 15000 });
  await page.evaluate(() => document.getElementById('connectButton').click());
  await page.waitForFunction(() => window.__e2e?.hasLocalPlayer === true, null, { timeout: 45000 });
  // Sample other players' positions twice, 6s apart
  const others = async () => page.evaluate(() => {
    const players = window.__e2e.playersSnapshot || window.__e2e.players || [];
    return players;
  });
  const has = await page.evaluate(() => !!(window.__e2e.playersSnapshot || window.__e2e.players));
  console.log('players snapshot available:', has);
  if (has) {
    const a = await others();
    await page.waitForTimeout(6000);
    const b = await others();
    const moved = a.filter(p => {
      const q = b.find(x => (x.id ?? x.player_id) === (p.id ?? p.player_id));
      return q && Math.hypot((q.x - p.x), (q.y - p.y)) > 5;
    }).length;
    console.log(`remote players: ${a.length}; moved >5px in 6s: ${moved}`);
  } else {
    // fall back to state snapshot inspection
    const info = await page.evaluate(() => Object.keys(window.__e2e || {}));
    console.log('e2e keys:', info.slice(0, 40).join(','));
  }
  // also check kill feed advancing + match timer while sampling
  const kf0 = await page.evaluate(() => window.__e2e.killFeedEntries?.length ?? -1);
  await page.waitForTimeout(5000);
  const kf1 = await page.evaluate(() => window.__e2e.killFeedEntries?.length ?? -1);
  console.log('killfeed entries:', kf0, '->', kf1);
  await page.screenshot({ path: '/tmp/remote-motion.png' });
  await browser.close();
})();
