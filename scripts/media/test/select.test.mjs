import { test } from 'node:test';
import assert from 'node:assert/strict';
import { selectHighlights, killEvents } from '../lib/select.mjs';

// Build a synthetic normalized replay: 2 bots die sparsely early, then a
// 4-kill cluster near 60s, 90s total, 20fps.
function makeReplay() {
  const deaths = [
    { id: 'a', t: 10_000, x: 0, y: 0, team: 2 },
    { id: 'b', t: 25_000, x: 100, y: 0, team: 2 },
    { id: 'c', t: 58_000, x: 200, y: 50, team: 2 },
    { id: 'd', t: 59_500, x: 210, y: 60, team: 1 },
    { id: 'e', t: 61_000, x: 190, y: 40, team: 2 },
    { id: 'f', t: 62_000, x: 205, y: 55, team: 1 },
  ];
  const ids = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'];
  const frames = [];
  for (let t = 0; t <= 90_000; t += 50) {
    frames.push({
      t,
      players: ids.map((id, i) => {
        const death = deaths.find((d) => d.id === id);
        const alive = !death || t < death.t;
        return {
          id, name: `#${i + 1} Bot ${id}`, x: i * 100 - 350, y: (i % 2) * 200 - 100,
          vx: 10, vy: 5, hp: alive ? 80 : 0, alive, team: i < 4 ? 1 : 2,
        };
      }),
    });
  }
  return { frames, durationMs: 90_000, meta: {}, deaths };
}

test('kill events are detected as alive->dead transitions', () => {
  const replay = makeReplay();
  const kills = killEvents(replay);
  assert.equal(kills.length, 6);
  assert.deepEqual(kills.map((k) => k.t), [10_000, 25_000, 58_000, 59_500, 61_000, 62_000]);
});

test('kill cluster beats sparse kills', () => {
  const replay = makeReplay();
  const clips = selectHighlights(replay, { maxClips: 3 });
  assert.ok(clips.length >= 2);
  const clusterClip = clips.find((c) => c.startMs <= 58_000 && c.endMs >= 62_000);
  assert.ok(clusterClip, 'expected a clip covering the 58-62s cluster');
  assert.match(clusterClip.reason, /Kill cluster/);
  // The cluster clip must outscore every clip that contains only sparse kills.
  const sparseClip = clips.find((c) => c !== clusterClip && c.startMs <= 10_000 && c.endMs >= 10_000);
  if (sparseClip) assert.ok(clusterClip.score > sparseClip.score);
});

test('window bounds are sane', () => {
  const replay = makeReplay();
  const clips = selectHighlights(replay, { maxClips: 3 });
  assert.ok(clips.length > 0);
  for (const c of clips) {
    assert.ok(c.startMs >= 0, `startMs ${c.startMs} >= 0`);
    assert.ok(c.endMs <= replay.durationMs, `endMs ${c.endMs} <= duration`);
    const len = c.endMs - c.startMs;
    assert.ok(len >= 10_000 && len <= 45_000, `length ${len} in [10s, 45s]`);
    assert.ok(typeof c.reason === 'string' && c.reason.length > 0);
    assert.ok(Number.isFinite(c.score));
  }
  // No pair overlaps by more than 30%.
  for (let i = 0; i < clips.length; i++) {
    for (let j = i + 1; j < clips.length; j++) {
      const inter = Math.min(clips[i].endMs, clips[j].endMs) - Math.max(clips[i].startMs, clips[j].startMs);
      const minLen = Math.min(clips[i].endMs - clips[i].startMs, clips[j].endMs - clips[j].startMs);
      assert.ok(inter <= 0.3 * minLen + 1, `clips ${i} and ${j} overlap too much`);
    }
  }
});

test('short replay returns whole match', () => {
  const replay = { frames: [{ t: 0, players: [] }, { t: 5_000, players: [] }], durationMs: 5_000, meta: {} };
  const clips = selectHighlights(replay, { maxClips: 3 });
  assert.deepEqual(clips, [{ startMs: 0, endMs: 5_000, reason: 'Full match', score: 1 }]);
});
