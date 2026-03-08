const { test, expect } = require('@playwright/test');
const { registerServerLifecycle, resolveBaseUrl } = require('./helpers/serverLifecycle');
const { connectClient } = require('./helpers/gameClient');

const ADMIN_TOKEN = process.env.E2E_ADMIN_TOKEN || 'match-lifecycle-e2e-admin';

registerServerLifecycle(test, {
  env: {
    MGS_MATCH_DURATION_OVERRIDE_SECS: '15',
    MGS_TARGET_BOT_COUNT: '0',
    MGS_ADMIN_TOKEN: ADMIN_TOKEN,
  },
});

test.describe.configure({ timeout: 180000, retries: 1 });

test('short match ends and exposes post-match summary through UI and admin API', async ({ page, request }) => {
  await connectClient(page, {
    name: 'MatchLifecycleE2E',
    query: '/client.html?disable_stun=1&match_type=quick',
    matchType: 'quick',
    timeout: 90000,
  });

  await page.waitForFunction(
    () => {
      const panel = document.getElementById('postMatchPanel');
      return !!panel && panel.classList.contains('post-match-panel--visible');
    },
    null,
    { timeout: 70000 }
  );

  const summarySnapshot = await page.evaluate(() => ({
    matchInfo: window.__e2e?.matchInfoSnapshot || null,
    postMatchMeta: document.getElementById('postMatchMeta')?.textContent || '',
    postMatchCallout: document.getElementById('postMatchCallout')?.textContent || '',
  }));
  expect(summarySnapshot.postMatchMeta.length).toBeGreaterThan(0);
  expect(summarySnapshot.matchInfo).toBeTruthy();

  const response = await request.get(`${resolveBaseUrl()}/api/ops/match-summary/latest`, {
    headers: {
      Authorization: `Bearer ${ADMIN_TOKEN}`,
    },
  });
  expect(response.ok()).toBeTruthy();
  const payload = await response.json();
  expect(payload.ok).toBe(true);
  expect(payload.summary).toBeTruthy();
  expect(Array.isArray(payload.summary.players)).toBe(true);
});
