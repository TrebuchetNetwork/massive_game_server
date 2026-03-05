import test from "node:test";
import assert from "node:assert/strict";

import { createInterpolationManager } from "../client_logic/InterpolationManager.js";

function makeCtx(overrides = {}) {
    return {
        players: new Map(),
        projectiles: new Map(),
        myPlayerId: "local",
        localPlayerState: null,
        app: { screen: { width: 1280, height: 720 } },
        adaptiveInterpolationDelayMs: 0,
        projectileRawModeActive: false,
        serverUpdates: [],
        isMobileDevice: false,
        applyRenderTarget: (state, x, y, rotation) => {
            state.render_x = x;
            state.render_y = y;
            state.render_rotation = rotation;
        },
        getProjectileInterpolationSet: () => new Set(),
        forEachInterpolatedProjectile: (_set, _fn) => {},
        INTERPOLATION_RETENTION_MS: 300,
        POSITION_SNAP_DISTANCE_SQ: 999999,
        PROJECTILE_SNAP_DISTANCE_SQ: 999999,
        PLAYER_EXTRAPOLATION_LIMIT_MS: 120,
        PROJECTILE_EXTRAPOLATION_LIMIT_MS: 80,
        INTERPOLATION_PLAYER_LIMIT: 1024,
        INTERPOLATION_PROJECTILE_LIMIT: 4096,
        INTERPOLATION_SNAPSHOT_INTERVAL_MS: 0,
        MAX_INTERPOLATION_SNAPSHOTS: 64,
        updateAdaptiveInterpolationDelay: () => {},
        ...overrides,
    };
}

test("maybeRecordInterpolationSnapshot clears snapshots when entity caps are exceeded", () => {
    const players = new Map([
        ["p1", { x: 0, y: 0, rotation: 0, velocity_x: 0, velocity_y: 0 }],
        ["p2", { x: 10, y: 0, rotation: 0, velocity_x: 0, velocity_y: 0 }],
    ]);
    const ctx = makeCtx({
        players,
        projectiles: new Map(),
        INTERPOLATION_PLAYER_LIMIT: 1,
        serverUpdates: [{ timestamp: 1, players: new Map(), projectiles: new Map() }],
    });

    const manager = createInterpolationManager(() => ctx);
    manager.maybeRecordInterpolationSnapshot(10_000);

    assert.equal(ctx.serverUpdates.length, 0);
});

test("maybeRecordInterpolationSnapshot prunes disconnected players/projectiles from queued snapshots", () => {
    const players = new Map([["p1", { x: 5, y: 6, rotation: 0.3, velocity_x: 1, velocity_y: 2 }]]);
    const projectiles = new Map([["proj1", { x: 8, y: 9, velocity_x: 0.5, velocity_y: -0.25 }]]);

    const staleSnapshot = {
        timestamp: 1,
        players: new Map([
            ["p1", { x: 1, y: 1, rotation: 0, velocity_x: 0, velocity_y: 0 }],
            ["p-stale", { x: 2, y: 2, rotation: 0, velocity_x: 0, velocity_y: 0 }],
        ]),
        projectiles: new Map([
            ["proj1", { x: 3, y: 3, velocity_x: 0, velocity_y: 0 }],
            ["proj-stale", { x: 4, y: 4, velocity_x: 0, velocity_y: 0 }],
        ]),
    };

    const ctx = makeCtx({
        players,
        projectiles,
        serverUpdates: [staleSnapshot],
        INTERPOLATION_SNAPSHOT_INTERVAL_MS: 0,
    });

    const manager = createInterpolationManager(() => ctx);

    // Trigger periodic prune path (every 12 snapshots).
    for (let i = 0; i < 12; i += 1) {
        manager.maybeRecordInterpolationSnapshot(2_000 + i);
    }

    assert.equal(staleSnapshot.players.has("p-stale"), false);
    assert.equal(staleSnapshot.projectiles.has("proj-stale"), false);
    assert.equal(staleSnapshot.players.has("p1"), true);
    assert.equal(staleSnapshot.projectiles.has("proj1"), true);
});

test("interpolateEntities keeps source snapshot array intact while trimming stale render window", () => {
    const remotePlayer = {
        x: 0,
        y: 0,
        rotation: 0,
        velocity_x: 0,
        velocity_y: 0,
        render_x: 0,
        render_y: 0,
        render_rotation: 0,
    };

    const players = new Map([["remote", remotePlayer]]);
    const now = Date.now();
    const serverUpdates = [
        {
            timestamp: now - 1_000,
            players: new Map([["remote", { x: 1, y: 1, rotation: 0.1, velocity_x: 0, velocity_y: 0 }]]),
            projectiles: new Map(),
        },
        {
            timestamp: now - 600,
            players: new Map([["remote", { x: 3, y: 3, rotation: 0.2, velocity_x: 0, velocity_y: 0 }]]),
            projectiles: new Map(),
        },
        {
            timestamp: now - 100,
            players: new Map([["remote", { x: 7, y: 9, rotation: 0.4, velocity_x: 0, velocity_y: 0 }]]),
            projectiles: new Map(),
        },
    ];
    const identityBefore = [...serverUpdates];

    const ctx = makeCtx({
        players,
        serverUpdates,
        INTERPOLATION_RETENTION_MS: 250,
        adaptiveInterpolationDelayMs: 0,
    });
    const manager = createInterpolationManager(() => ctx);

    manager.interpolateEntities(1 / 60);

    assert.equal(serverUpdates.length, 3);
    assert.deepEqual(serverUpdates, identityBefore);
    assert.ok(Number.isFinite(remotePlayer.render_x));
    assert.ok(Number.isFinite(remotePlayer.render_y));
});

test("resetSnapshotState allows immediate post-reset snapshot capture", () => {
    const players = new Map([["p1", { x: 2, y: 3, rotation: 0, velocity_x: 0, velocity_y: 0 }]]);
    const ctx = makeCtx({
        players,
        INTERPOLATION_SNAPSHOT_INTERVAL_MS: 1_000,
    });
    const manager = createInterpolationManager(() => ctx);

    manager.maybeRecordInterpolationSnapshot(2_000);
    manager.maybeRecordInterpolationSnapshot(2_100);
    assert.equal(ctx.serverUpdates.length, 1);

    manager.resetSnapshotState();
    manager.maybeRecordInterpolationSnapshot(2_101);
    assert.equal(ctx.serverUpdates.length, 2);
});
