const assert = require('node:assert/strict');
const { chromium } = require('@playwright/test');

const CLIENT_URL = process.env.ARENA_TV_CLIENT_URL
  || 'https://space.selfware.design/client.html?spectator=1&broadcast=1&auto_connect=1';
const SCREENSHOT_PATH = process.env.ARENA_TV_SCREENSHOT || '/tmp/arena-tv-client.png';

(async () => {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    const errors = [];
    const consoleErrors = [];
    page.on('pageerror', error => errors.push(error.stack || String(error)));
    page.on('console', message => {
      if (message.type() === 'error') consoleErrors.push(message.text());
    });
    await page.addInitScript(() => {
      window.__arenaTvMessages = [];
      window.__arenaTvProbeFrames = [];
      const parent = window.parent;
      const originalPostMessage = parent.postMessage;
      parent.postMessage = function (message, targetOrigin, transfer) {
        window.__arenaTvMessages.push({ message, targetOrigin });
        return originalPostMessage.call(this, message, targetOrigin, transfer);
      };
    });

    await page.goto(CLIENT_URL, { waitUntil: 'domcontentloaded', timeout: 30000 });
    await page.waitForFunction(() => window.__arenaTvMessages.some(
      entry => entry.message?.type === 'arena-tv' && entry.message?.status === 'live',
    ), null, { timeout: 30000 });

    await page.waitForFunction(() => window.__arenaTv?.director, null, { timeout: 5000 });
    await page.evaluate(() => {
      const director = window.__arenaTv.director;
      if (director.__arenaTvProbeWrapped) return;
      window.__arenaTvProbeCalls = [];
      const originalCameraFrame = director.cameraFrame;
      director.cameraFrame = function (...args) {
        const frame = originalCameraFrame.apply(this, args);
        const calls = window.__arenaTvProbeCalls;
        calls.push({
          playerCount: (args[0] || []).length,
          samplePlayer: args[0]?.[0] ? { id: args[0][0].id, teamId: args[0][0].teamId } : null,
          leaderId: args[1],
          frame,
        });
        if (calls.length > 10) calls.shift();
        if (frame) {
          const samples = window.__arenaTvProbeFrames;
          samples.push({ targetId: frame.targetId, focusX: frame.focusX, focusY: frame.focusY });
          if (samples.length > 100) samples.shift();
        }
        return frame;
      };
      director.__arenaTvProbeWrapped = true;
    });
    await page.waitForTimeout(3000);

    const result = await page.evaluate(() => {
      const playerOnlyChrome = [
        '#controlHintsOverlay', '#audioUnlockNudge', '#musicPlayer', '#tipsToast', '#joinProgressOverlay',
      ];
      return {
        overlayExists: !!document.querySelector('#arenaTvRoot .arena-tv-overlay'),
        scoreboardText: document.querySelector('#arenaTvRoot .arena-tv-top')?.textContent?.trim() || '',
        messages: window.__arenaTvMessages.map(entry => entry.message),
        frames: window.__arenaTvProbeFrames,
        calls: window.__arenaTvProbeCalls,
        renderFrames: window.__e2e?.renderFrames,
        playerCount: window.__e2e?.playerCount,
        chromeDisplays: Object.fromEntries(playerOnlyChrome.map(selector => [
          selector,
          document.querySelector(selector) ? getComputedStyle(document.querySelector(selector)).display : 'missing',
        ])),
        logText: document.getElementById('log')?.textContent?.slice(-5000) || '',
      };
    });
    const uniqueTargets = new Set(result.frames.map(frame => frame.targetId));
    console.log('arena-tv messages:', JSON.stringify(result.messages));
    console.log('overlay exists:', result.overlayExists);
    console.log('scoreboard text:', JSON.stringify(result.scoreboardText));
    console.log('director frame samples:', result.frames.length, 'unique targets:', [...uniqueTargets]);
    console.log('director call samples:', JSON.stringify(result.calls));
    console.log('render frames / players:', result.renderFrames, result.playerCount);
    console.log('player-only chrome displays:', JSON.stringify(result.chromeDisplays));
    if (!result.frames.length || errors.length) console.log('client log tail:', result.logText);
    console.log('page errors:', errors.length ? errors.join(' | ') : 'none');
    console.log('console errors:', consoleErrors.length ? consoleErrors.join(' | ') : 'none');

    await page.screenshot({ path: SCREENSHOT_PATH, fullPage: false });
    console.log('screenshot:', SCREENSHOT_PATH);
    assert.deepEqual(errors, [], 'broadcast client produced page errors');
    assert.equal(result.overlayExists, true, 'Arena TV overlay is missing');
    assert.ok(result.scoreboardText.length > 0, 'scoreboard text is empty');
    for (const [selector, display] of Object.entries(result.chromeDisplays)) {
      assert.equal(display, 'none', `${selector} must be hidden in broadcast mode`);
    }
    assert.ok(
      uniqueTargets.size > 1 || result.frames.some(frame => frame.targetId != null),
      'director never selected a camera target',
    );
  } finally {
    await browser.close();
  }
})().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
