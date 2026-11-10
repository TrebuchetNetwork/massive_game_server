const assert = require('node:assert/strict');
const { chromium } = require('@playwright/test');

const CLIENT_URL = process.env.ARENA_TV_CLIENT_URL
  || 'https://space.selfware.design/client.html?spectator=1&broadcast=1&auto_connect=1';
const RUN_RECONNECT_CHECK = process.env.ARENA_TV_RECONNECT_CHECK === '1'
  || ['localhost', '127.0.0.1'].includes(new URL(CLIENT_URL).hostname);
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
    let delayNextScoreboard = false;
    let delayedScoreboardRoute = null;
    let notifyScoreboardDelayed;
    const scoreboardDelayed = new Promise(resolve => { notifyScoreboardDelayed = resolve; });
    let releaseDelayedScoreboard;
    const delayedScoreboardRelease = new Promise(resolve => { releaseDelayedScoreboard = resolve; });
    let notifyStaleScoreboardFulfilled;
    const staleScoreboardFulfilled = new Promise(resolve => { notifyStaleScoreboardFulfilled = resolve; });
    await page.route('**/api/public/match/scoreboard', async route => {
      if (!delayNextScoreboard) {
        await route.continue();
        return;
      }
      if (delayedScoreboardRoute) {
        await route.abort();
        return;
      }
      delayedScoreboardRoute = route;
      notifyScoreboardDelayed();
      await delayedScoreboardRelease;
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          team_scores: [{ team_id: 1, score: 0 }, { team_id: 2, score: 999999 }],
          time_remaining: 50,
        }),
      });
      notifyStaleScoreboardFulfilled();
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
      window.__arenaTvProbeCallCount = 0;
      const originalCameraFrame = director.cameraFrame;
      director.cameraFrame = function (...args) {
        const frame = originalCameraFrame.apply(this, args);
        window.__arenaTvProbeCallCount += 1;
        const calls = window.__arenaTvProbeCalls;
        const target = (args[0] || []).find((player) => player.id === frame?.targetId);
        calls.push({
          playerCount: (args[0] || []).length,
          leaderId: args[1],
          target: target ? {
            id: target.id,
            username: target.username,
            teamId: target.teamId,
            hp: target.hp,
            maxHp: target.maxHp,
          } : null,
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
        '#controlHintsOverlay', '#joinOnboarding', '#audioUnlockNudge', '#musicPlayer', '#tipsToast', '#joinProgressOverlay',
        '#hudMenuToggle', '#mobileCombatHud', '#mobileCombatHud .mobile-combat-hud__stats',
        '#mobileCombatHud .mobile-combat-hud__objective', '#connectionStatus',
        '#combatHud', '#combatRadialHud', '#combatMomentum', '#deathMomentBanner', '#deathRecap',
        '#killcamPanel', '#postMatchPanel', '#pingWheel', '#boundaryWarning', '#fpsCounter',
        '#arenaEvolutionChip', '.arena-brand-chip', '.arena-watermark', '#matchInfo', '#minimapContainer',
        '#combatOverlay', '#gameModeIntro', '#objectiveUrgency',
      ];
      return {
        overlayExists: !!document.querySelector('#arenaTvRoot .arena-tv-overlay'),
        scoreboardText: document.querySelector('#arenaTvRoot .arena-tv-top')?.textContent?.trim() || '',
        messages: window.__arenaTvMessages.map(entry => entry.message),
        frames: window.__arenaTvProbeFrames,
        calls: window.__arenaTvProbeCalls,
        focus: {
          hidden: document.querySelector('.arena-tv-focus')?.hidden,
          name: document.querySelector('.arena-tv-focus-name')?.textContent || '',
          meta: document.querySelector('.arena-tv-focus-meta')?.textContent || '',
        },
        renderFrames: window.__e2e?.renderFrames,
        playerCount: window.__e2e?.playerCount,
        autoReconnectEnabled: window.__e2e?.autoReconnectEnabled,
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
    assert.equal(result.autoReconnectEnabled, true, 'broadcast auto-reconnect must stay enabled');
    assert.ok(result.scoreboardText.length > 0, 'scoreboard text is empty');
    for (const [selector, display] of Object.entries(result.chromeDisplays)) {
      assert.equal(display, 'none', `${selector} must be hidden in broadcast mode`);
    }
    assert.ok(
      uniqueTargets.size > 1 || result.frames.some(frame => frame.targetId != null),
      'director never selected a camera target',
    );
    const latest = result.calls[result.calls.length - 1];
    assert.ok(latest, 'director never produced a camera call');
    if (!latest.frame) {
      assert.equal(result.focus.hidden, true, 'fighter spotlight is visible without a camera target');
    } else {
      assert.ok(latest.target, 'director never exposed its selected candidate');
      assert.equal(result.focus.hidden, false, 'fighter spotlight is hidden during a live target');
      assert.equal(result.focus.name, latest.target.username || latest.target.id);
      assert.ok(result.focus.meta.includes(`TEAM ${latest.target.teamId}`));
      if (Number.isFinite(latest.target.hp) && Number.isFinite(latest.target.maxHp) && latest.target.maxHp > 0) {
        const hp = Math.ceil(Math.max(0, latest.target.hp));
        assert.ok(result.focus.meta.includes(`HP ${hp}/${Math.ceil(latest.target.maxHp)}`));
      }
    }

    console.log('reconnect check enabled:', RUN_RECONNECT_CHECK);
    if (RUN_RECONNECT_CHECK) {
      delayNextScoreboard = true;
      await scoreboardDelayed;
      const reconnectResult = await page.evaluate(() => {
        const markers = {
          messageCount: window.__arenaTvMessages.length,
          cameraCallCount: window.__arenaTvProbeCallCount,
        };
        window.__e2e.forceReconnectNow();
        return {
          markers,
          resetState: {
            hidden: document.querySelector('.arena-tv-focus').hidden,
            name: document.querySelector('.arena-tv-focus-name').textContent,
            commentary: document.querySelector('.arena-tv-commentary').textContent,
            feed: document.querySelector('.arena-tv-killfeed').textContent,
            scores: document.querySelector('.arena-tv-scores').textContent,
            clock: document.querySelector('.arena-tv-clock').textContent,
          },
        };
      });
      const { markers: reconnectMarkers, resetState } = reconnectResult;
      console.log('reconnect reset state:', JSON.stringify(resetState));
      assert.deepEqual(resetState, {
        hidden: true,
        name: '',
        commentary: '',
        feed: '',
        scores: '',
        clock: '--:--',
      });

      releaseDelayedScoreboard();
      await staleScoreboardFulfilled;
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      const staleState = await page.evaluate(() => ({
        scores: document.querySelector('.arena-tv-scores').textContent,
        clock: document.querySelector('.arena-tv-clock').textContent,
        commentary: document.querySelector('.arena-tv-commentary').textContent,
      }));
      console.log('post-reconnect stale response state:', JSON.stringify(staleState));
      assert.deepEqual(staleState, { scores: '', clock: '--:--', commentary: '' });

      await page.unroute('**/api/public/match/scoreboard');
      const readRecoveryState = () => page.evaluate((markers) => {
        const calls = window.__arenaTvProbeCalls || [];
        const latestCall = calls[calls.length - 1];
        const messages = window.__arenaTvMessages.slice(markers.messageCount);
        const newLive = messages.some(
          entry => entry.message?.type === 'arena-tv' && entry.message?.status === 'live',
        );
        const freshTarget = window.__arenaTvProbeCallCount > markers.cameraCallCount
          && !!latestCall?.frame && !!latestCall.target;
        const focus = document.querySelector('.arena-tv-focus');
        const focusName = document.querySelector('.arena-tv-focus-name')?.textContent || '';
        const focusMatchesTarget = freshTarget && focus?.hidden === false
          && focusName === (latestCall.target.username || latestCall.target.id);
        return {
          dataChannelOpen: window.__e2e?.dataChannelOpen === true,
          newLive,
          freshTarget,
          focusMatchesTarget,
          focusName,
          cameraCallCount: window.__arenaTvProbeCallCount,
          messages: messages.map(entry => entry.message),
          connectionStatus: window.__e2e?.connectionStatus,
          logText: document.getElementById('log')?.textContent?.slice(-2000) || '',
        };
      }, reconnectMarkers);
      try {
        await page.waitForFunction((markers) => {
          const calls = window.__arenaTvProbeCalls || [];
          const latestCall = calls[calls.length - 1];
          const newLive = window.__arenaTvMessages.slice(markers.messageCount).some(
            entry => entry.message?.type === 'arena-tv' && entry.message?.status === 'live',
          );
          const freshTarget = window.__arenaTvProbeCallCount > markers.cameraCallCount
            && !!latestCall?.frame && !!latestCall.target;
          const focus = document.querySelector('.arena-tv-focus');
          const focusName = document.querySelector('.arena-tv-focus-name')?.textContent || '';
          const focusMatchesTarget = freshTarget && focus?.hidden === false
            && focusName === (latestCall.target.username || latestCall.target.id);
          return window.__e2e?.dataChannelOpen === true && newLive && focusMatchesTarget;
        }, reconnectMarkers, { timeout: 30000 });
      } catch (error) {
        console.error('reconnect recovery failure state:', JSON.stringify(await readRecoveryState()));
        throw error;
      }
      const recoveryState = await readRecoveryState();
      console.log('reconnect recovery state:', JSON.stringify(recoveryState));
      assert.equal(recoveryState.dataChannelOpen, true, 'data channel did not recover');
      assert.equal(recoveryState.newLive, true, 'no new live status arrived after reconnect');
      assert.equal(recoveryState.freshTarget, true, 'director did not select a fresh target');
      assert.equal(recoveryState.focusMatchesTarget, true, 'fresh target is not visible in focus');
      assert.deepEqual(errors, [], 'broadcast reconnect produced page errors');
    }
  } finally {
    await browser.close();
  }
})().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
