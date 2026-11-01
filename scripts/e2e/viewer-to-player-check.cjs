const assert = require('node:assert/strict');
const { chromium } = require('@playwright/test');

const BASE_URL = (process.env.VIEWER_TO_PLAYER_BASE_URL || 'https://space.selfware.design').replace(/\/$/, '');
const MOBILE_SCREENSHOT = process.env.VIEWER_TO_PLAYER_MOBILE_SCREENSHOT || '/tmp/viewer-to-player-mobile.png';
const DESKTOP_SCREENSHOT = process.env.VIEWER_TO_PLAYER_DESKTOP_SCREENSHOT || '/tmp/viewer-to-player-desktop.png';
const STEP_TIMEOUT_MS = 20000;
const JOIN_TIMEOUT_MS = 45000;
const ATTEMPT_TIMEOUT_MS = 1200;

function watchPage(page) {
  const errors = [];
  page.on('pageerror', error => errors.push(error.stack || String(error)));
  return errors;
}

async function createPlayerPage(browser, name, contextOptions) {
  const context = await browser.newContext(contextOptions);
  const page = await context.newPage();
  const errors = watchPage(page);
  await page.addInitScript(playerName => {
    try {
      localStorage.setItem('mgs_player_name', playerName);
      localStorage.removeItem('mgs_join_onboarding_v1');
    } catch (_) {}
    window.__viewerToPlayerTransitions = [];
    let previousStep = null;
    const observe = () => {
      const e2e = window.__e2e;
      const step = e2e?.joinOnboarding?.step;
      if (step && step !== previousStep) {
        window.__viewerToPlayerTransitions.push({
          at: Math.round(performance.now()),
          step,
          x: e2e.localPlayerSnapshot?.x ?? null,
          y: e2e.localPlayerSnapshot?.y ?? null,
          alive: e2e.localPlayerSnapshot?.alive ?? null,
          status: e2e.connectionStatus?.statusKey || null,
        });
        previousStep = step;
      }
      requestAnimationFrame(observe);
    };
    requestAnimationFrame(observe);
  }, name);
  return { context, page, errors };
}

async function readState(page) {
  return page.evaluate(() => {
    const e2e = window.__e2e || {};
    const snapshot = e2e.localPlayerSnapshot;
    let rotation = null;
    const stack = window.app?.stage ? [window.app.stage] : [];
    while (stack.length) {
      const node = stack.pop();
      if (node?.localIndicator && node.playerId) {
        rotation = Number(node.rotation);
        break;
      }
      if (node?.children) stack.push(...node.children);
    }
    return {
      step: e2e.joinOnboarding?.step || 'missing',
      visible: e2e.joinOnboarding?.visible === true,
      owned: Number(e2e.localOwnedProjectileCount) || 0,
      hasLocal: e2e.hasLocalPlayer === true,
      alive: typeof snapshot?.alive === 'boolean' ? snapshot.alive : null,
      dataChannelOpen: e2e.dataChannelOpen === true,
      status: e2e.connectionStatus?.statusKey || 'unknown',
      rotation,
      aimActive: e2e.mobileAimActive === true,
      aimAssist: document.getElementById('mobileAimAssistToggle')?.getAttribute('aria-pressed') || null,
    };
  });
}

function remaining(deadline, label, state) {
  const timeout = deadline - Date.now();
  if (timeout <= 0) throw new Error(`${label} timed out; last state: ${JSON.stringify(state)}`);
  return timeout;
}

async function ensurePlayable(page, deadline, journey) {
  let state = await readState(page);
  if (state.status === 'error') {
    throw new Error(`${journey} connection failed during onboarding: ${JSON.stringify(state)}`);
  }
  if (state.hasLocal && state.alive !== false && state.dataChannelOpen) return state;

  console.log(`${journey} waiting for alive/respawn state: ${JSON.stringify(state)}`);
  await page.waitForFunction(() => {
    const e2e = window.__e2e;
    return e2e?.hasLocalPlayer === true
      && e2e?.localPlayerSnapshot?.alive !== false
      && e2e?.dataChannelOpen === true;
  }, null, { timeout: remaining(deadline, `${journey} respawn`, state) });
  state = await readState(page);
  console.log(`${journey} resumed after death/respawn: ${JSON.stringify(state)}`);
  return state;
}

