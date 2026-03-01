/**
 * Legacy compatibility shim.
 *
 * Canonical implementation lives in `effects_audio_runtime.js`.
 */

import { createEffectsAudioRuntime } from "./effects_audio_runtime.js";

class NoopEffectsManager {
    constructor() {}
    processGameEvent() {}
    update() {}
    destroy() {}
}

let RuntimeEffectsManager = NoopEffectsManager;
try {
    const runtime = createEffectsAudioRuntime();
    if (runtime && typeof runtime.EffectsManager === 'function') {
        RuntimeEffectsManager = runtime.EffectsManager;
    }
} catch (error) {
    console.warn('[EffectsManager] Runtime bootstrap failed, using no-op fallback.', error);
}

export class EffectsManager extends RuntimeEffectsManager {}
