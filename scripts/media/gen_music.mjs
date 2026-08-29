#!/usr/bin/env node
/**
 * Generates dark synthwave / chiptune soundtrack loops for the client as
 * 44.1kHz stereo 16-bit PCM wavs (RIFF header written by hand, no deps).
 *
 * Usage: node scripts/media/gen_music.mjs [--out DIR]
 * Output: <DIR>/{arena-drift,analog-dreams,pulse-runner,redline}.wav
 *   (default DIR: scripts/media/build — encode to mp3 separately with the
 *   vendored ffmpeg, e.g. -codec:a libmp3lame -b:a 160k)
 *
 * Deterministic: all randomness comes from seeded PRNGs, so re-running
 * produces byte-identical files. Loops are seamless: every phrase is
 * scheduled on the 16-step bar grid, all note envelopes close inside their
 * bar, and there are no delay/reverb tails, so the last bar flows into the
 * first. Master chain: DC block -> soft clip (tanh) -> normalize to PEAK.
 */

import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const SAMPLE_RATE = 44100;
export const PEAK = 0.8;

// ---------------------------------------------------------------------------
// PRNG and basic DSP helpers (mulberry32, same as gen_sfx.mjs).
// ---------------------------------------------------------------------------

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

export function midiToFreq(midi) {
    return 440 * Math.pow(2, (midi - 69) / 12);
}

// PolyBLEP residual for band-limited saw/square (kills aliasing harshness).
function polyblep(t, dt) {
    if (t < dt) {
        const x = t / dt;
        return x + x - x * x - 1;
    }
    if (t > 1 - dt) {
        const x = (t - 1) / dt;
        return x * x + x + x + 1;
    }
    return 0;
}

function oscSaw(phase, dt) {
    return 2 * phase - 1 - polyblep(phase, dt);
}

function oscSquare(phase, dt, duty = 0.5) {
    let v = phase < duty ? 1 : -1;
    v += polyblep(phase, dt);
    let t2 = phase - duty;
    if (t2 < 0) t2 += 1;
    v -= polyblep(t2, dt);
    return v;
}

function oscTri(phase) {
    return 4 * Math.abs(phase - Math.floor(phase + 0.5)) - 1;
}

// Classic ADSR. Times in seconds, sustain as level 0..1, note length n samples.
function adsrGain(i, n, a, d, s, r) {
    const aN = Math.max(1, Math.round(a * SAMPLE_RATE));
    const dN = Math.max(1, Math.round(d * SAMPLE_RATE));
    const rN = Math.max(1, Math.round(r * SAMPLE_RATE));
    const relStart = Math.max(aN, n - rN);
    if (i >= relStart) {
        const level = sustainAt(relStart, aN, dN, s);
        return level * Math.max(0, 1 - (i - relStart) / rN);
    }
    return sustainAt(i, aN, dN, s);
}

function sustainAt(i, aN, dN, s) {
    if (i < aN) return i / aN;
    const di = i - aN;
    return s + (1 - s) * Math.exp((-4 * di) / dN); // ~s after d seconds
}

// Equal-power pan of a mono sample into the stereo bus.
function panAdd(L, R, idx, sample, pan) {
    const angle = ((pan + 1) * Math.PI) / 4; // pan -1..1 -> 0..pi/2
    L[idx] += sample * Math.cos(angle);
    R[idx] += sample * Math.sin(angle);
}

// ---------------------------------------------------------------------------
// Voices. Each renders one event straight into the stereo bus.
// ---------------------------------------------------------------------------

// Bass: saw + square sub one octave down, one-pole lowpass whose cutoff is
// driven by a fast decay envelope (acid-ish pluck). Mono, panned center.
function renderBassNote(L, R, start, freq, durSec, cfg) {
    const n = Math.round(durSec * SAMPLE_RATE);
    const dt = freq / SAMPLE_RATE;
    const { attack, decay, sustain, release, cutoffBase, cutoffEnv, cutoffDecay, subGain } = cfg.bass;
    let phase = 0;
    let subPhase = 0;
    let lp = 0;
    for (let i = 0; i < n; i += 1) {
        const idx = start + i;
        if (idx >= L.length) break;
        phase += dt;
        if (phase >= 1) phase -= 1;
        subPhase += dt / 2;
        if (subPhase >= 1) subPhase -= 1;
        const env = adsrGain(i, n, attack, decay, sustain, release);
        const cutoff = cutoffBase + cutoffEnv * Math.exp(-i / (cutoffDecay * SAMPLE_RATE));
        const alpha = 1 - Math.exp((-2 * Math.PI * Math.min(cutoff, 12000)) / SAMPLE_RATE);
        const raw = oscSaw(phase, dt) * 0.65 + oscSquare(subPhase, dt / 2) * subGain;
        lp += alpha * (raw - lp);
        const s = lp * env * cfg.gains.bass;
        L[idx] += s;
        R[idx] += s;
    }
}