async function waitForStepOrInterruption(page, targetStep, timeout) {
  try {
    await page.waitForFunction(target => {
      const e2e = window.__e2e;
      return e2e?.joinOnboarding?.step === target
        || e2e?.localPlayerSnapshot?.alive === false
        || e2e?.hasLocalPlayer === false;
    }, targetStep, { timeout });
  } catch (error) {
    if (error.name !== 'TimeoutError') throw error;
  }
}

async function waitForDoneOrInterruption(page, timeout) {
  try {
    await page.waitForFunction(() => {
      const e2e = window.__e2e;
      return (e2e?.joinOnboarding?.step === 'done' && Number(e2e?.localOwnedProjectileCount) > 0)
        || e2e?.localPlayerSnapshot?.alive === false
        || e2e?.hasLocalPlayer === false;
    }, null, { timeout });
  } catch (error) {
    if (error.name !== 'TimeoutError') throw error;
  }
}

function logTransition(journey, from, to, state) {
  assert.equal(state.step, to, `${journey} did not reach ${to}`);
  console.log(`${journey} transition ${from} -> ${to}: ${JSON.stringify(state)}`);
}

async function joinFromLiveLanding(page, mobile, journey) {
  await page.goto(`${BASE_URL}/index.html`, { waitUntil: 'domcontentloaded', timeout: 30000 });
  await page.waitForFunction(() => {
    const field = document.querySelector('.arena-field');
    const join = document.querySelector('[data-join-live]');
    return field?.classList.contains('arena-field--tv-live')
      && join && !join.hidden && getComputedStyle(join).display !== 'none';
  }, null, { timeout: 30000 });
  const join = page.locator('[data-join-live]');
  assert.equal(await join.getAttribute('href'), '/client.html?match_type=mobile_blitz');
  console.log(`${journey} live Arena TV Join CTA visible`);

  // Exercise the visible CTA with hit-tested input, then wait explicitly for
  // the client document rather than relying on navigation auto-wait.
  if (mobile) {
    await join.tap();
  } else {
    await join.click({ noWaitAfter: true });
  }
  await page.waitForURL(url => url.pathname === '/client.html'
    && url.searchParams.get('match_type') === 'mobile_blitz', { timeout: 15000 });
  await page.waitForFunction(() => !!window.__e2e && !!document.getElementById('connectButton'), null, { timeout: 30000 });

  if (mobile) {
    const enter = page.locator('#mobileArenaJoinButton');
    await enter.waitFor({ state: 'visible', timeout: 15000 });
    await enter.tap();
  } else {
    const connect = page.locator('#connectButton');
    await connect.waitFor({ state: 'visible', timeout: 15000 });
    await connect.click();
  }

  await page.waitForFunction(() => {
    const e2e = window.__e2e;
    return e2e?.hasLocalPlayer === true
      && e2e?.dataChannelOpen === true
      && !!e2e?.localPlayerSnapshot
      && ['move', 'aim', 'fire', 'done'].includes(e2e?.joinOnboarding?.step);
  }, null, { timeout: JOIN_TIMEOUT_MS });
  const state = await readState(page);
  const transitions = await page.evaluate(() => window.__viewerToPlayerTransitions);
  console.log(`${journey} joined as human: ${JSON.stringify(state)}`);
  console.log(`${journey} observed transitions: ${JSON.stringify(transitions)}`);
  assert.equal(state.step, 'move', `${journey} fresh onboarding must begin at Move`);
}

