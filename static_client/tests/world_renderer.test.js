import test from 'node:test';
import assert from 'node:assert/strict';

import { createWorldRenderer } from '../client_logic/WorldRenderer.js';

test('broadcast camera follows the director frame with cinematic smoothing', () => {
    const gameScene = {
        scale: {
            x: 1,
            y: 1,
            set(value) {
                this.x = value;
                this.y = value;
            },
        },
        position: { x: 0, y: 0 },
    };
    const ctx = {
        app: { screen: { width: 800, height: 600 }, ticker: { deltaMS: 16.67 } },
        gameScene,
        dynamicsTuning: { cameraBaseScale: 1 },
        isBroadcastMode: () => true,
        getBroadcastCameraFrame: () => ({ focusX: 100, focusY: 50, zoom: 1.15, targetId: 'a' }),
    };
    const renderer = createWorldRenderer(() => ctx);

    renderer.updateCamera();

    const expectedScale = 1 + (1.15 - 1) * 0.15;
    assert.ok(Math.abs(gameScene.scale.x - expectedScale) < 1e-9);
    assert.ok(Math.abs(gameScene.position.x - (400 - 100 * expectedScale) * 0.06) < 1e-9);
    assert.ok(Math.abs(gameScene.position.y - (300 - 50 * expectedScale) * 0.06) < 1e-9);
});
