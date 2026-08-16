import test from "node:test";
import assert from "node:assert/strict";

import { createGameRenderer } from "../client_logic/GameRenderer.js";

function createRenderer() {
    return createGameRenderer({
        PIXI: {},
        GP: {},
        PLAYER_RADIUS: 0,
        teamColors: {},
        weaponNames: {},
        weaponColors: {},
        pickupColors: {},
    });
}

function createFakeContainer(zoom = 1) {
    return {
        position: { x: 0, y: 0 },
        parent: { scale: { x: zoom } },
    };
}

const MAX_SHAKE_PX = 200;

test("screen shake: no offset is ever applied when trauma is 0", () => {
    const renderer = createRenderer();
    const container = createFakeContainer();
    for (let i = 0; i < 10; i++) {
        renderer.updateScreenShake(container, 16.67);
        assert.equal(container.position.x, 0);
        assert.equal(container.position.y, 0);
    }
});

test("screen shake: trauma rises on call and offsets the container within bounds", () => {
    const renderer = createRenderer();
    const container = createFakeContainer();
    renderer.applyScreenShake(null, 100, 10);
    renderer.updateScreenShake(container, 0);
    const magnitude = Math.hypot(container.position.x, container.position.y);
    assert.ok(magnitude > 0, "expected a non-zero shake offset");
    // A lone shake's peak amplitude equals its legacy intensity (100px).
    assert.ok(Math.abs(container.position.x) <= 100);
    assert.ok(Math.abs(container.position.y) <= 100);
});

test("screen shake: trauma decays to exactly 0 and the offset becomes exactly 0", () => {
    const renderer = createRenderer();
    const container = createFakeContainer();
    renderer.applyScreenShake(null, 100, 10);
    let settled = false;
    for (let i = 0; i < 120; i++) {
        renderer.updateScreenShake(container, 16.67);
        if (container.position.x === 0 && container.position.y === 0) {
            settled = true;
            break;
        }
    }
    assert.ok(settled, "shake offset must return to exactly 0 after decay");
    // And it must stay there: no residual drift, no restore of stale positions.
    renderer.updateScreenShake(container, 16.67);
    assert.equal(container.position.x, 0);
    assert.equal(container.position.y, 0);
});

test("screen shake: stacked shakes are capped at MAX_SHAKE_PX (screen px)", () => {
    const renderer = createRenderer();
    const container = createFakeContainer();
    // Two huge shakes far beyond the cap.
    renderer.applyScreenShake(null, 10000, 60);
    renderer.applyScreenShake(null, 10000, 60);
    for (let i = 0; i < 60; i++) {
        renderer.updateScreenShake(container, 16.67);
        assert.ok(Math.abs(container.position.x) <= MAX_SHAKE_PX);
        assert.ok(Math.abs(container.position.y) <= MAX_SHAKE_PX);
    }
});

test("screen shake: zoom compensation keeps the offset in screen pixels", () => {
    const renderer = createRenderer();
    const container = createFakeContainer(0.5);
    renderer.applyScreenShake(null, 100, 10);
    renderer.updateScreenShake(container, 0);
    // World-space offset may exceed 100px, but screen-space (x * zoom) may not.
    assert.ok(Math.abs(container.position.x * 0.5) <= 100);
    assert.ok(Math.abs(container.position.y * 0.5) <= 100);
});

test("screen shake: onConnectionReset clears trauma and zeroes the container", () => {
    const renderer = createRenderer();
    const container = createFakeContainer();
    renderer.applyScreenShake(null, 100, 600);
    renderer.updateScreenShake(container, 0);
    assert.notEqual(Math.hypot(container.position.x, container.position.y), 0);
    renderer.onConnectionReset();
    assert.equal(container.position.x, 0);
    assert.equal(container.position.y, 0);
    renderer.updateScreenShake(container, 16.67);
    assert.equal(container.position.x, 0);
    assert.equal(container.position.y, 0);
});