async function touchStart(cdp, point) {
  await cdp.send('Input.dispatchTouchEvent', {
    type: 'touchStart',
    touchPoints: [{ x: point.x, y: point.y, id: point.id }],
  });
}

async function touchMove(cdp, point) {
  await cdp.send('Input.dispatchTouchEvent', {
    type: 'touchMove',
    touchPoints: [{ x: point.x, y: point.y, id: point.id }],
  });
}

async function touchEnd(cdp) {
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
}

async function driveMobileMovement(page, cdp) {
  const deadline = Date.now() + STEP_TIMEOUT_MS;
  const directions = [
    { x: 0, y: -80 },
    { x: 80, y: 0 },
    { x: 0, y: 80 },
    { x: -80, y: 0 },
  ];
  for (let attempt = 0; ; attempt += 1) {
    let state = await ensurePlayable(page, deadline, 'mobile');
    if (attempt > 0 && state.step === 'aim') return logTransition('mobile', 'move', 'aim', state);
    assert.equal(state.step, 'move', 'mobile Move journey left its expected state');

    const box = await page.locator('#mobileMove').boundingBox();
    assert.ok(box, 'mobile movement joystick has no interaction box');
    const start = { x: box.x + box.width / 2, y: box.y + box.height / 2, id: 1 };
    const direction = directions[attempt % directions.length];
    const viewport = page.viewportSize();
    const end = {
      x: Math.max(4, Math.min(viewport.width - 4, start.x + direction.x)),
      y: Math.max(4, Math.min(viewport.height - 4, start.y + direction.y)),
      id: 1,
    };
    assert.ok(Math.hypot(end.x - start.x, end.y - start.y) >= 60, 'mobile joystick drag must be at least 60px');

    await touchStart(cdp, start);
    try {
      await touchMove(cdp, end);
      await waitForStepOrInterruption(page, 'aim', Math.min(
        ATTEMPT_TIMEOUT_MS,
        remaining(deadline, 'mobile Move', state),
      ));
    } finally {
      await touchEnd(cdp);
    }
    state = await readState(page);
    console.log(`mobile joystick input ${attempt + 1}: ${JSON.stringify(state)}`);
  }
}

async function driveMobileAim(page, cdp) {
  const deadline = Date.now() + STEP_TIMEOUT_MS;
  for (let attempt = 0; ; attempt += 1) {
    let state = await ensurePlayable(page, deadline, 'mobile');
    if (attempt > 0 && state.step === 'fire') return logTransition('mobile', 'aim', 'fire', state);
    assert.equal(state.step, 'aim', 'mobile Aim journey left its expected state');

    const box = await page.locator('#mobileAim').boundingBox();
    assert.ok(box, 'mobile aim area has no interaction box');
    const leftToRight = attempt % 2 === 0;
    const start = {
      x: box.x + box.width * (leftToRight ? 0.35 : 0.9),
      y: box.y + box.height * (leftToRight ? 0.3 : 0.48),
      id: 2,
    };
    const end = {
      x: box.x + box.width * (leftToRight ? 0.9 : 0.35),
      y: box.y + box.height * (leftToRight ? 0.48 : 0.3),
      id: 2,
    };
    const hitTarget = await page.evaluate(point => {
      const element = document.elementFromPoint(point.x, point.y);
      return {
        id: element?.id || null,
        className: element?.className || null,
        insideAim: !!element?.closest('#mobileAim'),
      };
    }, start);
    console.log(`mobile aim hit target ${attempt + 1}: ${JSON.stringify(hitTarget)}`);
    await touchStart(cdp, start);
    try {
      for (let index = 1; index <= 4; index += 1) {
        await touchMove(cdp, {
          x: start.x + ((end.x - start.x) * index / 4),
          y: start.y + ((end.y - start.y) * index / 4),
          id: 2,
        });
      }
      await waitForStepOrInterruption(page, 'fire', Math.min(
        ATTEMPT_TIMEOUT_MS,
        remaining(deadline, 'mobile Aim', state),
      ));
    } finally {
      await touchEnd(cdp);
    }
    state = await readState(page);
    console.log(`mobile aim drag ${attempt + 1}: ${JSON.stringify(state)}`);
  }
}

