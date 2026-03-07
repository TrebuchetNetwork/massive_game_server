const path = require('path');
const { test, expect } = require('@playwright/test');
const { prepareSmsCaptureFile, waitForSmsCode } = require('./helpers/authSms');
const { registerServerLifecycle } = require('./helpers/serverLifecycle');

const smsCapturePath = prepareSmsCaptureFile(
  path.resolve(__dirname, '..', 'test-results', 'auth-session-expiry-otp.txt')
);
const smsCaptureScript = path.resolve(__dirname, '..', '..', 'test_support', 'capture_sms.sh');
const authSessionPort = 20000 + Math.floor(Math.random() * 10000);
const authSessionBaseUrl = `http://127.0.0.1:${authSessionPort}`;

registerServerLifecycle(test, {
  baseUrl: authSessionBaseUrl,
  env: {
    MGS_SMS_COMMAND: smsCaptureScript,
    MGS_TEST_SMS_CAPTURE_PATH: smsCapturePath,
    MGS_AUTH_USE_COOKIES: '1',
    MGS_SMS_DEV_MODE: '0',
    MGS_TEST_DISABLE_OTP_IP_RATE_LIMIT: '1',
    MGS_AUTH_SESSION_TTL_SECONDS: '5',
    MGS_TEST_ALLOW_SHORT_SESSION_TTL: '1',
  },
});

test.describe.configure({ timeout: 180000, retries: 1 });

test('short-lived auth session expires during gameplay and protected APIs return 401', async ({ page }) => {
  const phoneNumber = '+15555550222';
  const baseUrlObject = new URL(authSessionBaseUrl);
  baseUrlObject.hostname = 'localhost';
  const baseUrl = baseUrlObject.toString().replace(/\/$/, '');
  const wsUrl = `${baseUrlObject.protocol === 'https:' ? 'wss:' : 'ws:'}//${baseUrlObject.host}/ws`;

  await page.addInitScript(() => {
    try {
      localStorage.setItem('mgs_player_name', 'ExpiryE2E');
    } catch (_) {}
  });

  await page.goto(`${baseUrl}/client.html?disable_stun=1&match_type=quick`, {
    waitUntil: 'domcontentloaded',
  });
  await page.waitForSelector('#authPhoneInput', { state: 'visible' });

  const requestCodeResult = await page.evaluate(async (phone) => {
    const response = await fetch('/auth/phone/request-code', {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ phone_number: phone }),
    });
    return { status: response.status };
  }, phoneNumber);
  expect(requestCodeResult.status).toBe(200);

  const otpCode = await waitForSmsCode(smsCapturePath, 30000);
  await page.locator('#authPhoneInput').fill(phoneNumber);
  await page.locator('#authCodeInput').fill(otpCode);
  await page.locator('#authVerifyCodeButton').click();

  await page.waitForFunction(() => {
    const signedIn = document.getElementById('authSignedIn');
    return signedIn && !signedIn.classList.contains('hidden');
  }, null, { timeout: 60000 });

  const matchTypeSelect = page.locator('#matchTypeSelect');
  if (await matchTypeSelect.count()) {
    await matchTypeSelect.selectOption('quick');
  }
  const wsInput = page.locator('#wsUrl');
  if (await wsInput.count()) {
    await wsInput.fill(wsUrl);
  }
  await page.click('#connectButton', { force: true });

  await page.waitForFunction(
    () => window.__e2e && window.__e2e.matchInfoReady === true,
    null,
    { timeout: 60000 }
  );
  await page.waitForFunction(
    () => window.__e2e && window.__e2e.dataChannelOpen === true,
    null,
    { timeout: 60000 }
  );

  await page.waitForTimeout(7000);

  const meAfterExpiry = await page.evaluate(async () => {
    const response = await fetch('/auth/me', {
      method: 'GET',
      credentials: 'same-origin',
    });
    const payload = await response.json().catch(() => null);
    return {
      status: response.status,
      code: payload?.error?.code || null,
      connectionStatus: window.__e2e?.connectionStatus?.statusKey || null,
    };
  });

  expect(meAfterExpiry.status).toBe(401);
  expect(meAfterExpiry.code).toBe('session_invalid');
  expect(['waiting', 'playing', 'respawn']).toContain(meAfterExpiry.connectionStatus);
});
