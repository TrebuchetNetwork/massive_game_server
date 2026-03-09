/**
 * Legacy compatibility shim.
 *
 * Canonical implementation lives in `effects_audio_runtime.js`.
 */

import { createEffectsAudioRuntime } from "./effects_audio_runtime.js";
import { emitClientLog } from "./client_logger.js";

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
    emitClientLog(
        "[EffectsManager] Runtime bootstrap failed, using no-op fallback.",
        "warn",
        error
    );
}

export class EffectsManager extends RuntimeEffectsManager {}
