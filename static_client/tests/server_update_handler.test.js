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

test("processServerUpdate fans game events, kill feed, and snapshots out to broadcast hooks", () => {
    const gameEvents = [{ event_type: 3, instigator_id: "attacker", value: 25 }];
    const killFeed = [{ killer_name: "Attacker", victim_name: "Victim", timestamp: 10 }];
    const received = { events: [], feeds: [], snapshots: 0 };
    const ctx = makeCtx({
        effectsManager: {
            processGameEvent: (event) => received.events.push(event),
        },
        broadcastHooks: {
            onGameEvent: (event) => received.events.push(event),
            onKillFeed: (feed) => received.feeds.push(feed),
            onSnapshot: () => { received.snapshots += 1; },
        },
    });
    const handler = createServerUpdateHandler(() => ctx);

    handler.processServerUpdate({ timestamp: 10, game_events: gameEvents, kill_feed: killFeed }, false);

    assert.deepEqual(received.events, [gameEvents[0], gameEvents[0]]);
    assert.deepEqual(received.feeds, [killFeed]);
    assert.equal(received.snapshots, 1);
});

test("processDeltaStateFast fans parsed game events, kill feed, and snapshots out to broadcast hooks", () => {
    const received = { events: [], feeds: [], snapshots: 0 };
    const killFeedRow = {
        killerPosition: () => null,
        victimPosition: () => null,
        killerName: () => "Attacker",
        victimName: () => "Victim",
        weapon: () => 1,
        timestamp: () => 10,
        isHeadshot: () => false,
        killContext: () => 0,
    };
    const gameEvent = {
        position: () => null,
        eventType: () => 3,
        instigatorId: () => "attacker",
        targetId: () => "victim",
        weaponType: () => 1,
        value: () => 25,
        falloffMultiplier: () => 1,
    };
    const zeroLength = () => 0;
    const delta = {
        lastProcessedInputSequence: () => 0,
        timestamp: () => 10,
        destroyedWallIdsLength: zeroLength,
        updatedWallsLength: zeroLength,
        wallsLength: zeroLength,
        playersLength: zeroLength,
        changedPlayerFieldsLength: zeroLength,
        removedPlayerIdsLength: zeroLength,
        projectilesLength: zeroLength,
        removedProjectilesLength: zeroLength,
        pickupsLength: zeroLength,
        deactivatedPickupIdsLength: zeroLength,
        killFeedLength: () => 1,
        killFeed: () => killFeedRow,
        matchInfo: () => null,
        flagStatesLength: zeroLength,
        gameEventsLength: () => 1,
        gameEvents: () => gameEvent,
    };
    const ctx = makeCtx({
        effectsManager: {
            processGameEvent: (event) => received.events.push(event),
        },
        broadcastHooks: {
            onGameEvent: (event) => received.events.push(event),
            onKillFeed: (feed) => received.feeds.push(feed),
            onSnapshot: () => { received.snapshots += 1; },
        },
        parseMatchInfo: () => null,
        flatbufferParseScratch: {
            wall: {}, playerState: {}, projectileState: {}, pickup: {}, vec2: {},
            killFeedEntry: {}, matchInfo: {}, flagState: {}, gameEvent: {},
        },
        DELTA_SUPPORTS_REMOVED_PLAYER_IDS: false,
        DELTA_SUPPORTS_CHANGED_PLAYER_FIELDS: false,
        DELTA_SUPPORTS_UPDATED_WALLS: false,
        DELTA_SUPPORTS_FULL_WALLS: false,
        PLAYER_DELTA_FULL_MASK: 0,
    });
    const handler = createServerUpdateHandler(() => ctx);

    handler.processDeltaStateFast(delta);

    assert.equal(received.events.length, 2);
    assert.deepEqual(received.events[0], received.events[1]);
    assert.equal(received.events[1].instigator_id, "attacker");
    assert.equal(received.events[1].value, 25);
    assert.equal(received.feeds.length, 1);
    assert.equal(received.feeds[0][0].killer_name, "Attacker");
    assert.equal(received.snapshots, 1);
});

