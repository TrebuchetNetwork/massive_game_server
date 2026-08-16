#!/usr/bin/env node
// Render highlight clips (.webm + .gif + poster .png) from persisted live
// replays into static_client/media/highlights/YYYY-MM-DD/ and merge metadata
// into static_client/media/highlights/index.json (newest first).
//
// Usage:
//   node render_highlights.mjs [--replay-dir DIR] [--replay FILE]
//                              [--out-dir DIR] [--max-clips N] [--fps N]
import { spawn } from 'node:child_process';
import { mkdir, readdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { loadReplay } from './lib/replay.mjs';
import { selectHighlights, killEvents } from './lib/select.mjs';
import { WIDTH, HEIGHT, prepareScene, renderFrame, encodePng } from './lib/raster.mjs';

const require = createRequire(import.meta.url);
const FFMPEG = require('ffmpeg-static');

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, '../..');

function parseArgs(argv) {
  const opts = {
    replayDir: path.join(REPO_ROOT, 'data/live_replay/matches'),
    replay: null,
    outDir: path.join(REPO_ROOT, 'static_client/media/highlights'),
    maxClips: 3,
    fps: 15,
  };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--replay-dir') opts.replayDir = path.resolve(argv[++i]);
    else if (a === '--replay') opts.replay = path.resolve(argv[++i]);
    else if (a === '--out-dir') opts.outDir = path.resolve(argv[++i]);
    else if (a === '--max-clips') opts.maxClips = Number(argv[++i]);
    else if (a === '--fps') opts.fps = Number(argv[++i]);
    else throw new Error(`unknown arg: ${a}`);
  }
  return opts;
}

const FFMPEG_TIMEOUT_MS = 180_000;

function runFfmpeg(args, { input = null, timeoutMs = FFMPEG_TIMEOUT_MS } = {}) {
  return new Promise((resolve, reject) => {
    const proc = spawn(FFMPEG, ['-hide_banner', '-loglevel', 'error', '-y', ...args], {
      stdio: [input ? 'pipe' : 'ignore', 'ignore', 'pipe'],
    });
    const err = [];
    let settled = false;
    const fail = (e) => {
      if (!settled) { settled = true; clearTimeout(timer); reject(e); }
    };
    const timer = setTimeout(() => {
      proc.kill('SIGKILL');
      fail(new Error(`ffmpeg timed out after ${timeoutMs}ms: ${args[args.length - 1] || ''}`));
    }, timeoutMs);
    proc.stderr.on('data', (c) => err.push(c));
    proc.on('error', fail);
    proc.on('close', (code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (code !== 0) reject(new Error(`ffmpeg exited ${code}: ${Buffer.concat(err).toString('utf8')}`));
      else resolve();
    });
    if (input) {
      // If ffmpeg exits early, writes to its dead stdin raise EPIPE which would
      // mask the real exit code/stderr — swallow feeder errors here; the close
      // handler above reports the actual cause.
      proc.stdin.on('error', () => {});
      Promise.resolve(input(proc.stdin)).catch(() => {});
    }
  });
}

async function renderClip(replay, clip, kills, killCounts, outBase, fps) {
  const durationS = (clip.endMs - clip.startMs) / 1000;
  const nFrames = Math.max(1, Math.round(durationS * fps));
  const scene = prepareScene(replay, { startMs: clip.startMs, endMs: clip.endMs, kills, killCounts });

  // Encode webm (VP9) from raw RGBA frames over stdin.
  await runFfmpeg(
    ['-f', 'rawvideo', '-pix_fmt', 'rgba', '-s', `${WIDTH}x${HEIGHT}`, '-r', String(fps),
      '-i', 'pipe:0', '-an', '-c:v', 'libvpx-vp9', '-crf', '34', '-b:v', '0',
      '-row-mt', '1', '-cpu-used', '4', '-pix_fmt', 'yuv420p', `${outBase}.webm`],
    {
      input: async (stdin) => {
        let dead = false;
        const markDead = () => { dead = true; };
        stdin.on('error', markDead);
        stdin.on('close', markDead);
        for (let i = 0; i < nFrames && !dead; i++) {
          const t = clip.startMs + (i / fps) * 1000;
          const cv = renderFrame(replay, scene, t);
          const buf = Buffer.from(cv.data.buffer, cv.data.byteOffset, cv.data.byteLength);
          if (!stdin.write(buf)) {
            // Resume on drain, but bail if the stream dies first.
            await new Promise((res) => {
              const onDrain = () => { stdin.removeListener('close', onClose); res(); };
              const onClose = () => { stdin.removeListener('drain', onDrain); res(); };
              stdin.once('drain', onDrain);
              stdin.once('close', onClose);
            });
          }
        }
        if (!dead) stdin.end();
      },
    },
  );

  // GIF (640px wide, 12fps, palette) derived from the webm; poster PNG is
  // rendered directly (no lossy round-trip) at 40% into the clip.
  await runFfmpeg(['-i', `${outBase}.webm`, '-vf',
    'fps=12,scale=640:-1:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=128[p];[s1][p]paletteuse=dither=bayer:bayer_scale=4',
    `${outBase}.gif`]);
  const poster = renderFrame(replay, scene, clip.startMs + (clip.endMs - clip.startMs) * 0.4);
  await writeFile(`${outBase}.png`, encodePng(poster));
}

