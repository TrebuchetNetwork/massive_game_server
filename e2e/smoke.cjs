// Live in-game smoke test for space.selfware.design (via localhost:8080).
// Verifies: client loads, connect flow works, local player spawns, movement
// changes position, firing spawns projectiles, walls exist, teams/score tick.
const { chromium } = require('playwright-core');

const BASE = process.env.ARENA_BASE || 'https://space.selfware.design';
const WS_URL = process.env.ARENA_WS || BASE.replace(/^http/, 'ws') + '/ws';
const result = { ok: true, checks: {}, notes: [] };
const check = (name, pass, detail) => {
  result.checks[name] = { pass: !!pass, detail };
  if (!pass) result.ok = false;
};

const metric = async (name) => {
  const res = await fetch('http://127.0.0.1:9090/metrics');
  const text = await res.text();
  const line = text.split('\n').find((l) => l.startsWith(name + ' '));
  return line ? Number(line.split(' ')[1]) : null;
};

(async () => {
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  page.on('pageerror', (err) => result.notes.push(`pageerror: ${err.message}`));

  await page.goto(`${BASE}/client.html`, { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => !!window.__e2e, null, { timeout: 15000 });
  check('client_loads_with_e2e_hook', true);

  // Username modal: skip if present.
  const skip = page.locator('#usernameSkipButton');
  if (await skip.isVisible().catch(() => false)) await skip.click();

  await page.fill('#wsUrl', WS_URL);
  await page.click('#connectButton');

  // Signaling + data channel (WebRTC host candidates on localhost).
  let dcOpen = false;
  try {
    await page.waitForFunction(() => window.__e2e.dataChannelOpen === true, null, { timeout: 30000 });
    dcOpen = true;
  } catch {
    result.notes.push(`datachannel timeout; connectionStatus=${await page.evaluate(() => window.__e2e.connectionStatus)}`);
  }
  check('datachannel_open', dcOpen);

  await page.waitForFunction(() => window.__e2e.hasLocalPlayer === true, null, { timeout: 20000 })
    .catch(() => {});
  const hasPlayer = await page.evaluate(() => window.__e2e.hasLocalPlayer);
  check('local_player_spawned', hasPlayer);

  const snap1 = await page.evaluate(() => window.__e2e.localPlayerSnapshot);
  // Joining mid-match can land us in matchState 2 (ended) where firing is
  // disabled; wait for a fresh running match with enough clock to test in.
  let matchInfo = await page.evaluate(() => window.__e2e.matchInfoSnapshot);
  const t0 = Date.now();
  while ((!matchInfo || matchInfo.matchState !== 1 || matchInfo.timeRemaining < 20) && Date.now() - t0 < 90000) {
    await page.waitForTimeout(2000);
    matchInfo = await page.evaluate(() => window.__e2e.matchInfoSnapshot);
  }
  check('match_running', !!matchInfo && matchInfo.matchState === 1, JSON.stringify(matchInfo)?.slice(0, 200));

  // Fire first (fresh match, known-good state): real mouse input, read mid-burst.
  // Bots fight back — if we die mid-burst the server (correctly) spawns no
  // projectiles, so wait for respawn and retry instead of failing the check.
  const damageBefore = (await metric('game_damage_events_total{weapon="rifle"}')) ?? 0;
  let projectiles = 0;
  let projHook = {};
  for (let attempt = 0; attempt < 3 && projectiles === 0; attempt += 1) {
    const aliveBefore = await page.evaluate(() => window.__e2e.localPlayerSnapshot?.alive ?? false);
    if (!aliveBefore) {
      await page.waitForTimeout(2500); // respawn window
      continue;
    }
    await page.mouse.move(1000, 400); // aim right of center
    await page.mouse.down();
    await page.waitForTimeout(1200);
    // Read mid-burst: rifle projectiles expire fast, they're gone right after mouse-up.
    projectiles = (await page.evaluate(() =>
      typeof window.__e2e.projectileCount === 'number' ? window.__e2e.projectileCount : 0)) ?? 0;
    projHook = await page.evaluate(() => ({
      visible: window.__e2e.visibleProjectileCount ?? null,
      shooting: window.__e2e.mouseDownShootingSet ?? null,
      alive: window.__e2e.localPlayerSnapshot?.alive ?? null,
      state: window.__e2e.matchInfoSnapshot?.matchState ?? null,
    }));
    await page.mouse.up();
  }
  check('firing_spawns_projectiles', projectiles > 0 || (projHook.visible ?? 0) > 0,
    `projectileCount=${projectiles} visible=${projHook.visible} shootingFlag=${projHook.shooting} aliveAtRead=${projHook.alive} matchState=${projHook.state}`);

  // Movement: hold KeyW for 1.2s and compare positions.
  await page.keyboard.down('w');
  await page.waitForTimeout(1200);
  await page.keyboard.up('w');
  const snap2 = await page.evaluate(() => window.__e2e.localPlayerSnapshot);
  const moved = snap1 && snap2 && (Math.abs(snap2.x - snap1.x) + Math.abs(snap2.y - snap1.y)) > 1;
  check('movement_changes_position', moved,
    `from (${snap1?.x?.toFixed(1)}, ${snap1?.y?.toFixed(1)}) to (${snap2?.x?.toFixed(1)}, ${snap2?.y?.toFixed(1)})`);

  // World state.
  const walls = await page.evaluate(() => window.__e2e.wallCount);
  const players = await page.evaluate(() => window.__e2e.playerCount);
  const scores = await page.evaluate(() => window.__e2e.teamScores);
  check('walls_present', walls > 0, `wallCount=${walls}`);
  check('players_in_match', players >= 1, `playerCount=${players}`);
  check('team_scores_readable', scores != null, JSON.stringify(scores)?.slice(0, 120));

  const damageAfter = (await metric('game_damage_events_total{weapon="rifle"}')) ?? 0;
  result.notes.push(`server rifle damage events: ${damageBefore} -> ${damageAfter} (bots fight continuously; delta optional)`);

  await page.screenshot({ path: require('path').join(__dirname, 'ingame.png') }).catch(() => {});
  const renderFramesAdvance = await page.evaluate(async () => {
    const a = window.__e2e.renderFrames ?? null;
    await new Promise((r) => setTimeout(r, 500));
    const b = window.__e2e.renderFrames ?? null;
    return a != null && b != null ? b - a : null;
  });
  if (renderFramesAdvance != null) check('renderer_advancing', renderFramesAdvance > 0, `+${renderFramesAdvance} frames/500ms`);

  await browser.close();
  console.log(JSON.stringify(result, null, 2));
  process.exit(result.ok ? 0 : 1);
})().catch((err) => {
  console.error('HARNESS FAILURE:', err.message);
  console.log(JSON.stringify(result, null, 2));
  process.exit(2);
});
