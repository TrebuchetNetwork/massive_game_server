const { test, expect } = require('@playwright/test');
const { registerServerLifecycle } = require('./helpers/serverLifecycle');

test.use({
  viewport: { width: 390, height: 844 },
  isMobile: true,
  hasTouch: true,
});

registerServerLifecycle(test);

test.describe.configure({ timeout: 120000 });

async function readMobileLayout(page) {
  return page.evaluate(() => {
    const controlsRect = document.querySelector('.mobile-controls__buttons')?.getBoundingClientRect();
    const joystickRect = document.querySelector('.mobile-controls__left')?.getBoundingClientRect();
    const settingsRect = document.querySelector('.mobile-controls__settings')?.getBoundingClientRect();
    const ratingsChipRect = document.getElementById('arenaEvolutionChip')?.getBoundingClientRect();
    const actionButtons = [...document.querySelectorAll('.mobile-controls__buttons button')];
    const actionRects = actionButtons.map((button) => button.getBoundingClientRect());
    const visibleTargets = [
      ...actionButtons,
      document.getElementById('mobileAimAssistToggle'),
      document.getElementById('mobilePing'),
      document.getElementById('hudMenuToggle'),
      document.getElementById('arenaEvolutionChip'),
    ].filter((element) => {
      if (!element) return false;
      const style = getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      return style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0 && rect.height > 0;
    });
    const targetSizes = visibleTargets.map((element) => {
      const rect = element.getBoundingClientRect();
      return Math.min(rect.width, rect.height);
    });
    const hud = document.getElementById('mobileCombatHud');
    const hudRect = hud?.getBoundingClientRect();
    const sourceText = (id) => document.getElementById(id)?.textContent?.trim() || '';
    return {
      mobileDynamicsEnabled: !!window.__e2e?.mobileDynamicsEnabled,
      bodyMobileMode: !!window.__e2e?.bodyMobileMode,
      coarsePointer: window.matchMedia('(pointer: coarse)').matches,
      dataSaverMode: !!window.__e2e?.dataSaverMode,
      selectedMatchType: window.__e2e?.selectedMatchType || '',
      statusKey: window.__e2e?.connectionStatus?.statusKey || null,
      hasHorizontalOverflow: document.documentElement.scrollWidth > window.innerWidth,
      actionClusterWidth: controlsRect?.width || 0,
      actionClusterHeight: controlsRect?.height || 0,
      thumbZonesOverlap: !!(
        controlsRect && joystickRect &&
        joystickRect.right > controlsRect.left &&
        joystickRect.bottom > controlsRect.top
      ),
      topControlsOverlap: !!(
        settingsRect && ratingsChipRect &&
        settingsRect.left < ratingsChipRect.right && settingsRect.right > ratingsChipRect.left &&
        settingsRect.top < ratingsChipRect.bottom && settingsRect.bottom > ratingsChipRect.top
      ),
      actionsInsideViewport: actionRects.every((rect) => (
        rect.left >= 0 && rect.top >= 0 &&
        rect.right <= window.innerWidth && rect.bottom <= window.innerHeight
      )),
      actionsMeetTouchTarget: actionRects.every((rect) => rect.width >= 44 && rect.height >= 44),
      visibleTargetsMeetTouchTarget: targetSizes.length > 0 && targetSizes.every((size) => size >= 44),
      actionTextReadable: actionButtons.every((button) => parseFloat(getComputedStyle(button).fontSize) >= 10),
      hudTextReadable: [...document.querySelectorAll('#mobileCombatHud small')]
        .every((element) => parseFloat(getComputedStyle(element).fontSize) >= 10),
      hudVisible: !!hudRect && getComputedStyle(hud).display !== 'none',
      hudInsideViewport: !!hudRect && hudRect.left >= 0 && hudRect.top >= 0
        && hudRect.right <= window.innerWidth && hudRect.bottom <= window.innerHeight,
      hud: {
        health: sourceText('mobileHudHealth'),
        shield: sourceText('mobileHudShield'),
        ammo: sourceText('mobileHudAmmo'),
        weapon: sourceText('mobileHudWeapon'),
        score: sourceText('mobileHudScore'),
        ping: sourceText('mobileHudPing'),
        objective: sourceText('mobileHudObjective'),
        matchScore: sourceText('mobileHudMatchScore'),
      },
      source: {
        health: sourceText('playerHealth'),
        shield: sourceText('playerShield'),
        ammo: sourceText('playerAmmo'),
        weapon: sourceText('playerWeapon'),
        score: sourceText('playerScore'),
        ping: sourceText('pingDisplay'),
      },
      controlsPanelHidden: getComputedStyle(document.getElementById('controlsPanel')).display === 'none',
      musicPlayerHidden: getComputedStyle(document.getElementById('musicPlayer')).display === 'none',
      debugLogHidden: document.getElementById('log')?.getClientRects().length === 0,
      canvasCount: document.querySelectorAll('canvas').length,
    };
  });
}

