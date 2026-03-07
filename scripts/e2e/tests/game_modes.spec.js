const { test, expect } = require('@playwright/test');
const { registerServerLifecycle } = require('./helpers/serverLifecycle');
const { connectClient } = require('./helpers/gameClient');

registerServerLifecycle(test, {
  env: {
    MGS_DYNAMIC_MODE_TRANSITIONS: '1',
    MGS_MATCH_DURATION_OVERRIDE_SECS: '36',
    MGS_TARGET_BOT_COUNT: '0',
  },
});

test.describe.configure({ timeout: 180000, retries: 1 });

test('full-match browser flow surfaces scripted mode or late-phase events', async ({ page }) => {
  const pageErrors = [];
  page.on('pageerror', (err) => pageErrors.push(err.message || String(err)));

  await connectClient(page, {
    name: 'GameModesE2E',
    query: '/client.html?disable_stun=1&match_type=full',
    matchType: 'full',
    timeout: 90000,
  });

  await expect(page.locator('#matchInfo')).toContainText('FFA', { timeout: 30000 });
  const initialSnapshot = await page.evaluate(() => ({
    matchInfoText: document.getElementById('matchInfo')?.textContent || '',
    timeRemaining: Number(window.__e2e?.matchInfoSnapshot?.timeRemaining || 0),
  }));

  const observedProgressHandle = await page.waitForFunction(
    (initial) => {
      const urgencyText = String(window.__e2e?.objectiveUrgencyText || '');
      const currentMatchInfoText = String(document.getElementById('matchInfo')?.textContent || '');
      const currentTimeRemaining = Number(window.__e2e?.matchInfoSnapshot?.timeRemaining || 0);
      if (currentMatchInfoText !== String(initial?.matchInfoText || '')) {
        return 'match-info-changed';
      }
      if (
        Number.isFinite(currentTimeRemaining) &&
        Number.isFinite(Number(initial?.timeRemaining || 0)) &&
        currentTimeRemaining < Number(initial?.timeRemaining || 0)
      ) {
        return 'time-advanced';
      }
      if (
        urgencyText.includes('Mode shift in') ||
        urgencyText.includes('Mode shifted:') ||
        urgencyText.includes('Supply drop incoming') ||
        urgencyText.includes('Zone surge active') ||
        urgencyText.includes('FINAL STAND')
      ) {
        return 'urgency-event';
      }
      return null;
    },
    initialSnapshot,
    { timeout: 50000 }
  );
  const observedProgress = await observedProgressHandle.jsonValue();

  const snapshot = await page.evaluate(() => ({
    selectedMatchType: window.__e2e?.selectedMatchType || '',
    matchInfo: window.__e2e?.matchInfoSnapshot || null,
    matchInfoText: document.getElementById('matchInfo')?.textContent || '',
    objectiveUrgencyText: window.__e2e?.objectiveUrgencyText || '',
    modeIntroText: window.__e2e?.modeIntroText || '',
  }));

  expect(snapshot.selectedMatchType).toBe('full');
  expect(snapshot.matchInfo).toBeTruthy();
  expect(observedProgress).toBeTruthy();
  expect(pageErrors).toEqual([]);
});
