/**
 * Active barrel module for client logic used by static_client/client.html.
 */

export { Minimap, NetworkIndicator } from './ui_widgets.js';
export { buildRuntimeConfig } from './runtime_config.js';
export {
    buildPeerConnectionConfig,
    getDefaultWsUrl,
    normalizeSignalingUrl,
    summarizeSignalingError,
} from './networking_utils.js';
export { clamp, lerp, normalizeAngle, smoothFollowGain } from './math_utils.js';
export { createAuthHelpers } from './auth_utils.js';
export { createReconnectHelpers } from './reconnect_utils.js';
export { createAcceleratedLayerRuntime } from './accelerated_layers.js';
export { createEffectsAudioRuntime } from './effects_audio_runtime.js';
export {
    getRendererBackendSummary,
    readWebGpuAutorunConfigFromUrl,
    runWebGPUTest,
} from './webgpu_test.js';
export {
    populateSyntheticProjectiles,
    removeSyntheticProjectiles,
} from './synthetic_projectiles.js';
export {
    applyConnectionStatusUi,
    normalizeConnectionErrorDetail,
} from './connection_status.js';
export { createGameRenderer } from './GameRenderer.js';
export { createProtocolHandler } from './ProtocolHandler.js';
export { createInputManager } from './InputManager.js';
export { createGameState } from './GameState.js';
export { createCombatFeedback } from './CombatFeedback.js';
export { createAimingSystem } from './AimingSystem.js';
export { createUIManager } from './UIManager.js';
export { createSpriteManager } from './SpriteManager.js';
export { createServerUpdateHandler } from './ServerUpdateHandler.js';
export { createConnectionManager } from './ConnectionManager.js';
export { createWorldRenderer } from './WorldRenderer.js';
export { createInterpolationManager } from './InterpolationManager.js';
export { createDiagnosticsManager } from './DiagnosticsManager.js';
export { createRenderAssetManager } from './RenderAssetManager.js';
export { createUpdateSprites } from './UpdateSprites.js';
export { createPerformanceBudget } from './PerformanceBudget.js';
