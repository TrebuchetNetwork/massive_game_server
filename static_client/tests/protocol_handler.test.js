import test from "node:test";
import assert from "node:assert/strict";

import { createProtocolHandler } from "../client_logic/ProtocolHandler.js";

function makeProtocolHandler(overrides = {}) {
    class StubType {}
    class StubByteBuffer {
        constructor(bytes) {
            this.bytes_ = bytes;
            this.position_ = 0;
        }
    }

    const GP = {
        InitialStateMessage: StubType,
        InitialState: StubType,
        DeltaStateMessage: StubType,
        DeltaState: StubType,
        GameMessage: StubType,
        WelcomeMessage: StubType,
        ChatMessage: StubType,
        MatchInfo: StubType,
        PlayerState: StubType,
        ProjectileState: StubType,
        Pickup: StubType,
        Wall: StubType,
        FlagState: StubType,
        KillFeedEntry: StubType,
        GameEvent: StubType,
        Vec2: StubType,
    };

    return createProtocolHandler({
        flatbuffers: { ByteBuffer: StubByteBuffer },
        GameProtocol: overrides.GameProtocol || {},
        GP,
        log: () => {},
        isWallDebugEnabled: () => false,
        logWallDebug: () => {},
    });
}

function buildCoalescedPacket(payloads) {
    const headerBytes = 7;
    const entryHeaderBytes = 4;
    const totalPayloadBytes = payloads.reduce((sum, payload) => sum + payload.length, 0);
    const totalEntryHeaderBytes = payloads.length * entryHeaderBytes;
    const totalBytes = headerBytes + totalEntryHeaderBytes + totalPayloadBytes;
    const packet = new Uint8Array(totalBytes);
    const view = new DataView(packet.buffer);

    packet[0] = 0x4d; // M
    packet[1] = 0x47; // G
    packet[2] = 0x53; // S
    packet[3] = 0x42; // B
    packet[4] = 1; // version
    view.setUint16(5, payloads.length, true);

    let cursor = headerBytes;
    for (const payload of payloads) {
        view.setUint32(cursor, payload.length, true);
        cursor += entryHeaderBytes;
        packet.set(payload, cursor);
        cursor += payload.length;
    }
    return packet;
}

test("toBinaryView accepts typed arrays, buffers, and views", () => {
    const handler = makeProtocolHandler();
    const bytes = new Uint8Array([1, 2, 3, 4]);
    const arrayBuffer = bytes.buffer.slice(0);
    const dataView = new DataView(arrayBuffer);

    assert.equal(handler.toBinaryView(bytes), bytes);
    assert.deepEqual(Array.from(handler.toBinaryView(arrayBuffer)), [1, 2, 3, 4]);
    assert.deepEqual(Array.from(handler.toBinaryView(dataView)), [1, 2, 3, 4]);
    assert.equal(handler.toBinaryView("invalid"), null);
});

test("unpackCoalescedPackets parses valid packet envelope and falls back on invalid header", () => {
    const handler = makeProtocolHandler();
    const payloadA = new Uint8Array([10, 11, 12]);
    const payloadB = new Uint8Array([20, 21]);
    const coalesced = buildCoalescedPacket([payloadA, payloadB]);

    const unpacked = handler.unpackCoalescedPackets(coalesced);
    assert.equal(unpacked.length, 2);
    assert.deepEqual(Array.from(unpacked[0]), [10, 11, 12]);
    assert.deepEqual(Array.from(unpacked[1]), [20, 21]);

    const fallback = handler.unpackCoalescedPackets(new Uint8Array([1, 2, 3]));
    assert.equal(fallback.length, 1);
    assert.deepEqual(Array.from(fallback[0]), [1, 2, 3]);
});

test("normalizePlayerDeltaMask enforces full mask for invalid or zero masks", () => {
    const handler = makeProtocolHandler();
    const fullMask = handler.PLAYER_DELTA_FULL_MASK;

    assert.equal(handler.normalizePlayerDeltaMask(0), fullMask);
    assert.equal(handler.normalizePlayerDeltaMask(NaN), fullMask);
    assert.equal(handler.normalizePlayerDeltaMask(Infinity), fullMask);
    assert.equal(handler.normalizePlayerDeltaMask(3), 3);
    assert.equal(handler.normalizePlayerDeltaMask(3, true), fullMask);
});

test("parseMatchInfo includes base fields and optional winner/commander metadata", () => {
    const handler = makeProtocolHandler();
    const matchInfoTable = {
        matchState: () => 2,
        gameMode: () => 4,
        timeRemaining: () => 99,
        teamScoresLength: () => 2,
        teamScores: (index) =>
            index === 0
                ? { teamId: () => 1, score: () => 40 }
                : { teamId: () => 2, score: () => 35 },
        winnerId: () => "winner-1",
        winnerName: () => "Top Player",
        team1CommanderWaypoint: () => ({ x: () => 120, y: () => 220 }),
        team2CommanderWaypoint: () => null,
    };

    const parsed = handler.parseMatchInfo(matchInfoTable);
    assert.equal(parsed.match_state, 2);
    assert.equal(parsed.game_mode, 4);
    assert.equal(parsed.time_remaining, 99);
    assert.deepEqual(parsed.team_scores, [
        { team_id: 1, score: 40 },
        { team_id: 2, score: 35 },
    ]);
    assert.equal(parsed.winner_id, "winner-1");
    assert.equal(parsed.winner_name, "Top Player");
    assert.deepEqual(parsed.team1_commander_waypoint, { x: 120, y: 220 });
    assert.equal(parsed.team2_commander_waypoint, null);
});

test("parseFlatBufferMessage reports protocol mismatch before payload parsing", () => {
    const handler = makeProtocolHandler({
        GameProtocol: {
            MessageType: { Welcome: 1 },
            GameMessage: {
                getRootAsGameMessage() {
                    return {
                        protocolVersion: () => 999,
                        msgType: () => 1,
                    };
                },
            },
        },
    });

    const parsed = handler.parseFlatBufferMessage(new Uint8Array([1, 2, 3]));
    assert.equal(parsed.type, "protocol_error");
    assert.equal(parsed.serverProtocolVersion, 999);
    assert.match(parsed.detail, /Protocol mismatch/);
});

test("parseFlatBufferMessage includes welcome protocol version diagnostics", () => {
    const handler = makeProtocolHandler({
        GameProtocol: {
            MessageType: { Welcome: 1 },
            GameMessage: {
                getRootAsGameMessage(_buf, _scratch) {
                    return {
                        protocolVersion: () => 1,
                        msgType: () => 1,
                        actualMessage: () => ({
                            playerId: () => "player-1",
                            message: () => "hello",
                            serverTickRate: () => 60,
                            serverProtocolVersion: () => 1,
                        }),
                    };
                },
            },
        },
    });

    const parsed = handler.parseFlatBufferMessage(new Uint8Array([9, 9, 9]));
    assert.equal(parsed.type, "welcome");
    assert.equal(parsed.playerId, "player-1");
    assert.equal(parsed.serverTickRate, 60);
    assert.equal(parsed.serverProtocolVersion, 1);
});