async function driveMobileFire(page, cdp) {
  const deadline = Date.now() + STEP_TIMEOUT_MS;
  for (let attempt = 0; ; attempt += 1) {
    let state = await ensurePlayable(page, deadline, 'mobile');
    if (attempt > 0 && state.step === 'done' && state.owned > 0) {
      return logTransition('mobile', 'fire', 'done', state);
    }
    assert.equal(state.step, 'fire', 'mobile Fire journey left its expected state');

    const box = await page.locator('#mobileFire').boundingBox();
    assert.ok(box, 'mobile Fire button has no interaction box');
    await touchStart(cdp, { x: box.x + box.width / 2, y: box.y + box.height / 2, id: 3 });
    try {
      await waitForDoneOrInterruption(page, remaining(deadline, 'mobile Fire', state));
    } finally {
      await touchEnd(cdp);
    }
    state = await readState(page);
    console.log(`mobile Fire hold ${attempt + 1}: ${JSON.stringify(state)}`);
  }
}

async function driveDesktopMovement(page) {
  const deadline = Date.now() + STEP_TIMEOUT_MS;
  const keys = ['KeyW', 'KeyD', 'KeyS', 'KeyA'];
  for (let attempt = 0; ; attempt += 1) {
    let state = await ensurePlayable(page, deadline, 'desktop');
    if (attempt > 0 && state.step === 'aim') return logTransition('desktop', 'move', 'aim', state);
    assert.equal(state.step, 'move', 'desktop Move journey left its expected state');

    const key = keys[attempt % keys.length];
    await page.keyboard.down(key);
    try {
      await waitForStepOrInterruption(page, 'aim', Math.min(
        ATTEMPT_TIMEOUT_MS,
        remaining(deadline, 'desktop Move', state),
      ));
    } finally {
      await page.keyboard.up(key);
    }
    state = await readState(page);
    console.log(`desktop ${key} movement ${attempt + 1}: ${JSON.stringify(state)}`);
  }
}

async function driveDesktopAim(page) {
  const deadline = Date.now() + STEP_TIMEOUT_MS;
  for (let attempt = 0; ; attempt += 1) {
    let state = await ensurePlayable(page, deadline, 'desktop');
    if (attempt > 0 && state.step === 'fire') return logTransition('desktop', 'aim', 'fire', state);
    assert.equal(state.step, 'aim', 'desktop Aim journey left its expected state');

    const box = await page.locator('#pixiContainer canvas').boundingBox();
    assert.ok(box, 'desktop game canvas has no interaction box');
    const leftToRight = attempt % 2 === 0;
    await page.mouse.move(
      box.x + box.width * (leftToRight ? 0.2 : 0.8),
      box.y + box.height * (leftToRight ? 0.4 : 0.6),
      { steps: 6 },
    );
    await page.mouse.move(
      box.x + box.width * (leftToRight ? 0.8 : 0.2),
      box.y + box.height * (leftToRight ? 0.6 : 0.4),
      { steps: 6 },
    );
    await waitForStepOrInterruption(page, 'fire', Math.min(
      ATTEMPT_TIMEOUT_MS,
      remaining(deadline, 'desktop Aim', state),
    ));
    state = await readState(page);
    console.log(`desktop mouse aim ${attempt + 1}: ${JSON.stringify(state)}`);
  }
}

