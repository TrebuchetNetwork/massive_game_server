const { test, expect } = require('@playwright/test');
const { resolveBaseUrl } = require('./helpers/serverLifecycle');
const { connectClient } = require('./helpers/gameClient');

test.describe.configure({ timeout: 180000, retries: 1 });

function requireHttpsEdge(testInfo) {
  const baseUrl = resolveBaseUrl();
  test.skip(!baseUrl.startsWith('https://'), `${testInfo.title} requires an HTTPS edge base URL`);
  return baseUrl;
}

test('@critical edge serves hardened headers and cache policy over https', async ({ request }, testInfo) => {
  const baseUrl = requireHttpsEdge(testInfo);

  const landingResponse = await request.get(`${baseUrl}/index.html`);
  expect(landingResponse.ok()).toBeTruthy();
  const landingHeaders = landingResponse.headers();
  expect(landingHeaders['content-security-policy']).toContain("script-src 'self'");
  expect(landingHeaders['content-security-policy']).toContain("style-src 'self'");
  expect(landingHeaders['content-security-policy']).not.toContain("'unsafe-inline'");
  expect(landingHeaders['content-security-policy']).not.toContain("'unsafe-eval'");

  const htmlResponse = await request.get(`${baseUrl}/client.html`);
  expect(htmlResponse.ok()).toBeTruthy();
  const htmlHeaders = htmlResponse.headers();
  expect(htmlHeaders['strict-transport-security']).toContain('max-age=');
  expect(htmlHeaders['x-frame-options']).toBe('DENY');
  expect(htmlHeaders['content-security-policy']).toContain("frame-ancestors 'none'");
  expect(htmlHeaders['content-security-policy']).toContain("script-src 'self'");
  expect(htmlHeaders['content-security-policy']).not.toContain("'unsafe-inline'");

  const assetResponse = await request.get(`${baseUrl}/vendor/pixi.min.js?v=20260308a`);
  expect(assetResponse.ok()).toBeTruthy();
  const assetHeaders = assetResponse.headers();
  expect(assetHeaders['cache-control']).toContain('immutable');
  expect(assetHeaders['x-cache-status']).toBeTruthy();
});

test('@critical edge landing page is self-hosted and browser-clean', async ({ page }, testInfo) => {
  const baseUrl = requireHttpsEdge(testInfo);
  const cspViolations = [];

  page.on('console', (msg) => {
    const text = msg.text();
    if (msg.type() === 'error' && text.includes('Content Security Policy')) {
      cspViolations.push(text);
    }
  });

  const response = await page.goto(`${baseUrl}/index.html`, { waitUntil: 'domcontentloaded' });
  expect(response?.ok()).toBeTruthy();
  await expect(page.locator('h1')).toContainText('Space combat with shooter discipline');

  const resourceOrigins = await page.evaluate(() =>
    performance
      .getEntriesByType('resource')
      .map((entry) => entry.name)
      .filter((name) => !name.startsWith(window.location.origin))
  );

  expect(resourceOrigins).toEqual([]);
  expect(cspViolations).toEqual([]);
});

test('@critical edge promotes gameplay traffic to wss', async ({ page }, testInfo) => {
  requireHttpsEdge(testInfo);

  await connectClient(page, {
    name: 'EdgeReleaseE2E',
    query: '/client.html?auto_reconnect=1&disable_stun=1&match_type=quick',
    matchType: 'quick',
    timeout: 90000,
  });

  const connectionSnapshot = await page.evaluate(() => ({
    pageProtocol: window.location.protocol,
    activeSignalingUrl: window.__e2e?.activeSignalingUrl || '',
    dataChannelOpen: !!window.__e2e?.dataChannelOpen,
    status: window.__e2e?.connectionStatus?.statusKey || null,
  }));

  expect(connectionSnapshot.pageProtocol).toBe('https:');
  expect(connectionSnapshot.activeSignalingUrl.startsWith('wss://')).toBeTruthy();
  expect(connectionSnapshot.dataChannelOpen).toBe(true);
  expect(['waiting', 'playing', 'respawn']).toContain(connectionSnapshot.status);
});
