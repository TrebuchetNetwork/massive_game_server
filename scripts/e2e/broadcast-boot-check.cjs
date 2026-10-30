const assert = require('node:assert/strict');
const { chromium } = require('@playwright/test');

(async () => {
  const baseUrl = process.env.ARENA_TV_BASE_URL || 'http://127.0.0.1:8080';
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.stack || String(error)));
    await page.addInitScript(() => {
      Object.defineProperty(navigator, 'webdriver', { get: () => false });
      window.__arenaTvMessages = [];
      const parent = window.parent;
      const originalPostMessage = parent.postMessage;
      parent.postMessage = function (message, targetOrigin, transfer) {
        window.__arenaTvMessages.push({ message, targetOrigin });
        return originalPostMessage.call(this, message, targetOrigin, transfer);
      };
    });

    await page.goto(
      `${baseUrl}/client.html?spectator=1&broadcast=1&auto_connect=1`,
      { waitUntil: 'domcontentloaded', timeout: 30000 },
    );
    await page.waitForTimeout(12000);

    const usernameModalVisible = await page.locator('#usernameModal').isVisible();
    const result = await page.evaluate(() => {
      const controls = document.getElementById('controls');
      const hadHiddenClass = controls.classList.contains('hidden');
      controls.classList.remove('hidden');
      const chatControlsDisplay = getComputedStyle(controls).display;
      controls.classList.toggle('hidden', hadHiddenClass);
      return {
        broadcastMode: document.body.classList.contains('broadcast-mode'),
        chatControlsDisplay,
        inputSendCalls: window.__e2e?.inputSendCalls || 0,
        messages: window.__arenaTvMessages,
      };
    });
    const arenaTvMessages = result.messages
      .filter(entry => entry.message?.type === 'arena-tv')
      .map(entry => ({ ...entry.message, targetOrigin: entry.targetOrigin }));

    console.log('broadcast body class:', result.broadcastMode);
    console.log('username modal visible:', usernameModalVisible);
    console.log('chat controls display:', result.chatControlsDisplay);
    console.log('input send calls:', result.inputSendCalls);
    console.log('page errors:', errors.length ? errors.join(' | ') : 'none');
    console.log('arena-tv messages:', JSON.stringify(arenaTvMessages));

    assert.equal(result.broadcastMode, true, 'broadcast mode body class is missing');
    assert.equal(usernameModalVisible, false, 'username modal must stay hidden');
    assert.equal(result.chatControlsDisplay, 'none', 'chat input controls must stay hidden');
    assert.equal(result.inputSendCalls, 0, 'render loop must not send inputs in broadcast mode');
    assert.deepEqual(errors, [], 'broadcast boot produced page errors');
    assert.ok(
      arenaTvMessages.some(message =>
        message.status === 'fallback' && message.reason === 'connect_timeout'),
      'watchdog did not report fallback/connect_timeout',
    );
  } finally {
    await browser.close();
  }
})().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
