/**
 * Legacy compatibility shim.
 *
 * Canonical implementation lives in `effects_audio_runtime.js`.
 */

import { createEffectsAudioRuntime } from "./effects_audio_runtime.js";

const { EffectsManager: RuntimeEffectsManager } = createEffectsAudioRuntime();

export class EffectsManager extends RuntimeEffectsManager {}
