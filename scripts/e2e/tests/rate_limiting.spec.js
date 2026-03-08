const { test, expect } = require('@playwright/test');
const { registerServerLifecycle } = require('./helpers/serverLifecycle');

registerServerLifecycle(test, {
  env: {
    MGS_SMS_DEV_MODE: '1',
    MGS_TARGET_BOT_COUNT: '0',
  },
});

test.describe.configure({ timeout: 180000, retries: 1 });

test('@critical browser-side OTP and body-size limits surface correct HTTP statuses', async ({ page }) => {
  await page.goto('/client.html?disable_stun=1&match_type=quick', {
    waitUntil: 'domcontentloaded',
  });

  const requestStatuses = await page.evaluate(async () => {
    const statuses = [];
    for (let index = 0; index < 6; index += 1) {
      const response = await fetch('/auth/phone/request-code', {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          phone_number: `+1555555${String(1000 + index).padStart(4, '0')}`,
        }),
      });
      statuses.push(response.status);
    }
    return statuses;
  });

  expect(requestStatuses).toHaveLength(6);
  for (const status of requestStatuses) {
    expect([200, 429]).toContain(status);
  }
  expect(requestStatuses).toContain(429);

  const oversizedStatus = await page.evaluate(async () => {
    const response = await fetch('/auth/phone/request-code', {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        phone_number: `+1${'9'.repeat(70 * 1024)}`,
      }),
    });
    return response.status;
  });

  expect(oversizedStatus).toBe(413);
});
