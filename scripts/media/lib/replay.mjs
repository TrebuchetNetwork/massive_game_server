// Load a persisted live-replay file (zstd-compressed JSON) and normalize it
// into a flat frame model the renderer can consume.
import { spawn } from 'node:child_process';
import { readFile } from 'node:fs/promises';

const ZSTD = process.env.ZSTD_BIN || '/usr/bin/zstd';
const ZSTD_TIMEOUT_MS = 60_000;

function decompressZstd(path) {
  return new Promise((resolve, reject) => {
    const proc = spawn(ZSTD, ['-dc', path], { stdio: ['ignore', 'pipe', 'pipe'] });
    const chunks = [];
    const errChunks = [];
    const timer = setTimeout(() => {
      proc.kill('SIGKILL');
      reject(new Error(`zstd -dc ${path} timed out after ${ZSTD_TIMEOUT_MS}ms`));
    }, ZSTD_TIMEOUT_MS);
    proc.stdout.on('data', (c) => chunks.push(c));
    proc.stderr.on('data', (c) => errChunks.push(c));
    proc.on('error', (err) => {
      clearTimeout(timer);
      reject(err);
    });
    proc.on('close', (code) => {
      clearTimeout(timer);
      if (code !== 0) {
        reject(new Error(`zstd -dc ${path} exited ${code}: ${Buffer.concat(errChunks).toString('utf8')}`));
      } else {
        resolve(Buffer.concat(chunks));
      }
    });
  });
}

/**
 * loadReplay(path) -> {
 *   frames: [{ t, players: [{ id, name, x, y, vx, vy, hp, alive, team }] }],
 *   durationMs,
 *   meta: { mapName, reason, generatedAtMs },
 * }
 * `t` is milliseconds relative to the first frame.
 */
export async function loadReplay(path) {
  const raw = path.endsWith('.zst') ? await decompressZstd(path) : await readFile(path);
  const doc = JSON.parse(raw.toString('utf8'));
  const srcFrames = doc.frames || [];
  const t0 = srcFrames.length ? srcFrames[0].timestamp_ms : 0;

  const frames = srcFrames.map((f) => {
    const players = (f.sampled_players || f.players || []).map((p) => ({
      id: p.player_id,
      name: p.username,
      x: p.x,
      y: p.y,
      vx: p.velocity_x || 0,
      vy: p.velocity_y || 0,
      hp: p.health,
      alive: p.alive !== false,
      team: p.team_id,
    }));
    return { t: Math.round(f.timestamp_ms - t0), players };
  });

  return {
    frames,
    durationMs: frames.length ? frames[frames.length - 1].t : 0,
    meta: {
      mapName: doc.map_name || null,
      reason: doc.reason || null,
      generatedAtMs: doc.generated_at_ms || null,
    },
  };
}
