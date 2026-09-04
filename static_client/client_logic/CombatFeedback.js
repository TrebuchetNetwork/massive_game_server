/**
 * CombatFeedback.js - Combat UI feedback system extracted from client.html
 *
 * Contains damage indicators, hit markers, streak announcements,
 * combat banners, death recap, objective urgency, radial HUD,
 * and all combat presentation logic.
 */

export function createCombatFeedback(getCtx) {
    let lastLocalDamageImpactAt = 0;
    const streakPingCooldownByPlayer = new Map();
    const TIP_STORAGE_KEY = 'mgs_first_time_tips_v1';
    const CAREER_PROFILE_KEY = 'mgs_career_profile_v1';
    const tipFlags = (() => {
        try {
            const raw = sessionStorage.getItem(TIP_STORAGE_KEY);
            const parsed = raw ? JSON.parse(raw) : [];
            return new Set(Array.isArray(parsed) ? parsed : []);
        } catch (_) {
            return new Set();
        }
    })();
    let lastModeIntroActive = false;
    const objectiveArrowPool = [];

    function persistTipFlags() {
        try {
            sessionStorage.setItem(TIP_STORAGE_KEY, JSON.stringify(Array.from(tipFlags.values()).slice(-24)));
        } catch (_) {}
    }

    function getWeaponDisplayInfo(weaponId) {
        const ctx = getCtx();
        const fallbackNames = {
            1: 'Pistol',
            2: 'Shotgun',
            3: 'Rifle',
            4: 'Sniper',
            5: 'Melee',
        };
        const fallbackColors = {
            1: 0xFBBF24,
            2: 0xFB923C,
            3: 0x60A5FA,
            4: 0xE879F9,
            5: 0xF87171,
        };
        return {
            name: ctx.weaponNames?.[weaponId] || fallbackNames[weaponId] || 'Weapon',
            color: Number(ctx.weaponColors?.[weaponId]) || fallbackColors[weaponId] || 0xF8FAFC,
        };
    }

    function persistWeaponMilestone(weaponName, milestone) {
        if (!weaponName || !Number.isFinite(milestone) || milestone <= 0) return;
        try {
            const raw = localStorage.getItem(CAREER_PROFILE_KEY);
            const parsed = raw ? JSON.parse(raw) : {};
            const nextProfile = parsed && typeof parsed === 'object' ? parsed : {};
            const weaponMastery = nextProfile.weapon_mastery && typeof nextProfile.weapon_mastery === 'object'
                ? { ...nextProfile.weapon_mastery }
                : {};
            const weaponMilestones = nextProfile.weapon_milestones && typeof nextProfile.weapon_milestones === 'object'
                ? { ...nextProfile.weapon_milestones }
                : {};
            const threshold = Math.max(0, Math.trunc(milestone));
            weaponMastery[weaponName] = Math.max(Math.trunc(Number(weaponMastery[weaponName]) || 0), threshold);
            weaponMilestones[weaponName] = Math.max(Math.trunc(Number(weaponMilestones[weaponName]) || 0), threshold);
            nextProfile.weapon_mastery = weaponMastery;
            nextProfile.weapon_milestones = weaponMilestones;
            localStorage.setItem(CAREER_PROFILE_KEY, JSON.stringify(nextProfile));
        } catch (_) {}
    }

    function showTipOnce(tipKey, message, durationMs = 5000) {
        const ctx = getCtx();
        if (!tipKey || !message) return;
        if (tipFlags.has(tipKey)) return;
        tipFlags.add(tipKey);
        persistTipFlags();
        if (!ctx.tipsToastDiv) return;
        if (!ctx.combatUiState.tipDismissBound) {
            ctx.tipsToastDiv.addEventListener('click', () => {
                const liveCtx = getCtx();
                liveCtx.combatUiState.tipUntilMs = 0;
                if (liveCtx.tipsToastDiv) {
                    liveCtx.tipsToastDiv.classList.remove('tips-toast--visible');
                }
            });
            ctx.combatUiState.tipDismissBound = true;
        }
        ctx.tipsToastDiv.textContent = String(message);
        ctx.combatUiState.tipUntilMs = Date.now() + Math.max(1800, Number(durationMs) || 5000);
        ctx.tipsToastDiv.classList.add('tips-toast--visible');
    }

    function clearDeathRecapMinimap() {
        const ctx = getCtx();
        const canvas = ctx.deathRecapMinimapCanvas;
        if (!canvas || typeof canvas.getContext !== 'function') return;
        const context2d = canvas.getContext('2d');
        if (!context2d) return;
        context2d.clearRect(0, 0, canvas.width, canvas.height);
        canvas.style.display = 'none';
    }

    function drawDeathRecapMinimap(entry) {
        const ctx = getCtx();
        const canvas = ctx.deathRecapMinimapCanvas;
        if (!canvas || typeof canvas.getContext !== 'function') return;
        const context2d = canvas.getContext('2d');
        if (!context2d) return;

        const killerPosition = entry?.killer_position;
        const victimPosition = entry?.victim_position;
        const hasPositions =
            Number.isFinite(Number(killerPosition?.x)) &&
            Number.isFinite(Number(killerPosition?.y)) &&
            Number.isFinite(Number(victimPosition?.x)) &&
            Number.isFinite(Number(victimPosition?.y));
        const wallEntries = ctx.walls instanceof Map ? Array.from(ctx.walls.values()) : [];
        if (!hasPositions && wallEntries.length === 0) {
            clearDeathRecapMinimap();
            return;
        }

        const mapScale = 0.05;
        const padding = 8;
        const width = canvas.width;
        const height = canvas.height;
        let centerX = 0;
        let centerY = 0;
        if (ctx.worldBoundsState?.valid) {
            centerX = (Number(ctx.worldBoundsState.minX) + Number(ctx.worldBoundsState.maxX)) * 0.5;
            centerY = (Number(ctx.worldBoundsState.minY) + Number(ctx.worldBoundsState.maxY)) * 0.5;
        } else if (hasPositions) {
            centerX = (Number(killerPosition.x) + Number(victimPosition.x)) * 0.5;
            centerY = (Number(killerPosition.y) + Number(victimPosition.y)) * 0.5;
        }

        const projectPoint = (worldX, worldY) => ({
            x: width * 0.5 + (Number(worldX) - centerX) * mapScale,
            y: height * 0.5 + (Number(worldY) - centerY) * mapScale,
        });

        context2d.clearRect(0, 0, width, height);
        context2d.fillStyle = 'rgba(2, 6, 23, 0.92)';
        context2d.fillRect(0, 0, width, height);
        context2d.strokeStyle = 'rgba(148, 163, 184, 0.28)';
        context2d.strokeRect(0.5, 0.5, width - 1, height - 1);

        context2d.save();
        context2d.beginPath();
        context2d.rect(padding, padding, width - padding * 2, height - padding * 2);
        context2d.clip();

        context2d.fillStyle = 'rgba(148, 163, 184, 0.45)';
        for (let i = 0; i < wallEntries.length; i += 1) {
            const wall = wallEntries[i];
            if (!wall) continue;
            const topLeft = projectPoint(Number(wall.x) || 0, Number(wall.y) || 0);
            const wallWidth = (Number(wall.width) || 0) * mapScale;
            const wallHeight = (Number(wall.height) || 0) * mapScale;
            if (wallWidth <= 0 || wallHeight <= 0) continue;
            context2d.fillRect(topLeft.x, topLeft.y, wallWidth, wallHeight);
        }

        if (hasPositions) {
            const killerPoint = projectPoint(killerPosition.x, killerPosition.y);
            const victimPoint = projectPoint(victimPosition.x, victimPosition.y);

            context2d.setLineDash([4, 3]);
            context2d.strokeStyle = 'rgba(248, 113, 113, 0.9)';
            context2d.lineWidth = 1.5;
            context2d.beginPath();
            context2d.moveTo(killerPoint.x, killerPoint.y);
            context2d.lineTo(victimPoint.x, victimPoint.y);
            context2d.stroke();
            context2d.setLineDash([]);

            context2d.fillStyle = '#EF4444';
            context2d.beginPath();
            context2d.arc(killerPoint.x, killerPoint.y, 4, 0, Math.PI * 2);
            context2d.fill();

            context2d.strokeStyle = '#F8FAFC';
            context2d.lineWidth = 2;
            context2d.beginPath();
            context2d.moveTo(victimPoint.x - 5, victimPoint.y - 5);
            context2d.lineTo(victimPoint.x + 5, victimPoint.y + 5);
            context2d.moveTo(victimPoint.x + 5, victimPoint.y - 5);
            context2d.lineTo(victimPoint.x - 5, victimPoint.y + 5);
            context2d.stroke();
        }

        context2d.restore();
        canvas.style.display = 'block';
    }

    function ensureObjectiveArrowPool(size) {
        const ctx = getCtx();
        if (!ctx.objectiveArrowLayerDiv) return;
        while (objectiveArrowPool.length < size) {
            const arrow = document.createElement('div');
            arrow.className = 'objective-arrow';
            const glyph = document.createElement('span');
            glyph.className = 'objective-arrow__glyph';
            glyph.textContent = '▲';
            const label = document.createElement('span');
            label.className = 'objective-arrow__label';
            const distance = document.createElement('span');
            distance.className = 'objective-arrow__distance';
            arrow.appendChild(glyph);
            arrow.appendChild(label);
            arrow.appendChild(distance);
            ctx.objectiveArrowLayerDiv.appendChild(arrow);
            objectiveArrowPool.push({ el: arrow, glyph, label, distance });
        }
    }

    function hideObjectiveArrows() {
        for (let i = 0; i < objectiveArrowPool.length; i += 1) {
            const item = objectiveArrowPool[i];
            if (item?.el) item.el.style.display = 'none';
        }
    }

    function worldToScreenPoint(x, y) {
        const ctx = getCtx();
        if (!ctx.gameScene || !ctx.app || !ctx.PIXI) return null;
        if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
        const point = ctx.gameScene.toGlobal(new ctx.PIXI.Point(x, y));
        if (!point || !Number.isFinite(point.x) || !Number.isFinite(point.y)) return null;
        return point;
    }

    function updateObjectiveArrows(compactMode = false) {
        const ctx = getCtx();
        if (!ctx.objectiveArrowLayerDiv || !ctx.localPlayerState || !ctx.app) return;
        const localX = Number(ctx.localPlayerState.render_x ?? ctx.localPlayerState.x);
        const localY = Number(ctx.localPlayerState.render_y ?? ctx.localPlayerState.y);
        if (!Number.isFinite(localX) || !Number.isFinite(localY)) {
            hideObjectiveArrows();
            return;
        }

        const maxTargets = compactMode ? 2 : 3;
        const targets = [];
        const gameMode = Number(ctx.matchInfo?.game_mode);
        if (gameMode === ctx.GP.GameModeType.CaptureTheFlag) {
            const myTeamId = Number(ctx.localPlayerState.team_id) || 0;
            const enemyTeamId = myTeamId === 1 ? 2 : (myTeamId === 2 ? 1 : 0);
            const myFlag = myTeamId ? ctx.flagStates.get(myTeamId) : null;
            const enemyFlag = enemyTeamId ? ctx.flagStates.get(enemyTeamId) : null;
            if (enemyFlag?.position) {
                targets.push({
                    x: Number(enemyFlag.position.x),
                    y: Number(enemyFlag.position.y),
                    label: enemyFlag.status === ctx.GP.FlagStatus.Carried ? 'ENEMY FLAG (C)' : 'ENEMY FLAG',
                    tone: 'critical',
                });
            }
            if (myFlag?.status === ctx.GP.FlagStatus.Dropped && myFlag.position) {
                targets.push({
                    x: Number(myFlag.position.x),
                    y: Number(myFlag.position.y),
                    label: 'RECOVER FLAG',
                    tone: 'positive',
                });
            }
        }
        if (ctx.hotZoneState?.active) {
            targets.push({
                x: Number(ctx.hotZoneState.centerX),
                y: Number(ctx.hotZoneState.centerY),
                label: 'HOT ZONE',
                tone: 'critical',
            });
        }

        if (targets.length === 0) {
            hideObjectiveArrows();
            return;
        }

        const selected = targets
            .filter((t) => Number.isFinite(t.x) && Number.isFinite(t.y))
            .map((t) => {
                const dx = t.x - localX;
                const dy = t.y - localY;
                return { ...t, distance: Math.hypot(dx, dy) };
            })
            .sort((a, b) => a.distance - b.distance)
            .slice(0, maxTargets);

        ensureObjectiveArrowPool(selected.length);
        const centerX = ctx.app.screen.width * 0.5;
        const centerY = ctx.app.screen.height * 0.5;
        const margin = compactMode ? 44 : 54;
        const maxX = ctx.app.screen.width - margin;
        const maxY = ctx.app.screen.height - margin;
        const minX = margin;
        const minY = margin;

        let visibleCount = 0;
        for (let i = 0; i < selected.length; i += 1) {
            const target = selected[i];
            const item = objectiveArrowPool[i];
            if (!item?.el || !item.glyph || !item.label || !item.distance) continue;
            const projected = worldToScreenPoint(target.x, target.y);
            if (!projected) {
                item.el.style.display = 'none';
                continue;
            }
            const onScreen =
                projected.x >= minX &&
                projected.x <= maxX &&
                projected.y >= minY &&
                projected.y <= maxY;
            if (onScreen) {
                item.el.style.display = 'none';
                continue;
            }
            const dx = projected.x - centerX;
            const dy = projected.y - centerY;
            const angle = Math.atan2(dy, dx);
            const radiusX = Math.max(30, centerX - margin);
            const radiusY = Math.max(30, centerY - margin);
            const t = 1 / Math.max(
                Math.abs(dx) / radiusX || 1e-5,
                Math.abs(dy) / radiusY || 1e-5
            );
            const px = centerX + dx * t;
            const py = centerY + dy * t;
            item.el.style.display = 'flex';
            item.el.style.transform = `translate(${Math.round(px)}px, ${Math.round(py)}px)`;
            item.glyph.style.transform = `rotate(${((angle * 180 / Math.PI) + 90).toFixed(1)}deg)`;
            item.el.classList.toggle('objective-arrow--critical', target.tone === 'critical');
            item.el.classList.toggle('objective-arrow--positive', target.tone !== 'critical');
            item.label.textContent = target.label;
            item.distance.textContent = `${Math.round(target.distance)}u`;
            visibleCount += 1;
        }
        for (let i = visibleCount; i < objectiveArrowPool.length; i += 1) {
            if (objectiveArrowPool[i]?.el) objectiveArrowPool[i].el.style.display = 'none';
        }
    }

    function updateBoundaryWarning(currentTime) {
        const ctx = getCtx();
        if (!ctx.boundaryWarningDiv || !ctx.worldBoundsState?.valid || !ctx.localPlayerState) return;
        const px = Number(ctx.localPlayerState.render_x ?? ctx.localPlayerState.x);
        const py = Number(ctx.localPlayerState.render_y ?? ctx.localPlayerState.y);
        if (!Number.isFinite(px) || !Number.isFinite(py)) {
            ctx.boundaryWarningDiv.classList.remove('boundary-warning--visible');
            return;
        }
        const minX = Number(ctx.worldBoundsState.minX);
        const minY = Number(ctx.worldBoundsState.minY);
        const maxX = Number(ctx.worldBoundsState.maxX);
        const maxY = Number(ctx.worldBoundsState.maxY);
        if (!Number.isFinite(minX) || !Number.isFinite(minY) || !Number.isFinite(maxX) || !Number.isFinite(maxY)) {
            ctx.boundaryWarningDiv.classList.remove('boundary-warning--visible');
            return;
        }
        const distance = Math.min(
            Math.abs(px - minX),
            Math.abs(maxX - px),
            Math.abs(py - minY),
            Math.abs(maxY - py),
        );
        if (distance <= 80) {
            const urgency = ctx.clamp(1 - (distance / 80), 0, 1);
            ctx.boundaryWarningDiv.classList.add('boundary-warning--visible');
            ctx.boundaryWarningDiv.style.opacity = String((0.45 + urgency * 0.55).toFixed(3));
            ctx.boundaryWarningDiv.textContent = distance <= 26 ? 'TURN BACK' : 'MAP BOUNDARY';
            if (distance <= 18 && (currentTime - (ctx.combatUiState.lastBoundaryTipAt || 0)) > 5000) {
                ctx.combatUiState.lastBoundaryTipAt = currentTime;
                showTipOnce('boundary_warning', 'Map edge reached. Turn back to avoid getting trapped.');
            }
        } else {
            ctx.boundaryWarningDiv.classList.remove('boundary-warning--visible');
        }
    }

    function updateFirstTimeTips(currentTime) {
        const ctx = getCtx();
        if (!ctx.localPlayerState) return;
        if (ctx.tipsToastDiv) {
            const visible = Number(ctx.combatUiState.tipUntilMs) > currentTime;
            ctx.tipsToastDiv.classList.toggle('tips-toast--visible', visible);
        }

        if (Number(ctx.localPlayerState.reload_progress) > 0.01) {
            showTipOnce('reload_manual', 'Reloading: press R to top up before re-engaging.');
        }

        if (Array.isArray(ctx.zones) || ctx.zones?.size) {
            const zoneValues = Array.isArray(ctx.zones) ? ctx.zones : Array.from(ctx.zones.values());
            const px = Number(ctx.localPlayerState.render_x ?? ctx.localPlayerState.x);
            const py = Number(ctx.localPlayerState.render_y ?? ctx.localPlayerState.y);
            for (let i = 0; i < zoneValues.length; i += 1) {
                const z = zoneValues[i];
                if (!z) continue;
                const zx = Number(z.x);
                const zy = Number(z.y);
                const zw = Number(z.width);
                const zh = Number(z.height);
                if (!Number.isFinite(zx) || !Number.isFinite(zy) || !Number.isFinite(zw) || !Number.isFinite(zh)) continue;
                const inside = px >= zx && px <= (zx + zw) && py >= zy && py <= (zy + zh);
                if (!inside) continue;
                if (Number(z.zone_type) === ctx.GP.ZoneType.SlowZone) {
                    showTipOnce('zone_slow', 'Slow Zone: movement is reduced while inside.');
                } else if (Number(z.zone_type) === ctx.GP.ZoneType.DamageZone) {
                    showTipOnce('zone_damage', 'Damage Zone: leave quickly to avoid constant damage.');
                } else if (Number(z.zone_type) === ctx.GP.ZoneType.BoostPad) {
                    showTipOnce('zone_boost', 'Boost Pad: step through it for a burst of speed.');
                }
                break;
            }
        }
    }

    function updateModeIntroOverlay(currentTime) {
        const ctx = getCtx();
        if (!ctx.gameModeIntroDiv || !ctx.matchInfo) return;
        const isActive = Number(ctx.matchInfo.match_state) === Number(ctx.GP.MatchStateType.Active);
        if (isActive && !lastModeIntroActive) {
            let introText = '';
            if (ctx.matchInfo.game_mode === ctx.GP.GameModeType.FreeForAll) {
                introText = 'FFA: Eliminate opponents. Highest score wins.';
            } else if (ctx.matchInfo.game_mode === ctx.GP.GameModeType.TeamDeathmatch) {
                introText = 'TDM: Coordinate with your team and outscore the enemy.';
            } else if (ctx.matchInfo.game_mode === ctx.GP.GameModeType.CaptureTheFlag) {
                introText = 'CTF: Steal the enemy flag and return it to base (+100).';
            }
            // Gauntlet: the wave announcement (from the `gauntlet_wave`
            // system event) outranks the generic TDM intro when it arrived
            // around this match start.
            const waveIntro = ctx.combatUiState.gauntletWaveIntro;
            let introHoldMs = 3000;
            if (waveIntro && waveIntro.text && Math.abs(currentTime - Number(waveIntro.atMs)) < 20000) {
                introText = waveIntro.text;
                introHoldMs = 6000;
            }
            if (introText) {
                ctx.gameModeIntroDiv.textContent = introText;
                ctx.combatUiState.modeIntroUntilMs = currentTime + introHoldMs;
                ctx.gameModeIntroDiv.classList.add('mode-intro--visible');
            }
        }
        lastModeIntroActive = isActive;
        if (Number(ctx.combatUiState.modeIntroUntilMs) > currentTime) {
            ctx.gameModeIntroDiv.classList.add('mode-intro--visible');
        } else {
            ctx.gameModeIntroDiv.classList.remove('mode-intro--visible');
        }
    }

    function getCombatUiQualityMode() {
        const ctx = getCtx();
        return ctx.normalizeCombatUiQuality(ctx.gameSettings?.combatUiQuality);
    }

    function getCombatUiPerfTier() {
        const ctx = getCtx();
        const qualityMode = getCombatUiQualityMode();
        if (qualityMode === 'high') return 0;
        if (qualityMode === 'low') return 2;
        if (ctx.ultraPerformanceMode || ctx.activeEffectsProfileName === 'ultra' || ctx.smoothedFrameMs >= 26 || ctx.players.size >= 130) return 2;
        if (ctx.activeEffectsProfileName === 'dense' || ctx.smoothedFrameMs >= 22 || ctx.players.size >= 80) return 1;
        return 0;
    }

    function ensureDamageIndicatorPool(size) {
        const ctx = getCtx();
        if (!ctx.damageDirectionLayerDiv) return;
        while (ctx.combatUiState.damageIndicatorElements.length < size) {
            const arrow = document.createElement('div');
            arrow.className = 'damage-direction-arrow';
            arrow.style.display = 'none';
            ctx.damageDirectionLayerDiv.appendChild(arrow);
            ctx.combatUiState.damageIndicatorElements.push(arrow);
        }
    }

    function clearDamageDirectionIndicators() {
        const ctx = getCtx();
        const pool = ctx.combatUiState.damageIndicatorElements;
        for (let i = 0; i < pool.length; i += 1) {
            const el = pool[i];
            if (el) el.style.display = 'none';
        }
        ctx.combatUiState.damageIndicatorVisibleCount = 0;
        ctx.combatUiState.lastDamageIndicatorPaintAt = 0;
    }

    function renderDamageDirectionIndicators(activeIndicators, currentTime, compactMode = false) {
        const ctx = getCtx();
        if (!ctx.damageDirectionLayerDiv) return;
        if (!Array.isArray(activeIndicators) || activeIndicators.length === 0) {
            if (ctx.combatUiState.damageIndicatorVisibleCount > 0) {
                clearDamageDirectionIndicators();
            }
            return;
        }
        const maxVisible = compactMode
            ? Math.min(ctx.COMBAT_VISIBLE_DAMAGE_INDICATORS, 2)
            : ctx.COMBAT_VISIBLE_DAMAGE_INDICATORS;
        const visibleCount = Math.min(maxVisible, activeIndicators.length);
        const updateIntervalMs = compactMode
            ? ctx.COMBAT_DAMAGE_INDICATOR_DENSE_UPDATE_MS
            : ctx.COMBAT_DAMAGE_INDICATOR_UPDATE_MS;
        const shouldRepaint = (currentTime - ctx.combatUiState.lastDamageIndicatorPaintAt) >= updateIntervalMs
            || ctx.combatUiState.damageIndicatorVisibleCount !== visibleCount;
        if (!shouldRepaint) return;

        ensureDamageIndicatorPool(visibleCount);
        const pool = ctx.combatUiState.damageIndicatorElements;
        const topIndicators = activeIndicators.slice(-visibleCount);
        for (let i = 0; i < visibleCount; i += 1) {
            const indicator = topIndicators[i];
            const arrow = pool[i];
            if (!indicator || !arrow) continue;
            const life = ctx.clamp((indicator.expiresAt - currentTime) / ctx.COMBAT_DAMAGE_INDICATOR_MS, 0, 1);
            const radius = 164 + (1 - life) * 18;
            const x = Math.cos(indicator.angle) * radius;
            const y = Math.sin(indicator.angle) * radius;
            const opacity = ctx.clamp(life * indicator.intensity, 0, 1);
            const scale = 0.86 + (indicator.intensity * 0.24);
            const rotDeg = (indicator.angle * 180 / Math.PI) + 90;
            arrow.style.display = 'block';
            arrow.style.opacity = opacity.toFixed(3);
            arrow.style.transform = `translate(${x.toFixed(1)}px, ${y.toFixed(1)}px) rotate(${rotDeg.toFixed(1)}deg) scale(${scale.toFixed(2)})`;
        }
        for (let i = visibleCount; i < pool.length; i += 1) {
            const arrow = pool[i];
            if (arrow && arrow.style.display !== 'none') {
                arrow.style.display = 'none';
            }
        }
        ctx.combatUiState.damageIndicatorVisibleCount = visibleCount;
        ctx.combatUiState.lastDamageIndicatorPaintAt = currentTime;
    }

    function triggerHaptic(pattern = 10) {
        const ctx = getCtx();
        if (!ctx.gameSettings.mobileHaptics || !ctx.isTouchDevice) return;
        if (typeof navigator === 'undefined' || typeof navigator.vibrate !== 'function') return;
        try { navigator.vibrate(pattern); } catch (_) {}
    }

    function playAnnouncerCue(cueType) {
        const ctx = getCtx();
        if (!ctx.audioManager || !ctx.gameSettings.soundEnabled) return;
        const cueSoundMap = {
            headshot: 'announcerHeadshot',
            double: 'announcerDoubleKill',
            triple: 'announcerTripleKill',
            rampage: 'announcerRampage'
        };
        const cueVolumeMap = { headshot: 0.38, double: 0.4, triple: 0.44, rampage: 0.48 };
        const cueSound = cueSoundMap[cueType];
        if (!cueSound) return;
        ctx.audioManager.playSound(cueSound, null, cueVolumeMap[cueType] || 0.4);
    }

    function triggerHitMarker(headshot = false, options = {}) {
        const ctx = getCtx();
        const optimistic = !!options.optimistic;
        const now = Date.now();
        const markerDurationMs = optimistic
            ? Math.max(60, Math.round((Number(ctx.COMBAT_HITMARKER_MS) || 90) * 0.55))
            : (Number(ctx.COMBAT_HITMARKER_MS) || 90);
        ctx.combatUiState.markerUntilMs = Math.max(
            Number(ctx.combatUiState.markerUntilMs) || 0,
            now + markerDurationMs
        );
        if (optimistic) {
            ctx.combatUiState.lastOptimisticHitAt = now;
        }
        if (headshot) {
            ctx.combatUiState.markerHeadshotUntilMs = now + ctx.COMBAT_HEADSHOT_MARKER_MS;
        }
        const hitstopWindowMs = optimistic ? 0 : Math.max(10, Number(ctx.COMBAT_HITSTOP_MS) || 0);
        if (hitstopWindowMs > 0) {
            const extraHeadshot = headshot ? 10 : 0;
            ctx.combatUiState.hitstopUntilMs = Math.max(
                Number(ctx.combatUiState.hitstopUntilMs) || 0,
                now + hitstopWindowMs + extraHeadshot
            );
        }
        if (headshot) {
            if (ctx.audioManager && ctx.gameSettings.soundEnabled) {
                ctx.audioManager.playSound('hitMarkerHeadshot', null, 0.3);
            }
            playAnnouncerCue('headshot');
        } else if (ctx.audioManager && ctx.gameSettings.soundEnabled) {
            const recentOptimisticHit =
                !optimistic &&
                now - (Number(ctx.combatUiState.lastOptimisticHitAt) || 0) < 220;
            const markerVolume = optimistic
                ? 0.11
                : (recentOptimisticHit ? 0.15 : 0.22);
            ctx.audioManager.playSound('hitMarker', null, markerVolume);
        }
        if (!optimistic) {
            triggerHaptic(headshot ? [12, 12, 20] : 8);
        }
    }

    function addDamageDirectionIndicator(sourceX, sourceY, intensity = 1) {
        const ctx = getCtx();
        if (!ctx.localPlayerState || !Number.isFinite(sourceX) || !Number.isFinite(sourceY)) return;
        const now = Date.now();
        const fromX = Number(ctx.localPlayerState.render_x !== undefined ? ctx.localPlayerState.render_x : ctx.localPlayerState.x) || 0;
        const fromY = Number(ctx.localPlayerState.render_y !== undefined ? ctx.localPlayerState.render_y : ctx.localPlayerState.y) || 0;
        const angle = Math.atan2(sourceY - fromY, sourceX - fromX);
        ctx.combatUiState.damageIndicators.push({
            angle,
            intensity: ctx.clamp(Number(intensity) || 0.45, 0.2, 1.2),
            expiresAt: now + ctx.COMBAT_DAMAGE_INDICATOR_MS
        });
        if (ctx.combatUiState.damageIndicators.length > ctx.COMBAT_MAX_DAMAGE_INDICATORS) {
            ctx.combatUiState.damageIndicators.splice(0, ctx.combatUiState.damageIndicators.length - ctx.COMBAT_MAX_DAMAGE_INDICATORS);
        }
    }

    function getStreakAnnouncement(streakCount) {
        if (streakCount >= 8) return { label: 'Godlike', cue: 'rampage' };
        if (streakCount >= 6) return { label: 'Unstoppable', cue: 'rampage' };
        if (streakCount === 5) return { label: 'Rampage', cue: 'rampage' };
        if (streakCount === 4) return { label: 'Mega Kill', cue: 'triple' };
        if (streakCount === 3) return { label: 'Triple Elim', cue: 'triple' };
        if (streakCount === 2) return { label: 'Double Elim', cue: 'double' };
        return null;
    }

    function showStreakMedal(text, durationMs, accentColor) {
        const ctx = getCtx();
        const duration = durationMs !== undefined ? durationMs : ctx.COMBAT_MEDAL_MS;
        if (!ctx.EXCITEMENT_UI_ENABLED || !ctx.streakMedalDiv || !text) return;
        ctx.combatUiState.medalText = String(text);
        ctx.combatUiState.medalUntilMs = Date.now() + Math.max(700, Number(duration) || ctx.COMBAT_MEDAL_MS);
        ctx.streakMedalDiv.textContent = ctx.combatUiState.medalText;
        if (Number.isFinite(accentColor)) {
            const hex = `#${Math.max(0, accentColor >>> 0).toString(16).padStart(6, '0').slice(-6)}`;
            ctx.streakMedalDiv.style.color = hex;
            ctx.streakMedalDiv.style.textShadow = `0 0 18px ${hex}`;
        } else {
            ctx.streakMedalDiv.style.color = '';
            ctx.streakMedalDiv.style.textShadow = '';
        }
        ctx.streakMedalDiv.classList.add('streak-medal--visible');
    }

    function showStreakAnnouncer(text, tone = 'critical', durationMs = 1700) {
        const ctx = getCtx();
        if (!ctx.EXCITEMENT_UI_ENABLED || !ctx.streakAnnouncerDiv || !text) return;
        const normalizedTone = tone === 'positive' ? 'positive' : 'critical';
        ctx.combatUiState.streakAnnouncerText = String(text);
        ctx.combatUiState.streakAnnouncerTone = normalizedTone;
        ctx.combatUiState.streakAnnouncerUntilMs = Date.now() + Math.max(900, Number(durationMs) || 1700);
        ctx.streakAnnouncerDiv.textContent = ctx.combatUiState.streakAnnouncerText;
        ctx.streakAnnouncerDiv.classList.remove('streak-announcer--critical', 'streak-announcer--positive');
        ctx.streakAnnouncerDiv.classList.add(
            normalizedTone === 'positive'
                ? 'streak-announcer--positive'
                : 'streak-announcer--critical'
        );
    }

    function getStreakBroadcastLabel(streakCount) {
        const value = Math.max(0, Math.trunc(Number(streakCount) || 0));
        if (value === 3) return 'ON FIRE';
        if (value === 5) return 'KILLING SPREE';
        if (value === 7) return 'DOMINATING';
        if (value === 8) return 'GODLIKE';
        if (value === 10) return 'LEGENDARY';
        return '';
    }

    function maybeEmitStreakTacticalPing(instigatorId, streakCount, isFriendly) {
        const ctx = getCtx();
        if (!Array.isArray(ctx.tacticalPings)) return;
        const player = ctx.players.get(instigatorId);
        if (!player) return;
        const x = Number.isFinite(player.render_x) ? Number(player.render_x) : Number(player.x);
        const y = Number.isFinite(player.render_y) ? Number(player.render_y) : Number(player.y);
        if (!Number.isFinite(x) || !Number.isFinite(y)) return;

        const now = Date.now();
        const identity = instigatorId || String(player.username || '').trim().toLowerCase();
        if (!identity) return;
        const cooldownMs = streakCount >= 10 ? 1200 : 2100;
        const lastPingAt = Number(streakPingCooldownByPlayer.get(identity)) || 0;
        if ((now - lastPingAt) < cooldownMs) return;
        streakPingCooldownByPlayer.set(identity, now);
        const streakStrength = (() => {
            if (streakCount >= 10) return 1.85;
            if (streakCount >= 8) return 1.65;
            if (streakCount >= 7) return 1.45;
            if (streakCount >= 5) return 1.3;
            return 1.15;
        })();
        const playerName = String(player.username || '').trim();
        const streakBroadcastLabel = getStreakBroadcastLabel(streakCount) || `STREAK x${Math.max(1, Math.trunc(Number(streakCount) || 0))}`;
        const worldLabel = playerName
            ? `${playerName} ${streakBroadcastLabel}`.slice(0, 36)
            : streakBroadcastLabel;

        ctx.tacticalPings.push({
            kind: isFriendly ? 'defend' : 'enemy',
            x,
            y,
            strength: streakStrength,
            source: 'killstreak',
            streak: Math.max(0, Number(streakCount) || 0),
            label: worldLabel,
            player_name: playerName,
            createdAt: now,
            expiresAt: now + Math.max(1600, Math.round((Number(ctx.TACTICAL_PING_MS) || 6200) * 0.55))
        });
        if (ctx.tacticalPings.length > 18) {
            ctx.tacticalPings.splice(0, ctx.tacticalPings.length - 18);
        }

        if (streakPingCooldownByPlayer.size > 192) {
            for (const [key, pingAt] of streakPingCooldownByPlayer.entries()) {
                if ((now - (Number(pingAt) || 0)) > 30000) {
                    streakPingCooldownByPlayer.delete(key);
                }
            }
        }
    }

    function setObjectiveUrgency(text, tone = 'critical', durationMs = 1300) {
        const ctx = getCtx();
        if (!ctx.EXCITEMENT_UI_ENABLED || !ctx.objectiveUrgencyDiv || !text) return;
        ctx.combatUiState.objectiveText = String(text);
        ctx.combatUiState.objectiveTone = tone === 'positive' ? 'positive' : 'critical';
        ctx.combatUiState.objectiveUntilMs = Date.now() + Math.max(500, Number(durationMs) || 1300);
        ctx.objectiveUrgencyDiv.textContent = ctx.combatUiState.objectiveText;
        ctx.objectiveUrgencyDiv.classList.add('objective-urgency--visible');
        ctx.objectiveUrgencyDiv.classList.toggle('objective-urgency--critical', ctx.combatUiState.objectiveTone !== 'positive');
        ctx.objectiveUrgencyDiv.classList.toggle('objective-urgency--positive', ctx.combatUiState.objectiveTone === 'positive');
    }

    function updateObjectiveUrgency(currentTime) {
        const ctx = getCtx();
        if (!ctx.EXCITEMENT_UI_ENABLED || !ctx.objectiveUrgencyDiv || !ctx.localPlayerState) return;
        if ((currentTime - ctx.combatUiState.lastObjectiveEvalAt) < ctx.OBJECTIVE_URGENCY_REFRESH_MS) {
            if (ctx.combatUiState.objectiveUntilMs <= currentTime) {
                ctx.objectiveUrgencyDiv.classList.remove('objective-urgency--visible');
            }
            return;
        }
        ctx.combatUiState.lastObjectiveEvalAt = currentTime;

        let emitted = false;
        if (ctx.matchInfo && ctx.matchInfo.match_state === ctx.GP.MatchStateType.Active) {
            const remaining = Number(ctx.matchInfo.time_remaining) || 0;
            if (remaining > 0 && remaining <= 30) {
                setObjectiveUrgency(`Final ${Math.ceil(remaining)}s - push objective`, 'critical', 1400);
                emitted = true;
            }
        }
        if (!emitted && ctx.matchInfo && ctx.matchInfo.game_mode === ctx.GP.GameModeType.CaptureTheFlag) {
            const myTeamId = Number(ctx.localPlayerState.team_id) || 0;
            const enemyTeamId = myTeamId === 1 ? 2 : (myTeamId === 2 ? 1 : 0);
            const myFlag = myTeamId ? ctx.flagStates.get(myTeamId) : null;
            const enemyFlag = enemyTeamId ? ctx.flagStates.get(enemyTeamId) : null;

            if (myFlag && myFlag.status === ctx.GP.FlagStatus.Carried && myFlag.carrier_id !== ctx.myPlayerId) {
                setObjectiveUrgency('Your flag was stolen - recover now', 'critical', 1100);
                emitted = true;
            } else if (enemyFlag && enemyFlag.status === ctx.GP.FlagStatus.Carried && enemyFlag.carrier_id === ctx.myPlayerId) {
                setObjectiveUrgency('You have enemy flag - escort needed', 'positive', 1000);
                emitted = true;
            } else if (enemyFlag && enemyFlag.status === ctx.GP.FlagStatus.Carried && enemyFlag.carrier_id && enemyFlag.carrier_id !== ctx.myPlayerId) {
                setObjectiveUrgency('Escort your flag carrier', 'positive', 1000);
                emitted = true;
            } else if (myFlag && myFlag.status === ctx.GP.FlagStatus.Dropped && Number(myFlag.respawn_timer) > 0 && Number(myFlag.respawn_timer) <= 5) {
                setObjectiveUrgency(`Flag returns in ${Math.ceil(Number(myFlag.respawn_timer))}s`, 'critical', 900);
                emitted = true;
            }
        }
        if (!emitted && ctx.combatUiState.objectiveUntilMs <= currentTime) {
            ctx.objectiveUrgencyDiv.classList.remove('objective-urgency--visible');
        }
    }

    function showDeathRecap(entry) {
        const ctx = getCtx();
        if (ctx.RESPAWN_ANIMATION_LIGHTWEIGHT) return;
        if (!ctx.deathRecapDiv || !ctx.deathRecapMainDiv || !ctx.deathRecapDistanceDiv || !ctx.deathRecapListDiv) return;
        const killerName = entry?.killer_name || 'Unknown';
        const weaponLabel = ctx.weaponNames[entry?.weapon] || 'Unknown';
        ctx.deathRecapMainDiv.textContent = `Killed by ${killerName} (${weaponLabel})`;

        let distanceText = 'Distance: unknown';
        if (entry?.killer_position && entry?.victim_position) {
            const dx = Number(entry.killer_position.x) - Number(entry.victim_position.x);
            const dy = Number(entry.killer_position.y) - Number(entry.victim_position.y);
            const distance = Math.hypot(dx, dy);
            if (Number.isFinite(distance)) {
                distanceText = `Distance: ${Math.round(distance)}u`;
            }
        }
        ctx.deathRecapDistanceDiv.textContent = distanceText;
        drawDeathRecapMinimap(entry);

        const recent = ctx.combatUiState.recentDamageSources
            .filter((row) => (Date.now() - row.at) <= 10000)
            .slice(-4)
            .reverse();
        const rows = recent.map((row) => ({
            name: row.name || 'Unknown',
            damage: Math.max(0, Math.round(Number(row.damage) || 0)),
            weaponLabelInner: ctx.weaponNames[row.weapon] || 'Unknown'
        }));
        const totalDamage = rows.reduce((sum, row) => sum + row.damage, 0);
        ctx.combatUiState.deathRecapRows = rows.map((row) => `${row.name} - ${row.damage} (${row.weaponLabelInner})`);
        ctx.deathRecapListDiv.replaceChildren();
        if (rows.length === 0) {
            const empty = document.createElement('li');
            empty.className = 'death-recap__item death-recap__item--empty';
            empty.textContent = 'No recent damage data';
            ctx.deathRecapListDiv.appendChild(empty);
        } else {
            const fragment = document.createDocumentFragment();
            rows.forEach((row, idx) => {
                const pct = totalDamage > 0 ? Math.round((row.damage / totalDamage) * 100) : 0;
                const rowClass = idx === 0 ? 'death-recap__item death-recap__item--primary' : 'death-recap__item';
                const li = document.createElement('li');
                li.className = rowClass;
                const source = document.createElement('span');
                source.className = 'death-recap__source';
                source.textContent = row.name;
                const meta = document.createElement('span');
                meta.className = 'death-recap__meta';
                meta.textContent = `${row.damage} (${pct}%) · ${row.weaponLabelInner}`;
                li.appendChild(source);
                li.appendChild(meta);
                fragment.appendChild(li);
            });
            ctx.deathRecapListDiv.appendChild(fragment);
        }
        ctx.combatUiState.deathRecapText = ctx.deathRecapMainDiv.textContent;
        ctx.combatUiState.deathRecapDistanceText = ctx.deathRecapDistanceDiv.textContent;
        ctx.combatUiState.deathRecapUntilMs = Date.now() + ctx.DEATH_RECAP_MS;
        ctx.deathRecapDiv.classList.add('death-recap--visible');
    }

    function registerCombatEventFeedback(event) {
        const ctx = getCtx();
        if (!ctx.EXCITEMENT_UI_ENABLED || !event) return;
        const EVENT_SHIELD_BROKEN = ctx.GP?.GameEventType?.ShieldBroken ?? 14;
        const EVENT_POWERUP_EXPIRING = ctx.GP?.GameEventType?.PowerupExpiring ?? 15;
        const EVENT_WEAPON_MILESTONE = ctx.GP?.GameEventType?.WeaponMilestone ?? 17;
        if (event.event_type === ctx.GP.GameEventType.FlagGrabbed) {
            setObjectiveUrgency('Flag stolen - collapse now', 'critical', 1100);
            if (event.instigator_id === ctx.myPlayerId) {
                showTipOnce('flag_capture_flow', 'You grabbed the flag. Return to your base to score.');
            }
            return;
        }
        if (event.event_type === ctx.GP.GameEventType.FlagCaptured) {
            setObjectiveUrgency('Flag captured - reset and defend', 'critical', 1300);
            const capturerId = typeof event.instigator_id === 'string' ? event.instigator_id : '';
            const localTeamId = Number(ctx.localPlayerState?.team_id) || 0;
            const capturerTeamId = Number(ctx.players.get(capturerId)?.team_id) || 0;
            if (
                localTeamId !== 0 &&
                capturerTeamId !== 0 &&
                localTeamId === capturerTeamId &&
                ctx.audioManager &&
                ctx.gameSettings?.soundEnabled
            ) {
                ctx.audioManager.playSound('flagFanfare', null, 0.42);
            }
            return;
        }
        if (event.event_type === ctx.GP.GameEventType.FlagReturned) {
            setObjectiveUrgency('Flag returned - push forward', 'positive', 1000);
            return;
        }
        if (event.event_type === ctx.GP.GameEventType.Killstreak) {
            const streakCount = Math.round(event.value || 0);
            const instigatorId = typeof event.instigator_id === 'string' ? event.instigator_id : '';
            const isLocal = instigatorId === ctx.myPlayerId;
            const killerName = ctx.players.get(instigatorId)?.username || 'Unknown';
            const killerTeamId = Number(ctx.players.get(instigatorId)?.team_id) || 0;
            const localTeamId = Number(ctx.localPlayerState?.team_id) || 0;
            const isFriendly = !isLocal && localTeamId !== 0 && killerTeamId !== 0 && localTeamId === killerTeamId;
            const broadcastLabel = getStreakBroadcastLabel(streakCount);
            if (broadcastLabel) {
                maybeEmitStreakTacticalPing(instigatorId, streakCount, isLocal || isFriendly);
                if (isLocal) {
                    showStreakAnnouncer(`${broadcastLabel}!`, 'positive', 2000);
                } else if (isFriendly) {
                    showStreakAnnouncer(`${killerName} ${broadcastLabel}!`, 'positive', 1850);
                } else {
                    showStreakAnnouncer(`${killerName} ${broadcastLabel}!`, 'critical', 1850);
                }
            }
            if (isLocal) {
                if (streakCount >= 10) {
                    showCombatBanner('LEGENDARY!', 'kill', 2000);
                    showStreakMedal('Legendary', 1800);
                    setObjectiveUrgency('LEGENDARY! Momentum score x3 active', 'critical', 3400);
                    playAnnouncerCue('rampage');
                    triggerHaptic([12, 14, 18, 14]);
                } else if (streakCount >= 8) {
                    showCombatBanner('GODLIKE!', 'kill', 1900);
                    showStreakMedal('Godlike', 1700);
                    setObjectiveUrgency('GODLIKE! Keep the pressure up', 'critical', 3200);
                    playAnnouncerCue('rampage');
                    triggerHaptic([12, 14, 16]);
                } else if (streakCount >= 7) {
                    showCombatBanner('DOMINATING!', 'kill', 1700);
                    setObjectiveUrgency('DOMINATING! Momentum score x3 active', 'critical', 3000);
                } else if (streakCount >= 5) {
                    showCombatBanner('KILLING SPREE!', 'kill', 1500);
                    setObjectiveUrgency('KILLING SPREE! Momentum score x2 active', 'positive', 2500);
                } else if (streakCount >= 3) {
                    showCombatBanner('ON FIRE!', 'kill', 1300);
                    setObjectiveUrgency('ON FIRE! Momentum score x1.5 active', 'positive', 2000);
                }
            } else {
                if (streakCount >= 10) {
                    showCombatBanner(`${killerName} LEGENDARY!`, 'kill', 2000);
                    setObjectiveUrgency(`${killerName} is LEGENDARY (x3 score)!`, 'critical', 2400);
                } else if (streakCount >= 8) {
                    showCombatBanner(`${killerName} GODLIKE!`, 'kill', 1900);
                    setObjectiveUrgency(`${killerName} is GODLIKE (x3 score)!`, 'critical', 2200);
                } else if (streakCount >= 7) {
                    showCombatBanner(`${killerName} DOMINATING!`, 'kill', 1700);
                    setObjectiveUrgency(`${killerName} is DOMINATING (x3 score)!`, 'critical', 2000);
                } else if (streakCount >= 5) {
                    showCombatBanner(`${killerName} KILLING SPREE!`, 'kill', 1500);
                    setObjectiveUrgency(`${killerName} is on a spree (x2 score)!`, 'critical', 1800);
                } else if (streakCount >= 3) {
                    showCombatBanner(`${killerName} ON FIRE!`, 'kill', 1300);
                }
            }
            return;
        }
        if (event.event_type === ctx.GP.GameEventType.AssistKill) {
            const instigatorId = typeof event.instigator_id === 'string' ? event.instigator_id : '';
            if (instigatorId === ctx.myPlayerId) {
                setObjectiveUrgency(`Assist! +${Math.round(event.value || 0)} pts`, 'positive', 1200);
            }
            return;
        }
        if (event.event_type === EVENT_WEAPON_MILESTONE) {
            const instigatorId = typeof event.instigator_id === 'string' ? event.instigator_id : '';
            if (instigatorId === ctx.myPlayerId) {
                const threshold = Math.max(1, Math.round(Number(event.value) || 0));
                const weapon = getWeaponDisplayInfo(event.weapon_type);
                showCombatBanner(`${threshold} ${weapon.name} Kills!`, 'positive', 1700);
                showStreakMedal(`${weapon.name} Mastery`, 1800, weapon.color);
                setObjectiveUrgency(`${weapon.name} milestone unlocked`, 'positive', 1900);
                persistWeaponMilestone(weapon.name, threshold);
                if (ctx.audioManager && ctx.gameSettings?.soundEnabled) {
                    ctx.audioManager.playSound('flagFanfare', null, 0.42);
                }
                triggerHaptic([12, 18, 12]);
            }
            return;
        }
        if (event.event_type === ctx.GP.GameEventType.TeamPing) {
            const instigatorId = typeof event.instigator_id === 'string' ? event.instigator_id : '';
            if (instigatorId && instigatorId !== ctx.myPlayerId) {
                const pingX = Number(event.position?.x);
                const pingY = Number(event.position?.y);
                if (Number.isFinite(pingX) && Number.isFinite(pingY)) {
                    const localTeamId = Number(ctx.localPlayerState?.team_id) || 0;
                    const localCommanderId = ctx.getCommanderIdForTeam(localTeamId);
                    const fromCommander = !!localCommanderId && String(localCommanderId) === String(instigatorId);
                    const issuerName = String(ctx.players.get(instigatorId)?.username || '').trim();
                    const pingLabel = fromCommander
                        ? `ORDER${issuerName ? `: ${issuerName}` : ''}`.slice(0, 34)
                        : `${issuerName || 'TEAM'} PING`.slice(0, 34);
                    ctx.tacticalPings.push({
                        kind: fromCommander ? 'defend' : 'group',
                        x: pingX,
                        y: pingY,
                        strength: fromCommander ? 1.25 : 1.0,
                        source: fromCommander ? 'commander' : 'teammate',
                        label: pingLabel,
                        player_name: issuerName,
                        createdAt: Date.now(),
                        expiresAt: Date.now() + ctx.TACTICAL_PING_MS
                    });
                    if (ctx.tacticalPings.length > 18) {
                        ctx.tacticalPings.splice(0, ctx.tacticalPings.length - 18);
                    }
                    setObjectiveUrgency(
                        fromCommander ? 'Commander issued an order' : 'Teammate pinged a location',
                        fromCommander ? 'critical' : 'info',
                        fromCommander ? 1100 : 900
                    );
                }
            }
            return;
        }
        if (event.event_type === EVENT_SHIELD_BROKEN) {
            if (event.target_id === ctx.myPlayerId || event.instigator_id === ctx.myPlayerId) {
                setObjectiveUrgency('Shield broken!', 'critical', 900);
            }
            return;
        }
        if (event.event_type === EVENT_POWERUP_EXPIRING) {
            if (event.instigator_id === ctx.myPlayerId) {
                const seconds = Math.max(0, Number(event.value) || 0);
                setObjectiveUrgency(`Powerup ending in ${Math.max(1, Math.ceil(seconds))}s`, 'critical', 900);
            }
            return;
        }
        if (event.event_type !== ctx.GP.GameEventType.PlayerDamageEffect) return;

        const sourcePlayer = event.instigator_id ? ctx.players.get(event.instigator_id) : null;
        const targetPlayer = event.target_id ? ctx.players.get(event.target_id) : null;

        if (event.target_id === ctx.myPlayerId) {
            showTipOnce('damage_direction', 'Incoming damage arrows point to the attacker.');
            const sourceX = sourcePlayer?.x ?? Number(event.position?.x);
            const sourceY = sourcePlayer?.y ?? Number(event.position?.y);
            addDamageDirectionIndicator(sourceX, sourceY, 1);
            ctx.combatUiState.damagePulse = Math.min(1, ctx.combatUiState.damagePulse + 0.4);
            ctx.combatUiState.recentDamageSources.push({
                at: Date.now(),
                instigatorId: event.instigator_id || '',
                name: sourcePlayer?.username || sourcePlayer?.id || 'Unknown',
                damage: Number(event.value) || 0,
                weapon: event.weapon_type
            });
            if (ctx.combatUiState.recentDamageSources.length > 24) {
                ctx.combatUiState.recentDamageSources.splice(0, ctx.combatUiState.recentDamageSources.length - 24);
            }
            const now = Date.now();
            if (now - lastLocalDamageImpactAt >= 95) {
                const damageValue = Math.max(0, Number(event.value) || 0);
                const normalizedImpact = ctx.clamp(damageValue / 42, 0.12, 1);
                if (
                    ctx.gameSettings?.screenShake &&
                    ctx.gameScene &&
                    !ctx.ultraPerformanceMode &&
                    typeof ctx.applyScreenShake === 'function'
                ) {
                    ctx.applyScreenShake(
                        ctx.gameScene,
                        1.15 + normalizedImpact * 1.55,
                        2 + Math.floor(normalizedImpact * 3)
                    );
                }
                if (
                    ctx.app &&
                    !ctx.ultraPerformanceMode &&
                    typeof ctx.createScreenFlash === 'function'
                ) {
                    ctx.createScreenFlash(
                        ctx.app,
                        0xFFB3B3,
                        6 + Math.floor(normalizedImpact * 4),
                        0.06 + normalizedImpact * 0.08
                    );
                }
                lastLocalDamageImpactAt = now;
            }
            triggerHaptic(30);
            return;
        }
        if (event.instigator_id === ctx.myPlayerId && event.target_id && event.target_id !== ctx.myPlayerId) {
            triggerHitMarker(false);
            const falloffMultiplier = Number(event.falloff_multiplier);
            if (Number.isFinite(falloffMultiplier) && falloffMultiplier < 0.8) {
                setObjectiveUrgency(`LONG RANGE x${falloffMultiplier.toFixed(2)}`, 'positive', 820);
            }
            if (targetPlayer && targetPlayer.position) {
                ctx.combatUiState.speedPulse = Math.min(1, ctx.combatUiState.speedPulse + 0.03);
            }
        }
    }

    function rememberProcessedKillFeedKey(key) {
        const ctx = getCtx();
        if (!key) return;
        if (ctx.combatUiState.processedKillFeedKeys.has(key)) return;
        ctx.combatUiState.processedKillFeedKeys.add(key);
        ctx.combatUiState.processedKillFeedQueue.push(key);
        if (ctx.combatUiState.processedKillFeedQueue.length > ctx.COMBAT_EVENT_RETENTION) {
            const stale = ctx.combatUiState.processedKillFeedQueue.shift();
            if (stale) ctx.combatUiState.processedKillFeedKeys.delete(stale);
        }
    }

    function showCombatBanner(text, tone = 'kill', durationMs = 1200) {
        const ctx = getCtx();
        if (!ctx.EXCITEMENT_UI_ENABLED || !ctx.combatBannerDiv || !ctx.combatBannerTextSpan || !text) return;
        ctx.combatUiState.bannerTone = tone;
        ctx.combatUiState.bannerUntilMs = Date.now() + Math.max(500, Number(durationMs) || 0);
        ctx.combatBannerTextSpan.textContent = String(text);
        ctx.combatBannerDiv.classList.remove('combat-banner--kill', 'combat-banner--headshot', 'combat-banner--death');
        ctx.combatBannerDiv.classList.add(`combat-banner--${tone}`);
    }

    function processKillFeedCombatMoments(entries) {
        const ctx = getCtx();
        if (!ctx.EXCITEMENT_UI_ENABLED || !Array.isArray(entries) || entries.length === 0) return;
        const now = Date.now();
        const recentEntries = entries.slice(-8);
        recentEntries.forEach((entry) => {
            if (!entry) return;
            const key = `${entry.killer_id || ''}:${entry.victim_id || ''}:${entry.weapon || 0}:${entry.timestamp || 0}:${entry.is_headshot ? 1 : 0}`;
            if (ctx.combatUiState.processedKillFeedKeys.has(key)) return;
            rememberProcessedKillFeedKey(key);
            if (!ctx.myPlayerId) return;
            const localName = String(ctx.localPlayerState?.username || '').trim();
            const killerMatchesId = entry.killer_id != null && String(entry.killer_id) === String(ctx.myPlayerId);
            const victimMatchesId = entry.victim_id != null && String(entry.victim_id) === String(ctx.myPlayerId);
            const killerMatchesName = !!localName && String(entry.killer_name || '').trim() === localName;
            const victimMatchesName = !!localName && String(entry.victim_name || '').trim() === localName;
            const isLocalKill = (killerMatchesId || killerMatchesName) && !(victimMatchesId || victimMatchesName);
            const isLocalDeath = victimMatchesId || victimMatchesName;
            if (!isLocalKill && !isLocalDeath) return;

            if (isLocalKill) {
                ctx.combatUiState.localKillStreak += 1;
                ctx.combatUiState.comboCount = now <= ctx.combatUiState.comboExpiresAt ? ctx.combatUiState.comboCount + 1 : 1;
                ctx.combatUiState.comboExpiresAt = now + ctx.COMBAT_COMBO_WINDOW_MS;
                ctx.combatUiState.momentum = Math.min(1, ctx.combatUiState.momentum + 0.18 + Math.min(0.14, ctx.combatUiState.localKillStreak * 0.02));
                ctx.combatUiState.speedPulse = Math.min(1, ctx.combatUiState.speedPulse + 0.22);
                ctx.combatUiState.markerKillUntilMs = now + 170;
                showTipOnce('first_kill', 'Eliminations build streak momentum and bonus points.');
                if (ctx.audioManager && ctx.gameSettings?.soundEnabled) {
                    ctx.audioManager.playSound('killConfirm', null, entry.is_headshot ? 0.46 : 0.38, {
                        prioritizeLocal: true,
                        bypassLimiter: true,
                        pitchJitter: 0.03,
                    });
                }
                const victimX = Number(entry?.victim_position?.x);
                const victimY = Number(entry?.victim_position?.y);
                if (
                    Number.isFinite(victimX) &&
                    Number.isFinite(victimY) &&
                    ctx.effectsManager &&
                    typeof ctx.effectsManager.createKillConfirmationMarker === 'function'
                ) {
                    ctx.effectsManager.createKillConfirmationMarker(
                        { x: victimX, y: victimY },
                        { isHeadshot: !!entry.is_headshot }
                    );
                }
                if (entry.is_headshot) {
                    triggerHitMarker(true);
                    showCombatBanner('Headshot', 'headshot', 1100);
                    showStreakMedal('Headshot');
                    setObjectiveUrgency('CRIT +50', 'positive', 850);
                    if (
                        Number.isFinite(victimX) &&
                        Number.isFinite(victimY) &&
                        ctx.effectsManager &&
                        typeof ctx.effectsManager.createEnhancedDamageNumbers === 'function'
                    ) {
                        ctx.effectsManager.createEnhancedDamageNumbers(
                            { x: victimX, y: victimY },
                            75,
                            'enemyDealt',
                            {
                                immediate: true,
                                targetId: entry?.victim_id,
                            }
                        );
                        if (typeof ctx.effectsManager.createEnhancedBulletImpact === 'function') {
                            const resolvedWeapon = Number(entry?.weapon);
                            ctx.effectsManager.createEnhancedBulletImpact(
                                { x: victimX, y: victimY },
                                Number.isFinite(resolvedWeapon)
                                    ? resolvedWeapon
                                    : ctx.GP.WeaponType.Sniper
                            );
                        }
                    }
                    if (ctx.app && typeof ctx.createScreenFlash === 'function') {
                        ctx.createScreenFlash(ctx.app, 0xFFE08A, 12, 0.16);
                    }
                    if (
                        ctx.gameSettings?.screenShake &&
                        ctx.gameScene &&
                        typeof ctx.applyScreenShake === 'function'
                    ) {
                        ctx.applyScreenShake(ctx.gameScene, 2.2, 4);
                    }
                } else {
                    const streakAnnouncement = getStreakAnnouncement(ctx.combatUiState.localKillStreak);
                    showCombatBanner(streakAnnouncement?.label || 'Elimination', 'kill', streakAnnouncement ? 1450 : 900);
                    if (streakAnnouncement?.label) showStreakMedal(streakAnnouncement.label);
                    if (streakAnnouncement?.cue) playAnnouncerCue(streakAnnouncement.cue);
                }
                triggerHaptic(entry.is_headshot ? [14, 12, 20] : [10, 14]);
                return;
            }
            ctx.combatUiState.localKillStreak = 0;
            ctx.combatUiState.comboCount = 0;
            ctx.combatUiState.comboExpiresAt = 0;
            ctx.combatUiState.damagePulse = Math.min(1, ctx.combatUiState.damagePulse + 0.75);
            ctx.combatUiState.momentum = Math.max(0, ctx.combatUiState.momentum - 0.16);
            if (!ctx.RESPAWN_ANIMATION_LIGHTWEIGHT) {
                showCombatBanner('Downed', 'death', 1100);
                showDeathRecap(entry);
                triggerHaptic([50, 40, 50]);
            }
        });
    }

    function updateCombatPresentation(currentTime, deltaSeconds) {
        const ctx = getCtx();
        if (
            !ctx.EXCITEMENT_UI_ENABLED || !ctx.combatOverlayDiv || !ctx.damageFlashLayerDiv ||
            !ctx.speedLinesLayerDiv || !ctx.damageDirectionLayerDiv || !ctx.hitMarkerDiv ||
            !ctx.streakAnnouncerDiv || !ctx.streakMedalDiv || !ctx.objectiveUrgencyDiv || !ctx.combatRadialHudDiv ||
            !ctx.combatMomentumDiv || !ctx.combatMomentumFillDiv || !ctx.combatMomentumValueDiv ||
            !ctx.combatStreakChipSpan || !ctx.combatComboChipSpan || !ctx.combatBannerDiv
        ) { return; }

        if (
            !ctx.myPlayerId || !ctx.localPlayerState || ctx.ultraPerformanceMode ||
            (ctx.RESPAWN_ANIMATION_LIGHTWEIGHT && !ctx.localPlayerState.alive)
        ) {
            ctx.damageFlashLayerDiv.style.opacity = '0';
            ctx.speedLinesLayerDiv.style.opacity = '0';
            ctx.combatMomentumFillDiv.style.transform = 'scaleX(0)';
            ctx.combatMomentumValueDiv.textContent = 'Momentum 0%';
            ctx.combatStreakChipSpan.textContent = 'Streak x0';
            ctx.combatComboChipSpan.textContent = 'Combo x0';
            ctx.combatComboChipSpan.classList.remove('combat-chip--active');
            ctx.combatMomentumDiv.classList.remove('combat-momentum--hot');
            ctx.combatBannerDiv.classList.remove('combat-banner--visible');
            ctx.streakAnnouncerDiv.classList.remove('streak-announcer--visible');
            ctx.streakMedalDiv.classList.remove('streak-medal--visible');
            ctx.objectiveUrgencyDiv.classList.remove('objective-urgency--visible');
            ctx.hitMarkerDiv.classList.remove('hit-marker--visible', 'hit-marker--headshot', 'hit-marker--kill');
            clearDamageDirectionIndicators();
            hideObjectiveArrows();
            if (ctx.tipsToastDiv) ctx.tipsToastDiv.classList.remove('tips-toast--visible');
            // The match/wave intro is a one-line announcement that must
            // survive this reduced-presentation path: it fires at match
            // start, when the local player may still be flagged dead from
            // the previous round, and in ultra-performance mode.
            if (ctx.gameModeIntroDiv) {
                const introHeld = Number(ctx.combatUiState.modeIntroUntilMs) > currentTime;
                ctx.gameModeIntroDiv.classList.toggle('mode-intro--visible', introHeld);
            }
            if (ctx.boundaryWarningDiv) ctx.boundaryWarningDiv.classList.remove('boundary-warning--visible');
            if (ctx.combatRadialHudDiv) ctx.combatRadialHudDiv.style.opacity = '0';
            if (ctx.combatUiState.radialHudCache) {
                const c = ctx.combatUiState.radialHudCache;
                c.lastPaintAt = 0; c.positionMode = ''; c.left = ''; c.top = ''; c.transform = '';
                c.reloadVisible = false; c.reloadDeg = -1; c.reloadLabel = '';
                c.abilityVisible = false; c.abilityDeg = -1; c.abilityColor = ''; c.abilityLabel = '';
                c.dashVisible = false; c.dashDeg = -1; c.dashLabel = ''; c.dashReadyVisible = false; c.dashReadyUntilMs = 0; c.dashLastRemaining = 0;
                c.dodgeVisible = false; c.dodgeDeg = -1; c.dodgeLabel = ''; c.dodgeReadyVisible = false; c.dodgeReadyUntilMs = 0; c.dodgeLastRemaining = 0;
                c.hudVisible = false;
            }
            if (ctx.deathRecapDiv) ctx.deathRecapDiv.classList.remove('death-recap--visible');
            clearDeathRecapMinimap();
            return;
        }

        const combatUiPerfTier = getCombatUiPerfTier();
        const compactCombatUi = combatUiPerfTier >= 1;
        const lowCombatUi = combatUiPerfTier >= 2;
        const dt = Math.max(0, Math.min(0.2, Number(deltaSeconds) || 0));
        ctx.combatUiState.momentum = ctx.clamp(ctx.combatUiState.momentum - ctx.COMBAT_MOMENTUM_DECAY_PER_SEC * dt, 0, 1);
        ctx.combatUiState.speedPulse = ctx.clamp(ctx.combatUiState.speedPulse - ctx.COMBAT_SPEED_DECAY_PER_SEC * dt, 0, 1);
        ctx.combatUiState.damagePulse = ctx.clamp(ctx.combatUiState.damagePulse - ctx.COMBAT_DAMAGE_DECAY_PER_SEC * dt, 0, 1);
        if (ctx.combatUiState.comboCount > 0 && currentTime > ctx.combatUiState.comboExpiresAt) {
            ctx.combatUiState.comboCount = 0;
        }

        const velocityX = Number(ctx.localPlayerState.velocity_x) || 0;
        const velocityY = Number(ctx.localPlayerState.velocity_y) || 0;
        const movementSpeed = Math.hypot(velocityX, velocityY);
        const speedBoostBias = ctx.localPlayerState.speed_boost_remaining > 0 ? 0.24 : 0;
        const speedContribution = ctx.clamp((movementSpeed - 50) / 220 + speedBoostBias, 0, 1);
        ctx.combatUiState.speedPulse = Math.max(ctx.combatUiState.speedPulse, speedContribution);
        if (ctx.localPlayerState.speed_boost_remaining > 0 || ctx.localPlayerState.damage_boost_remaining > 0) {
            ctx.combatUiState.momentum = Math.min(1, ctx.combatUiState.momentum + dt * 0.028);
        }

        const maxHealth = Math.max(1, Number(ctx.localPlayerState.max_health) || 100);
        const healthNow = ctx.clamp(Number(ctx.localPlayerState.health) || 0, 0, maxHealth);
        if (Number.isFinite(ctx.combatUiState.lastKnownHealth)) {
            const healthLoss = ctx.combatUiState.lastKnownHealth - healthNow;
            if (healthLoss > 0.5) {
                const severity = ctx.clamp((healthLoss / maxHealth) * 2.5, 0.12, 1);
                ctx.combatUiState.damagePulse = Math.min(1, ctx.combatUiState.damagePulse + severity);
                ctx.combatUiState.momentum = Math.min(1, ctx.combatUiState.momentum + severity * 0.16);
            }
        }
        ctx.combatUiState.lastKnownHealth = healthNow;

        const momentumValue = ctx.clamp(ctx.combatUiState.momentum, 0, 1);
        const momentumPct = Math.round(momentumValue * 100);
        ctx.combatMomentumFillDiv.style.transform = `scaleX(${momentumValue.toFixed(3)})`;
        ctx.combatMomentumValueDiv.textContent = `Momentum ${momentumPct}%`;

        const streak = Math.max(0, ctx.combatUiState.localKillStreak | 0);
        const combo = Math.max(0, ctx.combatUiState.comboCount | 0);
        ctx.combatStreakChipSpan.textContent = `Streak x${streak}`;
        ctx.combatComboChipSpan.textContent = combo > 1 ? `Combo x${combo}` : 'Combo x0';
        ctx.combatComboChipSpan.classList.toggle('combat-chip--active', combo > 1);
        ctx.combatMomentumDiv.classList.toggle('combat-momentum--hot', momentumValue >= 0.68);

        const lowHealthBias = ctx.clamp(1 - (healthNow / maxHealth), 0, 1) * 0.14;
        const damageOpacity = ctx.localPlayerState.alive
            ? ctx.clamp((ctx.combatUiState.damagePulse * 0.48) + lowHealthBias, 0, 0.66) : 0;
        const speedOpacity = ctx.localPlayerState.alive
            ? ctx.clamp(ctx.combatUiState.speedPulse * 0.56 + momentumValue * 0.18, 0, 0.78) : 0;
        ctx.damageFlashLayerDiv.style.opacity = damageOpacity.toFixed(3);
        if (ctx.COMBAT_SPEED_LINES_ENABLED) {
            ctx.speedLinesLayerDiv.style.opacity = speedOpacity.toFixed(3);
            ctx.speedLinesLayerDiv.style.transform = `translate3d(0, ${((currentTime * 0.2) % 160).toFixed(1)}px, 0)`;
        } else if (ctx.speedLinesLayerDiv.style.opacity !== '0') {
            ctx.speedLinesLayerDiv.style.opacity = '0';
        }

        const bannerVisible = ctx.combatUiState.bannerUntilMs > currentTime;
        ctx.combatBannerDiv.classList.toggle('combat-banner--visible', bannerVisible);
        if (bannerVisible) {
            ctx.combatBannerDiv.classList.remove('combat-banner--kill', 'combat-banner--headshot', 'combat-banner--death');
            ctx.combatBannerDiv.classList.add(`combat-banner--${ctx.combatUiState.bannerTone || 'kill'}`);
        }

        const announcerVisible = ctx.combatUiState.streakAnnouncerUntilMs > currentTime && !!ctx.combatUiState.streakAnnouncerText;
        if (announcerVisible) {
            ctx.streakAnnouncerDiv.textContent = ctx.combatUiState.streakAnnouncerText;
            ctx.streakAnnouncerDiv.classList.remove('streak-announcer--critical', 'streak-announcer--positive');
            ctx.streakAnnouncerDiv.classList.add(
                ctx.combatUiState.streakAnnouncerTone === 'positive'
                    ? 'streak-announcer--positive'
                    : 'streak-announcer--critical'
            );
        }
        ctx.streakAnnouncerDiv.classList.toggle('streak-announcer--visible', announcerVisible);

        const hitMarkerVisible = ctx.combatUiState.markerUntilMs > currentTime;
        ctx.hitMarkerDiv.classList.toggle('hit-marker--visible', hitMarkerVisible);
        ctx.hitMarkerDiv.classList.toggle('hit-marker--headshot', ctx.combatUiState.markerHeadshotUntilMs > currentTime);
        ctx.hitMarkerDiv.classList.toggle('hit-marker--kill', ctx.combatUiState.markerKillUntilMs > currentTime);

        const medalVisible = ctx.combatUiState.medalUntilMs > currentTime && !!ctx.combatUiState.medalText;
        if (medalVisible) ctx.streakMedalDiv.textContent = ctx.combatUiState.medalText;
        ctx.streakMedalDiv.classList.toggle('streak-medal--visible', medalVisible);

        const activeIndicators = ctx.combatUiState.damageIndicators.filter((row) => row && row.expiresAt > currentTime);
        ctx.combatUiState.damageIndicators = activeIndicators;
        renderDamageDirectionIndicators(activeIndicators, currentTime, compactCombatUi);

        updateObjectiveUrgency(currentTime);
        updateObjectiveArrows(compactCombatUi);
        updateBoundaryWarning(currentTime);
        updateModeIntroOverlay(currentTime);
        updateFirstTimeTips(currentTime);

        if (ctx.deathRecapDiv) {
            ctx.deathRecapDiv.classList.toggle('death-recap--visible', ctx.combatUiState.deathRecapUntilMs > currentTime);
        }

        if (
            ctx.combatRadialHudDiv &&
            ctx.abilityRadialDiv &&
            ctx.dashRadialDiv &&
            ctx.dodgeRadialDiv &&
            ctx.reloadRadialDiv &&
            ctx.abilityRadialLabelSpan &&
            ctx.dashRadialLabelSpan &&
            ctx.dodgeRadialLabelSpan &&
            ctx.reloadRadialLabelSpan
        ) {
            const radialCache = ctx.combatUiState.radialHudCache;
            const radialUpdateIntervalMs = lowCombatUi
                ? ctx.COMBAT_RADIAL_LOW_UPDATE_MS
                : (compactCombatUi ? ctx.COMBAT_RADIAL_DENSE_UPDATE_MS : ctx.COMBAT_RADIAL_UPDATE_MS);
            const radialProgressStepDeg = lowCombatUi
                ? ctx.COMBAT_RADIAL_LOW_PROGRESS_STEP_DEG
                : (compactCombatUi ? ctx.COMBAT_RADIAL_DENSE_PROGRESS_STEP_DEG : ctx.COMBAT_RADIAL_PROGRESS_STEP_DEG);
            const allowRadialPaint = (currentTime - radialCache.lastPaintAt) >= radialUpdateIntervalMs;
            let radialPainted = false;

            const isMobileHud = ctx.mobileDynamicsEnabled || ctx.forceMobileClient;
            const positionMode = isMobileHud ? 'mobile' : 'aim';
            let hudLeft = radialCache.left;
            let hudTop = radialCache.top;
            let hudTransform = radialCache.transform;
            if (isMobileHud) {
                hudLeft = '100%'; hudTop = '100%'; hudTransform = 'translate(-136px, -150px)';
            } else if (ctx.app && ctx.gameScene && Number.isFinite(ctx.mouseWorldPos.x) && Number.isFinite(ctx.mouseWorldPos.y)) {
                const point = ctx.gameScene.toGlobal(new ctx.PIXI.Point(ctx.mouseWorldPos.x, ctx.mouseWorldPos.y));
                const clampedX = ctx.clamp(point.x, 56, ctx.app.screen.width - 56);
                const clampedY = ctx.clamp(point.y, 70, ctx.app.screen.height - 70);
                hudLeft = `${clampedX}px`; hudTop = `${clampedY}px`; hudTransform = 'translate(0, 0)';
            }
            const positionChanged = hudLeft !== radialCache.left || hudTop !== radialCache.top || hudTransform !== radialCache.transform;
            const shouldPaintPosition =
                positionMode !== radialCache.positionMode ||
                (!compactCombatUi && positionChanged) ||
                (compactCombatUi && positionChanged && allowRadialPaint) ||
                (allowRadialPaint && isMobileHud);
            if (shouldPaintPosition) {
                if (hudLeft) ctx.combatRadialHudDiv.style.left = hudLeft;
                if (hudTop) ctx.combatRadialHudDiv.style.top = hudTop;
                if (hudTransform) ctx.combatRadialHudDiv.style.transform = hudTransform;
                radialCache.positionMode = positionMode;
                radialCache.left = hudLeft; radialCache.top = hudTop; radialCache.transform = hudTransform;
                radialPainted = true;
            }

            const reloadProgressRaw = Number(ctx.localPlayerState.reload_progress);
            const reloadActive = Number.isFinite(reloadProgressRaw) && reloadProgressRaw >= 0 && reloadProgressRaw <= 1;
            const reloadProgress = reloadActive ? ctx.clamp(reloadProgressRaw, 0, 1) : 0;
            const reloadDeg = Math.round(reloadProgress * 360);
            const reloadLabel = reloadActive ? `Reload ${Math.round(reloadProgress * 100)}%` : 'Reload';
            const reloadNeedsPaint = reloadActive && (reloadActive !== radialCache.reloadVisible || Math.abs(reloadDeg - radialCache.reloadDeg) >= radialProgressStepDeg);
            if (reloadNeedsPaint) {
                ctx.reloadRadialDiv.style.background = `radial-gradient(circle at 50% 50%, rgba(15, 23, 42, 0.9) 49%, rgba(15, 23, 42, 0.68) 63%, transparent 64%), conic-gradient(rgba(56, 189, 248, 0.96) ${reloadDeg}deg, rgba(30, 41, 59, 0.32) 0deg)`;
                radialPainted = true;
            }
            if (reloadActive !== radialCache.reloadVisible) {
                ctx.reloadRadialDiv.classList.toggle('radial-widget--visible', reloadActive);
                radialPainted = true;
            }
            if (reloadLabel !== radialCache.reloadLabel) ctx.reloadRadialLabelSpan.textContent = reloadLabel;
            radialCache.reloadVisible = reloadActive; radialCache.reloadDeg = reloadDeg; radialCache.reloadLabel = reloadLabel;

            const speedRemain = Number(ctx.localPlayerState.speed_boost_remaining) || 0;
            const damageRemain = Number(ctx.localPlayerState.damage_boost_remaining) || 0;
            if (speedRemain > ctx.combatUiState.trackedSpeedBoostMaxSec) ctx.combatUiState.trackedSpeedBoostMaxSec = speedRemain;
            if (damageRemain > ctx.combatUiState.trackedDamageBoostMaxSec) ctx.combatUiState.trackedDamageBoostMaxSec = damageRemain;
            const hasSpeed = speedRemain > 0.01;
            const hasDamage = damageRemain > 0.01;
            const abilityRemain = hasDamage ? damageRemain : speedRemain;
            const abilityMax = hasDamage ? Math.max(1, ctx.combatUiState.trackedDamageBoostMaxSec) : Math.max(1, ctx.combatUiState.trackedSpeedBoostMaxSec);
            const abilityProgress = abilityRemain > 0 ? ctx.clamp(abilityRemain / abilityMax, 0, 1) : 0;
            const abilityActive = abilityRemain > 0.01;
            const abilityDeg = Math.round(abilityProgress * 360);
            const abilityColor = hasDamage ? 'rgba(248, 113, 113, 0.96)' : 'rgba(16, 185, 129, 0.96)';
            const abilityLabel = abilityActive ? `${hasDamage ? 'Dmg' : 'Spd'} ${Math.ceil(abilityRemain)}s` : 'Ability';
            const abilityNeedsPaint = abilityActive && (abilityActive !== radialCache.abilityVisible || abilityColor !== radialCache.abilityColor || Math.abs(abilityDeg - radialCache.abilityDeg) >= radialProgressStepDeg);
            if (abilityNeedsPaint) {
                ctx.abilityRadialDiv.style.background = `radial-gradient(circle at 50% 50%, rgba(15, 23, 42, 0.9) 49%, rgba(15, 23, 42, 0.68) 63%, transparent 64%), conic-gradient(${abilityColor} ${abilityDeg}deg, rgba(30, 41, 59, 0.32) 0deg)`;
                radialPainted = true;
            }
            if (abilityActive !== radialCache.abilityVisible) {
                ctx.abilityRadialDiv.classList.toggle('radial-widget--visible', abilityActive);
                radialPainted = true;
            }
            if (abilityLabel !== radialCache.abilityLabel) ctx.abilityRadialLabelSpan.textContent = abilityLabel;
            radialCache.abilityVisible = abilityActive; radialCache.abilityDeg = abilityDeg; radialCache.abilityColor = abilityColor; radialCache.abilityLabel = abilityLabel;

            const updateCooldownWidget = (prefix, widgetDiv, labelSpan, remainingSec, maxSec, labelBase, color) => {
                const previousRemain = Number(radialCache[`${prefix}LastRemaining`]) || 0;
                const active = remainingSec > 0.01;
                if (!active && previousRemain > 0.01) {
                    radialCache[`${prefix}ReadyUntilMs`] = currentTime + 860;
                }
                const readyVisible = !active && (Number(radialCache[`${prefix}ReadyUntilMs`]) || 0) > currentTime;
                const progress = active ? ctx.clamp(remainingSec / maxSec, 0, 1) : 0;
                const deg = Math.round(progress * 360);
                const label = active ? `${labelBase} ${Math.ceil(remainingSec)}s` : labelBase;
                const readyColor = `rgba(250, 204, 21, ${0.72 + 0.18 * Math.sin(currentTime * 0.026)})`;
                const displayColor = active ? color : readyColor;
                const shouldPaint =
                    (active && (
                        active !== radialCache[`${prefix}Visible`] ||
                        Math.abs(deg - (Number(radialCache[`${prefix}Deg`]) || -1)) >= radialProgressStepDeg
                    )) ||
                    readyVisible !== radialCache[`${prefix}ReadyVisible`] ||
                    (!active && readyVisible && allowRadialPaint);
                if (shouldPaint) {
                    const fillDeg = active ? deg : 360;
                    widgetDiv.style.background = `radial-gradient(circle at 50% 50%, rgba(15, 23, 42, 0.9) 49%, rgba(15, 23, 42, 0.68) 63%, transparent 64%), conic-gradient(${displayColor} ${fillDeg}deg, rgba(30, 41, 59, 0.32) 0deg)`;
                    radialPainted = true;
                }
                if (active !== radialCache[`${prefix}Visible`]) {
                    widgetDiv.classList.toggle('radial-widget--visible', active);
                    radialPainted = true;
                }
                if (readyVisible !== radialCache[`${prefix}ReadyVisible`]) {
                    widgetDiv.classList.toggle('radial-widget--ready', readyVisible);
                    radialPainted = true;
                }
                if (label !== radialCache[`${prefix}Label`]) labelSpan.textContent = label;
                radialCache[`${prefix}Visible`] = active;
                radialCache[`${prefix}Deg`] = deg;
                radialCache[`${prefix}Label`] = label;
                radialCache[`${prefix}ReadyVisible`] = readyVisible;
                radialCache[`${prefix}LastRemaining`] = remainingSec;
                return active || readyVisible;
            };

            const dashRemain = Math.max(
                0,
                Number(ctx.localPlayerState.ability_1_cooldown_remaining ?? ctx.localPlayerState.dash_cooldown_remaining) || 0
            );
            const dodgeRemain = Math.max(
                0,
                Number(ctx.localPlayerState.ability_2_cooldown_remaining ?? ctx.localPlayerState.dodge_cooldown_remaining) || 0
            );
            const dashVisible = updateCooldownWidget(
                'dash',
                ctx.dashRadialDiv,
                ctx.dashRadialLabelSpan,
                dashRemain,
                6,
                'Dash',
                'rgba(96, 165, 250, 0.96)'
            );
            const dodgeVisible = updateCooldownWidget(
                'dodge',
                ctx.dodgeRadialDiv,
                ctx.dodgeRadialLabelSpan,
                dodgeRemain,
                9,
                'Dodge',
                'rgba(16, 185, 129, 0.96)'
            );

            const anyRadialVisible = reloadActive || abilityActive || dashVisible || dodgeVisible;
            if (anyRadialVisible !== radialCache.hudVisible) {
                ctx.combatRadialHudDiv.style.opacity = anyRadialVisible ? '1' : '0';
                radialCache.hudVisible = anyRadialVisible;
                radialPainted = true;
            }
            if (radialPainted || allowRadialPaint) radialCache.lastPaintAt = currentTime;
        }
    }

    function onConnectionReset() {
        const ctx = getCtx();
        lastLocalDamageImpactAt = 0;
        lastModeIntroActive = false;
        streakPingCooldownByPlayer.clear();
        hideObjectiveArrows();
        clearDamageDirectionIndicators();

        if (ctx.tipsToastDiv) {
            ctx.tipsToastDiv.classList.remove('tips-toast--visible');
            ctx.tipsToastDiv.textContent = '';
        }
        if (ctx.gameModeIntroDiv) {
            ctx.gameModeIntroDiv.classList.remove('mode-intro--visible');
        }
        if (ctx.boundaryWarningDiv) {
            ctx.boundaryWarningDiv.classList.remove('boundary-warning--visible');
        }
        if (ctx.objectiveUrgencyDiv) {
            ctx.objectiveUrgencyDiv.classList.remove(
                'objective-urgency--visible',
                'objective-urgency--critical',
                'objective-urgency--positive'
            );
            ctx.objectiveUrgencyDiv.textContent = '';
        }
        ctx.combatBannerDiv?.classList.remove(
            'combat-banner--visible',
            'combat-banner--kill',
            'combat-banner--headshot',
            'combat-banner--death',
            'combat-banner--shutdown',
            'combat-banner--revenge',
            'combat-banner--momentum'
        );
        ctx.streakAnnouncerDiv?.classList.remove(
            'streak-announcer--visible',
            'streak-announcer--critical',
            'streak-announcer--positive'
        );
        ctx.streakMedalDiv?.classList.remove('streak-medal--visible');
        ctx.hitMarkerDiv?.classList.remove('hit-marker--visible', 'hit-marker--headshot', 'hit-marker--kill');
        ctx.deathRecapDiv?.classList.remove('death-recap--visible');
        clearDeathRecapMinimap();
        if (ctx.combatRadialHudDiv) {
            ctx.combatRadialHudDiv.style.opacity = '0';
        }
        ctx.dashRadialDiv?.classList.remove('radial-widget--visible', 'radial-widget--ready');
        ctx.dodgeRadialDiv?.classList.remove('radial-widget--visible', 'radial-widget--ready');
        ctx.abilityRadialDiv?.classList.remove('radial-widget--visible', 'radial-widget--ready');
        ctx.reloadRadialDiv?.classList.remove('radial-widget--visible', 'radial-widget--ready');

        ctx.combatUiState.momentum = 0;
        ctx.combatUiState.speedPulse = 0;
        ctx.combatUiState.damagePulse = 0;
        ctx.combatUiState.localKillStreak = 0;
        ctx.combatUiState.comboCount = 0;
        ctx.combatUiState.comboExpiresAt = 0;
        ctx.combatUiState.bannerUntilMs = 0;
        ctx.combatUiState.bannerTone = 'kill';
        ctx.combatUiState.medalText = '';
        ctx.combatUiState.medalUntilMs = 0;
        ctx.combatUiState.streakAnnouncerText = '';
        ctx.combatUiState.streakAnnouncerTone = 'critical';
        ctx.combatUiState.streakAnnouncerUntilMs = 0;
        ctx.combatUiState.markerUntilMs = 0;
        ctx.combatUiState.markerHeadshotUntilMs = 0;
        ctx.combatUiState.markerKillUntilMs = 0;
        ctx.combatUiState.lastOptimisticHitAt = 0;
        ctx.combatUiState.hitstopUntilMs = 0;
        ctx.combatUiState.tipUntilMs = 0;
        ctx.combatUiState.modeIntroUntilMs = 0;
        ctx.combatUiState.lastBoundaryTipAt = 0;
        ctx.combatUiState.objectiveText = '';
        ctx.combatUiState.objectiveTone = 'critical';
        ctx.combatUiState.objectiveUntilMs = 0;
        ctx.combatUiState.lastObjectiveEvalAt = 0;
        ctx.combatUiState.damageIndicators.length = 0;
        ctx.combatUiState.recentDamageSources.length = 0;
        ctx.combatUiState.deathRecapUntilMs = 0;
        ctx.combatUiState.deathRecapText = '';
        ctx.combatUiState.deathRecapDistanceText = '';
        ctx.combatUiState.deathRecapRows = [];
        ctx.combatUiState.trackedSpeedBoostMaxSec = 0;
        ctx.combatUiState.trackedDamageBoostMaxSec = 0;
        ctx.combatUiState.lastKnownHealth = null;
        ctx.combatUiState.processedKillFeedKeys.clear();
        ctx.combatUiState.processedKillFeedQueue.length = 0;
        const radialCache = ctx.combatUiState.radialHudCache;
        radialCache.lastPaintAt = 0;
        radialCache.positionMode = '';
        radialCache.left = '';
        radialCache.top = '';
        radialCache.transform = '';
        radialCache.reloadVisible = false;
        radialCache.reloadDeg = -1;
        radialCache.reloadLabel = '';
        radialCache.abilityVisible = false;
        radialCache.abilityDeg = -1;
        radialCache.abilityColor = '';
        radialCache.abilityLabel = '';
        radialCache.dashVisible = false;
        radialCache.dashDeg = -1;
        radialCache.dashLabel = '';
        radialCache.dashReadyVisible = false;
        radialCache.dashReadyUntilMs = 0;
        radialCache.dashLastRemaining = 0;
        radialCache.dodgeVisible = false;
        radialCache.dodgeDeg = -1;
        radialCache.dodgeLabel = '';
        radialCache.dodgeReadyVisible = false;
        radialCache.dodgeReadyUntilMs = 0;
        radialCache.dodgeLastRemaining = 0;
        radialCache.hudVisible = false;
    }

    function destroy() {
        onConnectionReset();
        const ctx = getCtx();
        if (ctx.objectiveArrowLayerDiv) {
            for (let i = 0; i < objectiveArrowPool.length; i += 1) {
                objectiveArrowPool[i]?.el?.remove?.();
            }
        }
        objectiveArrowPool.length = 0;
    }

    return {
        getCombatUiQualityMode,
        getCombatUiPerfTier,
        ensureDamageIndicatorPool,
        clearDamageDirectionIndicators,
        renderDamageDirectionIndicators,
        triggerHaptic,
        playAnnouncerCue,
        triggerHitMarker,
        addDamageDirectionIndicator,
        getStreakAnnouncement,
        showStreakMedal,
        showStreakAnnouncer,
        setObjectiveUrgency,
        updateObjectiveUrgency,
        showDeathRecap,
        registerCombatEventFeedback,
        rememberProcessedKillFeedKey,
        showCombatBanner,
        processKillFeedCombatMoments,
        updateCombatPresentation,
        onConnectionReset,
        destroy,
    };
}
