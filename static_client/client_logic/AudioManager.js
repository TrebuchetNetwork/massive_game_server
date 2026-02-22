/**
 * Legacy compatibility shim.
 *
 * Canonical implementation lives in `effects_audio_runtime.js`.
 */

import { createEffectsAudioRuntime } from "./effects_audio_runtime.js";

const { AudioManager: RuntimeAudioManager } = createEffectsAudioRuntime();

export class AudioManager extends RuntimeAudioManager {}
