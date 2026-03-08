export function buildRuntimeConfig(search, GP) {
    const uiModeParams = new URLSearchParams(search || "");

    const parseToggleParam = (rawValue) => {
        const normalized = String(rawValue || "").trim().toLowerCase();
        if (!normalized) return { state: "unset", raw: "" };
        if (normalized === "auto") return { state: "auto", raw: normalized };
        if (
            normalized === "1" ||
            normalized === "true" ||
            normalized === "on" ||
            normalized === "yes"
        ) {
            return { state: "on", raw: normalized };
        }
        if (
            normalized === "0" ||
            normalized === "false" ||
            normalized === "off" ||
            normalized === "no" ||
            normalized === "disabled"
        ) {
            return { state: "off", raw: normalized };
        }
        return { state: "unset", raw: normalized };
    };

    const BENCH_MODE =
        uiModeParams.get("mode") === "bench" || uiModeParams.get("bench") === "1";
    const MASS_MODE_FORCED =
        uiModeParams.get("mode") === "mass" || uiModeParams.get("mass") === "1";
    const STABLE_MODE_FORCED =
        MASS_MODE_FORCED ||
        uiModeParams.get("mode") === "stable" ||
        uiModeParams.get("stable") === "1";
    const TOURNAMENT_MODE_FORCED =
        uiModeParams.get("mode") === "tournament" ||
        uiModeParams.get("tournament") === "1";
    const LOW_OVERHEAD_MODE = BENCH_MODE || STABLE_MODE_FORCED;
    const WORKER_CULL_PARAM = uiModeParams.get("worker_cull");
    const WORKER_CULL_ENABLED =
        WORKER_CULL_PARAM === "0"
            ? false
            : WORKER_CULL_PARAM === "1" ||
              MASS_MODE_FORCED ||
              STABLE_MODE_FORCED ||
              TOURNAMENT_MODE_FORCED;
    const WORKER_CULL_INTERVAL_MS = Math.max(
        16,
        Math.min(
            250,
            Math.floor(Number(uiModeParams.get("worker_cull_interval_ms")) || 33)
        )
    );
    const DEFAULT_WORKER_CULL_WASM_URL = "./workers/entity_cull_kernel.wasm";
    const WORKER_CULL_WASM_URL = String(uiModeParams.get("worker_wasm_url") || "").trim();
    const WORKER_CULL_MODE = (() => {
        const raw = String(uiModeParams.get("worker_cull_mode") || "auto")
            .trim()
            .toLowerCase();
        if (raw === "linear" || raw === "quadtree") {
            return raw;
        }
        return "auto";
    })();
    const WORKER_CULL_QUADTREE_THRESHOLD = Math.max(
        24,
        Math.min(
            5000,
            Math.floor(Number(uiModeParams.get("worker_cull_quadtree_threshold")) || 96)
        )
    );
    const PERF_PROFILE_FORCED =
        uiModeParams.get("perf") === "1" || uiModeParams.get("profile") === "1";
    const ULTRA_MODE_FORCED =
        uiModeParams.get("mode") === "ultra" || uiModeParams.get("ultra") === "1";
    const WEBGPU_TEST_MODE =
        uiModeParams.get("mode") === "webgpu" ||
        uiModeParams.get("webgpu") === "1" ||
        uiModeParams.get("webgpu_test") === "1";
    const WEBGPU_INSTANCES_TOGGLE = parseToggleParam(uiModeParams.get("webgpu_instances"));
    const WEBGPU_PROJECTILES_TOGGLE = parseToggleParam(
        uiModeParams.get("webgpu_projectiles")
    );
    const WEBGPU_PLAYERS_TOGGLE = parseToggleParam(uiModeParams.get("webgpu_players"));
    const WEBGPU_INSTANCE_AUTO_DEFAULT =
        BENCH_MODE ||
        MASS_MODE_FORCED ||
        STABLE_MODE_FORCED ||
        TOURNAMENT_MODE_FORCED ||
        ULTRA_MODE_FORCED;
    const WEBGPU_INSTANCE_AUTO_ENABLED =
        WEBGPU_INSTANCES_TOGGLE.state !== "off" &&
        (WEBGPU_INSTANCES_TOGGLE.state === "auto" ||
            (WEBGPU_INSTANCES_TOGGLE.state === "unset" && WEBGPU_INSTANCE_AUTO_DEFAULT));
    const WEBGPU_INSTANCE_FORCE_ENABLED = WEBGPU_INSTANCES_TOGGLE.state === "on";
    const WEBGPU_FORCE_ACTIVE =
        parseToggleParam(uiModeParams.get("webgpu_force_active")).state === "on";
    const isMobileUA = /Android|iPhone|iPad|iPod|Mobile/i.test(navigator.userAgent);
    // Disable WebGPU layers on low-end mobile (unlikely to support it, saves GPU memory)
    const isLowEndMobile = isMobileUA && (navigator.hardwareConcurrency || 2) < 4;
    const WEBGPU_PROJECTILE_LAYER_ENABLED = !isLowEndMobile && (
        WEBGPU_PROJECTILES_TOGGLE.state === "on" ||
        (WEBGPU_PROJECTILES_TOGGLE.state !== "off" &&
            (WEBGPU_INSTANCE_FORCE_ENABLED || WEBGPU_INSTANCE_AUTO_ENABLED)));
    const WEBGPU_PLAYER_LAYER_ENABLED = !isLowEndMobile && (
        WEBGPU_PLAYERS_TOGGLE.state === "on" ||
        (WEBGPU_PLAYERS_TOGGLE.state !== "off" &&
            (WEBGPU_INSTANCE_FORCE_ENABLED || WEBGPU_INSTANCE_AUTO_ENABLED)));
    const WEBGPU_REQUIRED = uiModeParams.get("require_webgpu") === "1";
    const WEBGL2_FALLBACK_TOGGLE = parseToggleParam(uiModeParams.get("webgl2_fallback"));
    const WEBGL2_FALLBACK_ENABLED = WEBGL2_FALLBACK_TOGGLE.state !== "off";
    const FOG_TOGGLE = parseToggleParam(uiModeParams.get("fog"));
    const FOG_ENABLED = FOG_TOGGLE.state !== "off";
    const HYPE_UI_TOGGLE = parseToggleParam(uiModeParams.get("hype_ui"));
    const EXCITEMENT_UI_ENABLED = !BENCH_MODE && HYPE_UI_TOGGLE.state !== "off";
    const COMBAT_SPEED_LINES_ENABLED = false;
    const TAB_THROTTLE_TOGGLE = parseToggleParam(uiModeParams.get("tab_throttle"));
    const TAB_THROTTLE_ENABLED = TAB_THROTTLE_TOGGLE.state !== "off";
    const COMBAT_UI_QUALITY_OVERRIDE = (() => {
        const raw = String(
            uiModeParams.get("combat_ui") || uiModeParams.get("combat_ui_quality") || ""
        )
            .trim()
            .toLowerCase();
        if (raw === "auto" || raw === "high" || raw === "low") {
            return raw;
        }
        return null;
    })();
    const WEBGL2_SUPPORTED = (() => {
        try {
            const probeCanvas = document.createElement("canvas");
            return !!probeCanvas.getContext("webgl2");
        } catch (_) {
            return false;
        }
    })();
    const SPRITE_CADENCE_MODE = String(uiModeParams.get("sprite_cadence") || "")
        .trim()
        .toLowerCase();
    const SPRITE_CADENCE_ENABLED =
        SPRITE_CADENCE_MODE !== "off" && SPRITE_CADENCE_MODE !== "disabled";
    const DAMAGE_BATCH_MODE = String(uiModeParams.get("damage_batch") || "")
        .trim()
        .toLowerCase();
    const DAMAGE_BATCH_ENABLED =
        DAMAGE_BATCH_MODE !== "off" && DAMAGE_BATCH_MODE !== "disabled";
    const benchMaxFpsParam = Number.parseInt(uiModeParams.get("bench_max_fps") || "", 10);
    const BENCH_MAX_FPS =
        Number.isFinite(benchMaxFpsParam) && benchMaxFpsParam >= 30
            ? Math.min(240, benchMaxFpsParam)
            : 30;
    const PERF_CONSOLE_OUTPUT_ENABLED = uiModeParams.get("perf_console") === "1";
    const ULTRA_AUTO_UPSHIFT_PLAYERS = STABLE_MODE_FORCED ? 36 : 90;
    const ULTRA_AUTO_DOWNSHIFT_PLAYERS = STABLE_MODE_FORCED ? 24 : 65;
    const TARGET_FRAME_MS_60FPS = 16.67;
    const STABLE_DENSE_FRAME_MS = 17.3;
    const STABLE_ULTRA_FRAME_MS = 18.8;
    const AIMING_LITE_PLAYER_THRESHOLD = 10;
    const INPUT_ROTATION_QUANT_STEP = 0.012;
    const INPUT_IDLE_HEARTBEAT_MS = 120;
    const INPUT_MOVEMENT_HEARTBEAT_MS = 66;
    const BACKGROUND_INPUT_SEND_RATE = 8;
    const BACKGROUND_INPUT_HEARTBEAT_MS = 240;
    const BACKGROUND_TAB_MAX_FPS = 20;
    const EFFECTS_PROFILE_PRIORITY = Object.freeze({
        ultra: 0,
        dense: 1,
        medium: 2,
        high: 3,
    });
    const EFFECTS_ADAPTIVE_EVAL_INTERVAL_MS = 500;

    // Game constants
    const INTERPOLATION_DELAY = isMobileUA ? 90 : 70; // ms - lower baseline for snappier feel
    const MIN_INTERPOLATION_DELAY_MS = isMobileUA ? 70 : 55;
    const MAX_INTERPOLATION_DELAY_MS = isMobileUA ? 250 : 180;
    const PLAYER_EXTRAPOLATION_LIMIT_MS = isMobileUA ? 250 : 120; // Extended for mobile networks
    const PROJECTILE_EXTRAPOLATION_LIMIT_MS = isMobileUA ? 280 : 160;
    const PROJECTILE_CLIENT_PREDICTION_LIMIT_MS = 1200;
    const NETWORK_TIMING_EMA_ALPHA = 0.18;
    const POSITION_SNAP_DISTANCE_SQ = isMobileUA ? (200 * 200) : (140 * 140); // Wider snap threshold for mobile
    const PROJECTILE_SNAP_DISTANCE_SQ = isMobileUA ? (300 * 300) : (220 * 220);
    const INPUT_SEND_RATE = 60; // Hz
    const RECONCILIATION_BUFFER_SIZE = 120;
    const PLAYER_RADIUS = 15;
    const PICKUP_RADIUS = 20;
    const MIN_PLAYERS_TO_START = 1;
    const MAX_CHAT_MESSAGE_LENGTH = 100;
    const MAX_LOG_ENTRIES = 200;
    const SERVER_TICK_RATE = 60;
    const DEBUG_WALL_UPDATES = false;
    const INTERPOLATION_RETENTION_MS = 500;
    const INTERPOLATION_SNAPSHOT_INTERVAL_MS = 50;
    const MAX_INTERPOLATION_SNAPSHOTS = 40;
    const INTERPOLATION_PLAYER_LIMIT = 120;
    const INTERPOLATION_PROJECTILE_LIMIT = 600;

    // Team colors
    const teamColors = {
        0: 0xa0a0a0, // Neutral/FFA - A distinct Grey
        1: 0xff6b6b, // Team 1 - Red
        2: 0x4ecdc4, // Team 2 - Teal/Blue
    };
    const defaultEnemyColor = 0xf87171; // Less critical if all players get team colors

    // Weapon data
    const weaponNames = {
        [GP.WeaponType.Pistol]: "Pistol",
        [GP.WeaponType.Shotgun]: "Shotgun",
        [GP.WeaponType.Rifle]: "Rifle",
        [GP.WeaponType.Sniper]: "Sniper",
        [GP.WeaponType.Melee]: "Melee",
    };

    const weaponColors = {
        [GP.WeaponType.Pistol]: 0xFBBF24,
        [GP.WeaponType.Shotgun]: 0xFB923C,
        [GP.WeaponType.Rifle]: 0x60A5FA,
        [GP.WeaponType.Sniper]: 0xE879F9,
        [GP.WeaponType.Melee]: 0xF87171,
    };

    // Weapon velocity data (pixels per second)
    const weaponVelocities = {
        [GP.WeaponType.Pistol]: 800,
        [GP.WeaponType.Shotgun]: 600,
        [GP.WeaponType.Rifle]: 1200,
        [GP.WeaponType.Sniper]: 1600,
        [GP.WeaponType.Melee]: 0,
    };

    // Pickup data
    const pickupTypes = {
        [GP.PickupType.Health]: "Health",
        [GP.PickupType.Ammo]: "Ammo",
        [GP.PickupType.WeaponCrate]: "Weapon",
        [GP.PickupType.SpeedBoost]: "Speed",
        [GP.PickupType.DamageBoost]: "Damage",
        [GP.PickupType.Shield]: "Shield",
        [GP.PickupType.FlagRed]: "Red Flag",
        [GP.PickupType.FlagBlue]: "Blue Flag",
    };

    const pickupColors = {
        [GP.PickupType.Health]: 0x10b981,
        [GP.PickupType.Ammo]: 0xf59e0b,
        [GP.PickupType.WeaponCrate]: 0x60a5fa,
        [GP.PickupType.SpeedBoost]: 0x00ffff,
        [GP.PickupType.DamageBoost]: 0xff6b6b,
        [GP.PickupType.Shield]: 0x00bfff,
        [GP.PickupType.FlagRed]: 0xff0000,
        [GP.PickupType.FlagBlue]: 0x0000ff,
    };

    return {
        uiModeParams,
        BENCH_MODE,
        MASS_MODE_FORCED,
        STABLE_MODE_FORCED,
        TOURNAMENT_MODE_FORCED,
        LOW_OVERHEAD_MODE,
        WORKER_CULL_PARAM,
        WORKER_CULL_ENABLED,
        WORKER_CULL_INTERVAL_MS,
        DEFAULT_WORKER_CULL_WASM_URL,
        WORKER_CULL_WASM_URL,
        WORKER_CULL_MODE,
        WORKER_CULL_QUADTREE_THRESHOLD,
        PERF_PROFILE_FORCED,
        ULTRA_MODE_FORCED,
        WEBGPU_TEST_MODE,
        WEBGPU_INSTANCES_TOGGLE,
        WEBGPU_PROJECTILES_TOGGLE,
        WEBGPU_PLAYERS_TOGGLE,
        WEBGPU_INSTANCE_AUTO_DEFAULT,
        WEBGPU_INSTANCE_AUTO_ENABLED,
        WEBGPU_INSTANCE_FORCE_ENABLED,
        WEBGPU_FORCE_ACTIVE,
        WEBGPU_PROJECTILE_LAYER_ENABLED,
        WEBGPU_PLAYER_LAYER_ENABLED,
        WEBGPU_REQUIRED,
        WEBGL2_FALLBACK_TOGGLE,
        WEBGL2_FALLBACK_ENABLED,
        FOG_TOGGLE,
        FOG_ENABLED,
        HYPE_UI_TOGGLE,
        EXCITEMENT_UI_ENABLED,
        COMBAT_SPEED_LINES_ENABLED,
        TAB_THROTTLE_TOGGLE,
        TAB_THROTTLE_ENABLED,
        COMBAT_UI_QUALITY_OVERRIDE,
        WEBGL2_SUPPORTED,
        SPRITE_CADENCE_MODE,
        SPRITE_CADENCE_ENABLED,
        DAMAGE_BATCH_MODE,
        DAMAGE_BATCH_ENABLED,
        BENCH_MAX_FPS,
        PERF_CONSOLE_OUTPUT_ENABLED,
        ULTRA_AUTO_UPSHIFT_PLAYERS,
        ULTRA_AUTO_DOWNSHIFT_PLAYERS,
        TARGET_FRAME_MS_60FPS,
        STABLE_DENSE_FRAME_MS,
        STABLE_ULTRA_FRAME_MS,
        AIMING_LITE_PLAYER_THRESHOLD,
        INPUT_ROTATION_QUANT_STEP,
        INPUT_IDLE_HEARTBEAT_MS,
        INPUT_MOVEMENT_HEARTBEAT_MS,
        BACKGROUND_INPUT_SEND_RATE,
        BACKGROUND_INPUT_HEARTBEAT_MS,
        BACKGROUND_TAB_MAX_FPS,
        EFFECTS_PROFILE_PRIORITY,
        EFFECTS_ADAPTIVE_EVAL_INTERVAL_MS,
        INTERPOLATION_DELAY,
        MIN_INTERPOLATION_DELAY_MS,
        MAX_INTERPOLATION_DELAY_MS,
        PLAYER_EXTRAPOLATION_LIMIT_MS,
        PROJECTILE_EXTRAPOLATION_LIMIT_MS,
        PROJECTILE_CLIENT_PREDICTION_LIMIT_MS,
        NETWORK_TIMING_EMA_ALPHA,
        POSITION_SNAP_DISTANCE_SQ,
        PROJECTILE_SNAP_DISTANCE_SQ,
        INPUT_SEND_RATE,
        RECONCILIATION_BUFFER_SIZE,
        PLAYER_RADIUS,
        PICKUP_RADIUS,
        MIN_PLAYERS_TO_START,
        MAX_CHAT_MESSAGE_LENGTH,
        MAX_LOG_ENTRIES,
        SERVER_TICK_RATE,
        DEBUG_WALL_UPDATES,
        INTERPOLATION_RETENTION_MS,
        INTERPOLATION_SNAPSHOT_INTERVAL_MS,
        MAX_INTERPOLATION_SNAPSHOTS,
        INTERPOLATION_PLAYER_LIMIT,
        INTERPOLATION_PROJECTILE_LIMIT,
        teamColors,
        defaultEnemyColor,
        weaponNames,
        weaponColors,
        weaponVelocities,
        pickupTypes,
        pickupColors,
    };
}
