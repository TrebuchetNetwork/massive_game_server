#!/usr/bin/env node

const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');
const { chromium } = require('playwright');
const { connectClient, waitForPlaying } = require('./tests/helpers/gameClient');
const { startServerOnPort, stopServer } = require('./tests/helpers/serverLifecycle');

const repoRoot = path.resolve(__dirname, '..', '..');
const showcasePort = Number.parseInt(process.env.SHOWCASE_PORT || '19080', 10) || 19080;
const baseUrl = process.env.SHOWCASE_BASE_URL || `http://127.0.0.1:${showcasePort}`;
const outputGameplayGif = path.join(repoRoot, 'docs/media/gameplay/gameplay_showcase.gif');
const outputEffectsGif = path.join(repoRoot, 'docs/media/gameplay/effects_showcase.gif');
const outputGameplayMp4 = path.join(repoRoot, 'docs/media/videos/gameplay_showcase.mp4');
const outputEffectsMp4 = path.join(repoRoot, 'docs/media/videos/effects_showcase.mp4');

function runOrThrow(cmd, args, options = {}) {
  const result = spawnSync(cmd, args, {
    stdio: 'inherit',
    ...options,
  });
  if (result.status !== 0) {
    throw new Error(`${cmd} exited with status ${result.status}`);
  }
}

async function sleep(ms) {
  await new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitForFileReady(filePath, timeoutMs = 30000) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    if (fs.existsSync(filePath)) {
      const size = fs.statSync(filePath).size;
      if (size > 0) return;
    }
    await sleep(250);
  }
  throw new Error(`Timed out waiting for recorded video at ${filePath}`);
}

async function setAimOffset(page, dx, dy) {
  await page.evaluate(
    ({ dx, dy }) => {
      const e2e = window.__e2e;
      const snapshot = e2e?.localPlayerSnapshot;
      if (!snapshot || typeof e2e?.setAimWorldPosition !== 'function') return false;
      e2e.setAimWorldPosition((Number(snapshot.x) || 0) + dx, (Number(snapshot.y) || 0) + dy);
      return true;
    },
    { dx, dy }
  );
}

async function hideTransientUi(page) {
  const minimizeHudButton = page.locator('#minimizeHudButton');
  if (await minimizeHudButton.isVisible().catch(() => false)) {
    await minimizeHudButton.click({ force: true });
  }
  const closeMusicPlayer = page.locator('#closeMusicPlayer');
  if (await closeMusicPlayer.isVisible().catch(() => false)) {
    await closeMusicPlayer.click({ force: true });
  }
  await page.evaluate(() => {
    document.getElementById('settingsMenu')?.classList.add('hidden');
    document.getElementById('scoreboard')?.classList.add('hidden');
    const fpsCounter = document.getElementById('fpsCounter');
    if (fpsCounter) {
      fpsCounter.classList.add('hidden');
      fpsCounter.style.display = 'none';
    }
    const hudMenuToggle = document.getElementById('hudMenuToggle');
    if (hudMenuToggle) {
      hudMenuToggle.classList.add('hidden');
      hudMenuToggle.style.display = 'none';
    }
  });
}

async function preparePage(page, label) {
  console.log(`[showcase] connecting ${label}`);
  await connectClient(page, {
    baseUrl,
    query: '/client.html?auto_reconnect=1&disable_stun=1&match_type=quick',
    matchType: 'quick',
    name: `Showcase-${label}`,
    requireLocalPlayer: true,
    timeout: 90000,
  });
  await waitForPlaying(page, 90000);
  await hideTransientUi(page);
  await sleep(1200);
}

async function performGameplaySequence(page) {
  await setAimOffset(page, 240, -20);
  await page.keyboard.down('KeyW');
  await page.evaluate(() => window.__e2e?.forcePrimaryFire?.(true));
  await sleep(1100);

  await page.keyboard.press('KeyQ');
  await setAimOffset(page, 180, 110);
  await sleep(700);

  await page.keyboard.up('KeyW');
  await page.keyboard.down('KeyD');
  await page.keyboard.press('Digit2');
  await sleep(500);

  await setAimOffset(page, -170, 55);
  await sleep(850);

  await page.keyboard.press('Digit1');
  await page.keyboard.press('KeyE');
  await page.keyboard.up('KeyD');
  await page.keyboard.down('KeyS');
  await setAimOffset(page, 60, -210);
  await sleep(900);

  await page.keyboard.up('KeyS');
  await page.keyboard.press('KeyR');
  await sleep(650);
  await page.keyboard.press('KeyV');
  await sleep(650);

  await setAimOffset(page, 230, 35);
  await sleep(1200);

  await page.evaluate(() => window.__e2e?.forcePrimaryFire?.(false));
  await sleep(1400);
}

