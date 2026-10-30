const assert = require('node:assert/strict');
const { chromium } = require('@playwright/test');

const BASE_URL = process.env.ARENA_TV_BASE_URL || 'https://space.selfware.design';
const SCREENSHOT_PATH = process.env.ARENA_TV_SCREENSHOT || '/tmp/arena-tv-landing.png';

const watchPage = (page) => {
  const state = { errors: [], tvRequests: 0 };
  page.on('pageerror', error => state.errors.push(error.stack || String(error)));
  page.on('request', request => {
    const url = new URL(request.url());
    if (url.pathname === '/client.html' && url.searchParams.get('broadcast') === '1') {
      state.tvRequests += 1;
    }
  });
  return state;
};

const waitForHero = async (page) => {
  await page.waitForFunction(() => {
    const field = document.querySelector('.arena-field');
    return field?.classList.contains('arena-field--tv-live')
      || !!field?.querySelector('video.arena-field__video');
  }, null, { timeout: 20000 });
  await page.waitForTimeout(1000);
  return page.evaluate(() => {
    const field = document.querySelector('.arena-field');
    const rect = field?.getBoundingClientRect();
    return {
      live: !!field?.classList.contains('arena-field--tv-live'),
      fallback: !!field?.querySelector('video.arena-field__video'),
      frames: field?.querySelectorAll('iframe.arena-tv-frame').length || 0,
      strategyFeedDisplay: getComputedStyle(document.querySelector('.strategy-feed')).display,
      fieldHeight: rect?.height || 0,
    };
  });
};

(async () => {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    const defaultState = watchPage(page);
    await page.goto(`${BASE_URL}/index.html`, { waitUntil: 'domcontentloaded', timeout: 30000 });
    const defaultResult = await waitForHero(page);
    await page.screenshot({ path: SCREENSHOT_PATH, fullPage: false });

    const optOutPage = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    const optOutState = watchPage(optOutPage);
    await optOutPage.addInitScript(() => {
      window.__arenaTvFrameInserted = false;
      const observer = new MutationObserver(records => {
        for (const record of records) {
          for (const node of record.addedNodes) {
            if (node.nodeType !== Node.ELEMENT_NODE) continue;
            if (node.matches('iframe.arena-tv-frame') || node.querySelector('iframe.arena-tv-frame')) {
              window.__arenaTvFrameInserted = true;
            }
          }
        }
      });
      observer.observe(document, { childList: true, subtree: true });
    });
    await optOutPage.goto(`${BASE_URL}/index.html?tv=0`, { waitUntil: 'domcontentloaded', timeout: 30000 });
    const optOutResult = await waitForHero(optOutPage);
    optOutResult.frameInserted = await optOutPage.evaluate(() => window.__arenaTvFrameInserted);

    console.log('default hero:', JSON.stringify(defaultResult));
    console.log('default broadcast requests:', defaultState.tvRequests);
    console.log('opt-out hero:', JSON.stringify(optOutResult));
    console.log('opt-out broadcast requests:', optOutState.tvRequests);
    console.log('page errors:', JSON.stringify([...defaultState.errors, ...optOutState.errors]));
    console.log('screenshot:', SCREENSHOT_PATH);

    assert.ok(defaultState.tvRequests > 0, 'Arena TV iframe was never requested');
    assert.notEqual(defaultResult.live, defaultResult.fallback, 'default hero must be exactly one of live or highlight fallback');
    assert.equal(defaultResult.frames, defaultResult.live ? 1 : 0, 'unexpected iframe count after hero settled');
    assert.deepEqual(defaultState.errors, [], 'default landing page produced page errors');

    assert.equal(optOutResult.live, false, '?tv=0 must not enter the live state');
    assert.equal(optOutResult.fallback, true, '?tv=0 must retain highlight footage');
    assert.equal(optOutResult.frames, 0, '?tv=0 left an Arena TV iframe in the document');
    assert.equal(optOutResult.frameInserted, false, '?tv=0 inserted an Arena TV iframe');
    assert.equal(optOutState.tvRequests, 0, '?tv=0 requested the broadcast client');
    assert.deepEqual(optOutState.errors, [], '?tv=0 landing page produced page errors');
  } finally {
    await browser.close();
  }
})().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
