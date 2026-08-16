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

function createManager(deviceClass) {
    const PIXI = createPixiStub();
    const runtime = createEffectsAudioRuntime({
        PIXI,
        GP: {},
        DAMAGE_NUMBER_POOL_PREALLOC: 0,
        getDeviceClassification: () => deviceClass,
    });
    const container = new PIXI.Container();
    return new runtime.EffectsManager(null, container, null);
}

test("engine trail: emissions are fully gated off on low device class", () => {
    const mgr = createManager("low");
    for (let i = 0; i < 4; i += 1) {
        assert.equal(mgr.emitEngineTrail(0, 0, 0xFFFFFF, 1), false);
    }
    assert.equal(mgr.engineTrailPool.length, 0);
});

test("engine trail: emissions are halved on mid device class", () => {
    const mgr = createManager("mid");
    const results = [0, 1, 2, 3].map(() => mgr.emitEngineTrail(0, 0, 0xFFFFFF, 1));
    assert.deepEqual(results, [false, true, false, true]);
    assert.equal(mgr.engineTrailPool.length, 2);
});

test("engine trail: full emission rate on high device class", () => {
    const mgr = createManager("high");
    for (let i = 0; i < 4; i += 1) {
        assert.equal(mgr.emitEngineTrail(0, 0, 0xFFFFFF, 1), true);
    }
    // Each emission stays active (no update ticks), so each gets its own sprite.
    assert.equal(mgr.engineTrailPool.length, 4);
});

test("engine trail: clearAllEffects resets the pool so emissions work again", () => {
    const mgr = createManager("high");
    for (let i = 0; i < 3; i += 1) {
        assert.equal(mgr.emitEngineTrail(0, 0, 0xFFFFFF, 1), true);
    }
    assert.equal(mgr.engineTrailPool.length, 3);

    mgr.clearAllEffects();
    assert.equal(mgr.engineTrailPool.length, 0);
    assert.equal(mgr.engineTrailPoolCursor, 0);

    assert.equal(mgr.emitEngineTrail(0, 0, 0xFFFFFF, 1), true);
    assert.equal(mgr.engineTrailPool.length, 1);
    assert.equal(mgr.engineTrailPool[0].destroyed, false);
});
