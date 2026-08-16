#!/usr/bin/env node
/**
 * Generates footstep / material-impact SFX for the client as 44.1kHz mono
 * 16-bit PCM wavs (RIFF header written by hand, no dependencies).
 *
 * Usage: node scripts/media/gen_sfx.mjs
 * Output: static_client/sfx/{footstep_a,footstep_b,impact_soft,impact_hard}.wav
 *
 * Deterministic: noise comes from a seeded PRNG, so re-running produces
 * byte-identical files.
 */

import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const SAMPLE_RATE = 44100;
const PEAK = 0.7;
const OUT_DIR = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'static_client', 'sfx');

// Seeded PRNG (mulberry32) for reproducible noise.
function makeRng(seed) {
    let state = seed >>> 0;
    return () => {
        state = (state + 0x6d2b79f5) >>> 0;
        let t = state;
        t = Math.imul(t ^ (t >>> 15), t | 1);
        t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
        return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
}

function makeNoise(rng, length) {
    const data = new Float64Array(length);
    for (let i = 0; i < length; i += 1) {
        data[i] = rng() * 2 - 1;
    }
    return data;
}

// One-pole low-pass: smooths toward the input with cutoff hz.
function lowpass(samples, cutoffHz) {
    const alpha = 1 - Math.exp((-2 * Math.PI * cutoffHz) / SAMPLE_RATE);
    const out = new Float64Array(samples.length);
    let prev = 0;
    for (let i = 0; i < samples.length; i += 1) {
        prev += alpha * (samples[i] - prev);
        out[i] = prev;
    }
    return out;
}

// One-pole high-pass: input minus its low-passed self.
function highpass(samples, cutoffHz) {
    const low = lowpass(samples, cutoffHz);
    const out = new Float64Array(samples.length);
    for (let i = 0; i < samples.length; i += 1) {
        out[i] = samples[i] - low[i];
    }
    return out;
}

function sineSweep(length, startHz, endHz) {
    const out = new Float64Array(length);
    let phase = 0;
    for (let i = 0; i < length; i += 1) {
        const t = i / Math.max(1, length - 1);
        const hz = startHz + (endHz - startHz) * t;
        phase += (2 * Math.PI * hz) / SAMPLE_RATE;
        out[i] = Math.sin(phase);
    }
    return out;
}

// Fast attack, exponential decay envelope. attackSec/tauSec in seconds.
function envelope(length, attackSec, tauSec) {
    const out = new Float64Array(length);
    const attackSamples = Math.max(1, Math.round(attackSec * SAMPLE_RATE));
    for (let i = 0; i < length; i += 1) {
        const attack = i < attackSamples ? i / attackSamples : 1;
        out[i] = attack * Math.exp(-i / (tauSec * SAMPLE_RATE));
    }
    return out;
}

function mix(...layers) {
    const length = Math.max(...layers.map((l) => l.length));
    const out = new Float64Array(length);
    for (const layer of layers) {
        for (let i = 0; i < layer.length; i += 1) {
            out[i] += layer[i];
        }
    }
    return out;
}

function scale(samples, gain) {
    const out = new Float64Array(samples.length);
    for (let i = 0; i < samples.length; i += 1) {
        out[i] = samples[i] * gain;
    }
    return out;
}

function multiply(a, b) {
    const out = new Float64Array(Math.min(a.length, b.length));
    for (let i = 0; i < out.length; i += 1) {
        out[i] = a[i] * b[i];
    }
    return out;
}

function normalize(samples, peak = PEAK) {
    let max = 0;
    for (let i = 0; i < samples.length; i += 1) {
        const abs = Math.abs(samples[i]);
        if (abs > max) max = abs;
    }
    if (max <= 0) return samples;
    return scale(samples, peak / max);
}

function writeWav(filePath, samples) {
    const dataSize = samples.length * 2;
    const buffer = Buffer.alloc(44 + dataSize);
    buffer.write('RIFF', 0, 'ascii');
    buffer.writeUInt32LE(36 + dataSize, 4);
    buffer.write('WAVE', 8, 'ascii');
    buffer.write('fmt ', 12, 'ascii');
    buffer.writeUInt32LE(16, 16); // fmt chunk size
    buffer.writeUInt16LE(1, 20); // PCM
    buffer.writeUInt16LE(1, 22); // mono
    buffer.writeUInt32LE(SAMPLE_RATE, 24);
    buffer.writeUInt32LE(SAMPLE_RATE * 2, 28); // byte rate
    buffer.writeUInt16LE(2, 32); // block align
    buffer.writeUInt16LE(16, 34); // bits per sample
    buffer.write('data', 36, 'ascii');
    buffer.writeUInt32LE(dataSize, 40);
    for (let i = 0; i < samples.length; i += 1) {
        const clamped = Math.max(-1, Math.min(1, samples[i]));
        buffer.writeInt16LE(Math.round(clamped * 32767), 44 + i * 2);
    }
    writeFileSync(filePath, buffer);
    return buffer.length;
}

function seconds(len) {
    return Math.round(len * SAMPLE_RATE);
}

// Footstep variant A: mid band noise tap with a soft low thump (concrete-ish).
function genFootstepA() {
    const rng = makeRng(0x5eed0001);
    const noise = multiply(
        lowpass(highpass(makeNoise(rng, seconds(0.09)), 160), 950),
        envelope(seconds(0.09), 0.002, 0.016)
    );
    const thump = multiply(
        sineSweep(seconds(0.06), 120, 70),
        envelope(seconds(0.06), 0.002, 0.014)
    );
    return normalize(mix(scale(noise, 0.85), scale(thump, 0.4)));
}

// Footstep variant B: slightly lower, shorter tap (wood-ish alternate step).
function genFootstepB() {
    const rng = makeRng(0x5eed0002);
    const noise = multiply(
        lowpass(highpass(makeNoise(rng, seconds(0.08)), 220), 720),
        envelope(seconds(0.08), 0.002, 0.013)
    );
    const thump = multiply(
        sineSweep(seconds(0.05), 95, 60),
        envelope(seconds(0.05), 0.002, 0.012)
    );
    return normalize(mix(scale(noise, 0.8), scale(thump, 0.45)));
}

// Soft impact: low sine burst + low-passed noise body, ~0.15s thud.
function genImpactSoft() {
    const rng = makeRng(0x5eed0003);
    const body = multiply(
        sineSweep(seconds(0.15), 85, 42),
        envelope(seconds(0.15), 0.003, 0.038)
    );
    const noise = multiply(
        lowpass(makeNoise(rng, seconds(0.1)), 420),
        envelope(seconds(0.1), 0.002, 0.022)
    );
    return normalize(mix(scale(body, 0.9), scale(noise, 0.35)));
}

// Hard impact: sharper crack — noise burst + high click transient, ~0.2s.
function genImpactHard() {
    const rng = makeRng(0x5eed0004);
    const crack = multiply(
        lowpass(highpass(makeNoise(rng, seconds(0.14)), 350), 3800),
        envelope(seconds(0.14), 0.001, 0.02)
    );
    const click = multiply(
        highpass(makeNoise(rng, seconds(0.02)), 2400),
        envelope(seconds(0.02), 0.0005, 0.004)
    );
    const knock = multiply(
        sineSweep(seconds(0.12), 190, 90),
        envelope(seconds(0.12), 0.001, 0.024)
    );
    return normalize(mix(scale(crack, 0.75), scale(click, 0.5), scale(knock, 0.4)));
}

const generators = {
    'footstep_a.wav': genFootstepA,
    'footstep_b.wav': genFootstepB,
    'impact_soft.wav': genImpactSoft,
    'impact_hard.wav': genImpactHard,
};

mkdirSync(OUT_DIR, { recursive: true });
for (const [fileName, generate] of Object.entries(generators)) {
    const samples = generate();
    const bytes = writeWav(join(OUT_DIR, fileName), samples);
    const durationSec = (samples.length / SAMPLE_RATE).toFixed(3);
    console.log(`wrote ${fileName}: ${durationSec}s, ${samples.length} samples, ${bytes} bytes`);
}
