import test from "node:test";
import assert from "node:assert/strict";

import { createCombatFeedback } from "../client_logic/CombatFeedback.js";

function createClassList() {
    const values = new Set();
    return {
        add(...classes) {
            classes.forEach((value) => values.add(value));
        },
        remove(...classes) {
            classes.forEach((value) => values.delete(value));
        },
        toggle(className, force) {
            if (force === undefined) {
                if (values.has(className)) {
                    values.delete(className);
                    return false;
                }
                values.add(className);
                return true;
            }
            if (force) {
                values.add(className);
                return true;
            }
            values.delete(className);
            return false;
        },
        contains(className) {
            return values.has(className);
        },
    };
}

function createElement() {
    return {
        classList: createClassList(),
        style: {},
        textContent: "",
    };
}

function createCombatUiState() {
    return {
        momentum: 0,
        speedPulse: 0,
        damagePulse: 0,
        localKillStreak: 0,
        comboCount: 0,
        comboExpiresAt: 0,
        bannerUntilMs: 0,
        bannerTone: "kill",
        medalText: "",
        medalUntilMs: 0,
        streakAnnouncerText: "",
        streakAnnouncerTone: "critical",
        streakAnnouncerUntilMs: 0,
        markerUntilMs: 0,
        markerHeadshotUntilMs: 0,
        markerKillUntilMs: 0,
        lastOptimisticHitAt: 0,
        hitstopUntilMs: 0,
        tipUntilMs: 0,
        modeIntroUntilMs: 0,
        lastBoundaryTipAt: 0,
        objectiveText: "",
        objectiveTone: "critical",
        objectiveUntilMs: 0,
        lastObjectiveEvalAt: 0,
        damageIndicators: [],
        recentDamageSources: [],
        damageIndicatorElements: [],
        damageIndicatorVisibleCount: 0,
        lastDamageIndicatorPaintAt: 0,
        deathRecapUntilMs: 0,
        deathRecapText: "",
        deathRecapDistanceText: "",
        deathRecapRows: [],
        trackedSpeedBoostMaxSec: 0,
        trackedDamageBoostMaxSec: 0,
        lastKnownHealth: null,
        processedKillFeedKeys: new Set(),
        processedKillFeedQueue: [],
        radialHudCache: {
            lastPaintAt: 0,
            positionMode: "",
            left: "",
            top: "",
            transform: "",
            reloadVisible: false,
            reloadDeg: -1,
            reloadLabel: "",
            abilityVisible: false,
            abilityDeg: -1,
            abilityColor: "",
            abilityLabel: "",
            dashVisible: false,
            dashDeg: -1,
            dashLabel: "",
            dashReadyVisible: false,
            dashReadyUntilMs: 0,
            dashLastRemaining: 0,
            dodgeVisible: false,
            dodgeDeg: -1,
            dodgeLabel: "",
            dodgeReadyVisible: false,
            dodgeReadyUntilMs: 0,
            dodgeLastRemaining: 0,
            hudVisible: false,
        },
    };
}

function makeCtx() {
    return {
        tacticalPings: [],
        players: new Map([
            [
                "enemy-1",
                {
                    username: "Enemy",
                    x: 100,
                    y: 200,
                    team_id: 2,
                },
            ],
        ]),
        myPlayerId: "local-player",
        localPlayerState: { team_id: 1 },
        GP: {
            GameEventType: {
                Killstreak: 7,
            },
        },
        TACTICAL_PING_MS: 6200,
        EXCITEMENT_UI_ENABLED: true,
        objectiveUrgencyDiv: createElement(),
        combatBannerDiv: createElement(),
        streakAnnouncerDiv: createElement(),
        streakMedalDiv: createElement(),
        hitMarkerDiv: createElement(),
        deathRecapDiv: createElement(),
        tipsToastDiv: null,
        gameModeIntroDiv: null,
        boundaryWarningDiv: null,
        damageDirectionLayerDiv: null,
        combatRadialHudDiv: createElement(),
        dashRadialDiv: createElement(),
        dodgeRadialDiv: createElement(),
        abilityRadialDiv: createElement(),
        reloadRadialDiv: createElement(),
        deathRecapMainDiv: createElement(),
        deathRecapDistanceDiv: createElement(),
        deathRecapMinimapCanvas: null,
        objectiveArrowLayerDiv: null,
        combatUiState: createCombatUiState(),
    };
}

test("onConnectionReset clears streak ping cooldowns", () => {
    const ctx = makeCtx();
    const feedback = createCombatFeedback(() => ctx);
    const killstreakEvent = {
        event_type: ctx.GP.GameEventType.Killstreak,
        instigator_id: "enemy-1",
        value: 5,
    };

    feedback.registerCombatEventFeedback(killstreakEvent);
    assert.equal(ctx.tacticalPings.length, 1);

    feedback.registerCombatEventFeedback(killstreakEvent);
    assert.equal(ctx.tacticalPings.length, 1);

    feedback.onConnectionReset();
    feedback.registerCombatEventFeedback(killstreakEvent);
    assert.equal(ctx.tacticalPings.length, 2);
});