function expectPlayableMobileLayout(snapshot) {
  expect(snapshot.mobileDynamicsEnabled).toBeTruthy();
  expect(snapshot.bodyMobileMode).toBeTruthy();
  expect(snapshot.coarsePointer).toBeTruthy();
  expect(snapshot.selectedMatchType).toBe('mobile_blitz');
  expect(['waiting', 'playing', 'respawn']).toContain(snapshot.statusKey);
  expect(snapshot.hasHorizontalOverflow).toBeFalsy();
  expect(snapshot.actionClusterWidth).toBeLessThanOrEqual(180);
  expect(snapshot.actionClusterHeight).toBeLessThanOrEqual(160);
  expect(snapshot.thumbZonesOverlap).toBeFalsy();
  expect(snapshot.topControlsOverlap).toBeFalsy();
  expect(snapshot.actionsInsideViewport).toBeTruthy();
  expect(snapshot.actionsMeetTouchTarget).toBeTruthy();
  expect(snapshot.visibleTargetsMeetTouchTarget).toBeTruthy();
  expect(snapshot.actionTextReadable).toBeTruthy();
  expect(snapshot.hudTextReadable).toBeTruthy();
  expect(snapshot.hudVisible).toBeTruthy();
  expect(snapshot.hudInsideViewport).toBeTruthy();
  expect(snapshot.hud.objective.length).toBeGreaterThan(0);
  expect(snapshot.hud.matchScore).toMatch(/^R \d+ · B \d+ · \d+:\d{2}$/);
  expect(snapshot.hud).toMatchObject(snapshot.source);
  expect(snapshot.controlsPanelHidden).toBeTruthy();
  expect(snapshot.musicPlayerHidden).toBeTruthy();
  expect(snapshot.debugLogHidden).toBeTruthy();
  expect(snapshot.canvasCount).toBeLessThanOrEqual(3);
}