// Arp: square or triangle blip, short envelope, slight random detune per note.
function renderArpNote(L, R, start, freq, durSec, cfg, rng, pan) {
    const n = Math.round(durSec * SAMPLE_RATE);
    const detune = 1 + (rng() - 0.5) * 0.004;
    const f = freq * detune;
    const dt = f / SAMPLE_RATE;
    const { attack, decay, sustain, release, wave } = cfg.arp;
    let phase = rng(); // random phase so repeated notes don't phase-lock
    for (let i = 0; i < n; i += 1) {
        const idx = start + i;
        if (idx >= L.length) break;
        phase += dt;
        if (phase >= 1) phase -= 1;
        const osc = wave === 'tri' ? oscTri(phase) : oscSquare(phase, dt);
        const s = osc * adsrGain(i, n, attack, decay, sustain, release) * cfg.gains.arp;
        panAdd(L, R, idx, s, pan);
    }
}

// Pad: per chord tone, two detuned saws panned apart; slow attack, bar-long.
function renderPadChord(L, R, start, midiNotes, durSec, cfg) {
    const n = Math.round(durSec * SAMPLE_RATE);
    const { attack, decay, sustain, release, detuneCents, cutoff } = cfg.pad;
    for (let v = 0; v < midiNotes.length; v += 1) {
        const base = midiToFreq(midiNotes[v]);
        const spread = Math.pow(2, detuneCents / 1200);
        const pan = v % 2 === 0 ? -0.35 : 0.35;
        for (const mul of [1 / spread, spread]) {
            const f = base * mul;
            const dt = f / SAMPLE_RATE;
            let phase = 0;
            let lp = 0;
            const alpha = 1 - Math.exp((-2 * Math.PI * cutoff) / SAMPLE_RATE);
            for (let i = 0; i < n; i += 1) {
                const idx = start + i;
                if (idx >= L.length) break;
                phase += dt;
                if (phase >= 1) phase -= 1;
                lp += alpha * (oscSaw(phase, dt) - lp);
                const s = lp * adsrGain(i, n, attack, decay, sustain, release) * cfg.gains.pad;
                panAdd(L, R, idx, s, mul < 1 ? pan : -pan);
            }
        }
    }
}

// Kick: sine sweep 150->45Hz with a click transient.
function renderKick(L, R, start, cfg) {
    const n = Math.round(0.14 * SAMPLE_RATE);
    let phase = 0;
    for (let i = 0; i < n; i += 1) {
        const idx = start + i;
        if (idx >= L.length) break;
        const t = i / SAMPLE_RATE;
        const hz = 45 + 105 * Math.exp(-t / 0.03);
        phase += (2 * Math.PI * hz) / SAMPLE_RATE;
        const env = Math.exp(-t / 0.045);
        const click = i < 40 ? (1 - i / 40) * 0.4 : 0;
        const s = (Math.sin(phase) * env + click * env) * cfg.gains.kick;
        L[idx] += s;
        R[idx] += s;
    }
}

// Hat: high-passed noise, closed (30ms) or open (150ms) decay.
function renderHat(L, R, start, cfg, rng, open) {
    const dur = open ? 0.15 : 0.035;
    const n = Math.round(dur * SAMPLE_RATE);
    const tau = open ? 0.035 : 0.008;
    let lp = 0;
    const alpha = 1 - Math.exp((-2 * Math.PI * 7500) / SAMPLE_RATE);
    for (let i = 0; i < n; i += 1) {
        const idx = start + i;
        if (idx >= L.length) break;
        const white = rng() * 2 - 1;
        lp += alpha * (white - lp);
        const hp = white - lp;
        const s = hp * Math.exp(-i / (tau * SAMPLE_RATE)) * cfg.gains.hat;
        panAdd(L, R, idx, s, -0.2);
    }
}

