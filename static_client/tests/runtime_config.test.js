import test from "node:test";
import assert from "node:assert/strict";

import { buildRuntimeConfig } from "../client_logic/runtime_config.js";

const GP = {
    WeaponType: {
        Pistol: 1,
        Shotgun: 2,
        Rifle: 3,
        Sniper: 4,
        Melee: 5,
    },
    PickupType: {
        Health: 1,
        Ammo: 2,
        WeaponCrate: 3,
        SpeedBoost: 4,
        DamageBoost: 5,
        Shield: 6,
        FlagRed: 7,
        FlagBlue: 8,
    },
};

function installRuntimeEnv({
    userAgent = "Mozilla/5.0",
    hardwareConcurrency = 8,
    supportsWebgl2 = false,
} = {}) {
    Object.defineProperty(globalThis, "navigator", {
        value: { userAgent, hardwareConcurrency },
        configurable: true,
    });
    Object.defineProperty(globalThis, "document", {
        value: {
            createElement() {
                return {
                    getContext(kind) {
                        if (kind === "webgl2") {
                            return supportsWebgl2 ? {} : null;
                        }
                        return null;
                    },
                };
            },
        },
        configurable: true,
    });
}

test.afterEach(() => {
    delete globalThis.navigator;
    delete globalThis.document;
});

test("buildRuntimeConfig applies mobile-safe defaults and clamps values", () => {
    installRuntimeEnv({
        userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)",
        hardwareConcurrency: 2,
        supportsWebgl2: false,
    });

    const config = buildRuntimeConfig(
        "?mode=bench&worker_cull=1&worker_cull_interval_ms=999&bench_max_fps=500&webgpu_instances=on",
        GP
    );

    assert.equal(config.BENCH_MODE, true);
    assert.equal(config.WORKER_CULL_ENABLED, true);
    assert.equal(config.WORKER_CULL_INTERVAL_MS, 250);
    assert.equal(config.BENCH_MAX_FPS, 240);
    assert.equal(config.INTERPOLATION_DELAY, 90);
    assert.equal(config.MIN_INTERPOLATION_DELAY_MS, 70);
    assert.equal(config.WEBGPU_PLAYER_LAYER_ENABLED, false);
    assert.equal(config.WEBGPU_PROJECTILE_LAYER_ENABLED, false);
});

test("buildRuntimeConfig honors desktop toggles and stable mode flags", () => {
    installRuntimeEnv({
        userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)",
        hardwareConcurrency: 12,
        supportsWebgl2: true,
    });

    const config = buildRuntimeConfig(
        "?mode=stable&fog=off&tab_throttle=off&combat_ui=high&webgpu_players=off",
        GP
    );

    assert.equal(config.STABLE_MODE_FORCED, true);
    assert.equal(config.LOW_OVERHEAD_MODE, true);
    assert.equal(config.WEBGL2_SUPPORTED, true);
    assert.equal(config.FOG_ENABLED, false);
    assert.equal(config.TAB_THROTTLE_ENABLED, false);
    assert.equal(config.COMBAT_UI_QUALITY_OVERRIDE, "high");
    assert.equal(config.WEBGPU_PLAYER_LAYER_ENABLED, false);
    assert.equal(config.INTERPOLATION_DELAY, 70);
    assert.equal(config.MAX_INTERPOLATION_SNAPSHOTS, 40);
});
