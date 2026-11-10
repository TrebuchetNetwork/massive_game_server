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
      joinLive: (() => {
        const link = document.querySelector('[data-join-live]');
        return link ? {
          hidden: link.hidden,
          visible: !link.hidden && getComputedStyle(link).display !== 'none',
          href: link.getAttribute('href'),
        } : null;
      })(),
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
        joinHidden: document.querySelector('[data-join-live]')?.hidden ?? null,
        joinVisible: (() => {
          const link = document.querySelector('[data-join-live]');
          return !!link && !link.hidden && getComputedStyle(link).display !== 'none';
        })(),
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

    const stripPage = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    await stripPage.route('**/api/public/arena/overview', route => route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ ok: true, data: { active_models: 11781, pending_matches: 0, in_flight_matches: 0 } }),
    }));
    await stripPage.route('**/api/public/match/scoreboard', route => route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ ok: true, players: [{ alive: true }, { alive: false }, { alive: true }] }),
    }));
    await stripPage.route('**/api/public/arena/ratings', route => route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ ok: true, data: {
        active: true,
        roster: Array.from({ length: 40 }, (_, i) => ({ model_id: `test/model-${i}`, provider_model: `test/model-${i}` })),
      } }),
    }));
    await stripPage.route('**/models/league.json', route => route.fulfill({
      contentType: 'application/json', body: JSON.stringify({ day_index: 57 }),
    }));
    await stripPage.goto(`${BASE_URL}/index.html?tv=0`, { waitUntil: 'domcontentloaded', timeout: 30000 });
    await stripPage.waitForFunction(() => {
      const strip = document.querySelector('[data-live-strip]');
      return strip && !strip.hidden;
    }, null, { timeout: 10000 });
    const liveStrip = await stripPage.evaluate(() => {
      const items = [...document.querySelectorAll('[data-live-strip] .live-strip__item')];
      return {
        label: items[0]?.querySelector('b')?.textContent,
        models: items[1]?.querySelector('b')?.textContent,
        fighters: items[2]?.querySelector('b')?.textContent,
        fightersLabel: items[2]?.textContent,
        day: items[3]?.querySelector('b')?.textContent,
        liveClass: items[0]?.classList.contains('live-strip__item--live'),
        idleDot: items[0]?.querySelector('.live-strip__dot')?.classList.contains('live-strip__dot--idle'),
      };
    });

    const racePage = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    await racePage.addInitScript(() => {
      window.__liveStripRefresh = null;
      window.setInterval = (callback) => {
        window.__liveStripRefresh = callback;
        return 1;
      };
    });
    await racePage.route('**/api/public/arena/ratings', route => route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ ok: true, data: { active: true, roster: Array.from({ length: 40 }, () => ({})) } }),
    }));
    await racePage.route('**/models/league.json', route => route.fulfill({
      contentType: 'application/json', body: JSON.stringify({ day_index: 57 }),
    }));
    let racePoll = 0;
    await racePage.route('**/api/public/match/scoreboard', async route => {
      racePoll += 1;
      const thisPoll = racePoll;
      if (thisPoll === 2) await new Promise(resolve => setTimeout(resolve, 250));
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({ ok: true, players: thisPoll === 3 ? [] : [{ alive: true }] }),
      });
    });
    await racePage.goto(`${BASE_URL}/index.html?tv=0`, { waitUntil: 'domcontentloaded', timeout: 30000 });
    await racePage.waitForFunction(() => !document.querySelector('[data-live-strip]')?.hidden);
    await racePage.evaluate(async () => {
      const older = window.__liveStripRefresh();
      const newer = window.__liveStripRefresh();
      await Promise.all([older, newer]);
    });
    const racedStrip = await racePage.evaluate(() => {
      const items = [...document.querySelectorAll('[data-live-strip] .live-strip__item')];
      return {
        label: items[0]?.querySelector('b')?.textContent,
        fighters: items[2]?.querySelector('b')?.textContent,
        liveClass: items[0]?.classList.contains('live-strip__item--live'),
        idleDot: items[0]?.querySelector('.live-strip__dot')?.classList.contains('live-strip__dot--idle'),
      };
    });
    console.log('out-of-order live strip polls:', JSON.stringify(racedStrip));
    assert.deepEqual(racedStrip, {
      label: 'Standby', fighters: '0', liveClass: false, idleDot: true,
    }, 'an older poll must not overwrite newer fighter liveness');

    const stalledPage = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    await stalledPage.addInitScript(() => {
      window.__liveStripRefresh = null;
      window.setInterval = (callback) => {
        window.__liveStripRefresh = callback;
        return 1;
      };
    });
    await stalledPage.route('**/api/public/arena/ratings', route => route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ ok: true, data: { active: true, roster: Array.from({ length: 40 }, () => ({})) } }),
    }));
    await stalledPage.route('**/models/league.json', route => route.fulfill({
      contentType: 'application/json', body: JSON.stringify({ day_index: 57 }),
    }));
    await stalledPage.route('**/api/public/match/scoreboard', async () => {
      await new Promise(() => {});
    });
    await stalledPage.goto(`${BASE_URL}/index.html?tv=0`, { waitUntil: 'domcontentloaded', timeout: 30000 });
    await stalledPage.waitForTimeout(200);
    const stalledIntervalRegistered = await stalledPage.evaluate(() => typeof window.__liveStripRefresh === 'function');
    let stalledRecovery = null;
    if (stalledIntervalRegistered) {
      await stalledPage.unroute('**/api/public/match/scoreboard');
      await stalledPage.route('**/api/public/match/scoreboard', route => route.fulfill({
        contentType: 'application/json', body: JSON.stringify({ ok: true, players: [] }),
      }));
      await stalledPage.evaluate(() => window.__liveStripRefresh());
      stalledRecovery = await stalledPage.evaluate(() => ({
        hidden: document.querySelector('[data-live-strip]')?.hidden,
        label: document.querySelector('[data-live-strip] .live-strip__item b')?.textContent,
      }));
    }
    console.log('stalled initial poll recovery:', JSON.stringify({ stalledIntervalRegistered, stalledRecovery }));
    assert.equal(stalledIntervalRegistered, true, 'a stalled initial request must not prevent future polls');
    assert.deepEqual(stalledRecovery, { hidden: false, label: 'Standby' });

    console.log('default hero:', JSON.stringify(defaultResult));
    console.log('post-live fallback:', JSON.stringify(postLiveFallbackResult));
    console.log('default broadcast requests:', defaultState.tvRequests);
    console.log('default broadcast responses:', JSON.stringify(defaultState.tvResponses));
    console.log('default broadcast request failures:', JSON.stringify(defaultState.failedTvRequests));
    console.log('default WebSocket errors:', JSON.stringify(defaultState.webSocketErrors));
    console.log('opt-out hero:', JSON.stringify(optOutResult));
    console.log('opt-out broadcast requests:', optOutState.tvRequests);
    console.log('live strip with zero arena jobs:', JSON.stringify(liveStrip));
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
    assert.ok(defaultResult.joinLive, 'live Join CTA is missing');
    assert.equal(defaultResult.joinLive.href, '/client.html?match_type=mobile_blitz', 'unexpected live Join CTA href');
    assert.equal(defaultResult.joinLive.hidden, !defaultResult.live, 'live Join CTA hidden state does not match Arena TV state');
    assert.equal(defaultResult.joinLive.visible, defaultResult.live, 'live Join CTA visibility does not match Arena TV state');
    assert.ok(defaultResult.messages.length > 0, 'broadcast client never posted an arena-tv status');
    assert.ok(defaultResult.messages.every(message => message.originMatches && message.sourceMatches), 'arena-tv message failed origin/source validation');
    if (REQUIRE_LIVE) assert.equal(defaultResult.live, true, 'Arena TV was required to reach the live state');
    if (defaultResult.live) {
      assert.deepEqual(postLiveFallbackResult, { live: false, frames: 0, videos: 1, joinHidden: true, joinVisible: false });
    }
    assert.deepEqual(defaultState.errors, [], 'default landing page produced page errors');

    assert.equal(optOutResult.live, false, '?tv=0 must not enter the live state');
    assert.equal(optOutResult.fallback, true, '?tv=0 must retain highlight footage');
    assert.equal(optOutResult.frames, 0, '?tv=0 left an Arena TV iframe in the document');
    assert.equal(optOutResult.frameInserted, false, '?tv=0 inserted an Arena TV iframe');
    assert.equal(optOutState.tvRequests, 0, '?tv=0 requested the broadcast client');
    assert.ok(optOutResult.joinLive, '?tv=0 live Join CTA is missing');
    assert.equal(optOutResult.joinLive.href, '/client.html?match_type=mobile_blitz', 'unexpected ?tv=0 live Join CTA href');
    assert.equal(optOutResult.joinLive.hidden, true, '?tv=0 must hide the live Join CTA');
    assert.equal(optOutResult.joinLive.visible, false, '?tv=0 live Join CTA must not be visible');
    assert.deepEqual(optOutState.errors, [], '?tv=0 landing page produced page errors');

    assert.deepEqual(liveStrip, {
      label: 'Live',
      models: '40',
      fighters: '2',
      fightersLabel: '2 fighters live',
      day: '57',
      liveClass: true,
      idleDot: false,
    }, 'live strip must use alive scoreboard fighters and weekly roster size, not arena jobs');
  } finally {
    await browser.close();
  }
})().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
