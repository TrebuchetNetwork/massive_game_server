import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createCanvas, encodePng, drawText, prepareScene, renderFrame, mascotForName, WIDTH, HEIGHT } from '../lib/raster.mjs';

function parseChunks(png) {
  assert.deepEqual([...png.subarray(0, 8)], [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], 'PNG magic');
  const chunks = [];
  let off = 8;
  while (off < png.length) {
    const len = png.readUInt32BE(off);
    chunks.push({ type: png.toString('ascii', off + 4, off + 8), data: png.subarray(off + 8, off + 8 + len) });
    off += 12 + len;
  }
  return chunks;
}

test('PNG magic bytes, IHDR round-trip, non-empty IDAT', () => {
  const cv = createCanvas(64, 32);
  drawText(cv, 2, 2, 'HP: 100', { scale: 2, color: [255, 0, 0] });
  const png = encodePng(cv);
  const chunks = parseChunks(png);
  const ihdr = chunks.find((c) => c.type === 'IHDR');
  assert.equal(ihdr.data.readUInt32BE(0), 64, 'IHDR width');
  assert.equal(ihdr.data.readUInt32BE(4), 32, 'IHDR height');
  assert.equal(ihdr.data[8], 8, 'bit depth');
  assert.equal(ihdr.data[9], 6, 'color type RGBA');
  const idat = chunks.find((c) => c.type === 'IDAT');
  assert.ok(idat && idat.data.length > 20, 'IDAT non-empty');
  assert.ok(chunks.some((c) => c.type === 'IEND'));
});

function tinyReplay() {
  const frames = [];
  for (let t = 0; t <= 20_000; t += 50) {
    frames.push({
      t,
      players: [
        { id: 'p1', name: '#1 DeepSeek: DeepSeek V4 Pro', x: -400 + t / 50, y: 0, vx: 80, vy: 0, hp: 90, alive: t < 12_000, team: 1 },
        { id: 'p2', name: '#7 Tencent: Hy3', x: 400 - t / 60, y: 100, vx: -70, vy: 10, hp: 40, alive: true, team: 2 },
      ],
    });
  }
  return { frames, durationMs: 20_000, meta: {} };
}

test('renderFrame produces a non-trivial scene', () => {
  const replay = tinyReplay();
  const kills = [{ t: 12_000, id: 'p1', name: 'p1', x: -160, y: 0, team: 1 }];
  const scene = prepareScene(replay, { startMs: 0, endMs: 20_000, kills, killCounts: new Map([['p2', 1]]) });
  const countColors = (cv) => {
    const d = cv.data;
    let cyan = 0, red = 0, white = 0, bg = 0;
    for (let i = 0; i < d.length; i += 4) {
      const [r, g, b] = [d[i], d[i + 1], d[i + 2]];
      if (r < 60 && g > 180 && b > 200) cyan++;
      else if (r > 200 && g < 90 && b < 120) red++;
      else if (r > 230 && g > 230 && b > 230) white++;
      else if (b > r && r < 40) bg++;
    }
    return { cyan, red, white, bg };
  };

  // Mid-fight frame: both players alive as team-colored chevrons.
  const mid = countColors(renderFrame(replay, scene, 11_000));
  assert.ok(mid.cyan > 20, `cyan pixels: ${mid.cyan}`);
  assert.ok(mid.red > 20, `red pixels: ${mid.red}`);
  assert.ok(mid.bg > WIDTH * HEIGHT * 0.5, `background dominates: ${mid.bg}`);

  // Just after the kill: white flash ring visible.
  const flash = countColors(renderFrame(replay, scene, 12_200));
  assert.ok(flash.white > 5, `white flash/hud pixels: ${flash.white}`);

  const png = encodePng(renderFrame(replay, scene, 11_000));
  parseChunks(png); // valid structure
});

test('mascotForName resolves exhibition usernames', () => {
  const m = mascotForName('#9 DeepSeek: DeepSeek V4 Flash 0423');
  assert.equal(m.title, 'Surge');
  assert.equal(typeof m.color, 'string');
  const fallback = mascotForName('Some Unknown Bot');
  assert.ok(fallback.title && fallback.color);
});
