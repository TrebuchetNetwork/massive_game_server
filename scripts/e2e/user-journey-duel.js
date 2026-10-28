// Champion duel journey: lands on client.html?match_type=champion_duel like
// the landing-page "Challenge the champion" CTA, waits for the on-demand duel
// to form (the running round winds down first), then verifies the 1v1: exactly
// two players, the opponent is the published roster's top exhibition fighter,
// and the match-info banner carries the DUEL framing.
//
// Usage: node user-journey-duel.js [baseUrl]   (default http://localhost:8080)
const { chromium, devices } = require('@playwright/test');

const BASE = process.argv[2] || process.env.PROBE_BASE || 'http://localhost:8080';
const URL = `${BASE}/client.html?match_type=champion_duel`;
// Wind-down (8s) + Ended intermission (10s) + boundary + spawn slack.
const DUEL_FORM_TIMEOUT_MS = 120000;

(async () => {
  const results = [];
  const check = (name, ok, detail = '') => {
    results.push({ name, ok });
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? ' — ' + detail : ''}`);
  };

  const browser = await chromium.launch({ args: ['--use-fake-ui-for-media-stream'] });
  const ctx = await browser.newContext({ ...devices['iPhone 13'], browserName: 'chromium' });
  const page = await ctx.newPage();
  const consoleErrors = [];
  page.on('pageerror', (e) => consoleErrors.push(String(e).slice(0, 200)));

  await page.goto(URL, { waitUntil: 'domcontentloaded', timeout: 30000 });
  await page.waitForTimeout(5000);
  const state0 = await page.evaluate(() => ({
    autoConnect: window.__e2e?.autoConnectOnLoad,
    dc: window.__e2e?.dataChannelOpen,
    matchType: window.__e2e?.selectedMatchType,
    status: document.getElementById('connectionStatusDetail')?.textContent,
  }));
  check('client selected champion_duel ruleset', state0.matchType === 'champion_duel', `matchType=${state0.matchType}`);

  // Join like a user: phones get the big "Enter arena" button, desktops the
  // Connect button (callsign modal auto-resolves under automation).
  const mobileJoin = page.locator('#mobileArenaJoinButton');
  if (await mobileJoin.isVisible().catch(() => false)) {
    check('mobile join button visible (phone UI)', true);
    await mobileJoin.tap();
  } else {
    const skip = page.locator('#usernameSkipButton');
    if (await skip.isVisible().catch(() => false)) await skip.click();
    await page.waitForSelector('#connectButton', { state: 'visible', timeout: 15000 });
    await page.evaluate(() => document.getElementById('connectButton').click());
  }

  let spawned = false;
  try {
    await page.waitForFunction(() => window.__e2e?.hasLocalPlayer === true, null, { timeout: 60000 });
    spawned = true;
  } catch (_) {}
  check('player spawns into match', spawned,
    spawned ? '' : JSON.stringify(await page.evaluate(() => ({ dc: window.__e2e?.dataChannelOpen, status: document.getElementById('connectionStatusDetail')?.textContent }))));

  if (spawned) {
    // The challenger may land in the tail of the previous match; wait for the
    // boundary that reconfigures the world into the 1v1. Verify server-side
    // via the public scoreboard: the client's player map is AoI-culled and a
    // distant champion would be invisible to it.
    const scoreboard = async () => page.evaluate(async () => {
      try {
        const res = await fetch('/api/public/match/scoreboard', { headers: { Accept: 'application/json' } });
        if (!res.ok) return null;
        const payload = await res.json();
        return {
          players: (payload.players || []).filter(p => !p.is_spectator),
          game_mode: payload.game_mode,
        };
      } catch (_) { return null; }
    });
    let duelState = null;
    const deadline = Date.now() + DUEL_FORM_TIMEOUT_MS;
    while (Date.now() < deadline) {
      const board = await scoreboard();
      const banner = await page.evaluate(() => document.getElementById('matchInfo')?.textContent || '').catch(() => '');
      if (board) {
        duelState = {
          players: board.players.length,
          usernames: board.players.map(p => String(p.username || '')),
          banner,
        };
        if (board.players.length === 2) break;
      }
      await page.waitForTimeout(2000);
    }
    check('duel forms with exactly 2 players', duelState?.players === 2, `players=${duelState?.players} usernames=${JSON.stringify(duelState?.usernames)}`);

    const opponent = (duelState?.usernames || []).find(name => /^#\d+ /.test(name));
    check('opponent is a published roster fighter', !!opponent, `opponent="${opponent}"`);
    check('banner shows DUEL framing', /DUEL/i.test(duelState?.banner || ''), `banner="${(duelState?.banner || '').slice(0, 120)}"`);
    check('banner names the champion', !!opponent && (duelState?.banner || '').includes(opponent.replace(/^#\d+\s+/, '')), `banner="${(duelState?.banner || '').slice(0, 120)}"`);

    // Playable: movement input moves the local player.
    const before = await page.evaluate(() => window.__e2e?.localPlayerSnapshot);
    await page.keyboard.down('w');
    await page.waitForTimeout(2500);
    await page.keyboard.up('w');
    const after = await page.evaluate(() => window.__e2e?.localPlayerSnapshot);
    const moved = before && after ? Math.hypot(after.x - before.x, after.y - before.y) : 0;
    check('player moves in the duel', moved > 20, `moved ${moved.toFixed(0)}px`);

    await page.screenshot({ path: '/tmp/duel.png', timeout: 10000 }).catch(() => {});
    console.log('screenshot saved to /tmp/duel.png');
  }

  check('no fatal page errors', consoleErrors.length === 0, consoleErrors.slice(0, 2).join(' ; '));

  console.log('\n== SUMMARY ==');
  const fails = results.filter(r => !r.ok);
  console.log(`${results.length - fails.length}/${results.length} checks passed`);
  await browser.close();
  process.exit(fails.length === 0 ? 0 : 1);
})();
