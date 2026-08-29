import { test } from 'node:test';
import assert from 'node:assert/strict';
import { renderTrack, encodeWav, midiToFreq, SAMPLE_RATE, PEAK, TRACKS } from '../gen_music.mjs';

// Small 4-bar config so the test renders fast; mirrors a real track's shape.
function tinyConfig() {
  return {
    seed: 0x7e57,
    bpm: 120,
    bars: 4,
    chordBars: 2,
    chords: [
      { bass: 33, pad: [57, 60, 64], arp: [57, 60, 64] },
      { bass: 29, pad: [53, 57, 60], arp: [53, 57, 60] },
    ],
    patterns: { bass: 'drive8', kick: 'four', hat: 'eighth', snare: 'backbeat', hatOpen: true, fill: true },
    arp: { everySteps: 2, mode: 'up', octaveUpEvery4: true, panWidth: 0.4, wave: 'square', attack: 0.003, decay: 0.12, sustain: 0.1, release: 0.08 },
    bass: { attack: 0.004, decay: 0.15, sustain: 0.5, release: 0.06, cutoffBase: 260, cutoffEnv: 2200, cutoffDecay: 0.07, subGain: 0.5 },
    pad: { attack: 0.3, decay: 0.6, sustain: 0.6, release: 0.4, detuneCents: 10, cutoff: 1800 },
    gains: { bass: 0.4, arp: 0.15, pad: 0.08, kick: 0.6, hat: 0.09, snare: 0.4 },
    drive: 1.3,
  };
}

function expectedSamples(cfg) {
  return Math.round(cfg.bars * (60 / cfg.bpm) * 4 * SAMPLE_RATE);
}

test('renderTrack produces the configured duration', () => {
  const cfg = tinyConfig();
  const track = renderTrack(cfg);
  assert.equal(track.left.length, expectedSamples(cfg));
  assert.equal(track.right.length, expectedSamples(cfg));
  assert.ok(Math.abs(track.seconds - 8) < 0.01, `4 bars @120bpm = 8s, got ${track.seconds}`);
});

test('output is deterministic for the same seed', () => {
  const a = renderTrack(tinyConfig());
  const b = renderTrack(tinyConfig());
  assert.deepEqual([...a.left.slice(0, 5000)], [...b.left.slice(0, 5000)]);
  assert.deepEqual([...a.right.slice(0, 5000)], [...b.right.slice(0, 5000)]);
});

test('different seeds produce different output', () => {
  const a = renderTrack(tinyConfig());
  const cfg = tinyConfig();
  cfg.seed = 0x7e58;
  const b = renderTrack(cfg);
  assert.notDeepEqual([...a.left.slice(0, 5000)], [...b.left.slice(0, 5000)]);
});

test('master limiter keeps peak <= PEAK on both channels', () => {
  const track = renderTrack(tinyConfig());
  let maxL = 0;
  let maxR = 0;
  for (let i = 0; i < track.left.length; i += 1) {
    if (Math.abs(track.left[i]) > maxL) maxL = Math.abs(track.left[i]);
    if (Math.abs(track.right[i]) > maxR) maxR = Math.abs(track.right[i]);
  }
  assert.ok(maxL <= PEAK + 1e-9, `left peak ${maxL}`);
  assert.ok(maxR <= PEAK + 1e-9, `right peak ${maxR}`);
  assert.ok(maxL > 0.5, 'signal actually present after normalization');
});

test('encodeWav writes a valid 44.1kHz stereo 16-bit RIFF header', () => {
  const cfg = tinyConfig();
  const track = renderTrack(cfg);
  const wav = encodeWav(track);
  assert.equal(wav.toString('ascii', 0, 4), 'RIFF');
  assert.equal(wav.toString('ascii', 8, 12), 'WAVE');
  assert.equal(wav.toString('ascii', 12, 16), 'fmt ');
  assert.equal(wav.readUInt16LE(20), 1, 'PCM format');
  assert.equal(wav.readUInt16LE(22), 2, 'stereo');
  assert.equal(wav.readUInt32LE(24), 44100, 'sample rate');
  assert.equal(wav.readUInt16LE(34), 16, 'bits per sample');
  assert.equal(wav.toString('ascii', 36, 40), 'data');
  const dataSize = wav.readUInt32LE(40);
  assert.equal(dataSize, track.left.length * 4);
  assert.equal(wav.length, 44 + dataSize);
  assert.equal(wav.readUInt32LE(4), 36 + dataSize, 'RIFF chunk size');
});

test('loop point is smooth: first and last 100ms RMS are similar', () => {
  const track = renderTrack(tinyConfig());
  const win = Math.round(0.1 * SAMPLE_RATE);
  const rms = (ch, from) => {
    let sum = 0;
    for (let i = from; i < from + win; i += 1) sum += ch[i] * ch[i];
    return Math.sqrt(sum / win);
  };
  const startRms = (rms(track.left, 0) + rms(track.right, 0)) / 2;
  const endRms = (rms(track.left, track.left.length - win) + rms(track.right, track.right.length - win)) / 2;
  assert.ok(startRms > 0.001 && endRms > 0.001, 'both ends carry signal');
  const ratio = endRms / startRms;
  assert.ok(ratio > 0.5 && ratio < 2.0, `loop-end/start RMS ratio ${ratio.toFixed(2)}`);
});

test('midiToFreq maps A4 to 440Hz', () => {
  assert.ok(Math.abs(midiToFreq(69) - 440) < 1e-9);
  assert.ok(Math.abs(midiToFreq(57) - 220) < 1e-9);
});

test('all shipped track configs render within the 60-90s target window', () => {
  for (const [name, cfg] of Object.entries(TRACKS)) {
    const seconds = (cfg.bars * 4 * 60) / cfg.bpm;
    assert.ok(seconds >= 60 && seconds <= 90, `${name}: ${seconds.toFixed(1)}s out of range`);
  }
});
