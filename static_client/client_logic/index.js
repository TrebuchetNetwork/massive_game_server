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
export { CLIENT_PROTOCOL_VERSION } from './protocol_version.js';
export { createAuthHelpers } from './auth_utils.js';
export { createReconnectHelpers } from './reconnect_utils.js';
export { createAcceleratedLayerRuntime } from './accelerated_layers.js';
export { createEffectsAudioRuntime } from './effects_audio_runtime.js?v=20260225c';
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
export {
    JOIN_PROGRESS_STAGES,
    CONTROL_HINTS_STORAGE_KEY,
    createJoinProgressTracker,
    applyJoinProgressUi,
    getControlHintItems,
    shouldShowControlHints,
    markControlHintsSeen,
} from './join_progress.js?v=20260828-joinux1';
export { emitClientLog, flushPendingClientLogs } from './client_logger.js';
export { createErrorBoundary } from './ErrorBoundary.js';
export { createGameRenderer } from './GameRenderer.js';
export { createProtocolHandler } from './ProtocolHandler.js?v=20260309a';
export { createInputManager } from './InputManager.js?v=20260725-mobile-arena2';
export { createCombatFeedback } from './CombatFeedback.js';
export { createAimingSystem } from './AimingSystem.js';
export { createUIManager } from './UIManager.js';
export { createSpriteManager } from './SpriteManager.js';
export { createServerUpdateHandler } from './ServerUpdateHandler.js?v=20260306a';
export { createConnectionManager } from './ConnectionManager.js?v=20260725-mobile-arena2';
export { createWorldRenderer } from './WorldRenderer.js?v=20260306a';
export { createInterpolationManager } from './InterpolationManager.js';
export { createDiagnosticsManager } from './DiagnosticsManager.js';
export { createRenderAssetManager } from './RenderAssetManager.js?v=20260225a';
export { createUpdateSprites } from './UpdateSprites.js?v=20260225b';
export { createPerformanceBudget } from './PerformanceBudget.js';
export {
    MAP_THEMES,
    MAP_THEME_DEFAULT_NAME,
    createMapThemes,
    resolveMapTheme,
} from './MapThemes.js';
