import test from "node:test";
import assert from "node:assert/strict";

import { createServerUpdateHandler } from "../client_logic/ServerUpdateHandler.js";

function makeCtx(overrides = {}) {
    const ctx = {
        log: () => {},
        GP: {},
        normalizeAngle: (v) => v,
        walls: new Map(),
        players: new Map(),
        projectiles: new Map(),
        pickups: new Map(),
        zones: new Map(),
        localPlayerState: null,
        myPlayerId: "local-player",
        drawWalls: () => {},
        drawZones: () => {},
        removePlayerClientState: () => {},
        removeProjectileClientState: () => {},
        normalizePlayerDeltaMask: () => 0,
        assignPlayerStateFromObject: () => {},
        markProjectileServerUpdate: () => {},
        isWallDebugEnabled: () => false,
        logWallDebug: () => {},
        effectsManager: null,
        minimap: null,
        killFeed: [],
        matchInfo: null,
        updateKillFeed: () => {},
        refreshMatchInfoUi: () => {},
        updateFlags: () => {},
        RESPAWN_ANIMATION_LIGHTWEIGHT: false,
        pendingInputs: [],
        maybeRecordInterpolationSnapshot: () => {},
        minimapWallsCacheDirty: false,
        currentMapName: "",
        lastProcessedInput: 0,
        fastDeltaPathErrorCount: 0,
        setFastDeltaPathErrorCount: (v) => {
            ctx.fastDeltaPathErrorCount = v;
        },
        incrementFastDeltaPathErrorCount: () => {
            ctx.fastDeltaPathErrorCount += 1;
        },
        setLastProcessedInput: (v) => {
            ctx.lastProcessedInput = v;
        },
        setPendingInputs: (v) => {
            ctx.pendingInputs = v;
        },
        setLocalPlayerState: (v) => {
            ctx.localPlayerState = v;
        },
        setKillFeed: (v) => {
            ctx.killFeed = v;
        },
        setMatchInfo: (v) => {
            ctx.matchInfo = v;
        },
        setCurrentMapName: (v) => {
            ctx.currentMapName = v;
        },
        setMinimapWallsCacheDirty: (v) => {
            ctx.minimapWallsCacheDirty = v;
        },
        ...overrides,
    };
    return ctx;
}

test("processServerUpdate preserves existing initial walls when replaceInitialState is false", () => {
    let drawWallsCalls = 0;
    const walls = new Map([
        ["existing-wall", { id: "existing-wall", is_destructible: false, current_health: 999 }],
    ]);
    const ctx = makeCtx({
        walls,
        drawWalls: () => {
            drawWallsCalls += 1;
        },
    });

    const handler = createServerUpdateHandler(() => ctx);
    handler.processServerUpdate(
        {
            timestamp: 10,
            walls: [
                { id: "new-wall", x: 10, y: 10, width: 20, height: 20, is_destructible: false, current_health: 100 },
            ],
        },
        true,
        { replaceInitialState: false }
    );

    assert.equal(drawWallsCalls, 1);
    assert.equal(ctx.walls.has("existing-wall"), true);
    assert.equal(ctx.walls.has("new-wall"), true);
});

test("processServerUpdate clears initial walls when replaceInitialState is true/default", () => {
    const walls = new Map([
        ["existing-wall", { id: "existing-wall", is_destructible: false, current_health: 999 }],
    ]);
    const ctx = makeCtx({ walls });

    const handler = createServerUpdateHandler(() => ctx);
    handler.processServerUpdate(
        {
            timestamp: 11,
            walls: [
                { id: "new-wall", x: 10, y: 10, width: 20, height: 20, is_destructible: false, current_health: 100 },
            ],
        },
        true
    );

    assert.equal(ctx.walls.has("existing-wall"), false);
    assert.equal(ctx.walls.has("new-wall"), true);
});

test("processServerUpdate prunes pending inputs using last_processed_input_sequence", () => {
    const ctx = makeCtx({
        pendingInputs: [
            { sequence: 1, timestamp: 1000 },
            { sequence: 2, timestamp: 1016 },
            { sequence: 3, timestamp: 1032 },
        ],
    });
    const handler = createServerUpdateHandler(() => ctx);

    handler.processServerUpdate(
        {
            timestamp: 20,
            last_processed_input_sequence: 2,
        },
        false
    );

    assert.equal(ctx.lastProcessedInput, 2);
    assert.equal(ctx.pendingInputs.length, 1);
    assert.equal(ctx.pendingInputs[0].sequence, 3);
});
