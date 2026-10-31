import test from "node:test";
import assert from "node:assert/strict";

import { selectCtfObjectiveTargets } from "../client_logic/CombatFeedback.js";

const FlagStatus = { AtBase: 0, Carried: 1, Dropped: 2 };
const worldBoundsState = { valid: true, minX: -2000, maxX: 2000, minY: -1000, maxY: 1000 };
const enemyFlag = { status: FlagStatus.Carried, position: { x: 800, y: 40 }, carrier_id: "local-player" };

function selectTargets(args) {
    return selectCtfObjectiveTargets({ ...args, FlagStatus });
}

test("carrier with own flag at base is guided home", () => {
    const targets = selectTargets({
        localPlayerState: { team_id: 1, is_carrying_flag_team_id: 2 },
        myFlag: { status: FlagStatus.AtBase, position: { x: -1900, y: 0 } },
        enemyFlag,
        worldBoundsState,
        players: new Map(),
    });

    assert.deepEqual(targets, [{ x: -1900, y: 0, label: "RETURN TO BASE", tone: "positive" }]);
});

test("carrier with dropped own flag must recover it first", () => {
    const targets = selectTargets({
        localPlayerState: { team_id: 1, is_carrying_flag_team_id: 2 },
        myFlag: { status: FlagStatus.Dropped, position: { x: -75, y: 25 } },
        enemyFlag,
        worldBoundsState,
        players: new Map(),
    });

    assert.deepEqual(targets, [{ x: -75, y: 25, label: "RECOVER FLAG FIRST", tone: "critical" }]);
});

test("carrier with stolen own flag targets the live enemy carrier", () => {
    const targets = selectTargets({
        localPlayerState: { team_id: 1, is_carrying_flag_team_id: 2 },
        myFlag: { status: FlagStatus.Carried, position: { x: 9999, y: 9999 }, carrier_id: "enemy-carrier" },
        enemyFlag,
        worldBoundsState,
        players: new Map([["enemy-carrier", { x: 300, y: 40 }]]),
    });

    assert.deepEqual(targets, [{ x: 300, y: 40, label: "STOP ENEMY CARRIER", tone: "critical" }]);
});

test("carrier without a visible enemy carrier falls back to base recovery guidance", () => {
    const targets = selectTargets({
        localPlayerState: { team_id: 1, is_carrying_flag_team_id: 2 },
        myFlag: { status: FlagStatus.Carried, position: { x: 9999, y: 9999 }, carrier_id: "missing-carrier" },
        enemyFlag,
        worldBoundsState,
        players: new Map(),
    });

    assert.deepEqual(targets, [{ x: -1900, y: 0, label: "RECOVER FLAG FIRST", tone: "critical" }]);
});

test("non-carrier keeps the ordinary enemy flag guidance", () => {
    const targets = selectTargets({
        localPlayerState: { team_id: 1, is_carrying_flag_team_id: 0 },
        myFlag: { status: FlagStatus.AtBase, position: { x: -1900, y: 0 } },
        enemyFlag: { status: FlagStatus.AtBase, position: { x: 800, y: 40 } },
        worldBoundsState,
        players: new Map(),
    });

    assert.deepEqual(targets, [{ x: 800, y: 40, label: "ENEMY FLAG", tone: "critical" }]);
});
