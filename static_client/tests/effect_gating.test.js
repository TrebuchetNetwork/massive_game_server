import test from "node:test";
import assert from "node:assert/strict";

import { createEffectsAudioRuntime } from "../client_logic/effects_audio_runtime.js";

// Minimal PIXI stub: only the surface EffectsManager touches when the damage
// number pool preallocation is disabled and there is no renderer.
function createPixiStub() {
    class Container {
        constructor() {
            this.children = [];
            this.visible = false;
            this.renderable = false;
            this.destroyed = false;
        }
        addChild(child) {
            this.children.push(child);
            child.parent = this;
            return child;
        }
        removeChildren() {
            const removed = [...this.children];
            this.children.length = 0;
            removed.forEach((child) => { child.parent = null; });
            return removed;
        }
        destroy() { this.destroyed = true; }
    }
    class Sprite {
        constructor(texture) {
            this.texture = texture;
            this.anchor = { set() {} };
            this.position = { set() {} };
            this.scale = { set() {} };
            this.visible = false;
            this.alpha = 1;
            this.tint = 0;
            this.blendMode = 0;
            this.destroyed = false;
        }
        destroy() { this.destroyed = true; }
    }
    return {
        Container,
        Sprite,
        Texture: { WHITE: { isWhite: true } },
        BLEND_MODES: { ADD: 1 },
    };
}

function createManager({ deviceClass = "desktop", frameMs = 16, ultra = false } = {}) {
    const PIXI = createPixiStub();
    const runtime = createEffectsAudioRuntime({
        PIXI,
        GP: {},
        DAMAGE_NUMBER_POOL_PREALLOC: 0,
        getDeviceClassification: () => deviceClass,
        getSmoothedFrameMs: () => frameMs,
        getUltraPerformanceMode: () => ultra,
    });
    const container = new PIXI.Container();
    return new runtime.EffectsManager(null, container, null);
}

function countEmits(mgr, kind, calls) {
    let emitted = 0;
    for (let i = 0; i < calls; i += 1) {
        if (mgr.shouldEmitEffect(kind)) emitted += 1;
    }
    return emitted;
}

const KEY_KINDS = ["damage", "impact", "muzzle", "explosion", "powerup", "movement"];

test("effect gating: no load tier emits everything on every device class", () => {
    for (const deviceClass of ["desktop", "high", "mid", "low"]) {
        const mgr = createManager({ deviceClass, frameMs: 16 });
        for (const kind of [...KEY_KINDS, "flag", "generic"]) {
            assert.equal(countEmits(mgr, kind, 4), 4, `${deviceClass}/${kind}`);
        }
    }
});

test("effect gating: soft load keeps key combat kinds at full rate on mid", () => {
    for (const kind of KEY_KINDS) {
        const mgr = createManager({ deviceClass: "mid", frameMs: 22 });
        assert.equal(countEmits(mgr, kind, 6), 6, `mid/${kind}`);
    }
});

test("effect gating: soft load keeps key combat kinds at full rate on high", () => {
    for (const kind of KEY_KINDS) {
        const mgr = createManager({ deviceClass: "high", frameMs: 22 });
        assert.equal(countEmits(mgr, kind, 6), 6, `high/${kind}`);
    }
});

test("effect gating: soft load still strides non-key kinds on mid", () => {
    const mgr = createManager({ deviceClass: "mid", frameMs: 22 });
    assert.equal(countEmits(mgr, "flag", 6), 1);
    assert.equal(countEmits(mgr, "generic", 4), 2);
});

test("effect gating: soft load keeps full strides on low (suppression preserved)", () => {
    const mgr = createManager({ deviceClass: "low", frameMs: 22 });
    assert.equal(countEmits(mgr, "explosion", 6), 2); // stride 3
    assert.equal(countEmits(mgr, "muzzle", 6), 2); // stride 3
    assert.equal(countEmits(mgr, "powerup", 8), 2); // stride 4
});

test("effect gating: soft load keeps full strides on desktop (unchanged)", () => {
    const mgr = createManager({ deviceClass: "desktop", frameMs: 22 });
    assert.equal(countEmits(mgr, "muzzle", 6), 2); // stride 3
    assert.equal(countEmits(mgr, "explosion", 6), 2); // stride 3
});

test("effect gating: heavy load sheds key kinds at half rate on mid, not full stride", () => {
    const mgr = createManager({ deviceClass: "mid", frameMs: 30 });
    assert.equal(countEmits(mgr, "explosion", 6), 3); // min(6, 2)
    assert.equal(countEmits(mgr, "muzzle", 4), 2); // min(5, 2)
    // Non-key kinds keep the heavy stride table.
    assert.equal(countEmits(mgr, "flag", 10), 1);
});

test("effect gating: heavy load keeps full strides on low and desktop", () => {
    const low = createManager({ deviceClass: "low", frameMs: 30 });
    assert.equal(countEmits(low, "explosion", 6), 1);
    assert.equal(countEmits(low, "movement", 6), 1);

    const desktop = createManager({ deviceClass: "desktop", frameMs: 30 });
    assert.equal(countEmits(desktop, "explosion", 6), 1);
    assert.equal(countEmits(desktop, "muzzle", 5), 1);
});

test("effect gating: ultra performance mode keeps full strides even on mid", () => {
    const mgr = createManager({ deviceClass: "mid", frameMs: 16, ultra: true });
    assert.equal(countEmits(mgr, "explosion", 6), 1);
    assert.equal(countEmits(mgr, "damage", 3), 1);
});
