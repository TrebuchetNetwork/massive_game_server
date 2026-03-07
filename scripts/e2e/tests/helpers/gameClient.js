const { expect } = require('@playwright/test');
const { resolveWsUrl } = require('./serverLifecycle');

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
    wsUrl = resolveWsUrl(),
    requireLocalPlayer = true,
    timeout = 60000,
  } = options;

  await seedPlayerName(page, name);
  const response = await page.goto(query, { waitUntil: 'domcontentloaded' });
  if (!response || !response.ok()) {
    throw new Error(
      `Failed to load ${query}. Status: ${response ? response.status() : 'no response'}`
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
    await wsInput.fill(wsUrl);
  }

  await page.click('#connectButton', { force: true });

  await page.waitForFunction(
    () => window.__e2e && window.__e2e.matchInfoReady === true,
    null,
    { timeout }
  );
  await page.waitForFunction(
    () => window.__e2e && window.__e2e.dataChannelOpen === true,
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

module.exports = {
  connectClient,
  dismissUsernameModalIfVisible,
  getLocalPlayerPosition,
  readAmmo,
  seedPlayerName,
  waitForPlaying,
};
