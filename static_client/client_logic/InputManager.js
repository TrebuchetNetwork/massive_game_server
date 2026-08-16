/**
 * InputManager.js - Input handling extracted from client.html
 *
 * Contains keyboard, mouse, and touch input setup/teardown,
 * touch controls (joystick, aim, fire, ability), aim assist,
 * mobile button sizing, virtual crosshair, ping wheel,
 * and the sendInputsToServer loop.
 */

export function createInputManager({
    PIXI,
    GP,
    clamp,
    normalizeAngle,
    log,
    getMaxAmmoForWeaponClient,
    // DOM elements (passed as getters or refs)
    getApp,
    getGameScene,
    getLocalPlayerState,
    getLocalPlayerSprite,
    getMyPlayerId,
    getPlayers,
    getDataChannel,
    getInputState,
    getGameSettings,
    getMouseWorldPos,
    getDynamicsTuning,
    getOverviewMode,
    setOverviewMode,
    // Mobile DOM elements
    mobileControlsDiv,
    mobileMoveArea,
    mobileMoveKnob,
    mobileAimArea,
    mobileFireButton,
    mobileReloadButton,
    mobileMeleeButton,
    mobileWeaponPrimaryButton,
    mobileWeaponSecondaryButton,
    mobileAbilityDashButton,
    mobileAbilityDodgeButton,
    mobilePingButton,
    reloadPromptSpan,
    minimapContainerDiv,
    pingWheelDiv,
    chatInput,
    settingsMenuDiv,
    // Touch/mobile constants and state
    isTouchDevice,
    isMobileDevice,
    forceMobileClient,
    mobileDynamicsEnabled,
    // Functions called from input handlers
    setObjectiveUrgency,
    triggerHitMarkerFn,
    isLocalTeamCommander,
    getCommanderIdForTeam,
    createChatMessage,
    triggerHapticFn,
    enterOverviewMode,
    exitOverviewMode,
    toggleSettings,
    setFocusMode,
    getFocusModeEnabled,
    // Input timing constants
    INPUT_SEND_RATE,
    BACKGROUND_INPUT_SEND_RATE,
    INPUT_ROTATION_QUANT_STEP,
    INPUT_MOVEMENT_HEARTBEAT_MS,
    INPUT_IDLE_HEARTBEAT_MS,
    BACKGROUND_INPUT_HEARTBEAT_MS,
    RECONCILIATION_BUFFER_SIZE,
    // Callbacks
    createInputMessage,
    getBackgroundThrottleActive,
    getAudioManager,
    getMinimap,
    applyScreenShake,
    // Aim assist
    AIM_ASSIST_MAGNETISM,
    AIM_ASSIST_MAX_DISTANCE,
}) {
    // ── Local state ───────────────────────────────────────────────────
    let aimSensitivity = parseFloat(localStorage.getItem('aimSensitivity') || '1.0');
    let aimAssistEnabled = localStorage.getItem('aimAssist') !== 'false';
    let touchControlsInitialized = false;
    let virtualCrosshairSprite = null;

    let inputHandlersInitialized = false;
    let inputGameplayKeyDownHandler = null;
    let inputGameplayKeyUpHandler = null;
    let inputMouseMoveHandler = null;
    let inputMouseDownHandler = null;
    let inputMouseUpHandler = null;
    let inputContextMenuHandler = null;
    let inputUiKeyDownHandler = null;
    let inputUiKeyUpHandler = null;
    let inputWindowResizeHandler = null;
    let managedTouchListeners = [];
    let appViewRectCache = null;
    let appViewRectCachedAt = 0;
    let pingWheelTouchBoundsRect = null;

    let mobileAimActive = false;
    let mobileStickyFireArmed = false;
    let mobileFireTouchActive = false;
    let mobileActionReachPx = 0;
    let mobileActionReachSamples = 0;
    let mobileLastAdaptiveSizingAt = 0;
    let fireButtonTapCount = 0;
    let fireButtonTapWindowUntil = 0;

    let pingWheelOpen = false;
    let pingWheelAnchorWorld = null;
    let pingWheelLongPressTimer = null;
    let pingWheelTouchId = null;

    let lastInputSendTime = 0;
    let lastInputStateSentAt = 0;
    let lastSentInputMoveMask = -1;
    let lastSentInputRotationQuant = Number.NaN;
    let lastSentInputShooting = false;
    let inputSequence = 0;
    let pendingInputs = [];
    let lastShotFeedbackTime = 0;
    let lastPredictedWeaponSoundAt = 0;
    let lastOptimisticHitFeedbackAt = 0;
    let playedOutOfAmmoSoundRecently = false;
    let playedReloadNeededSoundRecently = false;
    let outOfAmmoSoundResetTimer = null;
    let reloadNeededSoundResetTimer = null;

    // Tactical pings
    const tacticalPings = [];
    let lastPingChatAt = 0;
    const TACTICAL_PING_MS = 6200;
    const TACTICAL_PING_CHAT_THROTTLE_MS = 900;
    const KILLSTREAK_PREF_DAMAGE_FIRST_SLOT = 3;
    const KILLSTREAK_PREF_SPEED_FIRST_SLOT = 4;

    function getWeaponFireShakeProfile(weaponType) {
        switch (weaponType) {
            case GP.WeaponType.Pistol: return { intensity: 1.5, frames: 2 };
            case GP.WeaponType.Shotgun: return { intensity: 6, frames: 4 };
            case GP.WeaponType.Rifle: return { intensity: 2, frames: 1 };
            case GP.WeaponType.Sniper: return { intensity: 8, frames: 6 };
            case GP.WeaponType.Melee: return { intensity: 4, frames: 3 };
            default: return null;
        }
    }

    function getPredictedWeaponSoundIntervalMs(weaponType) {
        switch (weaponType) {
            case GP.WeaponType.Pistol: return 170;
            case GP.WeaponType.Shotgun: return 420;
            case GP.WeaponType.Rifle: return 95;
            case GP.WeaponType.Sniper: return 680;
            default: return 180;
        }
    }

    function optimisticHitRangeForWeapon(weaponType) {
        switch (weaponType) {
            case GP.WeaponType.Shotgun: return 170;
            case GP.WeaponType.Pistol: return 360;
            case GP.WeaponType.Rifle: return 520;
            case GP.WeaponType.Sniper: return 1100;
            default: return 320;
        }
    }

    function optimisticHitLateralAllowanceForWeapon(weaponType, alongDistance) {
        const dist = Math.max(0, Number(alongDistance) || 0);
        const baseRadius = 18;
        switch (weaponType) {
            case GP.WeaponType.Shotgun:
                return baseRadius + Math.min(70, dist * 0.28);
            case GP.WeaponType.Rifle:
                return baseRadius + Math.min(24, dist * 0.05);
            case GP.WeaponType.Sniper:
                return baseRadius + Math.min(16, dist * 0.03);
            default:
                return baseRadius + Math.min(28, dist * 0.06);
        }
    }

    // ── Virtual crosshair ─────────────────────────────────────────────

    function createVirtualCrosshair() {
        if (virtualCrosshairSprite || !isTouchDevice) return;
        const g = new PIXI.Graphics();
        g.lineStyle(2, 0xFF4444, 0.6);
        g.moveTo(-10, 0); g.lineTo(-4, 0);
        g.moveTo(4, 0); g.lineTo(10, 0);
        g.moveTo(0, -10); g.lineTo(0, -4);
        g.moveTo(0, 4); g.lineTo(0, 10);
        g.beginFill(0xFF4444, 0.8);
        g.drawCircle(0, 0, 1.5);
        g.endFill();
        virtualCrosshairSprite = g;
    }

    function updateVirtualCrosshair() {
        const localPlayerState = getLocalPlayerState();
        if (!virtualCrosshairSprite || !localPlayerState || !localPlayerState.alive) {
            if (virtualCrosshairSprite) virtualCrosshairSprite.visible = false;
            return;
        }
        virtualCrosshairSprite.visible = true;
        const aimDist = 120;
        const rot = localPlayerState.rotation || 0;
        virtualCrosshairSprite.x = localPlayerState.x + Math.cos(rot) * aimDist;
        virtualCrosshairSprite.y = localPlayerState.y + Math.sin(rot) * aimDist;
    }

    function getVirtualCrosshairSprite() {
        return virtualCrosshairSprite;
    }

    // ── Aim assist ────────────────────────────────────────────────────

    function applyAimAssist(aimAngle) {
        const localPlayerState = getLocalPlayerState();
        const players = getPlayers();
        const myPlayerId = getMyPlayerId();
        if (!aimAssistEnabled || !isTouchDevice || !localPlayerState) return aimAngle;
        let bestAngleDiff = AIM_ASSIST_MAGNETISM;
        let bestAngle = aimAngle;
        for (const [pid, pdata] of players) {
            if (pid === myPlayerId || !pdata.alive || pdata.team_id === localPlayerState.team_id) continue;
            const dx = pdata.x - localPlayerState.x;
            const dy = pdata.y - localPlayerState.y;
            const dist = Math.sqrt(dx * dx + dy * dy);
            if (dist > AIM_ASSIST_MAX_DISTANCE || dist < 1) continue;
            const angleToEnemy = Math.atan2(dy, dx);
            let diff = angleToEnemy - aimAngle;
            while (diff > Math.PI) diff -= Math.PI * 2;
            while (diff < -Math.PI) diff += Math.PI * 2;
            if (Math.abs(diff) < bestAngleDiff) {
                bestAngleDiff = Math.abs(diff);
                bestAngle = aimAngle + diff * 0.3;
            }
        }
        return bestAngle;
    }

    function resolveAimAssistTarget(baseRotation) {
        const localPlayerState = getLocalPlayerState();
        const players = getPlayers();
        const myPlayerId = getMyPlayerId();
        const dynamicsTuning = getDynamicsTuning();
        const mobileBoost = (isTouchDevice && aimAssistEnabled) ? 1.5 : 1.0;
        const effectiveStrength = dynamicsTuning.aimAssistStrength * mobileBoost;
        if (effectiveStrength <= 0 || !localPlayerState || !localPlayerState.alive) {
            return null;
        }

        const fromX = localPlayerState.render_x !== undefined ? localPlayerState.render_x : localPlayerState.x;
        const fromY = localPlayerState.render_y !== undefined ? localPlayerState.render_y : localPlayerState.y;
        const maxDistance = isTouchDevice ? Math.max(dynamicsTuning.aimAssistRange, AIM_ASSIST_MAX_DISTANCE) : dynamicsTuning.aimAssistRange;
        const maxDistanceSq = maxDistance * maxDistance;
        const coneLimit = isTouchDevice ? Math.max(dynamicsTuning.aimAssistConeRad, AIM_ASSIST_MAGNETISM) : dynamicsTuning.aimAssistConeRad;
        let bestTarget = null;
        let bestScore = Number.POSITIVE_INFINITY;

        players.forEach((player, playerId) => {
            if (playerId === myPlayerId || !player || !player.alive) return;
            if (localPlayerState.team_id !== 0 && player.team_id === localPlayerState.team_id) return;

            const targetX = player.render_x !== undefined ? player.render_x : player.x;
            const targetY = player.render_y !== undefined ? player.render_y : player.y;
            const dx = targetX - fromX;
            const dy = targetY - fromY;
            const distanceSq = dx * dx + dy * dy;
            if (distanceSq <= 1 || distanceSq > maxDistanceSq) return;

            const targetRotation = Math.atan2(dy, dx);
            const diff = normalizeAngle(targetRotation - baseRotation);
            const absDiff = Math.abs(diff);
            if (absDiff > coneLimit) return;

            const distance = Math.sqrt(distanceSq);
            const score = absDiff + distance / maxDistance;
            if (score < bestScore) {
                bestScore = score;
                const angleFactor = 1 - Math.min(1, absDiff / Math.max(coneLimit, 0.0001));
                const distanceFactor = 1 - Math.min(1, distance / Math.max(maxDistance, 1));
                bestTarget = {
                    targetId: playerId,
                    diff,
                    distance,
                    strength: clamp(angleFactor * 0.7 + distanceFactor * 0.3, 0, 1),
                    rotationStrength: effectiveStrength,
                };
            }
        });

        return bestTarget;
    }

    function getAimAssistRotation(baseRotation) {
        const target = resolveAimAssistTarget(baseRotation);
        if (!target) return baseRotation;
        return normalizeAngle(baseRotation + target.diff * target.rotationStrength);
    }

    function getAimAssistTarget(baseRotation = Number(getLocalPlayerState()?.rotation) || 0) {
        if (!Number.isFinite(baseRotation)) return null;
        const target = resolveAimAssistTarget(baseRotation);
        if (!target) return null;
        return {
            targetId: target.targetId,
            strength: target.strength,
        };
    }

    // ── Mobile helper functions ───────────────────────────────────────

    function nowMs() {
        return (typeof performance !== 'undefined' && performance.now)
            ? performance.now()
            : Date.now();
    }

    function addManagedTouchListener(target, type, handler, options) {
        if (!target || typeof target.addEventListener !== 'function') return;
        target.addEventListener(type, handler, options);
        managedTouchListeners.push({ target, type, handler, options });
    }

    function teardownTouchControls() {
        if (!touchControlsInitialized) return;
        for (const entry of managedTouchListeners) {
            entry.target.removeEventListener(entry.type, entry.handler, entry.options);
        }
        managedTouchListeners = [];
        touchControlsInitialized = false;
        mobileAimActive = false;
        mobileStickyFireArmed = false;
        mobileFireTouchActive = false;
        pingWheelTouchBoundsRect = null;
    }

    function getAppViewRect(forceRefresh = false) {
        const app = getApp();
        if (!app || !app.view) return null;
        const now = nowMs();
        if (
            forceRefresh ||
            !appViewRectCache ||
            (now - appViewRectCachedAt) > 250
        ) {
            appViewRectCache = app.view.getBoundingClientRect();
            appViewRectCachedAt = now;
        }
        return appViewRectCache;
    }

    function invalidateAppViewRectCache() {
        appViewRectCache = null;
        appViewRectCachedAt = 0;
    }

    function setMoveInputFromVector(dx, dy, deadzone = 10) {
        const inputState = getInputState();
        inputState.move_left = dx < -deadzone;
        inputState.move_right = dx > deadzone;
        inputState.move_forward = dy < -deadzone;
        inputState.move_backward = dy > deadzone;
    }

    function updateAimFromTouch(touch, forceRectRefresh = false) {
        const app = getApp();
        const gameScene = getGameScene();
        const localPlayerState = getLocalPlayerState();
        const mouseWorldPos = getMouseWorldPos();
        const inputState = getInputState();
        if (!app || !app.view || !gameScene || !localPlayerState) return;
        const rect = getAppViewRect(forceRectRefresh);
        if (!rect) return;
        const touchGlobal = new PIXI.Point(touch.clientX - rect.left, touch.clientY - rect.top);
        const touchLocal = gameScene.toLocal(touchGlobal);
        mouseWorldPos.x = touchLocal.x;
        mouseWorldPos.y = touchLocal.y;

        const dx = touchLocal.x - (localPlayerState.render_x || localPlayerState.x);
        const dy = touchLocal.y - (localPlayerState.render_y || localPlayerState.y);
        inputState.rotation = Math.atan2(dy, dx);
    }

    function syncMobileFireButtonState() {
        if (!mobileFireButton) return;
        mobileFireButton.classList.toggle('mobile-button--latched', mobileStickyFireArmed);
        mobileFireButton.textContent = mobileStickyFireArmed ? 'Fire (Lock)' : 'Fire';
    }

    function updateMobileButtonSizing() {
        if (!mobileControlsDiv) return;
        const shortestSide = Math.max(280, Math.min(window.innerWidth, window.innerHeight));
        const scale = clamp(shortestSide / 420, 0.9, 1.26);
        const baseBottom = Math.round(24 * Math.min(scale, 1.15));
        let adaptiveBottom = baseBottom;
        if (mobileActionReachSamples >= 3) {
            const desiredBottom = clamp(
                Math.round(mobileActionReachPx - (52 * scale)),
                12,
                Math.round(window.innerHeight * 0.34)
            );
            adaptiveBottom = Math.round((baseBottom * 0.68) + (desiredBottom * 0.32));
        }
        mobileControlsDiv.style.setProperty('--mobile-button-width', `${Math.round(110 * scale)}px`);
        mobileControlsDiv.style.setProperty('--mobile-button-height', `${Math.round(48 * scale)}px`);
        mobileControlsDiv.style.setProperty('--mobile-buttons-bottom', `${adaptiveBottom}px`);
    }

    function recordMobileActionReachSample(touch) {
        if (!touch || !Number.isFinite(touch.clientY)) return;
        const fromBottom = clamp(window.innerHeight - touch.clientY, 0, window.innerHeight);
        if (fromBottom <= 0) return;
        mobileActionReachPx = mobileActionReachSamples === 0
            ? fromBottom
            : ((mobileActionReachPx * 0.8) + (fromBottom * 0.2));
        mobileActionReachSamples = Math.min(240, mobileActionReachSamples + 1);
        const now = nowMs();
        if ((now - mobileLastAdaptiveSizingAt) >= 120) {
            mobileLastAdaptiveSizingAt = now;
            updateMobileButtonSizing();
        }
    }

    function updateMobileControlsVisibility() {
        if (!mobileControlsDiv) return;
        const shouldShow = forceMobileClient || isTouchDevice || window.innerWidth <= 900;
        mobileControlsDiv.classList.toggle('hidden', !shouldShow);
        document.body.classList.toggle('mobile', shouldShow);
        document.body.classList.toggle('mobile-mode', shouldShow);
        if (shouldShow) {
            updateMobileButtonSizing();
        }
    }

    // ── Ping wheel ────────────────────────────────────────────────────

    function closePingWheel() {
        if (pingWheelLongPressTimer) {
            clearTimeout(pingWheelLongPressTimer);
            pingWheelLongPressTimer = null;
        }
        pingWheelOpen = false;
        pingWheelTouchId = null;
        pingWheelTouchBoundsRect = null;
        pingWheelAnchorWorld = null;
        if (pingWheelDiv) pingWheelDiv.classList.remove('ping-wheel--visible');
    }

    function openPingWheel(clientX, clientY, worldX, worldY) {
        if (!pingWheelDiv) return;
        pingWheelOpen = true;
        pingWheelAnchorWorld = { x: worldX, y: worldY };
        pingWheelDiv.style.left = `${clientX}px`;
        pingWheelDiv.style.top = `${clientY}px`;
        pingWheelDiv.classList.add('ping-wheel--visible');
    }

    function getWorldPointFromMinimap(clientX, clientY) {
        const minimap = getMinimap();
        const localPlayerState = getLocalPlayerState();
        if (!minimap || !localPlayerState || !minimapContainerDiv) return null;
        const rect = pingWheelTouchBoundsRect || minimapContainerDiv.getBoundingClientRect();
        if (!rect || rect.width <= 0 || rect.height <= 0) return null;
        const localOffsetX = clientX - rect.left - (rect.width / 2);
        const localOffsetY = clientY - rect.top - (rect.height / 2);
        return {
            x: (Number(localPlayerState.x) || 0) + (localOffsetX / minimap.mapScale),
            y: (Number(localPlayerState.y) || 0) + (localOffsetY / minimap.mapScale)
        };
    }

    function sendTacticalPing(kind, worldPoint) {
        const dataChannel = getDataChannel();
        const audioManager = getAudioManager();
        const gameSettings = getGameSettings();
        const inputState = getInputState();
        if (!worldPoint || !Number.isFinite(worldPoint.x) || !Number.isFinite(worldPoint.y)) return;
        const now = Date.now();
        const localCommander = isLocalTeamCommander();
        const normalizedKind = kind === 'enemy' ? 'enemy' : (kind === 'defend' ? 'defend' : 'group');
        const label = localCommander
            ? 'COMMAND ORDER'
            : (normalizedKind === 'enemy' ? 'ENEMY SPOTTED' : (normalizedKind === 'defend' ? 'DEFEND HERE' : 'GROUP UP'));
        tacticalPings.push({
            kind: localCommander
                ? 'defend'
                : normalizedKind,
            x: worldPoint.x,
            y: worldPoint.y,
            strength: localCommander ? 1.3 : (normalizedKind === 'enemy' ? 1.2 : 1.0),
            source: localCommander ? 'commander' : 'local',
            label,
            createdAt: now,
            expiresAt: now + TACTICAL_PING_MS
        });
        if (typeof window !== 'undefined' && window.__e2e) {
            window.__e2e.lastTacticalPing = {
                kind: localCommander ? 'defend' : normalizedKind,
                x: worldPoint.x,
                y: worldPoint.y,
                createdAt: now,
            };
        }
        if (tacticalPings.length > 18) {
            tacticalPings.splice(0, tacticalPings.length - 18);
        }

        if (audioManager && gameSettings.soundEnabled) {
            audioManager.playSound(kind === 'enemy' ? 'flagDropped' : 'flagGrabbed', null, 0.24);
        }
        triggerHapticFn(10);
        inputState.ping_x = Number(worldPoint.x) || 0;
        inputState.ping_y = Number(worldPoint.y) || 0;

        if (dataChannel && dataChannel.readyState === 'open' && (now - lastPingChatAt) >= TACTICAL_PING_CHAT_THROTTLE_MS) {
            const pingLabel = localCommander
                ? 'Commander'
                : (kind === 'enemy' ? 'Enemy' : (kind === 'defend' ? 'Defend' : 'Group'));
            const msg = `[PING] ${pingLabel} @ ${Math.round(worldPoint.x)}, ${Math.round(worldPoint.y)}`;
            try {
                dataChannel.send(createChatMessage(msg));
                lastPingChatAt = now;
            } catch (_) {}
        }
    }

    function issueCommanderOrder(worldPoint) {
        if (!worldPoint || !Number.isFinite(worldPoint.x) || !Number.isFinite(worldPoint.y)) {
            return false;
        }
        if (!isLocalTeamCommander()) {
            setObjectiveUrgency('Only the team commander can issue global orders', 'info', 800);
            return false;
        }
        sendTacticalPing('defend', worldPoint);
        setObjectiveUrgency('Commander order issued', 'positive', 950);
        return true;
    }

    // ── Core input handlers ───────────────────────────────────────────

    function handleKeyInput(event, isDown) {
        if (document.activeElement === chatInput || !settingsMenuDiv.classList.contains('hidden')) return;
        const localPlayerState = getLocalPlayerState();
        const inputState = getInputState();
        const gameSettings = getGameSettings();
        const audioManager = getAudioManager();
        const mouseWorldPos = getMouseWorldPos();

        let gameKeyProcessed = true;
        switch (event.code) {
            case 'KeyW':
            case 'ArrowUp':
                inputState.move_forward = isDown;
                break;
            case 'KeyS':
            case 'ArrowDown':
                inputState.move_backward = isDown;
                break;
            case 'KeyA':
            case 'ArrowLeft':
                inputState.move_left = isDown;
                break;
            case 'KeyD':
            case 'ArrowRight':
                inputState.move_right = isDown;
                break;
            case 'KeyR':
                if (isDown && localPlayerState && localPlayerState.weapon !== GP.WeaponType.Melee && localPlayerState.ammo < getMaxAmmoForWeaponClient(localPlayerState.weapon) && localPlayerState.reload_progress === -1) {
                    inputState.reload = true;
                    if (reloadPromptSpan) reloadPromptSpan.textContent = ' (Reloading...)';
                    if (audioManager && gameSettings.soundEnabled) audioManager.playSound('reloadStart', null, 0.3);
                }
                break;
            case 'KeyV':
                if (isDown) inputState.melee_attack = true;
                break;
            case 'Digit1':
                if (isDown && !event.repeat) {
                    inputState.change_weapon_slot = 1;
                    setObjectiveUrgency('Swapping to primary weapon', 'info', 650);
                    if (audioManager && gameSettings.soundEnabled) {
                        audioManager.playSound('weaponSwap', null, 0.24);
                    }
                }
                break;
            case 'Digit2':
                if (isDown && !event.repeat) {
                    inputState.change_weapon_slot = 2;
                    setObjectiveUrgency('Swapping to secondary weapon', 'info', 650);
                    if (audioManager && gameSettings.soundEnabled) {
                        audioManager.playSound('weaponSwap', null, 0.24);
                    }
                }
                break;
            case 'KeyQ':
                if (isDown && !event.repeat && localPlayerState && localPlayerState.alive) {
                    inputState.use_ability_slot = 1;
                    setObjectiveUrgency('Dash ability activated', 'positive', 600);
                }
                break;
            case 'KeyE':
                if (isDown && !event.repeat && localPlayerState && localPlayerState.alive) {
                    inputState.use_ability_slot = 2;
                    setObjectiveUrgency('Dodge ability activated', 'positive', 600);
                }
                break;
            case 'KeyZ':
                if (isDown && !event.repeat && localPlayerState && localPlayerState.alive) {
                    inputState.use_ability_slot = KILLSTREAK_PREF_DAMAGE_FIRST_SLOT;
                    setObjectiveUrgency('Killstreak rewards: damage first', 'info', 1000);
                }
                break;
            case 'KeyX':
                if (isDown && !event.repeat && localPlayerState && localPlayerState.alive) {
                    inputState.use_ability_slot = KILLSTREAK_PREF_SPEED_FIRST_SLOT;
                    setObjectiveUrgency('Killstreak rewards: speed first', 'info', 1000);
                }
                break;
            case 'KeyC':
                if (isDown && localPlayerState && localPlayerState.alive) {
                    const commandPoint = (Number.isFinite(mouseWorldPos.x) && Number.isFinite(mouseWorldPos.y))
                        ? { x: mouseWorldPos.x, y: mouseWorldPos.y }
                        : { x: Number(localPlayerState.x) || 0, y: Number(localPlayerState.y) || 0 };
                    issueCommanderOrder(commandPoint);
                }
                break;
            default:
                gameKeyProcessed = false;
                break;
        }
        if (gameKeyProcessed) event.preventDefault();
    }

    function handleMouseMove(event) {
        const app = getApp();
        const gameScene = getGameScene();
        const localPlayerSprite = getLocalPlayerSprite();
        const localPlayerState = getLocalPlayerState();
        const mouseWorldPos = getMouseWorldPos();
        const inputState = getInputState();
        if (!app || !app.view || !localPlayerSprite || !localPlayerState) return;
        const rect = getAppViewRect(false);
        if (!rect) return;
        const mouseGlobal = new PIXI.Point(event.clientX - rect.left, event.clientY - rect.top);
        const mouseLocalToGameScene = gameScene.toLocal(mouseGlobal);

        mouseWorldPos.x = mouseLocalToGameScene.x;
        mouseWorldPos.y = mouseLocalToGameScene.y;

        const dx = mouseLocalToGameScene.x - (localPlayerState.render_x || localPlayerState.x);
        const dy = mouseLocalToGameScene.y - (localPlayerState.render_y || localPlayerState.y);
        inputState.rotation = Math.atan2(dy, dx);
    }

    // ── Send inputs to server ─────────────────────────────────────────

    function sendInputsToServer(cameraCombatImpulseRef, combatUiStateRef, EXCITEMENT_UI_ENABLED) {
        if (window.__e2e) window.__e2e.inputSendCalls = (window.__e2e.inputSendCalls || 0) + 1;
        const dataChannel = getDataChannel();
        const localPlayerState = getLocalPlayerState();
        const inputState = getInputState();
        const dynamicsTuning = getDynamicsTuning();
        const backgroundThrottleActive = getBackgroundThrottleActive();
        if (!dataChannel || dataChannel.readyState !== 'open' || !localPlayerState || !localPlayerState.alive) {
            if (window.__e2e) window.__e2e.lastInputGate = 'prereq';
            return;
        }

        const now = Date.now();
        const inputSendRate = backgroundThrottleActive ? BACKGROUND_INPUT_SEND_RATE : INPUT_SEND_RATE;
        if (window.__e2e) {
            window.__e2e.inputGateDebug = {
                now,
                lastInputSendTime,
                inputSendRate,
                backgroundThrottleActive: !!backgroundThrottleActive,
            };
        }
        if (now - lastInputSendTime < 1000 / inputSendRate) {
            if (window.__e2e) window.__e2e.lastInputGate = 'ratecap';
            return;
        }

        let effectiveRotation = inputState.rotation;
        if (inputState.shooting || (isTouchDevice && aimAssistEnabled)) {
            effectiveRotation = getAimAssistRotation(effectiveRotation);
        }

        const moveMask =
            (inputState.move_forward ? 1 : 0) |
            (inputState.move_backward ? 2 : 0) |
            (inputState.move_left ? 4 : 0) |
            (inputState.move_right ? 8 : 0);
        const quantizedRotation = Number.isFinite(effectiveRotation)
            ? Math.round(effectiveRotation / INPUT_ROTATION_QUANT_STEP)
            : 0;
        const hasOneShotInput =
            !!inputState.reload ||
            !!inputState.melee_attack ||
            inputState.change_weapon_slot !== 0 ||
            inputState.use_ability_slot !== 0 ||
            Number(inputState.ping_x) !== 0 ||
            Number(inputState.ping_y) !== 0;
        const stickyStateChanged =
            moveMask !== lastSentInputMoveMask ||
            quantizedRotation !== lastSentInputRotationQuant ||
            !!inputState.shooting !== !!lastSentInputShooting;

        let heartbeatMs = inputState.shooting
            ? (1000 / inputSendRate)
            : (moveMask !== 0 ? INPUT_MOVEMENT_HEARTBEAT_MS : INPUT_IDLE_HEARTBEAT_MS);
        if (backgroundThrottleActive) {
            heartbeatMs = Math.max(heartbeatMs, BACKGROUND_INPUT_HEARTBEAT_MS);
        }
        const sinceLastStateMs = now - lastInputStateSentAt;
        if (!hasOneShotInput && !stickyStateChanged && sinceLastStateMs < heartbeatMs) {
            if (window.__e2e) window.__e2e.lastInputGate = 'heartbeat';
            return;
        }

        lastInputSendTime = now;

        if (window.__e2e) {
            window.__e2e.lastInputDebug = {
                shooting: !!inputState.shooting,
                moveMask,
                rotation: effectiveRotation,
                hasOneShotInput,
                stickyStateChanged,
                ammo: Number(localPlayerState.ammo) || 0,
                weapon: Number(localPlayerState.weapon) || 0,
            };
        }

        const currentFrameInput = {
            timestamp: now,
            sequence: ++inputSequence,
            move_forward: inputState.move_forward,
            move_backward: inputState.move_backward,
            move_left: inputState.move_left,
            move_right: inputState.move_right,
            shooting: inputState.shooting,
            reload: inputState.reload,
            rotation: effectiveRotation,
            melee_attack: inputState.melee_attack,
            change_weapon_slot: inputState.change_weapon_slot,
            use_ability_slot: inputState.use_ability_slot,
            ping_x: Number(inputState.ping_x) || 0,
            ping_y: Number(inputState.ping_y) || 0,
        };

        // Cosmetic combat feedback (predicted sounds, screen shake, hit
        // markers). A failure here must NEVER abort input sending, so it is
        // isolated in its own try/catch.
        try {
        const audioManager = getAudioManager();
        const canEmitPredictedWeaponSound =
            !!audioManager &&
            currentFrameInput.shooting &&
            localPlayerState.weapon !== GP.WeaponType.Melee &&
            localPlayerState.alive &&
            (Number(localPlayerState.ammo) || 0) > 0 &&
            ((localPlayerState.reload_progress ?? -1) < 0);
        if (canEmitPredictedWeaponSound) {
            const intervalMs = getPredictedWeaponSoundIntervalMs(localPlayerState.weapon);
            if ((now - lastPredictedWeaponSoundAt) >= intervalMs) {
                audioManager.playWeaponSound(
                    localPlayerState.weapon,
                    {
                        x: Number(localPlayerState.x) || 0,
                        y: Number(localPlayerState.y) || 0,
                    },
                    true,
                    {
                        predicted: true,
                        bypassLimiter: true,
                        volumeScale: 0.9,
                    }
                );
                lastPredictedWeaponSoundAt = now;
            }
        }

        const attackFeedbackActive = currentFrameInput.shooting || currentFrameInput.melee_attack;
        if (attackFeedbackActive && now - lastShotFeedbackTime >= 110) {
            cameraCombatImpulseRef.value = Math.min(
                dynamicsTuning.cameraMaxSpeedZoomOut,
                cameraCombatImpulseRef.value + dynamicsTuning.cameraCombatKick
            );
            if (getGameSettings().screenShake && typeof applyScreenShake === 'function') {
                const shakeProfile = getWeaponFireShakeProfile(localPlayerState.weapon);
                const scene = getGameScene();
                if (shakeProfile && scene) {
                    applyScreenShake(scene, shakeProfile.intensity, shakeProfile.frames);
                }
            }
            if (EXCITEMENT_UI_ENABLED) {
                combatUiStateRef.momentum = Math.min(1, combatUiStateRef.momentum + 0.016);
                combatUiStateRef.speedPulse = Math.min(1, combatUiStateRef.speedPulse + 0.08);
            }
            lastShotFeedbackTime = now;
        }

        if (
            currentFrameInput.shooting &&
            typeof triggerHitMarkerFn === 'function' &&
            (now - lastOptimisticHitFeedbackAt) >= 95
        ) {
            const players = getPlayers();
            const myPlayerId = getMyPlayerId();
            const originX = Number(localPlayerState.render_x !== undefined ? localPlayerState.render_x : localPlayerState.x) || 0;
            const originY = Number(localPlayerState.render_y !== undefined ? localPlayerState.render_y : localPlayerState.y) || 0;
            const rot = Number(localPlayerState.rotation) || 0;
            const dirX = Math.cos(rot);
            const dirY = Math.sin(rot);
            const maxRange = optimisticHitRangeForWeapon(localPlayerState.weapon);
            const myTeam = Number(localPlayerState.team_id) || 0;
            let optimisticHitFound = false;

            for (const [playerId, player] of players) {
                if (!player || playerId === myPlayerId || !player.alive) continue;
                const targetTeam = Number(player.team_id) || 0;
                if (myTeam !== 0 && targetTeam !== 0 && myTeam === targetTeam) continue;
                const targetX = Number(player.render_x !== undefined ? player.render_x : player.x);
                const targetY = Number(player.render_y !== undefined ? player.render_y : player.y);
                if (!Number.isFinite(targetX) || !Number.isFinite(targetY)) continue;

                const toX = targetX - originX;
                const toY = targetY - originY;
                const along = toX * dirX + toY * dirY;
                if (!Number.isFinite(along) || along <= 0 || along > maxRange) continue;
                const lateral = Math.abs(toX * dirY - toY * dirX);
                const lateralAllowance = optimisticHitLateralAllowanceForWeapon(localPlayerState.weapon, along);
                if (lateral > lateralAllowance) continue;
                optimisticHitFound = true;
                break;
            }

            if (optimisticHitFound) {
                triggerHitMarkerFn(false);
                lastOptimisticHitFeedbackAt = now;
            }
        }
        } catch (feedbackErr) {
            if (window.__e2e) {
                window.__e2e.inputFeedbackErrors = (window.__e2e.inputFeedbackErrors || 0) + 1;
                window.__e2e.lastInputFeedbackError = String(feedbackErr?.stack || feedbackErr);
            }
        }

        pendingInputs.push(currentFrameInput);
        if (pendingInputs.length > RECONCILIATION_BUFFER_SIZE) pendingInputs.shift();

        let bytes = null;
        try {
            bytes = createInputMessage(currentFrameInput);
        } catch (encodeErr) {
            if (window.__e2e) {
                window.__e2e.inputEncodeErrors = (window.__e2e.inputEncodeErrors || 0) + 1;
                window.__e2e.lastInputEncodeError = String(encodeErr?.stack || encodeErr);
            }
            return;
        }
        try {
            dataChannel.send(bytes);
            if (window.__e2e) {
                window.__e2e.inputsSentCount = (window.__e2e.inputsSentCount || 0) + 1;
                window.__e2e.dataChannelDebug = {
                    readyState: dataChannel.readyState,
                    bufferedAmount: dataChannel.bufferedAmount,
                };
            }
        } catch (_err) {
            if (window.__e2e) {
                window.__e2e.inputsSendErrors = (window.__e2e.inputsSendErrors || 0) + 1;
                window.__e2e.lastSendError = String(_err);
                window.__e2e.dataChannelDebug = {
                    readyState: dataChannel.readyState,
                    bufferedAmount: dataChannel.bufferedAmount,
                };
            }
            return;
        }
        lastInputStateSentAt = now;
        lastSentInputMoveMask = moveMask;
        lastSentInputRotationQuant = quantizedRotation;
        lastSentInputShooting = !!inputState.shooting;

        // Reset one-time inputs
        if (inputState.reload) inputState.reload = false;
        if (inputState.melee_attack) inputState.melee_attack = false;
        if (inputState.change_weapon_slot !== 0) inputState.change_weapon_slot = 0;
        if (inputState.use_ability_slot !== 0) inputState.use_ability_slot = 0;
        if (inputState.ping_x !== 0) inputState.ping_x = 0;
        if (inputState.ping_y !== 0) inputState.ping_y = 0;
    }

    // ── Setup / teardown ──────────────────────────────────────────────

    function teardownInputHandlers() {
        teardownTouchControls();
        if (!inputHandlersInitialized) return;
        const app = getApp();
        if (inputGameplayKeyDownHandler) {
            document.removeEventListener('keydown', inputGameplayKeyDownHandler);
        }
        if (inputGameplayKeyUpHandler) {
            document.removeEventListener('keyup', inputGameplayKeyUpHandler);
        }
        if (inputUiKeyDownHandler) {
            document.removeEventListener('keydown', inputUiKeyDownHandler);
        }
        if (inputUiKeyUpHandler) {
            document.removeEventListener('keyup', inputUiKeyUpHandler);
        }
        if (inputWindowResizeHandler) {
            window.removeEventListener('resize', inputWindowResizeHandler);
        }
        const view = app?.view || null;
        if (view && inputMouseMoveHandler) {
            view.removeEventListener('mousemove', inputMouseMoveHandler);
        }
        if (view && inputMouseDownHandler) {
            view.removeEventListener('mousedown', inputMouseDownHandler);
        }
        if (view && inputMouseUpHandler) {
            view.removeEventListener('mouseup', inputMouseUpHandler);
        }
        if (view && inputContextMenuHandler) {
            view.removeEventListener('contextmenu', inputContextMenuHandler);
        }
        inputGameplayKeyDownHandler = null;
        inputGameplayKeyUpHandler = null;
        inputMouseMoveHandler = null;
        inputMouseDownHandler = null;
        inputMouseUpHandler = null;
        inputContextMenuHandler = null;
        inputUiKeyDownHandler = null;
        inputUiKeyUpHandler = null;
        inputWindowResizeHandler = null;
        if (outOfAmmoSoundResetTimer) {
            clearTimeout(outOfAmmoSoundResetTimer);
            outOfAmmoSoundResetTimer = null;
        }
        if (reloadNeededSoundResetTimer) {
            clearTimeout(reloadNeededSoundResetTimer);
            reloadNeededSoundResetTimer = null;
        }
        playedOutOfAmmoSoundRecently = false;
        playedReloadNeededSoundRecently = false;
        invalidateAppViewRectCache();
        inputHandlersInitialized = false;
    }

    function setupInputHandlers() {
        const app = getApp();
        if (inputHandlersInitialized || !app || !app.view) return;
        inputHandlersInitialized = true;
        const inputState = getInputState();
        const gameSettings = getGameSettings();

        inputGameplayKeyDownHandler = (e) => handleKeyInput(e, true);
        inputGameplayKeyUpHandler = (e) => handleKeyInput(e, false);
        inputMouseMoveHandler = handleMouseMove;
        inputMouseDownHandler = (e) => {
            const localPlayerState = getLocalPlayerState();
            const audioManager = getAudioManager();
            if (e.button === 0) {
                if (localPlayerState && localPlayerState.weapon !== GP.WeaponType.Melee && localPlayerState.ammo === 0) {
                    if (audioManager && gameSettings.soundEnabled && !playedOutOfAmmoSoundRecently) {
                        audioManager.playSound('outOfAmmo', null, 0.4);
                        playedOutOfAmmoSoundRecently = true;
                        if (outOfAmmoSoundResetTimer) clearTimeout(outOfAmmoSoundResetTimer);
                        outOfAmmoSoundResetTimer = setTimeout(() => {
                            playedOutOfAmmoSoundRecently = false;
                            outOfAmmoSoundResetTimer = null;
                        }, 1000);
                    }
                    if (reloadPromptSpan && localPlayerState.reload_progress === -1) {
                        reloadPromptSpan.textContent = ' (Press R to Reload!)';
                        if (audioManager && gameSettings.soundEnabled && !playedReloadNeededSoundRecently) {
                            audioManager.playSound('reloadNeeded', null, 0.5);
                            playedReloadNeededSoundRecently = true;
                            if (reloadNeededSoundResetTimer) clearTimeout(reloadNeededSoundResetTimer);
                            reloadNeededSoundResetTimer = setTimeout(() => {
                                playedReloadNeededSoundRecently = false;
                                reloadNeededSoundResetTimer = null;
                            }, 2000);
                        }
                    }
                } else {
                    inputState.shooting = true;
                    if (window.__e2e) window.__e2e.mouseDownShootingSet = true;
                }
            }
        };
        inputMouseUpHandler = (e) => {
            if (e.button === 0) inputState.shooting = false;
        };
        inputContextMenuHandler = (e) => e.preventDefault();

        inputUiKeyDownHandler = (e) => {
            if (e.key === 'Tab') {
                e.preventDefault();
                if (!getOverviewMode()) enterOverviewMode();
                if (typeof window.toggleScoreboard === 'function') {
                    window.toggleScoreboard(true);
                }
            }
            if (e.key === 'Escape') {
                e.preventDefault();
                toggleSettings();
            }
            if (e.code === 'KeyH' && !e.repeat) {
                e.preventDefault();
                setFocusMode(!getFocusModeEnabled());
            }
        };

        inputUiKeyUpHandler = (e) => {
            if (e.key === 'Tab') {
                e.preventDefault();
                if (getOverviewMode()) exitOverviewMode();
                if (typeof window.toggleScoreboard === 'function') {
                    window.toggleScoreboard(false);
                }
            }
        };

        document.addEventListener('keydown', inputGameplayKeyDownHandler);
        document.addEventListener('keyup', inputGameplayKeyUpHandler);
        document.addEventListener('keydown', inputUiKeyDownHandler);
        document.addEventListener('keyup', inputUiKeyUpHandler);
        inputWindowResizeHandler = () => {
            invalidateAppViewRectCache();
            pingWheelTouchBoundsRect = minimapContainerDiv?.getBoundingClientRect() || null;
            if (touchControlsInitialized) {
                updateMobileControlsVisibility();
                updateMobileButtonSizing();
            }
        };
        window.addEventListener('resize', inputWindowResizeHandler);
        app.view.addEventListener('mousemove', inputMouseMoveHandler);
        app.view.addEventListener('mousedown', inputMouseDownHandler);
        app.view.addEventListener('mouseup', inputMouseUpHandler);
        app.view.addEventListener('contextmenu', inputContextMenuHandler);

        setupTouchControls();
    }

    // ── Touch controls setup ──────────────────────────────────────────

    function setupTouchControls() {
        if (touchControlsInitialized || !mobileControlsDiv) return;
        touchControlsInitialized = true;
        const inputState = getInputState();
        const gameSettings = getGameSettings();
        invalidateAppViewRectCache();

        updateMobileControlsVisibility();
        updateMobileButtonSizing();
        syncMobileFireButtonState();

        let moveTouchId = null;
        let moveOrigin = { x: 0, y: 0 };
        const moveMaxRadius = 45;

        const resetMoveStick = () => {
            if (mobileMoveKnob) {
                mobileMoveKnob.style.transform = 'translate(-50%, -50%)';
            }
            setMoveInputFromVector(0, 0, 10);
            moveTouchId = null;
        };

        const handleMoveTouch = (touch) => {
            const dx = touch.clientX - moveOrigin.x;
            const dy = touch.clientY - moveOrigin.y;
            const distance = Math.hypot(dx, dy);
            const clampedDistance = Math.min(distance, moveMaxRadius);
            const angle = Math.atan2(dy, dx);
            const clampedX = Math.cos(angle) * clampedDistance;
            const clampedY = Math.sin(angle) * clampedDistance;

            if (mobileMoveKnob) {
                mobileMoveKnob.style.transform = `translate(calc(-50% + ${clampedX}px), calc(-50% + ${clampedY}px))`;
            }
            setMoveInputFromVector(clampedX, clampedY);
        };

        addManagedTouchListener(mobileMoveArea, 'touchstart', (event) => {
            if (moveTouchId !== null) return;
            const touch = event.changedTouches[0];
            moveTouchId = touch.identifier;
            moveOrigin = { x: touch.clientX, y: touch.clientY };
            handleMoveTouch(touch);
            event.preventDefault();
        }, { passive: false });

        addManagedTouchListener(mobileMoveArea, 'touchmove', (event) => {
            if (moveTouchId === null) return;
            for (const touch of event.changedTouches) {
                if (touch.identifier === moveTouchId) {
                    handleMoveTouch(touch);
                    event.preventDefault();
                    break;
                }
            }
        }, { passive: false });

        addManagedTouchListener(mobileMoveArea, 'touchend', (event) => {
            if (moveTouchId === null) return;
            for (const touch of event.changedTouches) {
                if (touch.identifier === moveTouchId) {
                    resetMoveStick();
                    event.preventDefault();
                    break;
                }
            }
        }, { passive: false });

        addManagedTouchListener(mobileMoveArea, 'touchcancel', resetMoveStick, { passive: true });

        let aimTouchId = null;
        addManagedTouchListener(mobileAimArea, 'touchstart', (event) => {
            if (aimTouchId !== null) return;
            const touch = event.changedTouches[0];
            aimTouchId = touch.identifier;
            mobileAimActive = true;
            recordMobileActionReachSample(touch);
            updateAimFromTouch(touch, true);
            if (gameSettings.mobileAutoFireAim) {
                inputState.shooting = true;
                triggerHapticFn(10);
            }
            event.preventDefault();
        }, { passive: false });

        addManagedTouchListener(mobileAimArea, 'touchmove', (event) => {
            if (aimTouchId === null) return;
            for (const touch of event.changedTouches) {
                if (touch.identifier === aimTouchId) {
                    recordMobileActionReachSample(touch);
                    updateAimFromTouch(touch);
                    if (gameSettings.mobileAutoFireAim) inputState.shooting = true;
                    event.preventDefault();
                    break;
                }
            }
        }, { passive: false });

        const endAimTouch = (event) => {
            if (aimTouchId === null) return;
            for (const touch of event.changedTouches) {
                if (touch.identifier === aimTouchId) {
                    aimTouchId = null;
                    mobileAimActive = false;
                    if (gameSettings.mobileAutoFireAim && !mobileStickyFireArmed && !mobileFireTouchActive) {
                        inputState.shooting = false;
                    }
                    event.preventDefault();
                    break;
                }
            }
        };
        addManagedTouchListener(mobileAimArea, 'touchend', endAimTouch, { passive: false });
        addManagedTouchListener(mobileAimArea, 'touchcancel', endAimTouch, { passive: false });

        let fireTouchId = null;
        const endFire = () => {
            mobileFireTouchActive = false;
            if (!mobileStickyFireArmed && !mobileAimActive) inputState.shooting = false;
            fireTouchId = null;
        };

        addManagedTouchListener(mobileFireButton, 'touchstart', (event) => {
            if (fireTouchId !== null) return;
            const touch = event.changedTouches[0];
            fireTouchId = touch.identifier;
            mobileFireTouchActive = true;
            recordMobileActionReachSample(touch);

            const now = Date.now();
            if (now <= fireButtonTapWindowUntil) {
                fireButtonTapCount += 1;
            } else {
                fireButtonTapCount = 1;
            }
            fireButtonTapWindowUntil = now + 420;
            if (gameSettings.mobileStickyFire && fireButtonTapCount >= 2) {
                fireButtonTapCount = 0;
                mobileStickyFireArmed = !mobileStickyFireArmed;
                syncMobileFireButtonState();
                inputState.shooting = mobileStickyFireArmed || gameSettings.mobileAutoFireAim;
                triggerHapticFn(mobileStickyFireArmed ? [10, 12] : 8);
            } else {
                inputState.shooting = true;
                triggerHapticFn(10);
            }
            event.preventDefault();
        }, { passive: false });

        addManagedTouchListener(mobileFireButton, 'touchend', (event) => {
            if (fireTouchId === null) return;
            for (const touch of event.changedTouches) {
                if (touch.identifier === fireTouchId) {
                    endFire();
                    event.preventDefault();
                    break;
                }
            }
        }, { passive: false });
        addManagedTouchListener(mobileFireButton, 'touchcancel', endFire, { passive: true });

        addManagedTouchListener(mobileReloadButton, 'touchstart', (event) => {
            const localPlayerState = getLocalPlayerState();
            const audioManager = getAudioManager();
            recordMobileActionReachSample(event.changedTouches[0]);
            if (localPlayerState && localPlayerState.weapon !== GP.WeaponType.Melee && localPlayerState.ammo < getMaxAmmoForWeaponClient(localPlayerState.weapon) && localPlayerState.reload_progress === -1) {
                inputState.reload = true;
                if (reloadPromptSpan) reloadPromptSpan.textContent = ' (Reloading...)';
                if (audioManager && gameSettings.soundEnabled) audioManager.playSound('reloadStart', null, 0.3);
                triggerHapticFn([8, 10]);
            }
            event.preventDefault();
        }, { passive: false });

        addManagedTouchListener(mobileMeleeButton, 'touchstart', (event) => {
            recordMobileActionReachSample(event.changedTouches[0]);
            inputState.melee_attack = true;
            triggerHapticFn(8);
            event.preventDefault();
        }, { passive: false });

        addManagedTouchListener(mobileWeaponPrimaryButton, 'touchstart', (event) => {
            recordMobileActionReachSample(event.changedTouches[0]);
            inputState.change_weapon_slot = 1;
            setObjectiveUrgency('Swapping to primary weapon', 'info', 650);
            const audioManager = getAudioManager();
            if (audioManager && gameSettings.soundEnabled) {
                audioManager.playSound('weaponSwap', null, 0.24);
            }
            triggerHapticFn(8);
            event.preventDefault();
        }, { passive: false });

        addManagedTouchListener(mobileWeaponSecondaryButton, 'touchstart', (event) => {
            recordMobileActionReachSample(event.changedTouches[0]);
            inputState.change_weapon_slot = 2;
            setObjectiveUrgency('Swapping to secondary weapon', 'info', 650);
            const audioManager = getAudioManager();
            if (audioManager && gameSettings.soundEnabled) {
                audioManager.playSound('weaponSwap', null, 0.24);
            }
            triggerHapticFn(8);
            event.preventDefault();
        }, { passive: false });

        addManagedTouchListener(mobileAbilityDashButton, 'touchstart', (event) => {
            recordMobileActionReachSample(event.changedTouches[0]);
            inputState.use_ability_slot = 1;
            setObjectiveUrgency('Dash ability activated', 'positive', 600);
            triggerHapticFn([8, 10]);
            event.preventDefault();
        }, { passive: false });

        addManagedTouchListener(mobileAbilityDodgeButton, 'touchstart', (event) => {
            recordMobileActionReachSample(event.changedTouches[0]);
            inputState.use_ability_slot = 2;
            setObjectiveUrgency('Dodge ability activated', 'positive', 600);
            triggerHapticFn([8, 12]);
            event.preventDefault();
        }, { passive: false });

        addManagedTouchListener(mobilePingButton, 'touchstart', (event) => {
            const aimedPoint = getMouseWorldPos();
            const localPlayerState = getLocalPlayerState();
            const worldPoint = localPlayerState
                ? {
                    x: Number(localPlayerState.render_x ?? localPlayerState.x) || 0,
                    y: Number(localPlayerState.render_y ?? localPlayerState.y) || 0,
                }
                : (Number.isFinite(aimedPoint?.x) && Number.isFinite(aimedPoint?.y)
                    ? { x: aimedPoint.x, y: aimedPoint.y }
                    : null);
            if (worldPoint) {
                sendTacticalPing('group', worldPoint);
                setObjectiveUrgency('Group-up ping sent', 'positive', 800);
            }
            recordMobileActionReachSample(event.changedTouches?.[0]);
            event.preventDefault();
        }, { passive: false });

        // Mobile settings buttons
        const mobileDataSaverBtn = document.getElementById('mobileDataSaver');
        const mobileAimAssistBtn = document.getElementById('mobileAimAssistToggle');
        const mobileConnQualityDiv = document.getElementById('mobileConnectionQuality');

        if (mobileDataSaverBtn) {
            addManagedTouchListener(mobileDataSaverBtn, 'touchstart', (event) => {
                triggerHapticFn(8);
                event.preventDefault();
            }, { passive: false });
        }

        if (mobileAimAssistBtn) {
            if (aimAssistEnabled) mobileAimAssistBtn.classList.add('mobile-button--active');
            addManagedTouchListener(mobileAimAssistBtn, 'touchstart', (event) => {
                aimAssistEnabled = !aimAssistEnabled;
                localStorage.setItem('aimAssist', aimAssistEnabled);
                mobileAimAssistBtn.classList.toggle('mobile-button--active', aimAssistEnabled);
                triggerHapticFn(8);
                event.preventDefault();
            }, { passive: false });
        }

        if (mobileConnQualityDiv) {
            // Connection quality indicator updated elsewhere via updateConnectionQuality
        }

        // Ping wheel
        const schedulePingWheel = (touch, touchId = null) => {
            if (!touch) return;
            pingWheelTouchBoundsRect = minimapContainerDiv?.getBoundingClientRect() || null;
            const anchor = getWorldPointFromMinimap(touch.clientX, touch.clientY);
            if (!anchor) return;
            pingWheelTouchId = touchId;
            pingWheelLongPressTimer = setTimeout(() => {
                pingWheelLongPressTimer = null;
                openPingWheel(touch.clientX, touch.clientY, anchor.x, anchor.y);
                triggerHapticFn(8);
            }, 320);
        };

        addManagedTouchListener(minimapContainerDiv, 'touchstart', (event) => {
            if (pingWheelOpen) return;
            const touch = event.changedTouches[0];
            schedulePingWheel(touch, touch.identifier);
        }, { passive: true });

        addManagedTouchListener(minimapContainerDiv, 'touchmove', (event) => {
            if (pingWheelOpen || pingWheelTouchId === null) return;
            for (const touch of event.changedTouches) {
                if (touch.identifier === pingWheelTouchId) {
                    const anchor = getWorldPointFromMinimap(touch.clientX, touch.clientY);
                    if (!anchor) { closePingWheel(); return; }
                    const rect = pingWheelTouchBoundsRect || minimapContainerDiv.getBoundingClientRect();
                    if (touch.clientX < rect.left - 12 || touch.clientX > rect.right + 12 ||
                        touch.clientY < rect.top - 12 || touch.clientY > rect.bottom + 12) {
                        closePingWheel();
                    }
                }
            }
        }, { passive: true });

        addManagedTouchListener(minimapContainerDiv, 'touchend', () => {
            if (!pingWheelOpen) closePingWheel();
        }, { passive: true });
        addManagedTouchListener(minimapContainerDiv, 'touchcancel', closePingWheel, { passive: true });

        addManagedTouchListener(minimapContainerDiv, 'mousedown', (event) => {
            if (event.button !== 0 || pingWheelOpen) return;
            pingWheelTouchBoundsRect = minimapContainerDiv?.getBoundingClientRect() || null;
            const anchor = getWorldPointFromMinimap(event.clientX, event.clientY);
            if (!anchor) return;
            pingWheelLongPressTimer = setTimeout(() => {
                pingWheelLongPressTimer = null;
                openPingWheel(event.clientX, event.clientY, anchor.x, anchor.y);
            }, 320);
        });
        addManagedTouchListener(minimapContainerDiv, 'contextmenu', (event) => {
            event.preventDefault();
        });
        addManagedTouchListener(window, 'mouseup', (event) => {
            if (!pingWheelOpen) { closePingWheel(); return; }
            if (pingWheelDiv && event?.target && !pingWheelDiv.contains(event.target)) {
                closePingWheel();
            }
        });

        pingWheelDiv?.querySelectorAll('[data-ping-kind]')?.forEach((btn) => {
            addManagedTouchListener(btn, 'click', (event) => {
                event.preventDefault();
                const kind = btn.getAttribute('data-ping-kind') || 'group';
                if (pingWheelAnchorWorld) sendTacticalPing(kind, pingWheelAnchorWorld);
                closePingWheel();
            });
        });
        addManagedTouchListener(document, 'keydown', (event) => {
            if (event.key === 'Escape' && pingWheelOpen) closePingWheel();
        });

        inputWindowResizeHandler?.();
    }

    function onConnectionReset() {
        teardownInputHandlers();
        closePingWheel();
        mobileAimActive = false;
        mobileFireTouchActive = false;
        mobileStickyFireArmed = false;
        pendingInputs.length = 0;
        tacticalPings.length = 0;
        inputSequence = 0;
    }

    function destroy() {
        onConnectionReset();
    }

    return {
        setupInputHandlers,
        teardownInputHandlers,
        onConnectionReset,
        destroy,
        sendInputsToServer,
        handleKeyInput,
        handleMouseMove,
        createVirtualCrosshair,
        updateVirtualCrosshair,
        getVirtualCrosshairSprite,
        applyAimAssist,
        getAimAssistRotation,
        getAimAssistTarget,
        closePingWheel,
        openPingWheel,
        sendTacticalPing,
        issueCommanderOrder,
        updateMobileControlsVisibility,
        updateMobileButtonSizing,
        getWorldPointFromMinimap,
        setPendingInputsBuffer(nextPendingInputs) {
            pendingInputs = Array.isArray(nextPendingInputs) ? nextPendingInputs : [];
        },
        setInputSequenceValue(nextInputSequence) {
            if (Number.isFinite(nextInputSequence)) {
                inputSequence = Math.max(0, Math.floor(nextInputSequence));
            } else {
                inputSequence = 0;
            }
        },
        // Expose mutable state getters
        get tacticalPings() { return tacticalPings; },
        get pendingInputs() { return pendingInputs; },
        get inputSequence() { return inputSequence; },
        get aimAssistEnabled() { return aimAssistEnabled; },
        set aimAssistEnabled(v) { aimAssistEnabled = v; },
        get pingWheelOpen() { return pingWheelOpen; },
        get mobileAimActive() { return mobileAimActive; },
        get mobileFireTouchActive() { return mobileFireTouchActive; },
        get mobileStickyFireArmed() { return mobileStickyFireArmed; },
    };
}
