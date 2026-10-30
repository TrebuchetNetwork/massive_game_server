const assert = require('node:assert/strict');
const { chromium } = require('@playwright/test');

const BASE_URL = process.env.ARENA_TV_BASE_URL || 'https://space.selfware.design';
const SCREENSHOT_PATH = process.env.ARENA_TV_SCREENSHOT || '/tmp/arena-tv-landing.png';

const REQUIRE_LIVE = process.env.ARENA_TV_REQUIRE_LIVE === '1';

const watchPage = (page) => {
  const state = {
    errors: [], consoleErrors: [], failedTvRequests: [], tvResponses: [], webSocketErrors: [], tvRequests: 0,
  };
  page.on('pageerror', error => state.errors.push(error.stack || String(error)));
  page.on('console', message => {
    if (message.type() === 'error') {
      state.consoleErrors.push({ text: message.text(), location: message.location() });
    }
  });
  page.on('websocket', socket => {
    socket.on('socketerror', error => state.webSocketErrors.push({ url: socket.url(), error }));
  });
  page.on('request', request => {
    const url = new URL(request.url());
    if (url.pathname === '/client.html' && url.searchParams.get('broadcast') === '1') {
      state.tvRequests += 1;
    }
  });
  page.on('response', response => {
    const url = new URL(response.url());
    if (url.pathname === '/client.html' && url.searchParams.get('broadcast') === '1') {
      state.tvResponses.push(response.status());
    }
  });
  page.on('requestfailed', request => {
    const url = new URL(request.url());
    if (url.pathname === '/client.html' && url.searchParams.get('broadcast') === '1') {
      state.failedTvRequests.push(request.failure()?.errorText || 'unknown request failure');
    }
  });
  return state;
};

const captureArenaTvMessages = page => page.addInitScript(() => {
  window.__arenaTvMessages = [];
  window.addEventListener('message', event => {
    if (event.data?.type !== 'arena-tv') return;
    const frame = document.querySelector('.arena-tv-frame');
    window.__arenaTvMessages.push({
      status: event.data.status,
      reason: event.data.reason,
      originMatches: event.origin === window.location.origin,
      sourceMatches: !!frame && event.source === frame.contentWindow,
    });
  });
});

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
      messages: window.__arenaTvMessages || [],
    };
  });
};

(async () => {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    const defaultState = watchPage(page);
    await captureArenaTvMessages(page);
    await page.goto(`${BASE_URL}/index.html`, { waitUntil: 'domcontentloaded', timeout: 30000 });
    const defaultResult = await waitForHero(page);
    await page.screenshot({ path: SCREENSHOT_PATH, fullPage: false });

    let postLiveFallbackResult = null;
    if (defaultResult.live) {
      await page.evaluate(() => {
        const frame = document.querySelector('.arena-tv-frame');
        const source = frame.contentWindow;
        for (let i = 0; i < 2; i += 1) {
          window.dispatchEvent(new MessageEvent('message', {
            data: { type: 'arena-tv', status: 'fallback', reason: 'stale' },
            origin: window.location.origin,
            source,
          }));
        }
      });
      await page.waitForFunction(() => {
        const field = document.querySelector('.arena-field');
        return !field.classList.contains('arena-field--tv-live')
          && !field.querySelector('iframe.arena-tv-frame')
          && !!field.querySelector('video.arena-field__video');
      }, null, { timeout: 5000 });
      postLiveFallbackResult = await page.evaluate(() => ({
        live: document.querySelector('.arena-field').classList.contains('arena-field--tv-live'),
        frames: document.querySelectorAll('iframe.arena-tv-frame').length,
        videos: document.querySelectorAll('.arena-field video.arena-field__video').length,
      }));
    }

    const optOutPage = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    const optOutState = watchPage(optOutPage);
    await captureArenaTvMessages(optOutPage);
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
    console.log('post-live fallback:', JSON.stringify(postLiveFallbackResult));
    console.log('default broadcast requests:', defaultState.tvRequests);
    console.log('default broadcast responses:', JSON.stringify(defaultState.tvResponses));
    console.log('default broadcast request failures:', JSON.stringify(defaultState.failedTvRequests));
    console.log('default WebSocket errors:', JSON.stringify(defaultState.webSocketErrors));
    console.log('opt-out hero:', JSON.stringify(optOutResult));
    console.log('opt-out broadcast requests:', optOutState.tvRequests);
    console.log('page errors:', JSON.stringify([...defaultState.errors, ...optOutState.errors]));
    console.log('console errors:', JSON.stringify([...defaultState.consoleErrors, ...optOutState.consoleErrors]));
    console.log('screenshot:', SCREENSHOT_PATH);

    assert.ok(defaultState.tvRequests > 0, 'Arena TV iframe was never requested');
    assert.ok(defaultState.tvResponses.includes(200), 'broadcast client did not return HTTP 200');
    assert.deepEqual(defaultState.failedTvRequests, [], 'broadcast client request failed');
    assert.deepEqual(defaultState.consoleErrors, [], 'default landing page produced console errors');
    assert.deepEqual(defaultState.webSocketErrors, [], 'broadcast client produced WebSocket errors');
    assert.notEqual(defaultResult.live, defaultResult.fallback, 'default hero must be exactly one of live or highlight fallback');
    assert.equal(defaultResult.frames, defaultResult.live ? 1 : 0, 'unexpected iframe count after hero settled');
    assert.ok(defaultResult.messages.length > 0, 'broadcast client never posted an arena-tv status');
    assert.ok(defaultResult.messages.every(message => message.originMatches && message.sourceMatches), 'arena-tv message failed origin/source validation');
    if (REQUIRE_LIVE) assert.equal(defaultResult.live, true, 'Arena TV was required to reach the live state');
    if (defaultResult.live) {
      assert.deepEqual(postLiveFallbackResult, { live: false, frames: 0, videos: 1 });
    }
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
