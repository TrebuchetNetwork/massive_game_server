/**
 * CombatFeedback.js - Combat UI feedback system extracted from client.html
 *
 * Contains damage indicators, hit markers, streak announcements,
 * combat banners, death recap, objective urgency, radial HUD,
 * and all combat presentation logic.
 */

export function createCombatFeedback(getCtx) {
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

    function triggerHitMarker(headshot = false) {
        const ctx = getCtx();
        const now = Date.now();
        ctx.combatUiState.markerUntilMs = now + ctx.COMBAT_HITMARKER_MS;
        if (headshot) {
            ctx.combatUiState.markerHeadshotUntilMs = now + ctx.COMBAT_HEADSHOT_MARKER_MS;
        }
        const hitstopWindowMs = Math.max(10, Number(ctx.COMBAT_HITSTOP_MS) || 0);
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
            ctx.audioManager.playSound('hitMarker', null, 0.22);
        }
        triggerHaptic(headshot ? [12, 12, 20] : 8);
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

    function showStreakMedal(text, durationMs) {
        const ctx = getCtx();
        const duration = durationMs !== undefined ? durationMs : ctx.COMBAT_MEDAL_MS;
        if (!ctx.EXCITEMENT_UI_ENABLED || !ctx.streakMedalDiv || !text) return;
        ctx.combatUiState.medalText = String(text);
        ctx.combatUiState.medalUntilMs = Date.now() + Math.max(700, Number(duration) || ctx.COMBAT_MEDAL_MS);
        ctx.streakMedalDiv.textContent = ctx.combatUiState.medalText;
        ctx.streakMedalDiv.classList.add('streak-medal--visible');
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
        if (event.event_type === ctx.GP.GameEventType.FlagGrabbed) {
            setObjectiveUrgency('Flag stolen - collapse now', 'critical', 1100);
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
            if (isLocal) {
                if (streakCount >= 7) {
                    showCombatBanner('DOMINATING!', 'kill', 1700);
                    setObjectiveUrgency('DOMINATING! 7+ killstreak!', 'critical', 3000);
                } else if (streakCount >= 5) {
                    showCombatBanner('KILLING SPREE!', 'kill', 1500);
                    setObjectiveUrgency('KILLING SPREE! second streak reward active', 'positive', 2500);
                } else if (streakCount >= 3) {
                    showCombatBanner('ON FIRE!', 'kill', 1300);
                    setObjectiveUrgency('Triple kill! first streak reward active', 'positive', 2000);
                }
            } else {
                if (streakCount >= 7) {
                    showCombatBanner(`${killerName} DOMINATING!`, 'kill', 1700);
                    setObjectiveUrgency(`${killerName} is DOMINATING!`, 'critical', 2000);
                } else if (streakCount >= 5) {
                    showCombatBanner(`${killerName} KILLING SPREE!`, 'kill', 1500);
                    setObjectiveUrgency(`${killerName} is on a spree!`, 'critical', 1800);
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
        if (event.event_type === ctx.GP.GameEventType.TeamPing) {
            const instigatorId = typeof event.instigator_id === 'string' ? event.instigator_id : '';
            if (instigatorId && instigatorId !== ctx.myPlayerId) {
                const pingX = Number(event.position?.x);
                const pingY = Number(event.position?.y);
                if (Number.isFinite(pingX) && Number.isFinite(pingY)) {
                    const localTeamId = Number(ctx.localPlayerState?.team_id) || 0;
                    const localCommanderId = ctx.getCommanderIdForTeam(localTeamId);
                    const fromCommander = !!localCommanderId && String(localCommanderId) === String(instigatorId);
                    ctx.tacticalPings.push({
                        kind: fromCommander ? 'defend' : 'group',
                        x: pingX,
                        y: pingY,
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
        if (event.event_type !== ctx.GP.GameEventType.PlayerDamageEffect) return;

        const sourcePlayer = event.instigator_id ? ctx.players.get(event.instigator_id) : null;
        const targetPlayer = event.target_id ? ctx.players.get(event.target_id) : null;

        if (event.target_id === ctx.myPlayerId) {
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
            triggerHaptic([8, 12, 8]);
            return;
        }
        if (event.instigator_id === ctx.myPlayerId && event.target_id && event.target_id !== ctx.myPlayerId) {
            triggerHitMarker(false);
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
                if (entry.is_headshot) {
                    triggerHitMarker(true);
                    showCombatBanner('Headshot', 'headshot', 1100);
                    showStreakMedal('Headshot');
                    setObjectiveUrgency('CRIT +50', 'positive', 850);
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
                triggerHaptic([20, 20, 16]);
            }
        });
    }

    function updateCombatPresentation(currentTime, deltaSeconds) {
        const ctx = getCtx();
        if (
            !ctx.EXCITEMENT_UI_ENABLED || !ctx.combatOverlayDiv || !ctx.damageFlashLayerDiv ||
            !ctx.speedLinesLayerDiv || !ctx.damageDirectionLayerDiv || !ctx.hitMarkerDiv ||
            !ctx.streakMedalDiv || !ctx.objectiveUrgencyDiv || !ctx.combatRadialHudDiv ||
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
            ctx.streakMedalDiv.classList.remove('streak-medal--visible');
            ctx.objectiveUrgencyDiv.classList.remove('objective-urgency--visible');
            ctx.hitMarkerDiv.classList.remove('hit-marker--visible', 'hit-marker--headshot');
            clearDamageDirectionIndicators();
            if (ctx.combatRadialHudDiv) ctx.combatRadialHudDiv.style.opacity = '0';
            if (ctx.combatUiState.radialHudCache) {
                const c = ctx.combatUiState.radialHudCache;
                c.lastPaintAt = 0; c.positionMode = ''; c.left = ''; c.top = ''; c.transform = '';
                c.reloadVisible = false; c.reloadDeg = -1; c.reloadLabel = '';
                c.abilityVisible = false; c.abilityDeg = -1; c.abilityColor = ''; c.abilityLabel = '';
                c.hudVisible = false;
            }
            if (ctx.deathRecapDiv) ctx.deathRecapDiv.classList.remove('death-recap--visible');
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

        const hitMarkerVisible = ctx.combatUiState.markerUntilMs > currentTime;
        ctx.hitMarkerDiv.classList.toggle('hit-marker--visible', hitMarkerVisible);
        ctx.hitMarkerDiv.classList.toggle('hit-marker--headshot', ctx.combatUiState.markerHeadshotUntilMs > currentTime);

        const medalVisible = ctx.combatUiState.medalUntilMs > currentTime && !!ctx.combatUiState.medalText;
        if (medalVisible) ctx.streakMedalDiv.textContent = ctx.combatUiState.medalText;
        ctx.streakMedalDiv.classList.toggle('streak-medal--visible', medalVisible);

        const activeIndicators = ctx.combatUiState.damageIndicators.filter((row) => row && row.expiresAt > currentTime);
        ctx.combatUiState.damageIndicators = activeIndicators;
        renderDamageDirectionIndicators(activeIndicators, currentTime, compactCombatUi);

        updateObjectiveUrgency(currentTime);

        if (ctx.deathRecapDiv) {
            ctx.deathRecapDiv.classList.toggle('death-recap--visible', ctx.combatUiState.deathRecapUntilMs > currentTime);
        }

        if (ctx.combatRadialHudDiv && ctx.abilityRadialDiv && ctx.reloadRadialDiv && ctx.abilityRadialLabelSpan && ctx.reloadRadialLabelSpan) {
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

            const anyRadialVisible = reloadActive || abilityActive;
            if (anyRadialVisible !== radialCache.hudVisible) {
                ctx.combatRadialHudDiv.style.opacity = anyRadialVisible ? '1' : '0';
                radialCache.hudVisible = anyRadialVisible;
                radialPainted = true;
            }
            if (radialPainted || allowRadialPaint) radialCache.lastPaintAt = currentTime;
        }
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
        setObjectiveUrgency,
        updateObjectiveUrgency,
        showDeathRecap,
        registerCombatEventFeedback,
        rememberProcessedKillFeedKey,
        showCombatBanner,
        processKillFeedCombatMoments,
        updateCombatPresentation,
    };
}
