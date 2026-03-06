const path = require('path');
const { test, expect } = require('@playwright/test');
const { prepareSmsCaptureFile, waitForSmsCode } = require('./helpers/authSms');
const { registerServerLifecycle, resolveBaseUrl } = require('./helpers/serverLifecycle');

const smsCapturePath = prepareSmsCaptureFile(
  path.resolve(__dirname, '..', 'test-results', 'auth-flow-otp.txt')
);
const smsCaptureScript = path.resolve(__dirname, '..', '..', 'test_support', 'capture_sms.sh');

registerServerLifecycle(test, {
  env: {
    MGS_SMS_COMMAND: smsCaptureScript,
    MGS_TEST_SMS_CAPTURE_PATH: smsCapturePath,
    MGS_AUTH_USE_COOKIES: '1',
    MGS_SMS_DEV_MODE: '0',
    MGS_TEST_DISABLE_OTP_IP_RATE_LIMIT: '1',
  },
});

test('otp auth session survives gameplay and logout revokes it', async ({ page }) => {
  const phoneNumber = '+15555550111';
  const expectedDisplayName = 'Player0111';
  const baseUrlObject = new URL(resolveBaseUrl());
  baseUrlObject.hostname = 'localhost';
  const baseUrl = baseUrlObject.toString().replace(/\/$/, '');
  const wsUrl = `${baseUrlObject.protocol === 'https:' ? 'wss:' : 'ws:'}//${baseUrlObject.host}/ws`;

  await page.addInitScript(() => {
    try {
      localStorage.setItem('mgs_player_name', 'CookieAuthE2E');
    } catch (_) {}
  });

  await page.goto(`${baseUrl}/client.html?disable_stun=1&match_type=quick`, {
    waitUntil: 'domcontentloaded',
  });
  await page.waitForSelector('#authPhoneInput', { state: 'visible' });

  await page.locator('#authPhoneInput').fill(phoneNumber);
  const requestCodeResult = await page.evaluate(async (phone) => {
    const response = await fetch('/auth/phone/request-code', {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ phone_number: phone }),
    });
    const payload = await response.json().catch(() => null);
    return {
      status: response.status,
      ok: payload?.ok === true,
      code: payload?.error?.code || null,
      message: payload?.error?.message || '',
    };
  }, phoneNumber);
  expect(requestCodeResult.status).toBe(200);
  expect(requestCodeResult.ok).toBeTruthy();
  const otpCode = await waitForSmsCode(smsCapturePath, 30000);
  await page.locator('#authCodeInput').fill(otpCode);
  await page.locator('#authVerifyCodeButton').click();

  await page.waitForFunction(() => {
    const signedIn = document.getElementById('authSignedIn');
    return signedIn && !signedIn.classList.contains('hidden');
  }, null, { timeout: 60000 });

  await expect(page.locator('#authDisplayName')).toHaveText(expectedDisplayName);

  const meBeforeConnect = await page.evaluate(async () => {
    const response = await fetch('/auth/me', {
      method: 'GET',
      credentials: 'same-origin',
    });
    const payload = await response.json().catch(() => null);
    return {
      status: response.status,
      ok: payload?.ok === true,
      displayName: payload?.data?.profile?.display_name || null,
    };
  });
  expect(meBeforeConnect.status).toBe(200);
  expect(meBeforeConnect.ok).toBeTruthy();
  expect(meBeforeConnect.displayName).toBe(expectedDisplayName);

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

  const connectionSnapshot = await page.evaluate(() => ({
    status: window.__e2e?.connectionStatus?.statusKey || null,
    activeUrl: window.__e2e?.activeSignalingUrl || '',
  }));
  expect(['waiting', 'playing', 'respawn']).toContain(connectionSnapshot.status);
  expect(connectionSnapshot.activeUrl.includes('/ws')).toBeTruthy();

  await page.locator('#authSignOutButton').click();
  await page.waitForFunction(() => {
    const signedOut = document.getElementById('authSignedOut');
    return signedOut && !signedOut.classList.contains('hidden');
  }, null, { timeout: 30000 });

  const meAfterLogout = await page.evaluate(async () => {
    const response = await fetch('/auth/me', {
      method: 'GET',
      credentials: 'same-origin',
    });
    const payload = await response.json().catch(() => null);
    return {
      status: response.status,
      code: payload?.error?.code || null,
    };
  });
  expect(meAfterLogout.status).toBe(401);
  expect(meAfterLogout.code).toBe('session_invalid');
});