function matchDateMs(filePath, replay) {
  const m = path.basename(filePath).match(/replay_(\d+)/);
  if (m) return Number(m[1]);
  return replay.meta.generatedAtMs || Date.now();
}

const dateDir = (ms) => new Date(ms).toISOString().slice(0, 10);

async function pruneOldDirs(outDir, keepDays = 14) {
  const cutoff = Date.now() - keepDays * 86_400_000;
  const pruned = new Set();
  for (const entry of await readdir(outDir, { withFileTypes: true })) {
    if (!entry.isDirectory() || !/^\d{4}-\d{2}-\d{2}$/.test(entry.name)) continue;
    if (Date.parse(`${entry.name}T00:00:00Z`) < cutoff) {
      await rm(path.join(outDir, entry.name), { recursive: true, force: true });
      pruned.add(entry.name);
    }
  }
  return pruned;
}

async function main() {
  const opts = parseArgs(process.argv);
  await mkdir(opts.outDir, { recursive: true });
  const cutoff = Date.now() - 14 * 86_400_000;

  const files = opts.replay
    ? [opts.replay]
    : (await readdir(opts.replayDir))
        .filter((f) => /^replay_.*\.json\.zst$/.test(f))
        .sort()
        .map((f) => path.join(opts.replayDir, f))
        // Skip matches old enough that their output dir would be pruned anyway.
        .filter((f) => {
          const m = path.basename(f).match(/replay_(\d+)/);
          return !m || Number(m[1]) >= cutoff;
        });
  if (!files.length) {
    console.log('no replay files found');
    return;
  }

  // Gather candidate clips across matches, keep the global top maxClips.
  // A failed replay (corrupt file, hung zstd) is skipped, not fatal.
  const candidates = [];
  for (const file of files) {
    try {
      const replay = await loadReplay(file);
      const kills = killEvents(replay);
      const killCounts = new Map();
      // Attribute kills: nearest enemy within 120 units of the victim at death.
      for (const k of kills) {
        const frame = replay.frames.find((f) => f.t >= k.t) || replay.frames[replay.frames.length - 1];
        let best = null, bestD = 120;
        for (const p of frame.players) {
          if (!p.alive || p.team === k.team) continue;
          const d = Math.hypot(p.x - k.x, p.y - k.y);
          if (d < bestD) { bestD = d; best = p.id; }
        }
        if (best) killCounts.set(best, (killCounts.get(best) || 0) + 1);
      }
      for (const clip of selectHighlights(replay, { maxClips: opts.maxClips })) {
        candidates.push({ file, replay, clip, kills, killCounts });
      }
    } catch (err) {
      console.error(`skipping ${path.basename(file)}: ${err.message}`);
    }
  }
  candidates.sort((a, b) => b.clip.score - a.clip.score);
  const chosen = candidates.slice(0, opts.maxClips);

  const indexPath = path.join(opts.outDir, 'index.json');
  let index = [];
  try {
    index = JSON.parse(await readFile(indexPath, 'utf8'));
  } catch { /* fresh index */ }
  const pruned = await pruneOldDirs(opts.outDir);
  index = index.filter((e) => !pruned.has(e.date));

  for (const { file, replay, clip, kills, killCounts } of chosen) {
    const date = dateDir(matchDateMs(file, replay));
    const dir = path.join(opts.outDir, date);
    await mkdir(dir, { recursive: true });
    const base = path.basename(file).replace(/\.json\.zst$/, '').replace(/\.json$/, '');
    const outBase = path.join(dir, `${base}_${clip.startMs}-${clip.endMs}`);

    console.log(`rendering ${path.basename(outBase)} (${clip.reason}, score ${clip.score}, ${((clip.endMs - clip.startMs) / 1000).toFixed(1)}s)`);
    try {
      await renderClip(replay, clip, kills, killCounts, outBase, opts.fps);
    } catch (err) {
      // A failed clip (ffmpeg crash/timeout) is skipped, not fatal.
      console.error(`skipping clip ${path.basename(outBase)}: ${err.message}`);
      continue;
    }

    const scene = prepareScene(replay, { startMs: clip.startMs, endMs: clip.endMs, kills, killCounts });
    const rel = (ext) => `${date}/${path.basename(outBase)}${ext}`;
    index = index.filter((e) => e.webm !== rel('.webm'));
    index.unshift({
      date,
      webm: rel('.webm'),
      gif: rel('.gif'),
      poster: rel('.png'),
      reason: clip.reason,
      score: clip.score,
      players: scene.featured.map((f) => f.label),
    });
  }

  index.sort((a, b) => b.date.localeCompare(a.date));
  // Atomic write: tmp file + rename (same pattern as arena's atomicWriteJson).
  const tmp = `${indexPath}.tmp-${process.pid}-${Date.now()}`;
  await writeFile(tmp, JSON.stringify(index, null, 2) + '\n');
  await rename(tmp, indexPath);
  console.log(`index.json updated (${index.length} entries)`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
