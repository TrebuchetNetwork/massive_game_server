const { chromium } = require('@playwright/test');

(async () => {
  const browser = await chromium.launch();
  const ctx = await browser.newContext({
    viewport: { width: 1280, height: 720 },
    recordVideo: { dir: '/tmp/gameplay-video', size: { width: 1280, height: 720 } },
  });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => console.log('PAGEERROR:', String(e).slice(0, 150)));

  await page.goto('https://space.selfware.design/client.html?match_type=mobile_blitz', {
    waitUntil: 'domcontentloaded', timeout: 30000,
  });
  await page.waitForTimeout(2500);

  // Desktop UI: skip username modal if present, then Connect.
  const skip = page.locator('#usernameSkipButton');
  if (await skip.isVisible().catch(() => false)) await skip.click();
  await page.waitForSelector('#connectButton', { state: 'visible', timeout: 15000 });
  await page.evaluate(() => document.getElementById('connectButton').click());

  await page.waitForFunction(() => window.__e2e?.dataChannelOpen === true, null, { timeout: 45000 });
  await page.waitForFunction(() => window.__e2e?.hasLocalPlayer === true, null, { timeout: 30000 });
  console.log('joined and spawned');

  // Play: strafe runs with aiming and firing for ~70s.
  const moves = ['w', 'd', 's', 'a', 'w', 'a', 's', 'd'];
  for (let round = 0; round < 8; round++) {
    const key = moves[round];
    await page.keyboard.down(key);
    await page.mouse.move(300 + round * 90, 360);
    await page.mouse.down();
    await page.waitForTimeout(2200);
    await page.mouse.up();
    await page.keyboard.up(key);
    // ability spice every other round
    if (round % 2 === 1) { await page.keyboard.press('1'); }
    const snap = await page.evaluate(() => ({
      players: window.__e2e.playerCount,
      frames: window.__e2e.renderFrames,
      fps: Math.round(window.__e2e.renderFrames / ((performance.now() / 1000) || 1)),
    }));
    console.log(`round ${round}: players=${snap.players} frames=${snap.frames}`);
  }

  await page.screenshot({ path: '/tmp/gameplay-final.png' });
  await ctx.close(); // finalizes the video
  await browser.close();
  console.log('done');
})();