test('production CTA enters a playable coarse-touch layout without a mobile URL override', async ({ page }) => {
  await page.addInitScript(() => {
    try {
      localStorage.setItem('dataSaverMode', 'true');
      localStorage.setItem('mgs_player_name', 'MobileTouchE2E');
    } catch (_) {}
  });
  const pageErrors = [];
  page.on('pageerror', (err) => pageErrors.push(err.message || String(err)));

  await page.goto('/index.html', { waitUntil: 'domcontentloaded' });
  const productionCta = page.locator('[data-arena-entry]').filter({ hasText: 'Play as human' });
  await expect(productionCta).toHaveAttribute('href', '/client.html?match_type=mobile_blitz');
  // Dispatch the link's real click without Playwright's navigation auto-wait;
  // the client deliberately stays active on long-lived network resources.
  await productionCta.dispatchEvent('click');
  await expect(page).toHaveURL(/\/client\.html\?match_type=mobile_blitz$/);
  expect(new URL(page.url()).searchParams.has('mobile')).toBeFalsy();
  expect(new URL(page.url()).searchParams.has('platform')).toBeFalsy();

  await expect(page.locator('#mobileArenaGate')).toBeVisible();
  await expect(page.locator('.mobile-arena-gate__controls')).toContainText('Left thumb');
  await expect(page.locator('.mobile-arena-gate__controls')).toContainText('Drag right');
  await expect(page.locator('.mobile-arena-gate__controls')).toContainText('Tap buttons');
  await expect(page.locator('#controlsPanel')).toBeHidden();
  await expect(page.locator('#musicPlayer')).toBeHidden();
  await page.locator('#mobileArenaJoinButton').tap();

  await expect(page.locator('#mobileControls')).toBeVisible();
  await page.waitForFunction(
    () => window.__e2e?.mobileDynamicsEnabled === true && window.__e2e?.bodyMobileMode === true,
    null,
    { timeout: 30000 }
  );

  await expect(page.locator('#mobileFire')).toBeVisible();
  await expect(page.locator('#mobileReload')).toBeVisible();
  await expect(page.locator('#mobileAbilityDash')).toBeVisible();
  await expect(page.locator('#mobileAbilityDodge')).toBeVisible();
  await expect(page.locator('#mobilePing')).toBeVisible();

  await page.waitForFunction(
    () => ['waiting', 'playing', 'respawn'].includes(window.__e2e?.connectionStatus?.statusKey),
    null,
    { timeout: 30000 }
  );
  await expect(page.locator('#mobileArenaJoinButton')).toBeEnabled();
  await expect(page.locator('#mobileArenaJoinButton')).toContainText('Enter arena');
  await expect(page.locator('#mobileCombatHud')).toBeVisible();
  await expect(page.locator('#arenaEvolutionChip')).toBeVisible();
  await page.locator('#mobilePing').tap();
  await expect
    .poll(async () => page.evaluate(() => window.__e2e?.lastTacticalPing?.kind || ''))
    .toBe('group');
  await expect
    .poll(
      async () => page.locator('[data-client-arena-models]').textContent(),
      { timeout: 10000 }
    )
    .toMatch(/^[1-9][0-9]*$/);

  await page.locator('#arenaEvolutionChip').tap();
  await expect(page.locator('#arenaRatingsDialog')).toBeVisible();
  await expect(page.locator('#arenaRatingsTitle')).toHaveText('Weekly strategy tour');
  const ratingsDialogSnapshot = await page.locator('.arena-ratings-dialog__panel').evaluate((panel) => {
    const rect = panel.getBoundingClientRect();
    return {
      insideViewport: rect.left >= 0 && rect.top >= 0
        && rect.right <= window.innerWidth && rect.bottom <= window.innerHeight,
      closeTargetSize: (() => {
        const close = panel.querySelector('.arena-ratings-dialog__close')?.getBoundingClientRect();
        return close ? Math.min(close.width, close.height) : 0;
      })(),
    };
  });
  expect(ratingsDialogSnapshot.insideViewport).toBeTruthy();
  expect(ratingsDialogSnapshot.closeTargetSize).toBeGreaterThanOrEqual(44);
  await expect(page.locator('#mobileControls')).toBeHidden();
  await expect(page.locator('#mobileCombatHud')).toBeHidden();
  await expect(page.locator('#hudMenuToggle')).toBeHidden();
  await page.locator('.arena-ratings-dialog__close').tap();
  await expect(page.locator('#arenaRatingsDialog')).toBeHidden();
  await expect(page.locator('#mobileCombatHud')).toBeVisible();

  await expect
    .poll(async () => page.evaluate(() => !!window.__e2e?.dataSaverMode), { timeout: 10000 })
    .toBe(true);

  const fireButton = page.locator('#mobileFire');
  await fireButton.tap();
  await expect
    .poll(
      async () => page.evaluate(() => typeof window.__e2e?.mobileFireTouchActive === 'boolean'),
      { timeout: 5000 }
    )
    .toBe(true);


  await page.setViewportSize({ width: 568, height: 320 });
  await page.waitForTimeout(400);
  await expect(page.locator('#mobileControls')).toBeVisible();
  expectPlayableMobileLayout(await readMobileLayout(page));

  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(400);
  await expect(page.locator('#mobileControls')).toBeVisible();
  const mobileSnapshot = await readMobileLayout(page);
  expect(mobileSnapshot.dataSaverMode).toBeTruthy();
  expectPlayableMobileLayout(mobileSnapshot);

  const hooksEnabled = await page.evaluate(() => window.__e2e?.hooksEnabled === true);
  if (hooksEnabled) {
    const reconnectSnapshot = await page.evaluate(() => {
      window.__e2e.forceResetConnection();
      const retryButton = document.getElementById('mobileArenaJoinButton');
      return {
        disabled: retryButton?.disabled,
        text: retryButton?.textContent?.trim() || '',
        controlsPanelHidden: getComputedStyle(document.getElementById('controlsPanel')).display === 'none',
        statusKey: window.__e2e?.connectionStatus?.statusKey || '',
      };
    });
    expect(reconnectSnapshot).toMatchObject({
      disabled: true,
      controlsPanelHidden: true,
      statusKey: 'connecting',
    });
    expect(reconnectSnapshot.text).toContain('Joining arena');

    const retrySnapshot = await page.evaluate(() => {
      window.__e2e.forceMobileJoinError();
      const retryButton = document.getElementById('mobileArenaJoinButton');
      return {
        disabled: retryButton?.disabled,
        text: retryButton?.textContent?.trim() || '',
        controlsPanelHidden: getComputedStyle(document.getElementById('controlsPanel')).display === 'none',
        statusKey: window.__e2e?.connectionStatus?.statusKey || '',
      };
    });
    expect(retrySnapshot).toMatchObject({
      disabled: false,
      controlsPanelHidden: true,
      statusKey: 'error',
    });
    expect(retrySnapshot.text).toContain('Retry arena');
  }
  expect(pageErrors).toEqual([]);
});
