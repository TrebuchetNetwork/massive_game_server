import test from "node:test";
import assert from "node:assert/strict";

import {
    clamp,
    lerp,
    normalizeAngle,
    smoothFollowGain,
} from "../client_logic/math_utils.js";

test("normalizeAngle wraps finite values and guards invalid values", () => {
    assert.equal(normalizeAngle(0), 0);
    assert.equal(normalizeAngle(Number.NaN), 0);
    assert.equal(normalizeAngle(Number.POSITIVE_INFINITY), 0);

    const wrapped = normalizeAngle(Math.PI * 3);
    assert.ok(wrapped <= Math.PI);
    assert.ok(wrapped >= -Math.PI);
});

test("clamp constrains values to min/max", () => {
    assert.equal(clamp(5, 0, 10), 5);
    assert.equal(clamp(-2, 0, 10), 0);
    assert.equal(clamp(50, 0, 10), 10);
});

test("lerp interpolates linearly", () => {
    assert.equal(lerp(0, 10, 0), 0);
    assert.equal(lerp(0, 10, 0.5), 5);
    assert.equal(lerp(0, 10, 1), 10);
});

test("smoothFollowGain scales gain by frame delta and remains bounded", () => {
    const baseGain = 0.3;
    const smallDelta = smoothFollowGain(baseGain, 1 / 120);
    const mediumDelta = smoothFollowGain(baseGain, 1 / 60);
    const largeDelta = smoothFollowGain(baseGain, 1 / 10);

    assert.ok(smallDelta > 0 && smallDelta < 1);
    assert.ok(mediumDelta > 0 && mediumDelta < 1);
    assert.ok(largeDelta > 0 && largeDelta < 1);
    assert.ok(smallDelta <= mediumDelta);
    assert.ok(mediumDelta <= largeDelta);
});
