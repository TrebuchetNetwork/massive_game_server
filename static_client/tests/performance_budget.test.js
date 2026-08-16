import test from "node:test";
import assert from "node:assert/strict";

import { createPerformanceBudget } from "../client_logic/PerformanceBudget.js";

// Mobile render profiles cap the ticker (30fps mid / 20fps low), so a
// healthy capped device reports smoothed frames near the cap's frame time.
// The adaptive effects governor must treat that as the configured budget,
// not as performance distress.

function createCtx({ fpsCap = 60, smoothedFrameMs = 16.67, activeProfile = "medium" } = {}) {
    const ctx = {
        // Frame/adaptive state
        smoothedFrameMs,
        lowFpsFrameStreak: 0,
        lowFpsDurationMs: 0,
        recoveryFrameStreak: 0,
        recoveryDurationMs: 0,
        activeEffectsProfileName: activeProfile,
        lastAdaptiveEffectsEvalTime: 0,
        players: new Map(),
        localPlayerState: null,
        // Flags & constants (desktop, non-forced defaults)
        BENCH_MODE: false,
        STABLE_MODE_FORCED: false,
        TOURNAMENT_MODE_FORCED: false,
        ULTRA_MODE_FORCED: false,
        ultraPerformanceMode: false,
        STABLE_ULTRA_FRAME_MS: 26,
        STABLE_DENSE_FRAME_MS: 18,
        TARGET_FRAME_MS_60FPS: 18,
        HIGH_POPULATION_PLAYER_COUNT: 60,
        RESPAWN_ANIMATION_LIGHTWEIGHT: false,
        EFFECTS_ADAPTIVE_EVAL_INTERVAL_MS: 500,
        EFFECTS_PROFILE_PRIORITY: { ultra: 0, dense: 1, medium: 2, high: 3 },
        ULTRA_AUTO_RECOVERY_TRIGGER_MS: 14000,
        ULTRA_DOWNSHIFT_MAX_FRAME_MS: 18.2,
        gameSettings: { particleEffects: true },
        // The FPS cap the ticker is held to (client.html getEffectiveFPSCap).
        getEffectiveFPSCap: () => fpsCap,
        // Effects manager stub
        effectsManager: {
            activeEffects: [],
            particlesEnabled: null,
            setPerformanceProfile(name) {
                this.lastProfile = name;
                return name;
            },
            setParticlesEnabled(enabled) {
                this.particlesEnabled = enabled;
            },
        },
        // Setters used by the budget module
        setSmoothedFrameMs(v) { ctx.smoothedFrameMs = v; },
        setLowFpsFrameStreak(v) { ctx.lowFpsFrameStreak = v; },
        setLowFpsDurationMs(v) { ctx.lowFpsDurationMs = v; },
        setRecoveryFrameStreak(v) { ctx.recoveryFrameStreak = v; },
        setRecoveryDurationMs(v) { ctx.recoveryDurationMs = v; },
        setActiveEffectsProfileName(v) { ctx.activeEffectsProfileName = v; },
        setLastAdaptiveEffectsEvalTime(v) { ctx.lastAdaptiveEffectsEvalTime = v; },
    };
    return ctx;
}

function createBudget(ctx) {
    return createPerformanceBudget(() => ctx);
}

test("perf budget: uncapped desktop keeps legacy frame thresholds", () => {
    const ctx = createCtx({ fpsCap: 60, smoothedFrameMs: 33 });
    const budget = createBudget(ctx);
    assert.equal(budget.getTargetEffectsProfileName(), "ultra");
    ctx.smoothedFrameMs = 26;
    assert.equal(budget.getTargetEffectsProfileName(), "dense");
    ctx.smoothedFrameMs = 16.67;
    assert.equal(budget.getTargetEffectsProfileName(), "high");
});

test("perf budget: mid device at its 30fps cap is not distressed", () => {
    const ctx = createCtx({ fpsCap: 30, smoothedFrameMs: 33.3 });
    const budget = createBudget(ctx);
    const target = budget.getTargetEffectsProfileName();
    assert.ok(target === "high" || target === "medium", `expected no downshift, got ${target}`);
});

test("perf budget: mid device meaningfully exceeding its cap downshifts", () => {
    const ctx = createCtx({ fpsCap: 30, smoothedFrameMs: 45 });
    const budget = createBudget(ctx);
    const target = budget.getTargetEffectsProfileName();
    assert.ok(target === "dense" || target === "ultra", `expected downshift, got ${target}`);
});

test("perf budget: low device at its 20fps cap is not further distressed by frame time", () => {
    const ctx = createCtx({ fpsCap: 20, smoothedFrameMs: 50, activeProfile: "ultra" });
    const budget = createBudget(ctx);
    const target = budget.getTargetEffectsProfileName();
    assert.ok(target === "high" || target === "medium", `expected no frame-time distress, got ${target}`);
    ctx.smoothedFrameMs = 65;
    const distressed = budget.getTargetEffectsProfileName();
    assert.ok(distressed === "dense" || distressed === "ultra", `expected downshift, got ${distressed}`);
});

test("perf budget: capped frames do not build the low-fps distress streak", () => {
    const ctx = createCtx({ fpsCap: 30, smoothedFrameMs: 33.3 });
    const budget = createBudget(ctx);
    for (let i = 0; i < 120; i += 1) budget.updateFramePerformanceSignals(33.3);
    assert.equal(ctx.lowFpsFrameStreak, 0);
    assert.equal(ctx.lowFpsDurationMs, 0);

    // Genuine distress well above the cap still builds the streak.
    for (let i = 0; i < 30; i += 1) budget.updateFramePerformanceSignals(45);
    assert.ok(ctx.lowFpsFrameStreak > 0, "expected low-fps streak to build at 45ms under a 30fps cap");
});

test("perf budget: adaptive evaluation keeps deliberate medium profile at cap, downshifts on real distress", () => {
    globalThis.window = {};
    try {
        const ctx = createCtx({ fpsCap: 30, smoothedFrameMs: 33.3, activeProfile: "medium" });
        const budget = createBudget(ctx);
        budget.evaluateAdaptiveEffectsProfile(1000);
        assert.equal(ctx.activeEffectsProfileName, "medium");
        assert.equal(ctx.effectsManager.particlesEnabled, null, "no profile change => no particle sync");

        ctx.smoothedFrameMs = 45;
        budget.evaluateAdaptiveEffectsProfile(2000);
        assert.equal(ctx.activeEffectsProfileName, "dense");
    } finally {
        delete globalThis.window;
    }
});

test("perf budget: particles stay enabled for a healthy capped mid device", () => {
    const ctx = createCtx({ fpsCap: 30, smoothedFrameMs: 33.3, activeProfile: "medium" });
    const budget = createBudget(ctx);
    budget.syncParticlesBudget();
    assert.equal(ctx.effectsManager.particlesEnabled, true);

    ctx.smoothedFrameMs = 50;
    budget.syncParticlesBudget();
    assert.equal(ctx.effectsManager.particlesEnabled, false);
});
