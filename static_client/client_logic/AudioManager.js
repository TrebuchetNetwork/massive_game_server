/**
 * Legacy compatibility shim.
 *
 * Canonical implementation lives in `effects_audio_runtime.js`.
 */

import { createEffectsAudioRuntime } from "./effects_audio_runtime.js";
import { emitClientLog } from "./client_logger.js";

class NoopAudioManager {
    constructor() {}
    playSound() {}
    playWeaponSound() {}
    destroy() {}
}

let RuntimeAudioManager = NoopAudioManager;
try {
    const runtime = createEffectsAudioRuntime();
    if (runtime && typeof runtime.AudioManager === 'function') {
        RuntimeAudioManager = runtime.AudioManager;
    }
} catch (error) {
    emitClientLog(
        "[AudioManager] Runtime bootstrap failed, using no-op fallback.",
        "warn",
        error
    );
}

export class AudioManager extends RuntimeAudioManager {}