async function performEffectsSequence(page) {
  await page.evaluate(() => {
    window.__e2e?.applyFullFxMode?.();
    window.__e2e?.startFxStress?.({
      intensity: 10,
      intervalMs: 150,
      syntheticProjectiles: 180,
      includeScreenFx: true,
      seed: 1337,
    });
    window.__e2e?.forcePrimaryFire?.(true);
  });

  const aimOffsets = [
    [240, -60, 700],
    [-220, 80, 650],
    [120, 180, 650],
    [-140, -170, 650],
    [260, 40, 650],
    [20, -220, 650],
  ];

  for (const [dx, dy, ms] of aimOffsets) {
    await setAimOffset(page, dx, dy);
    await sleep(ms);
  }

  await page.keyboard.press('KeyQ');
  await sleep(350);
  await page.keyboard.press('KeyE');
  await sleep(1200);

  await page.evaluate(() => {
    window.__e2e?.forcePrimaryFire?.(false);
    window.__e2e?.stopFxStress?.(true);
  });
  await sleep(1200);
}

async function recordScenario(browser, label, performer) {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), `mgs-showcase-${label}-`));
  const contextStartedAt = Date.now();
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
    recordVideo: {
      dir: tempDir,
      size: { width: 1280, height: 720 },
    },
  });
  const page = await context.newPage();
  const video = page.video();

  console.log(`[showcase] scenario ${label} temp ${tempDir}`);
  await preparePage(page, label);
  const clipStartedAt = Date.now();
  await performer(page);
  const clipEndedAt = Date.now();
  await context.close();

  const recordedPath = await video.path();
  await waitForFileReady(recordedPath);

  return {
    recordedPath,
    trimStartSeconds: Math.max(0, (clipStartedAt - contextStartedAt) / 1000 - 0.15),
    trimDurationSeconds: Math.max(4, (clipEndedAt - clipStartedAt) / 1000 + 0.2),
  };
}

function renderMedia(inputVideo, outputMp4, outputGif, trimStartSeconds, trimDurationSeconds) {
  const palettePath = path.join(os.tmpdir(), `mgs-showcase-palette-${Date.now()}.png`);

  runOrThrow('ffmpeg', [
    '-y',
    '-ss', trimStartSeconds.toFixed(2),
    '-t', trimDurationSeconds.toFixed(2),
    '-i', inputVideo,
    '-vf', 'scale=1280:-2:flags=lanczos',
    '-an',
    '-c:v', 'libx264',
    '-preset', 'medium',
    '-crf', '24',
    '-pix_fmt', 'yuv420p',
    outputMp4,
  ]);

  runOrThrow('ffmpeg', [
    '-y',
    '-ss', trimStartSeconds.toFixed(2),
    '-t', trimDurationSeconds.toFixed(2),
    '-i', inputVideo,
    '-vf', 'fps=15,scale=960:-2:flags=lanczos,palettegen=stats_mode=diff',
    '-frames:v', '1',
    palettePath,
  ]);

  runOrThrow('ffmpeg', [
    '-y',
    '-ss', trimStartSeconds.toFixed(2),
    '-t', trimDurationSeconds.toFixed(2),
    '-i', inputVideo,
    '-i', palettePath,
    '-lavfi',
    'fps=15,scale=960:-2:flags=lanczos[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle',
    outputGif,
  ]);

  fs.rmSync(palettePath, { force: true });
}

async function main() {
  console.log(`[showcase] starting dedicated server at ${baseUrl}`);
  await startServerOnPort(showcasePort, {
    env: {
      MGS_TARGET_BOT_COUNT: process.env.SHOWCASE_BOT_COUNT || '8',
      MGS_AUTH_ENABLED: '0',
      MGS_MATCH_DURATION_OVERRIDE_SECS: process.env.SHOWCASE_MATCH_DURATION || '180',
    },
  });

  console.log('[showcase] launching browser');
  const browser = await chromium.launch({ headless: true });
  try {
    const gameplay = await recordScenario(browser, 'gameplay', performGameplaySequence);
    console.log('[showcase] rendering gameplay');
    renderMedia(
      gameplay.recordedPath,
      outputGameplayMp4,
      outputGameplayGif,
      gameplay.trimStartSeconds,
      gameplay.trimDurationSeconds
    );

    const effects = await recordScenario(browser, 'effects', performEffectsSequence);
    console.log('[showcase] rendering effects');
    renderMedia(
      effects.recordedPath,
      outputEffectsMp4,
      outputEffectsGif,
      effects.trimStartSeconds,
      effects.trimDurationSeconds
    );

    console.log(JSON.stringify({
      gameplay: { gif: outputGameplayGif, mp4: outputGameplayMp4 },
      effects: { gif: outputEffectsGif, mp4: outputEffectsMp4 },
    }, null, 2));
  } finally {
    await browser.close();
    await stopServer();
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
