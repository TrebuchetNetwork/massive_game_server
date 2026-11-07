import test from 'node:test';
import assert from 'node:assert/strict';

import { objectiveArrowPosition } from '../client_logic/CombatFeedback.js';

const viewport = { viewportWidth: 390, viewportHeight: 844, cardWidth: 100, cardHeight: 40, margin: 8 };

test('offscreen objective cards remain fully inside the right viewport edge', () => {
    assert.deepEqual(objectiveArrowPosition({ ...viewport, x: 370, y: 400 }), { x: 282, y: 380 });
});

test('objective cards clamp left, top, and bottom edges', () => {
    assert.deepEqual(objectiveArrowPosition({ ...viewport, x: 0, y: 0 }), { x: 8, y: 8 });
    assert.deepEqual(objectiveArrowPosition({ ...viewport, x: 195, y: 840 }), { x: 145, y: 796 });
});

test('objective cards stay centered on their anchor when space allows', () => {
    assert.deepEqual(objectiveArrowPosition({ ...viewport, x: 195, y: 422 }), { x: 145, y: 402 });
});