// Snare: noise burst + 180Hz body, on beats 2/4.
function renderSnare(L, R, start, cfg, rng) {
    const n = Math.round(0.16 * SAMPLE_RATE);
    let lp = 0;
    const alpha = 1 - Math.exp((-2 * Math.PI * 1800) / SAMPLE_RATE);
    let phase = 0;
    for (let i = 0; i < n; i += 1) {
        const idx = start + i;
        if (idx >= L.length) break;
        const t = i / SAMPLE_RATE;
        const white = rng() * 2 - 1;
        lp += alpha * (white - lp);
        const noise = (white - lp) * Math.exp(-t / 0.03);
        phase += (2 * Math.PI * 180) / SAMPLE_RATE;
        const body = Math.sin(phase) * Math.exp(-t / 0.045);
        const s = (noise * 0.7 + body * 0.5) * cfg.gains.snare;
        panAdd(L, R, idx, s, 0.1);
    }
}

// ---------------------------------------------------------------------------
// Scheduler: 16 steps per 4/4 bar. Patterns are arrays of 16 entries;
// null = rest, number = semitone offset (bass) or chord-tone index (arp).
// ---------------------------------------------------------------------------

const BASS_PATTERNS = {
    sparse: [0, null, null, null, null, null, null, null, 7, null, null, null, null, null, null, null],
    warm: [0, null, null, null, null, null, null, null, 12, null, null, null, null, null, null, null],
    drive8: [0, null, 0, null, 12, null, 0, null, 0, null, 0, null, 7, null, 12, null],
    drive16: [0, 0, null, 0, 0, null, 0, 12, 0, 0, null, 0, 0, 7, 0, 12],
};

const KICK_PATTERNS = {
    sparse: [0],
    half: [0, 8],
    four: [0, 4, 8, 12],
};

const HAT_PATTERNS = {
    offbeat: [2, 6, 10, 14],
    eighth: [0, 2, 4, 6, 8, 10, 12, 14],
    sixteen: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
};

const SNARE_PATTERNS = {
    none: [],
    backbeat: [4, 12],
};

// Arp direction helpers: cycle chord tones up, or up-down.
function arpIndex(step, toneCount, mode) {
    if (mode === 'updown') {
        const cycle = Math.max(1, toneCount * 2 - 2);
        const m = step % cycle;
        return m < toneCount ? m : cycle - m;
    }
    return step % toneCount;
}

export function renderTrack(cfg) {
    const rng = makeRng(cfg.seed);
    const barSec = (60 / cfg.bpm) * 4;
    const stepSec = barSec / 16;
    const total = Math.round(cfg.bars * barSec * SAMPLE_RATE);
    const L = new Float64Array(total);
    const R = new Float64Array(total);

    const bassPat = BASS_PATTERNS[cfg.patterns.bass];
    const kickPat = KICK_PATTERNS[cfg.patterns.kick];
    const hatPat = HAT_PATTERNS[cfg.patterns.hat];
    const snarePat = SNARE_PATTERNS[cfg.patterns.snare];

    let prevChordIdx = -1;

    for (let bar = 0; bar < cfg.bars; bar += 1) {
        const barStart = Math.round(bar * barSec * SAMPLE_RATE);
        const chordIdx = Math.floor(bar / cfg.chordBars) % cfg.chords.length;
        const chord = cfg.chords[chordIdx];
        const groupBar = bar % 4; // position inside a 4-bar phrase, for fills

        // Pad retriggers only when the chord changes, so held chords stay smooth.
        if (chordIdx !== prevChordIdx) {
            const holdBars = Math.min(cfg.chordBars, cfg.bars - bar);
            renderPadChord(L, R, barStart, chord.pad, holdBars * barSec, cfg);
            prevChordIdx = chordIdx;
        }

        for (let step = 0; step < 16; step += 1) {
            const at = barStart + Math.round(step * stepSec * SAMPLE_RATE);

            const bassOff = bassPat[step];
            if (bassOff !== null && bassOff !== undefined) {
                // Slightly shorter notes on 16th patterns keep the drive tight.
                const len = cfg.patterns.bass === 'drive16' ? stepSec * 0.9 : stepSec * 1.8;
                renderBassNote(L, R, at, midiToFreq(chord.bass + bassOff), len, cfg);
            }

            if (step % cfg.arp.everySteps === 0) {
                const toneIdx = arpIndex(step / cfg.arp.everySteps + bar * 3, chord.arp.length, cfg.arp.mode);
                let midi = chord.arp[toneIdx];
                if (cfg.arp.octaveUpEvery4 && groupBar === 3) midi += 12;
                const pan = ((step / cfg.arp.everySteps + bar) % 2 === 0 ? -1 : 1) * cfg.arp.panWidth;
                renderArpNote(L, R, at, midiToFreq(midi), stepSec * cfg.arp.everySteps * 0.95, cfg, rng, pan);
            }

            if (kickPat.includes(step)) renderKick(L, R, at, cfg);
            if (snarePat.includes(step)) renderSnare(L, R, at, cfg, rng);
            if (hatPat.includes(step)) {
                const open = cfg.patterns.hatOpen && (step === 14 || (groupBar === 3 && step === 10));
                renderHat(L, R, at, cfg, rng, open);
            }
            // Fill: extra kick on the last 16th of each 4-bar phrase.
            if (cfg.patterns.fill && groupBar === 3 && step === 15) renderKick(L, R, at, cfg);
        }
    }

    return master(L, R, cfg);
}

