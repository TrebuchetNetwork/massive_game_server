// Pure-JS rasterizer: 1280x720 RGBA scene rendering + hand-rolled PNG encoding
// (node:zlib deflateSync, IHDR/IDAT/IEND chunks, CRC32). No native deps.
import { deflateSync } from 'node:zlib';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { mascotFor } from '../../arena/mascots.mjs';

export const WIDTH = 1280;
export const HEIGHT = 720;
const HUD_H = 64;
const TRAIL_LEN = 8;
const KILL_FLASH_MS = 400;

const TEAM_COLORS = {
  1: [0x22, 0xd3, 0xee], // cyan
  2: [0xf4, 0x3f, 0x5e], // red
};
const FALLBACK_COLOR = [0x94, 0xa3, 0xb8];

// ---------------------------------------------------------------------------
// Canvas
// ---------------------------------------------------------------------------

export function createCanvas(width = WIDTH, height = HEIGHT) {
  return { width, height, data: new Uint8Array(width * height * 4) };
}

function hex(hexStr) {
  const n = parseInt(String(hexStr).replace('#', ''), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function blend(cv, x, y, r, g, b, a) {
  x |= 0; y |= 0;
  if (x < 0 || y < 0 || x >= cv.width || y >= cv.height || a <= 0) return;
  const i = (y * cv.width + x) * 4;
  const ia = 1 - a;
  cv.data[i] = r * a + cv.data[i] * ia;
  cv.data[i + 1] = g * a + cv.data[i + 1] * ia;
  cv.data[i + 2] = b * a + cv.data[i + 2] * ia;
  cv.data[i + 3] = 255;
}

function fillRect(cv, x0, y0, w, h, [r, g, b], a = 1) {
  for (let y = Math.max(0, y0); y < Math.min(cv.height, y0 + h); y++) {
    for (let x = Math.max(0, x0); x < Math.min(cv.width, x0 + w); x++) {
      blend(cv, x, y, r, g, b, a);
    }
  }
}

function disc(cv, cx, cy, radius, [r, g, b], a = 1) {
  const r2 = radius * radius;
  for (let y = Math.floor(cy - radius); y <= cy + radius; y++) {
    for (let x = Math.floor(cx - radius); x <= cx + radius; x++) {
      const d2 = (x - cx) ** 2 + (y - cy) ** 2;
      if (d2 <= r2) blend(cv, x, y, r, g, b, a);
    }
  }
}

function ring(cv, cx, cy, radius, thickness, [r, g, b], a = 1) {
  const rOut = radius + thickness / 2, rIn = Math.max(0, radius - thickness / 2);
  for (let y = Math.floor(cy - rOut); y <= cy + rOut; y++) {
    for (let x = Math.floor(cx - rOut); x <= cx + rOut; x++) {
      const d = Math.hypot(x - cx, y - cy);
      if (d >= rIn && d <= rOut) blend(cv, x, y, r, g, b, a);
    }
  }
}

function fillTri(cv, p0, p1, p2, [r, g, b], a = 1) {
  const minX = Math.max(0, Math.floor(Math.min(p0[0], p1[0], p2[0])));
  const maxX = Math.min(cv.width - 1, Math.ceil(Math.max(p0[0], p1[0], p2[0])));
  const minY = Math.max(0, Math.floor(Math.min(p0[1], p1[1], p2[1])));
  const maxY = Math.min(cv.height - 1, Math.ceil(Math.max(p0[1], p1[1], p2[1])));
  const edge = (a0, a1, px, py) => (px - a0[0]) * (a1[1] - a0[1]) - (py - a0[1]) * (a1[0] - a0[0]);
  const area = edge(p0, p1, p2[0], p2[1]);
  if (area === 0) return;
  for (let y = minY; y <= maxY; y++) {
    for (let x = minX; x <= maxX; x++) {
      const w0 = edge(p1, p2, x, y), w1 = edge(p2, p0, x, y), w2 = edge(p0, p1, x, y);
      if ((w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0)) {
        blend(cv, x, y, r, g, b, a);
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Minimal 3x5 bitmap font: A-Z, 0-9, space, ':', '-', '#'
// ---------------------------------------------------------------------------

const FONT = {
  A: ['.#.', '#.#', '###', '#.#', '#.#'],
  B: ['##.', '#.#', '##.', '#.#', '##.'],
  C: ['.##', '#..', '#..', '#..', '.##'],
  D: ['##.', '#.#', '#.#', '#.#', '##.'],
  E: ['###', '#..', '##.', '#..', '###'],
  F: ['###', '#..', '##.', '#..', '#..'],
  G: ['.##', '#..', '#.#', '#.#', '.##'],
  H: ['#.#', '#.#', '###', '#.#', '#.#'],
  I: ['###', '.#.', '.#.', '.#.', '###'],
  J: ['..#', '..#', '..#', '#.#', '.#.'],
  K: ['#.#', '#.#', '##.', '#.#', '#.#'],
  L: ['#..', '#..', '#..', '#..', '###'],
  M: ['#.#', '###', '#.#', '#.#', '#.#'],
  N: ['#.#', '###', '###', '#.#', '#.#'],
  O: ['.#.', '#.#', '#.#', '#.#', '.#.'],
  P: ['##.', '#.#', '##.', '#..', '#..'],
  Q: ['.#.', '#.#', '#.#', '##.', '.##'],
  R: ['##.', '#.#', '##.', '#.#', '#.#'],
  S: ['.##', '#..', '.#.', '..#', '##.'],
  T: ['###', '.#.', '.#.', '.#.', '.#.'],
  U: ['#.#', '#.#', '#.#', '#.#', '###'],
  V: ['#.#', '#.#', '#.#', '#.#', '.#.'],
  W: ['#.#', '#.#', '###', '###', '#.#'],
  X: ['#.#', '#.#', '.#.', '#.#', '#.#'],
  Y: ['#.#', '#.#', '.#.', '.#.', '.#.'],
  Z: ['###', '..#', '.#.', '#..', '###'],
  0: ['###', '#.#', '#.#', '#.#', '###'],
  1: ['.#.', '##.', '.#.', '.#.', '###'],
  2: ['###', '..#', '###', '#..', '###'],
  3: ['###', '..#', '###', '..#', '###'],
  4: ['#.#', '#.#', '###', '..#', '..#'],
  5: ['###', '#..', '###', '..#', '###'],
  6: ['###', '#..', '###', '#.#', '###'],
  7: ['###', '..#', '..#', '.#.', '.#.'],
  8: ['###', '#.#', '###', '#.#', '###'],
  9: ['###', '#.#', '###', '..#', '###'],
  ' ': ['...', '...', '...', '...', '...'],
  ':': ['...', '.#.', '...', '.#.', '...'],
  '-': ['...', '...', '###', '...', '...'],
  '#': ['#.#', '###', '#.#', '###', '#.#'],
};

export function drawText(cv, x, y, text, { scale = 2, color = [230, 230, 240], alpha = 1 } = {}) {
  let cx = x;
  for (const ch of String(text).toUpperCase()) {
    const glyph = FONT[ch] || FONT[' '];
    for (let gy = 0; gy < 5; gy++) {
      for (let gx = 0; gx < 3; gx++) {
        if (glyph[gy][gx] !== '#') continue;
        for (let sy = 0; sy < scale; sy++) {
          for (let sx = 0; sx < scale; sx++) {
            blend(cv, cx + gx * scale + sx, y + gy * scale + sy, color[0], color[1], color[2], alpha);
          }
        }
      }
    }
    cx += 4 * scale;
  }
  return cx - x; // drawn width
}

export function textWidth(text, scale = 2) {
  return String(text).length * 4 * scale - scale;
}

// ---------------------------------------------------------------------------
// Mascot lookup from bot usernames ("#9 DeepSeek: DeepSeek V4 Flash 0423")
// ---------------------------------------------------------------------------

let mascotKeys = null;
function registryKeys() {
  if (!mascotKeys) {
    const p = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../arena/mascots.json');
    mascotKeys = Object.keys(JSON.parse(readFileSync(p, 'utf8')).mascots);
  }
  return mascotKeys;
}

const norm = (s) => String(s || '').toLowerCase().replace(/[^a-z0-9]/g, '');

/** Strip the leading "#N " exhibition prefix. */
export function stripRank(username) {
  return String(username || '').replace(/^#\d+\s+/, '');
}

/** Resolve a bot username to {emoji,title,color,key} via normalized substring match. */
export function mascotForName(username) {
  const name = norm(stripRank(username));
  let best = null;
  for (const key of registryKeys()) {
    // Match on the model tail (after vendor/) so "deepseek/deepseek-v4-flash"
    // matches "DeepSeek: DeepSeek V4 Flash 0423".
    const tail = norm(key.split('/').pop());
    if (tail && name.includes(tail) && (!best || tail.length > norm(best.split('/').pop()).length)) {
      best = key;
    }
  }
  if (best) return mascotFor(best);
  return mascotFor(stripRank(username)); // deterministic fallback
}

// ---------------------------------------------------------------------------
// PNG encoding
// ---------------------------------------------------------------------------

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0);
  out.write(type, 4, 'ascii');
  data.copy(out, 8);
  out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length);
  return out;
}

/** Encode an RGBA canvas as a PNG Buffer. */
export function encodePng(cv) {
  const { width, height, data } = cv;
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // color type RGBA
  const raw = Buffer.alloc(height * (1 + width * 4));
  for (let y = 0; y < height; y++) {
    const rowStart = y * (1 + width * 4);
    raw[rowStart] = 0; // filter: none
    Buffer.from(data.buffer, y * width * 4, width * 4).copy(raw, rowStart + 1);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 6 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

// ---------------------------------------------------------------------------
// Scene rendering
// ---------------------------------------------------------------------------

// Pre-rendered dark void background: #05070f with a subtle vertical purple gradient.
let bgTemplate = null;
function background() {
  if (bgTemplate) return bgTemplate;
  const cv = createCanvas();
  const top = hex('#05070f');
  const bottom = [0x16, 0x0d, 0x2b]; // subtle purple
  for (let y = 0; y < cv.height; y++) {
    const f = y / (cv.height - 1);
    const r = top[0] + (bottom[0] - top[0]) * f;
    const g = top[1] + (bottom[1] - top[1]) * f;
    const b = top[2] + (bottom[2] - top[2]) * f;
    for (let x = 0; x < cv.width; x++) {
      const i = (y * cv.width + x) * 4;
      cv.data[i] = r; cv.data[i + 1] = g; cv.data[i + 2] = b; cv.data[i + 3] = 255;
    }
  }
  bgTemplate = cv;
  return cv;
}

function frameAtOrBefore(frames, t) {
  let lo = 0, hi = frames.length - 1, ans = 0;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (frames[mid].t <= t) { ans = mid; lo = mid + 1; } else hi = mid - 1;
  }
  return frames[ans];
}

/**
 * Precompute per-clip rendering context: world bounds, kill flashes,
 * featured players (top killers, else first seen), world->screen transform.
 */
export function prepareScene(replay, { startMs, endMs, kills = [], killCounts = new Map() }) {
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  for (const f of replay.frames) {
    if (f.t < startMs || f.t > endMs) continue;
    for (const p of f.players) {
      if (p.x < minX) minX = p.x;
      if (p.x > maxX) maxX = p.x;
      if (p.y < minY) minY = p.y;
      if (p.y > maxY) maxY = p.y;
    }
  }
  if (!isFinite(minX)) { minX = -1000; maxX = 1000; minY = -1000; maxY = 1000; }
  const pad = 80;
  minX -= pad; maxX += pad; minY -= pad; maxY += pad;

  const areaW = WIDTH, areaH = HEIGHT - HUD_H;
  const scale = Math.min(areaW / (maxX - minX), areaH / (maxY - minY));
  const cx = (minX + maxX) / 2, cy = (minY + maxY) / 2;
  const toScreen = (wx, wy) => [
    WIDTH / 2 + (wx - cx) * scale,
    HUD_H + areaH / 2 - (wy - cy) * scale, // +y is up
  ];

  const flashes = kills
    .filter((k) => k.t >= startMs && k.t <= endMs)
    .map((k) => ({ t: k.t, x: k.x, y: k.y }));

  // Featured players: most kills in clip, tiebreak by name; pad with first seen.
  const seen = new Map(); // id -> name
  for (const f of replay.frames) {
    if (f.t > endMs) break;
    for (const p of f.players) if (!seen.has(p.id)) seen.set(p.id, p.name);
  }
  const featured = [...seen.entries()]
    .map(([id, name]) => ({ id, name, kills: killCounts.get(id) || 0 }))
    .sort((a, b) => b.kills - a.kills || a.name.localeCompare(b.name))
    .slice(0, 4)
    .map(({ name, kills }) => {
      const m = mascotForName(name);
      const rank = (String(name).match(/^#(\d+)/) || [])[1];
      return { label: `${rank ? `#${rank} ` : ''}${m.title || 'BOT'}`, color: hex(m.color || '#94a3b8'), kills };
    });

  return { startMs, endMs, toScreen, flashes, featured, headings: new Map(), scale };
}

/** Render one frame of the clip at absolute replay time tMs into a fresh canvas. */
export function renderFrame(replay, scene, tMs) {
  const cv = createCanvas();
  cv.data.set(background().data);

  // Subtle world grid (200-unit spacing) for spatial reference.
  for (let gx = -1000; gx <= 1000; gx += 200) {
    for (let gy = -1000; gy <= 1000; gy += 200) {
      const [px, py] = scene.toScreen(gx, gy);
      if (px >= 0 && px < WIDTH && py >= HUD_H && py < HEIGHT) {
        blend(cv, px, py, 90, 80, 140, 0.18);
      }
    }
  }

  const frame = frameAtOrBefore(replay.frames, tMs);

  // Motion trails: last TRAIL_LEN positions (~60ms apart), fading.
  for (let k = TRAIL_LEN; k >= 1; k--) {
    const tf = frameAtOrBefore(replay.frames, tMs - k * 60);
    const a = 0.35 * (1 - k / (TRAIL_LEN + 1));
    for (const p of tf.players) {
      if (!p.alive) continue;
      const [px, py] = scene.toScreen(p.x, p.y);
      disc(cv, px, py, 4 - k * 0.3, TEAM_COLORS[p.team] || FALLBACK_COLOR, a);
    }
  }

  // Players as team-colored chevrons rotated by velocity.
  for (const p of frame.players) {
    if (!p.alive) continue;
    const speed = Math.hypot(p.vx, p.vy);
    let heading = scene.headings.get(p.id) || 0;
    if (speed > 1) {
      heading = Math.atan2(-p.vy, p.vx); // screen y is flipped
      scene.headings.set(p.id, heading);
    }
    const [px, py] = scene.toScreen(p.x, p.y);
    const cos = Math.cos(heading), sin = Math.sin(heading);
    const rot = ([x, y]) => [px + x * cos - y * sin, py + x * sin + y * cos];
    const nose = rot([13, 0]), tailL = rot([-9, 8]), notch = rot([-4, 0]), tailR = rot([-9, -8]);
    const col = TEAM_COLORS[p.team] || FALLBACK_COLOR;
    fillTri(cv, nose, tailL, notch, col, 1);
    fillTri(cv, nose, notch, tailR, col, 1);
    // Low-hp white core so damage reads at a glance.
    if (p.hp <= 30) disc(cv, px, py, 3, [255, 255, 255], 0.9);
  }

  // Kill flash rings: expanding white ring over KILL_FLASH_MS.
  for (const fl of scene.flashes) {
    const dt = tMs - fl.t;
    if (dt < 0 || dt > KILL_FLASH_MS) continue;
    const f = dt / KILL_FLASH_MS;
    const [px, py] = scene.toScreen(fl.x, fl.y);
    ring(cv, px, py, 8 + f * 46, 3, [255, 255, 255], 0.9 * (1 - f));
  }

  drawHud(cv, replay, scene, tMs, frame);
  return cv;
}

function drawHud(cv, replay, scene, tMs, frame) {
  fillRect(cv, 0, 0, WIDTH, HUD_H, [10, 12, 26], 0.92);
  fillRect(cv, 0, HUD_H - 2, WIDTH, 2, [80, 60, 140], 0.6);

  // Featured players: mascot color dot + title (emoji cannot be rasterized).
  let x = 16;
  for (const feat of scene.featured) {
    disc(cv, x + 7, HUD_H / 2, 7, feat.color, 1);
    ring(cv, x + 7, HUD_H / 2, 7, 1.5, [255, 255, 255], 0.5);
    const w = drawText(cv, x + 20, HUD_H / 2 - 5, feat.label, { scale: 2 });
    x += 20 + w + 22;
  }

  // Match clock (remaining) + alive counts per team, right-aligned.
  const remainMs = Math.max(0, replay.durationMs - tMs);
  const mm = Math.floor(remainMs / 60000);
  const ss = String(Math.floor((remainMs % 60000) / 1000)).padStart(2, '0');
  const clock = `${mm}:${ss}`;
  drawText(cv, WIDTH - 16 - textWidth(clock, 3), 10, clock, { scale: 3, color: [240, 240, 250] });

  const aliveByTeam = new Map();
  for (const p of frame.players) {
    if (p.alive) aliveByTeam.set(p.team, (aliveByTeam.get(p.team) || 0) + 1);
  }
  let ax = WIDTH - 16;
  for (const team of [...aliveByTeam.keys()].sort((a, b) => b - a)) {
    const label = String(aliveByTeam.get(team));
    ax -= textWidth(label, 2) + 8;
    drawText(cv, ax, HUD_H - 26, label, { scale: 2, color: TEAM_COLORS[team] || FALLBACK_COLOR });
    ax -= 8;
    disc(cv, ax, HUD_H - 21, 4, TEAM_COLORS[team] || FALLBACK_COLOR, 1);
    ax -= 16;
  }
}