async function driveDesktopFire(page) {
  const deadline = Date.now() + STEP_TIMEOUT_MS;
  for (let attempt = 0; ; attempt += 1) {
    let state = await ensurePlayable(page, deadline, 'desktop');
    if (attempt > 0 && state.step === 'done' && state.owned > 0) {
      return logTransition('desktop', 'fire', 'done', state);
    }
    assert.equal(state.step, 'fire', 'desktop Fire journey left its expected state');

    const box = await page.locator('#pixiContainer canvas').boundingBox();
    assert.ok(box, 'desktop game canvas has no interaction box');
    await page.mouse.move(box.x + box.width * 0.75, box.y + box.height * 0.5);
    await page.mouse.down({ button: 'left' });
    try {
      await waitForDoneOrInterruption(page, remaining(deadline, 'desktop Fire', state));
    } finally {
      await page.mouse.up({ button: 'left' });
    }
    state = await readState(page);
    console.log(`desktop canvas Fire hold ${attempt + 1}: ${JSON.stringify(state)}`);
  }
}

async function assertDoneAndCapture(page, journey, screenshot, errors) {
  const result = await page.evaluate(() => {
    const card = document.getElementById('joinOnboarding');
    return {
      step: window.__e2e?.joinOnboarding?.step,
      visible: window.__e2e?.joinOnboarding?.visible,
      owned: Number(window.__e2e?.localOwnedProjectileCount) || 0,
      cardDisplay: card ? getComputedStyle(card).display : 'missing',
      ariaHidden: card?.getAttribute('aria-hidden'),
      stored: localStorage.getItem('mgs_join_onboarding_v1'),
    };
  });
  assert.equal(result.step, 'done', `${journey} onboarding is not Done`);
  assert.ok(result.owned > 0, `${journey} produced no locally owned projectile`);
  assert.equal(result.visible, false, `${journey} Done probe still reports a visible card`);
  assert.equal(result.cardDisplay, 'none', `${journey} Done card is not hidden`);
  assert.equal(result.ariaHidden, 'true', `${journey} Done card is not aria-hidden`);
  assert.equal(result.stored, 'done', `${journey} Done was not persisted`);
  assert.deepEqual(errors, [], `${journey} produced page errors`);
  await page.screenshot({ path: screenshot, fullPage: false });
  console.log(`${journey} final state: ${JSON.stringify(result)}`);
  console.log(`${journey} screenshot: ${screenshot}`);
}

async function selectManualMobileFire(page) {
  // Auto-fire while aiming is a supported default, but this journey explicitly
  // validates the separate FIRE control. Disable it through the real settings UI.
  await page.locator('#hudMenuToggle').tap();
  await page.locator('#settingsButton').tap();
  const autoFire = page.locator('#mobileAutoFireAim');
  await autoFire.waitFor({ state: 'visible', timeout: 5000 });
  if (await autoFire.isChecked()) await autoFire.uncheck();
  await page.locator('#saveSettingsButton').tap();
  await page.locator('#settingsMenu').waitFor({ state: 'hidden', timeout: 5000 });
  await page.locator('#minimizeHudButton').tap();
  await page.locator('#controlsPanel').waitFor({ state: 'hidden', timeout: 5000 });
  const stored = await page.evaluate(() => JSON.parse(localStorage.getItem('gameSettings'))?.mobileAutoFireAim);
  assert.equal(stored, false, 'mobile auto-fire setting was not disabled');
  console.log('mobile manual FIRE selected through settings UI');
}

async function runMobileJourney(browser) {
  const { context, page, errors } = await createPlayerPage(browser, `V2PMobile${String(Date.now()).slice(-8)}`, {
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
    deviceScaleFactor: 1,
  });
  try {
    const cdp = await context.newCDPSession(page);
    await joinFromLiveLanding(page, true, 'mobile');
    await selectManualMobileFire(page);
    await driveMobileMovement(page, cdp);
    await driveMobileAim(page, cdp);
    await driveMobileFire(page, cdp);
    await assertDoneAndCapture(page, 'mobile', MOBILE_SCREENSHOT, errors);
  } finally {
    await context.close();
  }
}