test("processServerUpdate continues after every broadcast hook throws", (t) => {
    const warnings = [];
    t.mock.method(console, "warn", (...args) => warnings.push(args));
    const calls = { events: 0, feeds: 0, snapshots: 0, killFeedUi: 0, interpolation: 0 };
    const killFeed = [{ killer_name: "Attacker", victim_name: "Victim", timestamp: 10 }];
    const ctx = makeCtx({
        effectsManager: {
            processGameEvent: () => { calls.events += 1; },
        },
        updateKillFeed: () => { calls.killFeedUi += 1; },
        maybeRecordInterpolationSnapshot: () => { calls.interpolation += 1; },
        broadcastHooks: {
            onGameEvent: () => { throw new Error("event hook failed"); },
            onKillFeed: () => { throw new Error("feed hook failed"); },
            onSnapshot: () => { calls.snapshots += 1; throw new Error("snapshot hook failed"); },
        },
    });
    const handler = createServerUpdateHandler(() => ctx);
    const update = () => handler.processServerUpdate({
        timestamp: 10,
        game_events: [{ event_type: 3, instigator_id: "attacker", value: 25 }],
        kill_feed: killFeed,
    }, false);

    assert.doesNotThrow(update);
    assert.doesNotThrow(update);

    assert.equal(ctx.killFeed, killFeed);
    assert.equal(calls.events, 2);
    assert.equal(calls.snapshots, 2);
    assert.equal(calls.killFeedUi, 2);
    assert.equal(calls.interpolation, 2);
    assert.equal(warnings.length, 3, "each JSON hook site warns only once");
});

test("processDeltaStateFast continues after every broadcast hook throws", (t) => {
    const warnings = [];
    t.mock.method(console, "warn", (...args) => warnings.push(args));
    const calls = { events: 0, snapshots: 0, killFeedUi: 0, interpolation: 0 };
    const killFeedRow = {
        killerPosition: () => null,
        victimPosition: () => null,
        killerName: () => "Attacker",
        victimName: () => "Victim",
        weapon: () => 1,
        timestamp: () => 10,
        isHeadshot: () => false,
        killContext: () => 0,
    };
    const gameEvent = {
        position: () => null,
        eventType: () => 3,
        instigatorId: () => "attacker",
        targetId: () => "victim",
        weaponType: () => 1,
        value: () => 25,
        falloffMultiplier: () => 1,
    };
    const zeroLength = () => 0;
    const delta = {
        lastProcessedInputSequence: () => 0,
        timestamp: () => 10,
        destroyedWallIdsLength: zeroLength,
        updatedWallsLength: zeroLength,
        wallsLength: zeroLength,
        playersLength: zeroLength,
        changedPlayerFieldsLength: zeroLength,
        removedPlayerIdsLength: zeroLength,
        projectilesLength: zeroLength,
        removedProjectilesLength: zeroLength,
        pickupsLength: zeroLength,
        deactivatedPickupIdsLength: zeroLength,
        killFeedLength: () => 1,
        killFeed: () => killFeedRow,
        matchInfo: () => null,
        flagStatesLength: zeroLength,
        gameEventsLength: () => 1,
        gameEvents: () => gameEvent,
    };
    const ctx = makeCtx({
        effectsManager: {
            processGameEvent: () => { calls.events += 1; },
        },
        updateKillFeed: () => { calls.killFeedUi += 1; },
        maybeRecordInterpolationSnapshot: () => { calls.interpolation += 1; },
        broadcastHooks: {
            onGameEvent: () => { throw new Error("event hook failed"); },
            onKillFeed: () => { throw new Error("feed hook failed"); },
            onSnapshot: () => { calls.snapshots += 1; throw new Error("snapshot hook failed"); },
        },
        parseMatchInfo: () => null,
        flatbufferParseScratch: {
            wall: {}, playerState: {}, projectileState: {}, pickup: {}, vec2: {},
            killFeedEntry: {}, matchInfo: {}, flagState: {}, gameEvent: {},
        },
        DELTA_SUPPORTS_REMOVED_PLAYER_IDS: false,
        DELTA_SUPPORTS_CHANGED_PLAYER_FIELDS: false,
        DELTA_SUPPORTS_UPDATED_WALLS: false,
        DELTA_SUPPORTS_FULL_WALLS: false,
        PLAYER_DELTA_FULL_MASK: 0,
    });
    const handler = createServerUpdateHandler(() => ctx);

    assert.doesNotThrow(() => handler.processDeltaStateFast(delta));
    assert.doesNotThrow(() => handler.processDeltaStateFast(delta));

    assert.equal(ctx.killFeed.length, 1);
    assert.equal(ctx.killFeed[0].killer_name, "Attacker");
    assert.equal(calls.events, 2);
    assert.equal(calls.snapshots, 2);
    assert.equal(calls.killFeedUi, 2);
    assert.equal(calls.interpolation, 2);
    assert.equal(warnings.length, 3, "each delta hook site warns only once");
});
