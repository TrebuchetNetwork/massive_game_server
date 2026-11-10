import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadReplay } from '../lib/replay.mjs';
import { selectHighlights } from '../lib/select.mjs';

const SCRIPT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../render_highlights.mjs');

function harness(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'render-highlights-index-'));
  const outDir = path.join(root, 'highlights');
  const replayDir = path.join(root, 'replays');
  fs.mkdirSync(outDir, { recursive: true });
  fs.mkdirSync(replayDir, { recursive: true });
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));

  const date = new Date().toISOString().slice(0, 10);
  const staleDate = new Date(Date.now() - 86_400_000).toISOString().slice(0, 10);
  const clip = (clipDate, name) => ({
    date: clipDate,
    webm: `${clipDate}/${name}.webm`,
    gif: `${clipDate}/${name}.gif`,
    poster: `${clipDate}/${name}.png`,
    reason: 'test',
    score: 1,
    players: [],
  });
  const writeAssets = (entry, keys = ['webm', 'gif', 'poster']) => {
    for (const key of keys) {
      const target = path.join(outDir, entry[key]);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(target, '');
    }
  };
  const run = ({ entries, withReplay = true }) => {
    fs.writeFileSync(path.join(outDir, 'index.json'), JSON.stringify(entries));
    if (withReplay) fs.writeFileSync(path.join(replayDir, 'replay_invalid.json.zst'), 'invalid');
    const result = spawnSync(process.execPath, [
      SCRIPT, '--replay-dir', replayDir, '--out-dir', outDir,
    ], { encoding: 'utf8', timeout: 30_000 });
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    return JSON.parse(fs.readFileSync(path.join(outDir, 'index.json'), 'utf8'));
  };
  return { outDir, date, staleDate, clip, writeAssets, run };
}

test('renderer prunes an entry whose asset files are missing', (t) => {
  const h = harness(t);
  const valid = h.clip(h.date, 'valid');
  const broken = h.clip(h.staleDate, 'broken');
  h.writeAssets(valid);
  fs.mkdirSync(path.join(h.outDir, h.staleDate), { recursive: true });
  const index = h.run({ entries: [valid, broken] });
  assert.deepEqual(index.map((entry) => entry.webm), [valid.webm]);
});

test('renderer prunes an entry whose date directory is absent', (t) => {
  const h = harness(t);
  const valid = h.clip(h.date, 'valid');
  const broken = h.clip(h.staleDate, 'broken');
  h.writeAssets(valid);
  const index = h.run({ entries: [valid, broken] });
  assert.deepEqual(index.map((entry) => entry.webm), [valid.webm]);
});

test('renderer prunes a partial webm gif poster triplet', (t) => {
  const h = harness(t);
  const valid = h.clip(h.date, 'valid');
  const partial = h.clip(h.staleDate, 'partial');
  h.writeAssets(valid);
  h.writeAssets(partial, ['webm', 'poster']);
  const index = h.run({ entries: [valid, partial] });
  assert.deepEqual(index.map((entry) => entry.webm), [valid.webm]);
});

test('renderer persists a cleaned index when no new replay files exist', (t) => {
  const h = harness(t);
  const valid = h.clip(h.date, 'valid');
  const broken = h.clip(h.staleDate, 'broken');
  h.writeAssets(valid);
  const index = h.run({ entries: [valid, broken], withReplay: false });
  assert.deepEqual(index.map((entry) => entry.webm), [valid.webm]);
});

test('renderer does not re-encode a complete existing clip', async (t) => {
  const fixtureDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), 'fixtures');
  const replay = await loadReplay(path.join(fixtureDir, 'synthetic_match.json.zst'));
  const [clip] = selectHighlights(replay, { maxClips: 1 });
  assert.ok(clip, 'synthetic replay must produce a highlight');

  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'render-highlights-existing-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const replayDir = path.join(root, 'replays');
  const outDir = path.join(root, 'highlights');
  fs.mkdirSync(replayDir, { recursive: true });
  const replayMs = Date.now();
  const replayBase = `replay_${replayMs}_test`;
  fs.copyFileSync(
    path.join(fixtureDir, 'synthetic_match.json.zst'),
    path.join(replayDir, `${replayBase}.json.zst`),
  );
  const date = new Date(replayMs).toISOString().slice(0, 10);
  const outBase = path.join(outDir, date, `${replayBase}_${clip.startMs}-${clip.endMs}`);
  for (const ext of ['.webm', '.gif', '.png']) {
    fs.mkdirSync(path.dirname(outBase), { recursive: true });
    fs.writeFileSync(`${outBase}${ext}`, `sentinel${ext}`);
  }

  const result = spawnSync(process.execPath, [
    SCRIPT, '--replay-dir', replayDir, '--out-dir', outDir, '--max-clips', '1', '--fps', '1',
  ], { encoding: 'utf8', timeout: 120_000 });
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  for (const ext of ['.webm', '.gif', '.png']) {
    assert.equal(fs.readFileSync(`${outBase}${ext}`, 'utf8'), `sentinel${ext}`, `${ext} was re-encoded`);
  }
  const index = JSON.parse(fs.readFileSync(path.join(outDir, 'index.json'), 'utf8'));
  assert.equal(index.length, 1);
  assert.match(result.stdout, /skipping .*existing/);
});
