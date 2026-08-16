import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { createEffectsAudioRuntime } from "../client_logic/effects_audio_runtime.js";

const NEW_SAMPLE_ENTRIES = {
    footstepConcrete: "sfx/footstep_a.wav",
    footstepWood: "sfx/footstep_b.wav",
    footstepMetal: "sfx/footstep_a.wav",
    footstepGlass: "sfx/footstep_b.wav",
    impactConcrete: "sfx/impact_soft.wav",
    impactWood: "sfx/impact_soft.wav",
    impactMetal: "sfx/impact_hard.wav",
    impactGlass: "sfx/impact_hard.wav",
};

function createAudioManager({ localState = null, mobile = false } = {}) {
    const runtime = createEffectsAudioRuntime({
        PIXI: {},
        GP: {},
        getLocalPlayerState: () => localState,
        isMobileSoundBudget: mobile,
    });
    return new runtime.AudioManager();
}

function parseWav(relativePath) {
    const url = new URL(`../${relativePath}`, import.meta.url);
    const buf = readFileSync(url);
    assert.equal(buf.toString("ascii", 0, 4), "RIFF", `${relativePath}: RIFF magic`);
    assert.equal(buf.toString("ascii", 8, 12), "WAVE", `${relativePath}: WAVE magic`);
    assert.equal(buf.toString("ascii", 12, 16), "fmt ", `${relativePath}: fmt chunk`);
    assert.equal(buf.readUInt16LE(20), 1, `${relativePath}: PCM format`);
    assert.equal(buf.readUInt16LE(22), 1, `${relativePath}: mono`);
    assert.equal(buf.readUInt32LE(24), 44100, `${relativePath}: 44.1kHz`);
    assert.equal(buf.readUInt16LE(34), 16, `${relativePath}: 16-bit`);
    assert.equal(buf.toString("ascii", 36, 40), "data", `${relativePath}: data chunk`);
    const dataSize = buf.readUInt32LE(40);
    assert.equal(buf.length, 44 + dataSize, `${relativePath}: data size matches file length`);
    assert.equal(buf.readUInt32LE(4), 36 + dataSize, `${relativePath}: RIFF size field`);
    return { dataSize, durationSec: dataSize / 2 / 44100 };
}

test("sfx gap-fill: footstep and material-impact samples are registered", () => {
    const mgr = createAudioManager();
    for (const [soundName, samplePath] of Object.entries(NEW_SAMPLE_ENTRIES)) {
        assert.equal(mgr.soundSamples[soundName], samplePath, soundName);
        // Synth recipe must remain as fallback for each registered name.
        assert.ok(mgr.sounds[soundName], `synth fallback profile for ${soundName}`);
        assert.ok(mgr.soundLimits[soundName], `desktop rate limit for ${soundName}`);
        assert.ok(mgr.mobileSoundLimits[soundName], `mobile rate limit for ${soundName}`);
    }
});

test("sfx gap-fill: generated wavs are valid 44.1kHz mono 16-bit RIFF under 0.4s", () => {
    const uniquePaths = [...new Set(Object.values(NEW_SAMPLE_ENTRIES))];
    assert.equal(uniquePaths.length, 4);
    for (const samplePath of uniquePaths) {
        const { durationSec } = parseWav(samplePath);
        assert.ok(durationSec > 0.03, `${samplePath}: not suspiciously short`);
        assert.ok(durationSec < 0.4, `${samplePath}: under 0.4s (got ${durationSec})`);
    }
});

test("sfx gap-fill: footstep rate limits are tight (max 2-3/s)", () => {
    const mgr = createAudioManager();
    for (const name of ["footstepConcrete", "footstepMetal", "footstepWood", "footstepGlass"]) {
        assert.ok(mgr.soundLimits[name].maxPerWindow <= 3, `${name} desktop maxPerWindow`);
        assert.ok(mgr.mobileSoundLimits[name].maxPerWindow <= 2, `${name} mobile maxPerWindow`);
        assert.ok(mgr.soundLimits[name].minIntervalMs >= 100, `${name} desktop minIntervalMs`);
    }
});

test("sfx gap-fill: local footsteps follow movement with a stride interval", () => {
    const localState = { alive: true, velocity_x: 200, velocity_y: 0 };
    const mgr = createAudioManager({ localState });
    const played = [];
    mgr.playSound = (...args) => played.push(args);

    mgr.syncLocalFootsteps(1000);
    assert.equal(played.length, 1, "first step fires immediately when moving");
    assert.equal(played[0][0], "footstepConcrete");

    mgr.syncLocalFootsteps(1100);
    assert.equal(played.length, 1, "too soon for the next stride");

    mgr.syncLocalFootsteps(1400);
    assert.equal(played.length, 2, "second step after the stride interval");

    localState.velocity_x = 0;
    mgr.syncLocalFootsteps(2000);
    assert.equal(played.length, 2, "no steps while stationary");

    localState.velocity_x = 200;
    localState.alive = false;
    mgr.syncLocalFootsteps(3000);
    assert.equal(played.length, 2, "no steps while dead");
});

test("sfx gap-fill: mobile stride interval is longer than desktop", () => {
    const moving = () => ({ alive: true, velocity_x: 200, velocity_y: 0 });
    const desktop = createAudioManager({ localState: moving(), mobile: false });
    const mobile = createAudioManager({ localState: moving(), mobile: true });
    desktop.playSound = () => {};
    mobile.playSound = () => {};

    desktop.syncLocalFootsteps(1000);
    mobile.syncLocalFootsteps(1000);
    assert.ok(
        mobile.localFootstepNextAt > desktop.localFootstepNextAt,
        "mobile budgets space footsteps further apart"
    );
});
