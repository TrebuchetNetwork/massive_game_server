const { test, expect } = require('@playwright/test');
const { resolveBaseUrl } = require('./helpers/serverLifecycle');
const { connectClient } = require('./helpers/gameClient');

test.describe.configure({ timeout: 120000, retries: 1 });

test('@synthetic public browser path loads over https', async ({ page, request }, testInfo) => {
  const baseUrl = resolveBaseUrl();
  test.skip(!baseUrl.startsWith('https://'), `${testInfo.title} requires an HTTPS base URL`);

  const landingResponse = await request.get(`${baseUrl}/index.html`);
  expect(landingResponse.ok()).toBeTruthy();
  const landingHeaders = landingResponse.headers();
  expect(landingHeaders['content-security-policy']).toContain("script-src 'self'");
  expect(landingHeaders['content-security-policy']).toContain("style-src 'self'");

  const htmlResponse = await request.get(`${baseUrl}/client.html`);
  expect(htmlResponse.ok()).toBeTruthy();
  const htmlHeaders = htmlResponse.headers();
  expect(htmlHeaders['strict-transport-security']).toContain('max-age=');
  expect(htmlHeaders['content-security-policy']).toContain("frame-ancestors 'none'");

  const healthResponse = await request.get(`${baseUrl}/healthz`);
  expect(healthResponse.ok()).toBeTruthy();
  const healthPayload = await healthResponse.json();
  expect(healthPayload.ok).toBe(true);

  const readyResponse = await request.get(`${baseUrl}/readyz`);
  expect(readyResponse.ok()).toBeTruthy();
  const readyPayload = await readyResponse.json();
  expect(readyPayload.ok).toBe(true);

  await page.goto(`${baseUrl}/index.html`, { waitUntil: 'domcontentloaded' });
  await expect(page.locator('h1')).toContainText('Space combat with shooter discipline');

  await page.goto(`${baseUrl}/client.html`, { waitUntil: 'domcontentloaded' });
  await page.waitForSelector('#connectButton', { state: 'attached', timeout: 30000 });

  const pageSnapshot = await page.evaluate(() => ({
    title: document.title,
    protocol: window.location.protocol,
    readyState: document.readyState,
  }));
  expect(pageSnapshot.title.length).toBeGreaterThan(0);
  expect(pageSnapshot.protocol).toBe('https:');
  expect(pageSnapshot.readyState).toBe('complete');
});

test('@synthetic optional deep guest connect stays browser-backed', async ({ page }, testInfo) => {
  const baseUrl = resolveBaseUrl();
  test.skip(!baseUrl.startsWith('https://'), `${testInfo.title} requires an HTTPS base URL`);
  test.skip(process.env.PUBLIC_SYNTH_DEEP_CONNECT !== '1', 'Deep public connect is opt-in');

  await connectClient(page, {
    name: 'PublicSynthetic',
    query: '/client.html?auto_reconnect=1&disable_stun=1&match_type=quick',
    matchType: 'quick',
    timeout: 90000,
    requireLocalPlayer: false,
  });

  const connectionSnapshot = await page.evaluate(() => ({
    activeSignalingUrl: window.__e2e?.activeSignalingUrl || '',
    dataChannelOpen: !!window.__e2e?.dataChannelOpen,
    lastStateUpdate: Number(window.__e2e?.lastStateUpdate || 0),
  }));

  expect(connectionSnapshot.activeSignalingUrl.startsWith('wss://')).toBeTruthy();
  expect(connectionSnapshot.dataChannelOpen).toBe(true);
  expect(connectionSnapshot.lastStateUpdate).toBeGreaterThan(0);
});