// DC block -> soft clip -> normalize to PEAK. Peak never exceeds PEAK.
function master(L, R, cfg) {
    const drive = cfg.drive;
    const norm = Math.tanh(drive);
    const alpha = 1 - Math.exp((-2 * Math.PI * 25) / SAMPLE_RATE);
    for (const ch of [L, R]) {
        let lp = 0;
        let max = 0;
        for (let i = 0; i < ch.length; i += 1) {
            lp += alpha * (ch[i] - lp);
            ch[i] = Math.tanh((ch[i] - lp) * drive) / norm;
            const abs = Math.abs(ch[i]);
            if (abs > max) max = abs;
        }
        if (max > 0) {
            const g = PEAK / max;
            for (let i = 0; i < ch.length; i += 1) ch[i] *= g;
        }
    }
    return { left: L, right: R, sampleRate: SAMPLE_RATE, seconds: L.length / SAMPLE_RATE };
}

export function encodeWav(track) {
    const n = track.left.length;
    const dataSize = n * 4; // stereo 16-bit
    const buffer = Buffer.alloc(44 + dataSize);
    buffer.write('RIFF', 0, 'ascii');
    buffer.writeUInt32LE(36 + dataSize, 4);
    buffer.write('WAVE', 8, 'ascii');
    buffer.write('fmt ', 12, 'ascii');
    buffer.writeUInt32LE(16, 16);
    buffer.writeUInt16LE(1, 20); // PCM
    buffer.writeUInt16LE(2, 22); // stereo
    buffer.writeUInt32LE(SAMPLE_RATE, 24);
    buffer.writeUInt32LE(SAMPLE_RATE * 4, 28);
    buffer.writeUInt16LE(4, 32);
    buffer.writeUInt16LE(16, 34);
    buffer.write('data', 36, 'ascii');
    buffer.writeUInt32LE(dataSize, 40);
    for (let i = 0; i < n; i += 1) {
        const l = Math.max(-1, Math.min(1, track.left[i]));
        const r = Math.max(-1, Math.min(1, track.right[i]));
        buffer.writeInt16LE(Math.round(l * 32767), 44 + i * 4);
        buffer.writeInt16LE(Math.round(r * 32767), 44 + i * 4 + 2);
    }
    return buffer;
}

// ---------------------------------------------------------------------------
// Track definitions. Chords: bass = midi bass root, pad = held chord tones,
// arp = arpeggio tone pool (all minor-key, dark neon aesthetic).
// ---------------------------------------------------------------------------

