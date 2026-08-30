const { chromium, devices } = require('@playwright/test');
const fs = require('fs');

const OUT = '/home/habitat/site-screenshots-2026-08-28';
const BASE = 'https://space.selfware.design';

async function main() {
  fs.mkdirSync(OUT, { recursive: true });
  const browser = await chromium.launch();
  const shots = [];

  async function shot(page, name, { fullPage = false, wait = 1500 } = {}) {
    await page.waitForTimeout(wait);
    await page.screenshot({ path: `${OUT}/${name}.png`, fullPage });
    shots.push(name);
    console.log('shot:', name);
  }

  // ---------- Desktop ----------
  const dt = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const d = await dt.newPage();

  // 1. Landing hero (video playing)
  await d.goto(BASE + '/', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await d.waitForTimeout(4000);
  await shot(d, '01-landing-hero-desktop', { wait: 1000 });
  // 2. Landing full page
  await shot(d, '02-landing-full-desktop', { fullPage: true });

  // 3. Models index: chronicle (top)
  await d.goto(BASE + '/models/', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await d.waitForTimeout(2500);
  await shot(d, '03-models-chronicle-desktop');
  // 4. Models index full (toplist, matrix, standings, feed, HoF)
  await shot(d, '04-models-full-desktop', { fullPage: true });

  // 5. Top model page
  await d.goto(BASE + '/models/claude-opus-5.html', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await shot(d, '05-model-page-opus5-desktop', { fullPage: true, wait: 2500 });

  // 6. Game client: join + in-game
  await d.goto(BASE + '/client.html?match_type=mobile_blitz', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await d.waitForTimeout(2500);
  await shot(d, '06-game-join-panel-desktop');
  const skip = d.locator('#usernameSkipButton');
  if (await skip.isVisible().catch(() => false)) await skip.click();
  await d.waitForSelector('#connectButton', { state: 'visible', timeout: 15000 }).catch(() => {});
  await d.evaluate(() => document.getElementById('connectButton')?.click()).catch(() => {});
  // capture progress overlay mid-join
  await d.waitForTimeout(2500);
  await shot(d, '07-game-join-progress-desktop', { wait: 500 });
  try {
    await d.waitForFunction(() => window.__e2e?.hasLocalPlayer === true, null, { timeout: 45000 });
  } catch (e) { console.log('spawn wait timeout'); }
  await shot(d, '08-game-hints-desktop', { wait: 1500 });
  // play a little
  await d.keyboard.down('w');
  await d.mouse.move(800, 400);
  await d.mouse.down();
  await d.waitForTimeout(2500);
  await d.mouse.up();
  await d.keyboard.up('w');
  await shot(d, '09-game-ingame-desktop', { wait: 1000 });
  await dt.close();

  // ---------- Mobile (iPhone 13) ----------
  const mb = await browser.newContext({ ...devices['iPhone 13'] });
  const m = await mb.newPage();
  await m.goto(BASE + '/', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await m.waitForTimeout(3500);
  await shot(m, '10-landing-hero-mobile');
  await shot(m, '11-landing-full-mobile', { fullPage: true });

  await m.goto(BASE + '/models/', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await m.waitForTimeout(2500);
  await shot(m, '12-models-chronicle-mobile');
  await shot(m, '13-models-full-mobile', { fullPage: true });

  await m.goto(BASE + '/client.html?match_type=mobile_blitz', { waitUntil: 'domcontentloaded', timeout: 30000 });
  await m.waitForTimeout(2500);
  await shot(m, '14-game-gate-mobile');
  const join = m.locator('#mobileArenaJoinButton');
  if (await join.isVisible().catch(() => false)) await join.tap();
  try {
    await m.waitForFunction(() => window.__e2e?.dataChannelOpen === true, null, { timeout: 45000 });
    await m.waitForFunction(() => window.__e2e?.hasLocalPlayer === true, null, { timeout: 30000 });
  } catch (e) { console.log('mobile spawn wait timeout'); }
  await shot(m, '15-game-hints-mobile', { wait: 1500 });
  await m.waitForTimeout(6500); // let hints auto-dismiss
  await shot(m, '16-game-ingame-mobile', { wait: 500 });
  await mb.close();

  await browser.close();
  console.log('ALL DONE:', shots.length, 'screenshots in', OUT);
}

main().catch((e) => { console.error('FAILED:', e.message); process.exit(1); });
