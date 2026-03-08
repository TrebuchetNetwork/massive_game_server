const { test, expect } = require('@playwright/test');

test.describe.configure({ timeout: 120000, retries: 1 });

test('@smoke client shell loads and exposes diagnostics hooks', async ({ page, request }) => {
  const healthResponse = await request.get('/healthz');
  expect(healthResponse.ok()).toBeTruthy();
  const healthPayload = await healthResponse.json();
  expect(healthPayload.ok).toBe(true);

  const readyResponse = await request.get('/readyz');
  expect(readyResponse.ok()).toBeTruthy();
  const readyPayload = await readyResponse.json();
  expect(readyPayload.ok).toBe(true);

  const response = await page.goto('/client.html?disable_stun=1&match_type=quick', {
    waitUntil: 'domcontentloaded',
  });
  if (!response || !response.ok()) {
    throw new Error(`Failed to load /client.html. Status: ${response ? response.status() : 'no response'}`);
  }

  await page.waitForSelector('#connectButton', { state: 'attached', timeout: 30000 });

  const snapshot = await page.evaluate(() => ({
    diagnosticsPresent: typeof window.__e2e === 'object' && window.__e2e !== null,
    matchTypeValue: document.querySelector('#matchTypeSelect')?.value || '',
    readyState: document.readyState,
    title: document.title,
  }));

  expect(snapshot.diagnosticsPresent).toBe(true);
  expect(snapshot.matchTypeValue.length).toBeGreaterThan(0);
  expect(snapshot.readyState).toBe('complete');
  expect(snapshot.title.length).toBeGreaterThan(0);
});