export const TRACKS = {
    // Ambient: sparse, moody. Am9 -> Fmaj7 -> C -> G(add). 96 BPM, 32 bars = 80s.
    'arena-drift': {
        seed: 0xa3eada11,
        bpm: 96,
        bars: 32,
        chordBars: 2,
        chords: [
            { bass: 33, pad: [57, 60, 64, 71], arp: [57, 60, 64, 71] }, // Am add9
            { bass: 29, pad: [53, 57, 60, 64], arp: [53, 57, 60, 64] }, // Fmaj7
            { bass: 36, pad: [55, 60, 64, 67], arp: [60, 64, 67, 72] }, // C
            { bass: 31, pad: [55, 59, 62, 67], arp: [55, 59, 62, 67] }, // G
        ],
        patterns: { bass: 'sparse', kick: 'half', hat: 'offbeat', snare: 'none', hatOpen: false, fill: false },
        arp: { everySteps: 4, mode: 'updown', octaveUpEvery4: true, panWidth: 0.45, wave: 'tri', attack: 0.005, decay: 0.25, sustain: 0.15, release: 0.2 },
        bass: { attack: 0.005, decay: 0.3, sustain: 0.4, release: 0.25, cutoffBase: 220, cutoffEnv: 900, cutoffDecay: 0.12, subGain: 0.55 },
        pad: { attack: 1.4, decay: 1.0, sustain: 0.7, release: 1.2, detuneCents: 9, cutoff: 1400 },
        gains: { bass: 0.34, arp: 0.16, pad: 0.1, kick: 0.5, hat: 0.07, snare: 0 },
        drive: 1.1,
    },
    // Ambient 2: warmer, softer, major-leaning. Fmaj7 -> Am7 -> Dm7 -> Cmaj7. 85 BPM, 24 bars = ~67.8s.
    'analog-dreams': {
        seed: 0x0aa10602,
        bpm: 85,
        bars: 24,
        chordBars: 2,
        chords: [
            { bass: 29, pad: [53, 57, 60, 64], arp: [53, 57, 60, 64] }, // Fmaj7
            { bass: 33, pad: [55, 57, 60, 64], arp: [57, 60, 64, 67] }, // Am7
            { bass: 26, pad: [53, 57, 62, 65], arp: [50, 53, 57, 62] }, // Dm7
            { bass: 36, pad: [52, 55, 59, 64], arp: [48, 52, 55, 60] }, // Cmaj7
        ],
        patterns: { bass: 'warm', kick: 'sparse', hat: 'offbeat', snare: 'none', hatOpen: false, fill: false },
        arp: { everySteps: 4, mode: 'updown', octaveUpEvery4: false, panWidth: 0.5, wave: 'tri', attack: 0.02, decay: 0.35, sustain: 0.2, release: 0.3 },
        bass: { attack: 0.01, decay: 0.4, sustain: 0.5, release: 0.3, cutoffBase: 180, cutoffEnv: 500, cutoffDecay: 0.18, subGain: 0.65 },
        pad: { attack: 1.8, decay: 1.2, sustain: 0.75, release: 1.5, detuneCents: 7, cutoff: 1100 },
        gains: { bass: 0.32, arp: 0.14, pad: 0.12, kick: 0.42, hat: 0.05, snare: 0 },
        drive: 1.0,
    },
    // Action: driving 8th bass, four-on-floor. Dm -> Bb -> F -> C. 120 BPM, 32 bars = 64s.
    'pulse-runner': {
        seed: 0x9b15e203,
        bpm: 120,
        bars: 32,
        chordBars: 2,
        chords: [
            { bass: 26, pad: [50, 53, 57, 62], arp: [50, 53, 57, 62] }, // Dm
            { bass: 34, pad: [53, 58, 62, 65], arp: [46, 50, 53, 58] }, // Bb
            { bass: 29, pad: [53, 57, 60, 65], arp: [53, 57, 60, 65] }, // F
            { bass: 36, pad: [55, 60, 64, 67], arp: [48, 52, 55, 60] }, // C
        ],
        patterns: { bass: 'drive8', kick: 'four', hat: 'eighth', snare: 'backbeat', hatOpen: true, fill: true },
        arp: { everySteps: 2, mode: 'up', octaveUpEvery4: true, panWidth: 0.4, wave: 'square', attack: 0.003, decay: 0.12, sustain: 0.1, release: 0.08 },
        bass: { attack: 0.004, decay: 0.15, sustain: 0.5, release: 0.06, cutoffBase: 260, cutoffEnv: 2200, cutoffDecay: 0.07, subGain: 0.5 },
        pad: { attack: 0.5, decay: 0.8, sustain: 0.6, release: 0.5, detuneCents: 11, cutoff: 2000 },
        gains: { bass: 0.4, arp: 0.15, pad: 0.07, kick: 0.62, hat: 0.09, snare: 0.4 },
        drive: 1.35,
    },
    // Intense: aggressive 16th bass, faster. Em -> C -> G -> D. 135 BPM, 36 bars = 64s.
    redline: {
        seed: 0x3ed11e04,
        bpm: 135,
        bars: 36,
        chordBars: 2,
        chords: [
            { bass: 28, pad: [52, 55, 59, 64], arp: [52, 55, 59, 64] }, // Em
            { bass: 36, pad: [52, 55, 60, 64], arp: [48, 52, 55, 60] }, // C
            { bass: 31, pad: [55, 59, 62, 67], arp: [43, 47, 50, 55] }, // G
            { bass: 26, pad: [54, 57, 62, 66], arp: [50, 54, 57, 62] }, // D
        ],
        patterns: { bass: 'drive16', kick: 'four', hat: 'sixteen', snare: 'backbeat', hatOpen: true, fill: true },
        arp: { everySteps: 2, mode: 'up', octaveUpEvery4: true, panWidth: 0.35, wave: 'square', attack: 0.002, decay: 0.09, sustain: 0.08, release: 0.06 },
        bass: { attack: 0.003, decay: 0.1, sustain: 0.45, release: 0.04, cutoffBase: 300, cutoffEnv: 3200, cutoffDecay: 0.05, subGain: 0.45 },
        pad: { attack: 0.3, decay: 0.6, sustain: 0.55, release: 0.35, detuneCents: 13, cutoff: 2600 },
        gains: { bass: 0.42, arp: 0.14, pad: 0.06, kick: 0.66, hat: 0.1, snare: 0.45 },
        drive: 1.6,
    },
    // Ambient: colder neon, C# minor. C#m(add9) -> A -> E -> B. 92 BPM, 24 bars = ~62.6s.
    'neon-circuit': {
        seed: 0x0ec0c105,
        bpm: 92,
        bars: 24,
        chordBars: 2,
        chords: [
            { bass: 25, pad: [49, 51, 52, 56], arp: [49, 52, 56, 61] }, // C#m add9
            { bass: 33, pad: [52, 57, 61, 64], arp: [45, 49, 52, 57] }, // A
            { bass: 28, pad: [52, 56, 59, 64], arp: [52, 56, 59, 64] }, // E
            { bass: 35, pad: [47, 51, 54, 59], arp: [47, 51, 54, 59] }, // B
        ],
        patterns: { bass: 'sparse', kick: 'sparse', hat: 'offbeat', snare: 'none', hatOpen: false, fill: false },
        arp: { everySteps: 4, mode: 'up', octaveUpEvery4: true, panWidth: 0.55, wave: 'tri', attack: 0.008, decay: 0.3, sustain: 0.12, release: 0.25 },
        bass: { attack: 0.006, decay: 0.35, sustain: 0.35, release: 0.3, cutoffBase: 200, cutoffEnv: 700, cutoffDecay: 0.15, subGain: 0.6 },
        pad: { attack: 1.6, decay: 1.1, sustain: 0.7, release: 1.4, detuneCents: 8, cutoff: 1200 },
        gains: { bass: 0.32, arp: 0.15, pad: 0.11, kick: 0.46, hat: 0.06, snare: 0 },
        drive: 1.05,
    },
    // Ambient: chippier night grid, F# minor. F#m -> D -> A -> E, one chord per bar. 98 BPM, 28 bars = ~68.6s.
    'midnight-grid': {
        seed: 0x1d9b1d06,
        bpm: 98,
        bars: 28,
        chordBars: 1,
        chords: [
            { bass: 30, pad: [54, 57, 61, 66], arp: [54, 57, 61, 66] }, // F#m
            { bass: 26, pad: [50, 54, 57, 62], arp: [50, 54, 57, 62] }, // D
            { bass: 33, pad: [45, 49, 52, 57], arp: [57, 61, 64, 69] }, // A
            { bass: 28, pad: [52, 56, 59, 64], arp: [52, 56, 59, 64] }, // E
        ],
        patterns: { bass: 'sparse', kick: 'half', hat: 'eighth', snare: 'none', hatOpen: false, fill: false },
        arp: { everySteps: 4, mode: 'updown', octaveUpEvery4: false, panWidth: 0.4, wave: 'square', attack: 0.004, decay: 0.22, sustain: 0.1, release: 0.18 },
        bass: { attack: 0.005, decay: 0.28, sustain: 0.45, release: 0.22, cutoffBase: 240, cutoffEnv: 1000, cutoffDecay: 0.11, subGain: 0.5 },
        pad: { attack: 0.8, decay: 0.9, sustain: 0.65, release: 0.7, detuneCents: 10, cutoff: 1600 },
        gains: { bass: 0.35, arp: 0.13, pad: 0.09, kick: 0.5, hat: 0.055, snare: 0 },
        drive: 1.15,
    },
    // Ambient: warm tape-worn nostalgia, Bb major. Bbmaj7 -> Gm7 -> Ebmaj7 -> F. 88 BPM, 24 bars = ~65.5s.
    'tape-memories': {
        seed: 0x7a9e1e07,
        bpm: 88,
        bars: 24,
        chordBars: 2,
        chords: [
            { bass: 34, pad: [58, 62, 65, 69], arp: [58, 62, 65, 69] }, // Bbmaj7
            { bass: 31, pad: [55, 58, 62, 65], arp: [55, 58, 62, 65] }, // Gm7
            { bass: 27, pad: [51, 55, 58, 62], arp: [51, 55, 58, 62] }, // Ebmaj7
            { bass: 29, pad: [53, 57, 60, 65], arp: [53, 57, 60, 65] }, // F
        ],
        patterns: { bass: 'warm', kick: 'sparse', hat: 'offbeat', snare: 'none', hatOpen: false, fill: false },
        arp: { everySteps: 4, mode: 'updown', octaveUpEvery4: false, panWidth: 0.5, wave: 'tri', attack: 0.03, decay: 0.4, sustain: 0.18, release: 0.35 },
        bass: { attack: 0.012, decay: 0.45, sustain: 0.5, release: 0.35, cutoffBase: 170, cutoffEnv: 450, cutoffDecay: 0.2, subGain: 0.7 },
        pad: { attack: 2.0, decay: 1.3, sustain: 0.75, release: 1.6, detuneCents: 14, cutoff: 950 },
        gains: { bass: 0.3, arp: 0.13, pad: 0.13, kick: 0.4, hat: 0.045, snare: 0 },
        drive: 1.0,
    },
    // Action: stomping G minor groove. Gm -> Eb -> Bb -> F. 122 BPM, 32 bars = ~63s.
    'overdrive-protocol': {
        seed: 0x0de2d108,
        bpm: 122,
        bars: 32,
        chordBars: 2,
        chords: [
            { bass: 31, pad: [55, 58, 62, 67], arp: [55, 58, 62, 67] }, // Gm
            { bass: 27, pad: [51, 55, 58, 63], arp: [51, 55, 58, 63] }, // Eb
            { bass: 34, pad: [53, 58, 62, 65], arp: [46, 50, 53, 58] }, // Bb
            { bass: 29, pad: [53, 57, 60, 65], arp: [53, 57, 60, 65] }, // F
        ],
        patterns: { bass: 'drive8', kick: 'four', hat: 'eighth', snare: 'backbeat', hatOpen: true, fill: false },
        arp: { everySteps: 2, mode: 'updown', octaveUpEvery4: false, panWidth: 0.45, wave: 'square', attack: 0.003, decay: 0.14, sustain: 0.12, release: 0.1 },
        bass: { attack: 0.004, decay: 0.18, sustain: 0.5, release: 0.07, cutoffBase: 240, cutoffEnv: 1800, cutoffDecay: 0.08, subGain: 0.55 },
        pad: { attack: 0.6, decay: 0.9, sustain: 0.6, release: 0.6, detuneCents: 9, cutoff: 1800 },
        gains: { bass: 0.38, arp: 0.16, pad: 0.08, kick: 0.6, hat: 0.085, snare: 0.42 },
        drive: 1.3,
    },
    // Action: brooding F minor storm, busier hats. Fm -> Db -> Ab -> Eb. 118 BPM, 32 bars = ~65.1s.
    'ion-storm': {
        seed: 0x10b57009,
        bpm: 118,
        bars: 32,
        chordBars: 2,
        chords: [
            { bass: 29, pad: [53, 56, 60, 65], arp: [53, 56, 60, 65] }, // Fm
            { bass: 25, pad: [49, 53, 56, 61], arp: [49, 53, 56, 61] }, // Db
            { bass: 32, pad: [48, 51, 56, 60], arp: [44, 48, 51, 56] }, // Ab
            { bass: 27, pad: [51, 55, 58, 63], arp: [51, 55, 58, 63] }, // Eb
        ],
        patterns: { bass: 'drive8', kick: 'four', hat: 'sixteen', snare: 'backbeat', hatOpen: true, fill: true },
        arp: { everySteps: 2, mode: 'updown', octaveUpEvery4: true, panWidth: 0.5, wave: 'tri', attack: 0.004, decay: 0.13, sustain: 0.1, release: 0.09 },
        bass: { attack: 0.004, decay: 0.16, sustain: 0.55, release: 0.06, cutoffBase: 280, cutoffEnv: 2600, cutoffDecay: 0.06, subGain: 0.4 },
        pad: { attack: 0.4, decay: 0.7, sustain: 0.55, release: 0.45, detuneCents: 12, cutoff: 2200 },
        gains: { bass: 0.4, arp: 0.14, pad: 0.07, kick: 0.64, hat: 0.08, snare: 0.38 },
        drive: 1.4,
    },
    // Action: lean and fast, B minor. Bm -> G -> D -> A. 124 BPM, 32 bars = ~61.9s.
    velocity: {
        seed: 0x4e10c10a,
        bpm: 124,
        bars: 32,
        chordBars: 2,
        chords: [
            { bass: 35, pad: [47, 50, 54, 59], arp: [59, 62, 66, 71] }, // Bm
            { bass: 31, pad: [50, 55, 59, 62], arp: [55, 59, 62, 67] }, // G
            { bass: 26, pad: [50, 54, 57, 62], arp: [50, 54, 57, 62] }, // D
            { bass: 33, pad: [45, 49, 52, 57], arp: [57, 61, 64, 69] }, // A
        ],
        patterns: { bass: 'drive16', kick: 'four', hat: 'eighth', snare: 'backbeat', hatOpen: false, fill: true },
        arp: { everySteps: 2, mode: 'up', octaveUpEvery4: false, panWidth: 0.35, wave: 'square', attack: 0.002, decay: 0.1, sustain: 0.09, release: 0.07 },
        bass: { attack: 0.003, decay: 0.12, sustain: 0.5, release: 0.05, cutoffBase: 300, cutoffEnv: 2800, cutoffDecay: 0.055, subGain: 0.5 },
        pad: { attack: 0.35, decay: 0.65, sustain: 0.55, release: 0.4, detuneCents: 11, cutoff: 2400 },
        gains: { bass: 0.4, arp: 0.15, pad: 0.06, kick: 0.64, hat: 0.1, snare: 0.42 },
        drive: 1.45,
    },
    // Intense: heaviest, deep C minor root. Cm -> Ab -> Eb -> Bb. 138 BPM, 40 bars = ~69.6s.
    'critical-mass': {
        seed: 0xcc1ca50b,
        bpm: 138,
        bars: 40,
        chordBars: 2,
        chords: [
            { bass: 24, pad: [48, 51, 55, 60], arp: [60, 63, 67, 72] }, // Cm
            { bass: 32, pad: [48, 51, 56, 60], arp: [56, 60, 63, 68] }, // Ab
            { bass: 27, pad: [51, 55, 58, 63], arp: [51, 55, 58, 63] }, // Eb
            { bass: 34, pad: [50, 53, 58, 62], arp: [46, 50, 53, 58] }, // Bb
        ],
        patterns: { bass: 'drive16', kick: 'four', hat: 'sixteen', snare: 'backbeat', hatOpen: true, fill: true },
        arp: { everySteps: 2, mode: 'up', octaveUpEvery4: true, panWidth: 0.4, wave: 'square', attack: 0.002, decay: 0.08, sustain: 0.07, release: 0.05 },
        bass: { attack: 0.003, decay: 0.09, sustain: 0.5, release: 0.04, cutoffBase: 320, cutoffEnv: 3600, cutoffDecay: 0.045, subGain: 0.5 },
        pad: { attack: 0.25, decay: 0.55, sustain: 0.5, release: 0.3, detuneCents: 14, cutoff: 2800 },
        gains: { bass: 0.44, arp: 0.13, pad: 0.06, kick: 0.68, hat: 0.1, snare: 0.48 },
        drive: 1.7,
    },
};

// ---------------------------------------------------------------------------

function main() {
    const outIdx = process.argv.indexOf('--out');
    const outDir = outIdx >= 0
        ? resolve(process.argv[outIdx + 1])
        : join(dirname(fileURLToPath(import.meta.url)), 'build');
    mkdirSync(outDir, { recursive: true });
    for (const [name, cfg] of Object.entries(TRACKS)) {
        const track = renderTrack(cfg);
        const wav = encodeWav(track);
        const file = join(outDir, `${name}.wav`);
        writeFileSync(file, wav);
        console.log(`wrote ${file}: ${track.seconds.toFixed(2)}s, ${cfg.bpm} BPM, ${cfg.bars} bars, ${wav.length} bytes`);
    }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    main();
}
