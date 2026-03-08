const { expect } = require('@playwright/test');
const { resolveBaseUrl, resolveWsUrl } = require('./serverLifecycle');

async function seedPlayerName(page, name = 'E2EPlayer') {
  await page.addInitScript((value) => {
    try {
      localStorage.setItem('mgs_player_name', value);
    } catch (_) {}
  }, String(name || 'E2EPlayer'));
}

async function dismissUsernameModalIfVisible(page) {
  const modalVisible = await page.evaluate(() => {
    const modal = document.getElementById('usernameModal');
    return !!modal && !modal.classList.contains('hidden');
  });
  if (!modalVisible) return;

  const skipButton = page.locator('#usernameSkipButton');
  if (await skipButton.count()) {
    await skipButton.click({ force: true });
    return;
  }
  const saveButton = page.locator('#usernameSaveButton');
  if (await saveButton.count()) {
    await saveButton.click({ force: true });
  }
}

async function ensureConnectionPanelVisible(page, timeout = 10000) {
  const connectButton = page.locator('#connectButton');
  if (await connectButton.isVisible().catch(() => false)) {
    return;
  }

  const hudMenuToggle = page.locator('#hudMenuToggle');
  if (await hudMenuToggle.count()) {
    const visible = await hudMenuToggle.isVisible().catch(() => false);
    if (visible) {
      await hudMenuToggle.click({ force: true });
      await connectButton.waitFor({ state: 'visible', timeout });
      return;
    }
  }

  await connectButton.waitFor({ state: 'attached', timeout });
}

async function connectClient(page, options = {}) {
  const {
    name = 'E2EPlayer',
    query = '/client.html?disable_stun=1&match_type=quick',
    matchType = 'quick',
    baseUrl = resolveBaseUrl(),
    wsUrl,
    requireLocalPlayer = true,
    timeout = 60000,
  } = options;
  const resolvedWsUrl = wsUrl || resolveWsUrl(baseUrl);
  const targetUrl = query.startsWith('http://') || query.startsWith('https://')
    ? query
    : new URL(query, baseUrl).toString();

  await seedPlayerName(page, name);
  const response = await page.goto(targetUrl, { waitUntil: 'domcontentloaded' });
  if (!response || !response.ok()) {
    throw new Error(
      `Failed to load ${targetUrl}. Status: ${response ? response.status() : 'no response'}`
    );
  }

  await dismissUsernameModalIfVisible(page);
  await ensureConnectionPanelVisible(page, timeout);

  const matchTypeSelect = page.locator('#matchTypeSelect');
  if (matchType && await matchTypeSelect.count()) {
    await matchTypeSelect.selectOption(matchType);
  }

  const wsInput = page.locator('#wsUrl');
  if (await wsInput.count()) {
    await wsInput.fill(resolvedWsUrl);
  }

  let lastError = null;
  for (let attempt = 0; attempt < 2; attempt++) {
    await page.click('#connectButton', { force: true });

    try {
      await page.waitForFunction(
        () => {
          const e2e = window.__e2e;
          if (!e2e) return false;
          const key = e2e.connectionStatus?.statusKey;
          const liveStatus = key === 'waiting' || key === 'playing' || key === 'respawn';
          const hasLiveState = Number(e2e.lastStateUpdate || 0) > 0;
          return e2e.dataChannelOpen === true && (e2e.matchInfoReady === true || liveStatus || hasLiveState);
        },
        null,
        { timeout }
      );

      if (requireLocalPlayer) {
        await page.waitForFunction(
          () => window.__e2e && window.__e2e.hasLocalPlayer === true,
          null,
          { timeout }
        );
      }

      await page.waitForFunction(
        () => {
          const key = window.__e2e?.connectionStatus?.statusKey;
          return key === 'waiting' || key === 'playing' || key === 'respawn';
        },
        null,
        { timeout }
      );

      return;
    } catch (error) {
      lastError = error;
      if (!page.isClosed()) {
        await page.waitForTimeout(2000).catch(() => {});
      }
    }
  }

  throw lastError || new Error('Unable to establish connected gameplay state');
}

async function waitForPlaying(page, timeout = 60000) {
  await page.waitForFunction(
    () => window.__e2e?.connectionStatus?.statusKey === 'playing',
    null,
    { timeout }
  );
}

async function getLocalPlayerPosition(page) {
  const position = await page.evaluate(() => {
    const snapshot = window.__e2e?.localPlayerSnapshot;
    if (snapshot) {
      return {
        x: Number(snapshot.x) || 0,
        y: Number(snapshot.y) || 0,
      };
    }
    return null;
  });
  expect(position).toBeTruthy();
  return position;
}

async function readAmmo(page) {
  return page
    .locator('#playerAmmo')
    .innerText()
    .then((value) => Number.parseInt(value, 10) || 0);
}

async function connectMultipleClients(browser, count, options = {}) {
  const clients = [];
  for (let index = 0; index < count; index += 1) {
    const context = await browser.newContext(options.contextOptions || {});
    const page = await context.newPage();
    await connectClient(page, {
      ...options,
      name: options.nameFactory ? options.nameFactory(index) : `E2EPlayer${index + 1}`,
    });
    clients.push({ context, page });
  }
  return clients;
}

module.exports = {
  connectMultipleClients,
  connectClient,
  dismissUsernameModalIfVisible,
  getLocalPlayerPosition,
  readAmmo,
  seedPlayerName,
  waitForPlaying,
};