async function runDesktopJourney(browser) {
  const { context, page, errors } = await createPlayerPage(browser, `V2PDesktop${String(Date.now()).slice(-8)}`, {
    viewport: { width: 1440, height: 900 },
  });
  try {
    await joinFromLiveLanding(page, false, 'desktop');
    await driveDesktopMovement(page);
    await driveDesktopAim(page);
    await driveDesktopFire(page);
    await assertDoneAndCapture(page, 'desktop', DESKTOP_SCREENSHOT, errors);
  } finally {
    await context.close();
  }
}

async function runBroadcastNegative(browser) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  const errors = watchPage(page);
  try {
    await page.addInitScript(() => {
      window.__viewerToPlayerNegative = { onboardingEverVisible: false };
      const observe = () => {
        const card = document.getElementById('joinOnboarding');
        if (window.__e2e?.joinOnboarding?.visible === true
          || card?.classList.contains('join-onboarding--visible')) {
          window.__viewerToPlayerNegative.onboardingEverVisible = true;
        }
        requestAnimationFrame(observe);
      };
      requestAnimationFrame(observe);
    });
    await page.goto(`${BASE_URL}/client.html?spectator=1&broadcast=1&auto_connect=1`, {
      waitUntil: 'domcontentloaded',
      timeout: 30000,
    });
    await page.waitForFunction(() => document.body.classList.contains('broadcast-mode')
      && Number(window.__e2e?.renderFrames) >= 20
      && Number(window.__e2e?.playerCount) > 0, null, { timeout: 30000 });
    const result = await page.evaluate(() => ({
      everVisible: window.__viewerToPlayerNegative.onboardingEverVisible,
      visible: window.__e2e?.joinOnboarding?.visible,
      step: window.__e2e?.joinOnboarding?.step,
      cardDisplay: getComputedStyle(document.getElementById('joinOnboarding')).display,
      players: window.__e2e?.playerCount,
      frames: window.__e2e?.renderFrames,
    }));
    assert.equal(result.everVisible, false, 'broadcast spectator showed onboarding during the journey');
    assert.equal(result.visible, false, 'broadcast spectator onboarding probe is visible');
    assert.equal(result.cardDisplay, 'none', 'broadcast spectator onboarding card is not hidden');
    assert.deepEqual(errors, [], 'broadcast negative path produced page errors');
    console.log(`negative broadcast spectator: ${JSON.stringify(result)}`);
  } finally {
    await context.close();
  }
}

async function runJoinFailureNegative(browser) {
  const { context, page, errors } = await createPlayerPage(browser, `V2PFailure${String(Date.now()).slice(-8)}`, {
    viewport: { width: 1280, height: 800 },
  });
  try {
    await page.goto(`${BASE_URL}/client.html?match_type=mobile_blitz`, {
      waitUntil: 'domcontentloaded',
      timeout: 30000,
    });
    const connect = page.locator('#connectButton');
    await connect.waitFor({ state: 'visible', timeout: 15000 });
    await page.locator('#wsUrl').fill('ftp://join-failure.invalid/ws');
    await connect.click();
    await page.waitForFunction(() => window.__e2e?.connectionStatus?.statusKey === 'error', null, { timeout: 10000 });
    const state = await readState(page);
    assert.equal(state.status, 'error', 'invalid signaling URL did not exercise the real join failure path');
    assert.notEqual(state.step, 'done', 'failed join reported onboarding Done');
    assert.equal(state.visible, false, 'failed join showed onboarding');
    assert.equal(state.hasLocal, false, 'failed join unexpectedly created a local player');
    assert.equal(state.owned, 0, 'failed join reported owned projectiles');
    assert.deepEqual(errors, [], 'join-failure negative path produced page errors');
    console.log(`negative join failure: ${JSON.stringify(state)}`);
  } finally {
    await context.close();
  }
}

(async () => {
  const browser = await chromium.launch();
  try {
    await runMobileJourney(browser);
    await runDesktopJourney(browser);
    await runBroadcastNegative(browser);
    await runJoinFailureNegative(browser);
    console.log('viewer-to-player production check: PASS');
  } finally {
    await browser.close();
  }
})().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
