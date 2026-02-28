/**
 * UIManager.js - UI rendering and management extracted from client.html
 *
 * Contains post-match summary, killcam replay, kill feed, chat display,
 * match info HUD, network profiler, game stats, scoreboard, settings,
 * and system event handling.
 *
 * Uses getCtx callback pattern to access shared game state.
 */

export function createUIManager(getCtx) {

    function escapeHtml(unsafe) {
        const raw = String(unsafe ?? '');
        return raw
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;")
            .replace(/"/g, "&quot;")
            .replace(/'/g, "&#039;");
    }

    function toInt(value, fallback = 0) {
        const parsed = Number(value);
        return Number.isFinite(parsed) ? Math.trunc(parsed) : fallback;
    }

    function safeCssColor(value, fallback = '#94A3B8') {
        if (typeof value === 'number' && Number.isFinite(value)) {
            return `#${Math.max(0, value >>> 0).toString(16).padStart(6, '0').slice(-6)}`;
        }
        const normalized = String(value || '').trim();
        if (/^#[0-9a-fA-F]{3}$/.test(normalized) || /^#[0-9a-fA-F]{6}$/.test(normalized)) {
            return normalized;
        }
        return fallback;
    }

    function formatModeName(rawMode) {
        const normalized = String(rawMode || '').trim();
        if (!normalized) return 'Unknown';
        if (normalized === 'FreeForAll') return 'FFA';
        if (normalized === 'TeamDeathmatch') return 'TDM';
        if (normalized === 'CaptureTheFlag') return 'CTF';
        return normalized;
    }

    function closePostMatchSummary() {
        const ctx = getCtx();
        ctx.postMatchSummaryVisible = false;
        if (ctx.postMatchPanelDiv) {
            ctx.postMatchPanelDiv.classList.remove('post-match-panel--visible');
        }
    }

    function renderPostMatchSummary(summaryPayload) {
        const ctx = getCtx();
        if (!summaryPayload || typeof summaryPayload !== 'object') return;
        if (!ctx.postMatchPanelDiv || !ctx.postMatchMetaDiv || !ctx.postMatchMvpDiv || !ctx.postMatchTableDiv) return;

        const generatedAt = Number(summaryPayload.generated_at_ms || summaryPayload.generatedAtMs || 0);
        const summarySignature = `${generatedAt}:${summaryPayload.reason || ''}:${summaryPayload.match_duration || 0}:${(summaryPayload.players || []).length}`;
        if (summarySignature === ctx.postMatchSummarySignature && ctx.postMatchSummaryVisible) {
            return;
        }
        ctx.postMatchSummarySignature = summarySignature;

        const modeName = formatModeName(summaryPayload.game_mode || summaryPayload.gameMode || 'Unknown');
        const reasonRaw = String(summaryPayload.reason || 'match_complete').replace(/_/g, ' ');
        const durationSec = Math.max(0, Number(summaryPayload.match_duration || summaryPayload.matchDuration || 0));
        const minutes = Math.floor(durationSec / 60);
        const seconds = Math.floor(durationSec % 60);
        const winnerTeam = Number(summaryPayload.winning_team || summaryPayload.winningTeam || 0);
        const winnerText = winnerTeam > 0 ? `Winner: Team ${winnerTeam}` : 'Winner: Draw / FFA';
        const reasonText = reasonRaw.charAt(0).toUpperCase() + reasonRaw.slice(1);
        ctx.postMatchMetaDiv.textContent = `${modeName} \u00b7 ${winnerText} \u00b7 ${minutes}:${seconds.toString().padStart(2, '0')} \u00b7 ${reasonText}`;

        const mvpKills = summaryPayload.mvp_kills || summaryPayload.mvpKills || 'N/A';
        const mvpDamage = summaryPayload.mvp_damage || summaryPayload.mvpDamage || 'N/A';
        const mvpObjectives = summaryPayload.mvp_objectives || summaryPayload.mvpObjectives || 'N/A';
        ctx.postMatchMvpDiv.replaceChildren();
        [
            ['Most Kills:', mvpKills],
            ['Most Damage:', mvpDamage],
            ['Most Objective:', mvpObjectives],
        ].forEach(([label, value], index) => {
            if (index > 0) ctx.postMatchMvpDiv.appendChild(document.createTextNode(' '));
            const award = document.createElement('span');
            award.className = 'mvp-award';
            const boldLabel = document.createElement('b');
            boldLabel.textContent = String(label);
            award.appendChild(boldLabel);
            award.appendChild(document.createTextNode(` ${String(value)}`));
            ctx.postMatchMvpDiv.appendChild(award);
        });

        const playersRows = Array.isArray(summaryPayload.players) ? summaryPayload.players : [];
        ctx.postMatchTableDiv.replaceChildren();
        if (playersRows.length === 0) {
            const empty = document.createElement('div');
            empty.className = 'post-match-panel__empty';
            empty.textContent = 'No match stats available.';
            ctx.postMatchTableDiv.appendChild(empty);
        } else {
            const table = document.createElement('table');
            table.className = 'post-match-table';
            const thead = document.createElement('thead');
            const headerRow = document.createElement('tr');
            ['Player', 'Team', 'Score', 'K/D', 'Dmg', 'KD'].forEach((title) => {
                const th = document.createElement('th');
                th.textContent = title;
                headerRow.appendChild(th);
            });
            thead.appendChild(headerRow);
            table.appendChild(thead);

            const tbody = document.createElement('tbody');
            playersRows.slice(0, 10).forEach((row) => {
                const tr = document.createElement('tr');
                const name = String(row?.player_name || row?.playerName || 'Unknown');
                const team = Number(row?.team_id || row?.teamId || 0);
                const kills = Number(row?.kills || 0) | 0;
                const deaths = Number(row?.deaths || 0) | 0;
                const score = Number(row?.score || 0) | 0;
                const damage = Number(row?.damage_dealt || row?.damageDealt || 0) | 0;
                const kd = Number(row?.kd_ratio || row?.kdRatio || 0);
                [name, team || '-', score, `${kills}/${deaths}`, damage, kd.toFixed(2)].forEach((cellValue) => {
                    const td = document.createElement('td');
                    td.textContent = String(cellValue);
                    tr.appendChild(td);
                });
                tbody.appendChild(tr);
            });
            table.appendChild(tbody);
            ctx.postMatchTableDiv.appendChild(table);
        }

        // Weapon breakdown for local player
        const weaponBreakdownDiv = document.getElementById('postMatchWeaponBreakdown');
        if (weaponBreakdownDiv) {
            weaponBreakdownDiv.replaceChildren();
            const localRow = playersRows.find(r => {
                const pid = r?.player_id || r?.playerId;
                return pid && pid === ctx.myPlayerId;
            });
            const weaponKills = localRow?.weapon_kills || localRow?.weaponKills || {};
            const weaponEntries = Object.entries(weaponKills);
            if (weaponEntries.length > 0) {
                const totalKills = weaponEntries.reduce((sum, [, k]) => sum + Number(k), 0) || 1;
                const label = document.createElement('div');
                label.className = 'weapon-breakdown-label';
                label.textContent = 'Weapon Kills';
                weaponBreakdownDiv.appendChild(label);

                const barsContainer = document.createElement('div');
                barsContainer.className = 'weapon-breakdown-bars';
                weaponEntries.forEach(([wName, k]) => {
                    const pct = Math.round((Number(k) / totalKills) * 100);
                    const bar = document.createElement('div');
                    bar.className = 'weapon-bar';
                    bar.style.flex = String(Math.max(1, pct));
                    bar.style.background = safeCssColor(ctx.weaponColors[wName]);
                    bar.title = `${String(wName)}: ${k} kills (${pct}%)`;
                    bar.textContent = pct > 10 ? String(wName) : '';
                    barsContainer.appendChild(bar);
                });
                weaponBreakdownDiv.appendChild(barsContainer);
            }
        }

        // K/D trend across matches (localStorage)
        const trendDiv = document.getElementById('postMatchTrend');
        if (trendDiv) {
            trendDiv.replaceChildren();
            const localRow = playersRows.find(r => (r?.player_id || r?.playerId) === ctx.myPlayerId);
            if (localRow) {
                const kd = Number(localRow?.kd_ratio || localRow?.kdRatio || 0);
                let history = [];
                try { history = JSON.parse(localStorage.getItem('kdTrend') || '[]'); } catch(e) {}
                history.push({ kd: Math.round(kd * 100) / 100, ts: Date.now() });
                if (history.length > 20) history = history.slice(-20);
                localStorage.setItem('kdTrend', JSON.stringify(history));
                if (history.length >= 2) {
                    const maxKd = Math.max(...history.map(h => h.kd), 1);
                    const w = 200, h = 60;
                    const step = w / Math.max(history.length - 1, 1);
                    const points = history.map((p, i) => `${(i * step).toFixed(1)},${(h - (p.kd / maxKd) * h * 0.9).toFixed(1)}`).join(' ');
                    const label = document.createElement('div');
                    label.className = 'trend-label';
                    label.textContent = `K/D Trend (last ${history.length} matches)`;
                    trendDiv.appendChild(label);

                    const svgNs = 'http://www.w3.org/2000/svg';
                    const svg = document.createElementNS(svgNs, 'svg');
                    svg.setAttribute('width', String(w));
                    svg.setAttribute('height', String(h));
                    svg.setAttribute('viewBox', `0 0 ${w} ${h}`);
                    svg.style.display = 'block';
                    const polyline = document.createElementNS(svgNs, 'polyline');
                    polyline.setAttribute('points', points);
                    polyline.setAttribute('fill', 'none');
                    polyline.setAttribute('stroke', '#60A5FA');
                    polyline.setAttribute('stroke-width', '2');
                    svg.appendChild(polyline);
                    trendDiv.appendChild(svg);
                }
            }
        }

        ctx.postMatchSummaryVisible = true;
        ctx.postMatchPanelDiv.classList.add('post-match-panel--visible');
    }

    function clearKillcamPlayback() {
        const ctx = getCtx();
        ctx.activeKillcamPlayback = null;
        if (ctx.killcamPanelDiv) {
            ctx.killcamPanelDiv.classList.remove('killcam-panel--visible');
        }
        if (ctx.killcamProgressDiv) {
            ctx.killcamProgressDiv.style.width = '0%';
        }
        if (ctx.killcamCanvasCtx && ctx.killcamCanvas) {
            ctx.killcamCanvasCtx.clearRect(0, 0, ctx.killcamCanvas.width, ctx.killcamCanvas.height);
        }
    }

    function startKillcamPlayback(payload) {
        const ctx = getCtx();
        if (!payload || typeof payload !== 'object') {
            clearKillcamPlayback();
            return;
        }
        const rawSamples = Array.isArray(payload.samples) ? payload.samples : [];
        const samples = rawSamples
            .map((sample) => ({
                x: Number(sample?.x),
                y: Number(sample?.y),
                timestamp_ms: Number(sample?.timestamp_ms || sample?.timestampMs || 0),
                shooting: !!sample?.shooting,
            }))
            .filter((sample) => Number.isFinite(sample.x) && Number.isFinite(sample.y));

        if (samples.length < 2 || !ctx.killcamPanelDiv) {
            clearKillcamPlayback();
            return;
        }

        const firstTimestamp = Number(samples[0].timestamp_ms) || 0;
        const lastTimestamp = Number(samples[samples.length - 1].timestamp_ms) || firstTimestamp;
        const timelineSpanMs = Math.max(0, lastTimestamp - firstTimestamp);
        const durationMs = ctx.clamp(
            timelineSpanMs > 10 ? timelineSpanMs : samples.length * 55,
            ctx.KILLCAM_MIN_DURATION_MS,
            ctx.KILLCAM_MAX_DURATION_MS
        );
        ctx.activeKillcamPlayback = {
            startedAtMs: Date.now(),
            durationMs,
            killerName: String(payload.killer_name || payload.killerName || 'Unknown'),
            weapon: String(payload.weapon || 'Unknown'),
            samples,
            firstTimestamp,
            lastTimestamp,
        };

        if (ctx.killcamPanelDiv) {
            ctx.killcamPanelDiv.classList.add('killcam-panel--visible');
        }
        if (ctx.killcamMetaDiv) {
            ctx.killcamMetaDiv.textContent = `By ${ctx.activeKillcamPlayback.killerName} (${ctx.activeKillcamPlayback.weapon})`;
        }
    }

    function renderKillcamCanvas(progress) {
        const ctx = getCtx();
        if (!ctx.activeKillcamPlayback || !ctx.killcamCanvasCtx || !ctx.killcamCanvas) return;
        const samples = ctx.activeKillcamPlayback.samples;
        if (!samples || samples.length < 2) return;

        const canvasWidth = ctx.killcamCanvas.width;
        const canvasHeight = ctx.killcamCanvas.height;
        ctx.killcamCanvasCtx.clearRect(0, 0, canvasWidth, canvasHeight);
        ctx.killcamCanvasCtx.fillStyle = '#0b1220';
        ctx.killcamCanvasCtx.fillRect(0, 0, canvasWidth, canvasHeight);

        const padding = 12;
        let minX = samples[0].x;
        let maxX = samples[0].x;
        let minY = samples[0].y;
        let maxY = samples[0].y;
        for (let i = 1; i < samples.length; i += 1) {
            const sample = samples[i];
            minX = Math.min(minX, sample.x);
            maxX = Math.max(maxX, sample.x);
            minY = Math.min(minY, sample.y);
            maxY = Math.max(maxY, sample.y);
        }
        const spanX = Math.max(40, maxX - minX);
        const spanY = Math.max(40, maxY - minY);
        const scaleX = (canvasWidth - padding * 2) / spanX;
        const scaleY = (canvasHeight - padding * 2) / spanY;
        const scale = Math.min(scaleX, scaleY);
        const offsetX = (canvasWidth - spanX * scale) * 0.5;
        const offsetY = (canvasHeight - spanY * scale) * 0.5;

        const projectPoint = (sample) => ({
            x: offsetX + (sample.x - minX) * scale,
            y: offsetY + (sample.y - minY) * scale,
        });

        ctx.killcamCanvasCtx.lineWidth = 2;
        ctx.killcamCanvasCtx.strokeStyle = '#38bdf8';
        ctx.killcamCanvasCtx.beginPath();
        const firstPoint = projectPoint(samples[0]);
        ctx.killcamCanvasCtx.moveTo(firstPoint.x, firstPoint.y);
        for (let i = 1; i < samples.length; i += 1) {
            const projected = projectPoint(samples[i]);
            ctx.killcamCanvasCtx.lineTo(projected.x, projected.y);
        }
        ctx.killcamCanvasCtx.stroke();

        const samplePosition = progress * (samples.length - 1);
        const sampleFloor = Math.floor(samplePosition);
        const sampleCeil = Math.min(samples.length - 1, sampleFloor + 1);
        const sampleT = ctx.clamp(samplePosition - sampleFloor, 0, 1);
        const startSample = samples[sampleFloor];
        const endSample = samples[sampleCeil];
        const interpSample = {
            x: startSample.x + (endSample.x - startSample.x) * sampleT,
            y: startSample.y + (endSample.y - startSample.y) * sampleT,
        };
        const markerPoint = projectPoint(interpSample);
        ctx.killcamCanvasCtx.fillStyle = '#f87171';
        ctx.killcamCanvasCtx.beginPath();
        ctx.killcamCanvasCtx.arc(markerPoint.x, markerPoint.y, 4, 0, Math.PI * 2);
        ctx.killcamCanvasCtx.fill();
    }

    function updateKillcamReplayUi(currentTimeMs) {
        const ctx = getCtx();
        if (!ctx.activeKillcamPlayback || !ctx.killcamPanelDiv) return;
        if (ctx.localPlayerState && ctx.localPlayerState.alive) {
            clearKillcamPlayback();
            return;
        }
        const elapsedMs = currentTimeMs - ctx.activeKillcamPlayback.startedAtMs;
        if (elapsedMs >= ctx.activeKillcamPlayback.durationMs + ctx.KILLCAM_DISPLAY_GRACE_MS) {
            clearKillcamPlayback();
            return;
        }
        const progress = ctx.clamp(elapsedMs / ctx.activeKillcamPlayback.durationMs, 0, 1);
        if (ctx.killcamProgressDiv) {
            ctx.killcamProgressDiv.style.width = `${Math.round(progress * 100)}%`;
        }
        renderKillcamCanvas(progress);
    }

    function tryExtractSystemEvent(chatPayload) {
        if (!chatPayload) return null;
        const username = typeof chatPayload.username === 'string' ? chatPayload.username : '';
        const message = typeof chatPayload.message === 'string' ? chatPayload.message : '';
        if (username.toLowerCase() !== 'system' || message.length < 2 || message[0] !== '{') {
            return null;
        }
        try {
            const parsed = JSON.parse(message);
            if (!parsed || typeof parsed.event !== 'string') {
                return null;
            }
            return parsed;
        } catch (_) {
            return null;
        }
    }

    function handleSystemEvent(systemEvent) {
        const ctx = getCtx();
        if (!systemEvent || typeof systemEvent.event !== 'string') {
            return false;
        }

        const eventName = String(systemEvent.event);
        const payload = systemEvent.payload;
        if (eventName === 'killcam' && payload && typeof payload === 'object') {
            ctx.latestKillcam = payload;
            const killerName = payload.killer_name || payload.killerName || 'Unknown';
            const weapon = payload.weapon || 'Unknown';
            startKillcamPlayback(payload);
            ctx.setObjectiveUrgency(`Eliminated by ${killerName} (${weapon})`, 'critical', 2200);
            ctx.log(`Killcam: ${killerName} eliminated you with ${weapon}.`, 'warn');
            return true;
        }

        if (eventName === 'match_summary' && payload && typeof payload === 'object') {
            ctx.latestMatchSummary = payload;
            renderPostMatchSummary(payload);
            const winner = Number(payload.winning_team || 0);
            const reason = payload.reason || 'match_complete';
            if (winner > 0) {
                ctx.setObjectiveUrgency(`Match ended (${reason}). Winner: Team ${winner}`, 'info', 6000);
            } else {
                ctx.setObjectiveUrgency(`Match ended (${reason}).`, 'info', 6000);
            }
            ctx.log(`Match summary received (${reason}).`, 'info');
            return true;
        }

        if (eventName === 'mode_transition' && payload && typeof payload === 'object') {
            const phase = String(payload.phase || 'transition');
            const fromMode = formatModeName(payload.from_mode || payload.fromMode);
            const toMode = formatModeName(payload.to_mode || payload.toMode);
            const countdownSeconds = Number(payload.seconds_remaining ?? payload.secondsRemaining);
            if (phase === 'countdown' && Number.isFinite(countdownSeconds) && countdownSeconds > 0) {
                ctx.setObjectiveUrgency(
                    `Mode shift in ${Math.round(countdownSeconds)}s: ${fromMode} -> ${toMode}`,
                    'info',
                    2200
                );
            } else {
                ctx.setObjectiveUrgency(`Mode shifted: ${fromMode} -> ${toMode}`, 'positive', 2600);
            }
            ctx.log(`Mode transition event: ${fromMode} -> ${toMode} (${phase})`, 'info');
            return true;
        }

        return false;
    }

    function updateKillFeed() {
        const ctx = getCtx();
        ctx.processKillFeedCombatMoments(ctx.killFeed);
        const visibleEntries = ctx.killFeed.slice(-5).reverse();
        const signature = visibleEntries
            .map(entry => `${entry.killer_id}:${entry.victim_id}:${entry.weapon}:${entry.timestamp || 0}:${entry.is_headshot ? 1 : 0}`)
            .join('|');
        if (ctx.uiCache.killFeedSignature === signature) {
            return;
        }
        ctx.uiCache.killFeedSignature = signature;

        ctx.killFeedDiv.replaceChildren();
        if (visibleEntries.length === 0) {
            ctx.killFeedDiv.classList.add('hidden');
            return;
        }

        ctx.killFeedDiv.classList.remove('hidden');
        visibleEntries.forEach(entry => {
            const div = document.createElement('div');
            div.className = 'kill-entry';
            const weaponIcon = entry.is_headshot ? '\uD83C\uDFAF' : '';
            const killerColor = ctx.teamColors[ctx.players.get(entry.killer_id)?.team_id] || ctx.teamColors[0];
            const victimColor = ctx.teamColors[ctx.players.get(entry.victim_id)?.team_id] || ctx.teamColors[0];
            const localName = String(ctx.localPlayerState?.username || '').trim();
            const killerMatchesId = entry.killer_id != null && String(entry.killer_id) === String(ctx.myPlayerId);
            const victimMatchesId = entry.victim_id != null && String(entry.victim_id) === String(ctx.myPlayerId);
            const killerMatchesName = !!localName && String(entry.killer_name || '').trim() === localName;
            const victimMatchesName = !!localName && String(entry.victim_name || '').trim() === localName;
            if (killerMatchesId || killerMatchesName) div.classList.add('kill-entry--local');
            if (victimMatchesId || victimMatchesName) div.classList.add('kill-entry--death');

            const killerSpan = document.createElement('span');
            killerSpan.style.color = '#' + killerColor.toString(16).padStart(6, '0');
            killerSpan.textContent = String(entry.killer_name || 'Unknown');

            const weaponSpan = document.createElement('span');
            weaponSpan.style.color = '#A0A0A0';
            weaponSpan.textContent = `[${ctx.weaponNames[entry.weapon] || 'Unknown'}]`;

            const victimSpan = document.createElement('span');
            victimSpan.style.color = '#' + victimColor.toString(16).padStart(6, '0');
            victimSpan.textContent = String(entry.victim_name || 'Unknown');

            div.appendChild(killerSpan);
            div.appendChild(document.createTextNode(' '));
            div.appendChild(weaponSpan);
            div.appendChild(document.createTextNode(' '));
            div.appendChild(victimSpan);
            if (weaponIcon) {
                div.appendChild(document.createTextNode(` ${weaponIcon}`));
            }
            ctx.killFeedDiv.appendChild(div);
        });
    }

    function updateChatDisplay() {
        const ctx = getCtx();
        const visibleMessages = ctx.chatMessages.slice(-10);
        const signature = visibleMessages
            .map(msg => `${msg.seq || 0}:${msg.player_id}:${msg.username}:${msg.message}`)
            .join('|');
        if (ctx.uiCache.chatSignature === signature) {
            return;
        }
        ctx.uiCache.chatSignature = signature;

        ctx.chatDisplayDiv.replaceChildren();
        if (visibleMessages.length === 0) {
            ctx.chatDisplayDiv.classList.add('hidden');
            return;
        }

        ctx.chatDisplayDiv.classList.remove('hidden');
        visibleMessages.forEach(msg => {
            const div = document.createElement('div');
            div.className = 'chat-entry';
            const player = ctx.players.get(msg.player_id);
            const nameColor = player ? (ctx.teamColors[player.team_id] || ctx.teamColors[0]) : ctx.teamColors[0];
            const hexColor = '#' + nameColor.toString(16).padStart(6, '0');
            const username = document.createElement('span');
            username.className = 'username';
            username.style.color = hexColor;
            username.textContent = `${msg.username || 'System'}:`;

            const messageText = document.createTextNode(` ${msg.message || ''}`);
            div.appendChild(username);
            div.appendChild(messageText);
            ctx.chatDisplayDiv.appendChild(div);
        });
        ctx.chatDisplayDiv.scrollTop = ctx.chatDisplayDiv.scrollHeight;
    }

    function updateMatchInfo() {
        const ctx = getCtx();
        if (!ctx.matchInfo) {
            ctx.uiCache.matchInfoSignature = null;
            ctx.matchInfoDiv.classList.add('hidden');
            return;
        }
        const teamScoresSignature = (ctx.matchInfo.team_scores || [])
            .map(ts => `${ts.team_id}:${ts.score}`)
            .join('|');
        const commanderSignature = [
            ctx.matchInfo.team1_commander_id || '',
            ctx.matchInfo.team2_commander_id || '',
            ctx.matchInfo.team1_commander_waypoint
                ? `${Math.round(Number(ctx.matchInfo.team1_commander_waypoint.x) || 0)},${Math.round(Number(ctx.matchInfo.team1_commander_waypoint.y) || 0)}`
                : '',
            ctx.matchInfo.team2_commander_waypoint
                ? `${Math.round(Number(ctx.matchInfo.team2_commander_waypoint.x) || 0)},${Math.round(Number(ctx.matchInfo.team2_commander_waypoint.y) || 0)}`
                : '',
            Number(ctx.matchInfo.team1_commander_attack_bias || 0).toFixed(2),
            Number(ctx.matchInfo.team2_commander_attack_bias || 0).toFixed(2),
        ].join('|');
        const matchSignature = [
            ctx.matchInfo.match_state,
            ctx.matchInfo.game_mode,
            Math.floor(ctx.matchInfo.time_remaining || 0),
            ctx.matchInfo.winner_id || '',
            ctx.matchInfo.winner_name || '',
            ctx.players.size,
            teamScoresSignature,
            commanderSignature
        ].join(':');
        if (ctx.uiCache.matchInfoSignature === matchSignature) {
            return;
        }
        ctx.uiCache.matchInfoSignature = matchSignature;

        ctx.matchInfoDiv.classList.remove('hidden');
        const fragment = document.createDocumentFragment();
        const gameModeName = {
            [ctx.GP.GameModeType.FreeForAll]: "FFA",
            [ctx.GP.GameModeType.TeamDeathmatch]: "TDM",
            [ctx.GP.GameModeType.CaptureTheFlag]: "CTF"
        }[ctx.matchInfo.game_mode] || "Unknown Mode";

        const appendTextRow = (className, text) => {
            const row = document.createElement('div');
            row.className = className;
            row.textContent = text;
            fragment.appendChild(row);
            return row;
        };

        appendTextRow('font-semibold', gameModeName);

        const describeCommander = (teamId) => {
            const commanderId = ctx.getCommanderIdForTeam(teamId);
            if (!commanderId) return 'Unassigned';
            const commanderPlayer = ctx.players.get(commanderId);
            if (commanderPlayer && commanderPlayer.username) {
                return `${commanderPlayer.username}`;
            }
            return `#${String(commanderId).slice(0, 8)}`;
        };
        const describeWaypoint = (teamId) => {
            const waypoint = ctx.getCommanderWaypointForTeam(teamId);
            if (!waypoint || !Number.isFinite(waypoint.x) || !Number.isFinite(waypoint.y)) {
                return 'none';
            }
            return `${Math.round(waypoint.x)}, ${Math.round(waypoint.y)}`;
        };
        const localTeamId = Number(ctx.localPlayerState?.team_id) || 0;
        if (localTeamId === 1 || localTeamId === 2) {
            const roleText = ctx.isLocalTeamCommander() ? 'Commander' : 'Member';
            const waypointText = describeWaypoint(localTeamId);
            const commanderLine = document.createElement('div');
            commanderLine.className = 'commander-line';
            commanderLine.appendChild(document.createTextNode('Role: '));
            const roleSpan = document.createElement('span');
            roleSpan.className = 'commander-line__role';
            roleSpan.textContent = roleText;
            commanderLine.appendChild(roleSpan);
            commanderLine.appendChild(document.createTextNode(' \u00b7 Waypoint: '));
            const waypointSpan = document.createElement('span');
            waypointSpan.className = 'commander-line__waypoint';
            waypointSpan.textContent = waypointText;
            commanderLine.appendChild(waypointSpan);
            fragment.appendChild(commanderLine);
            if (localTeamId === 1 || localTeamId === 2) {
                const ownCommander = describeCommander(localTeamId);
                appendTextRow(
                    'commander-line commander-line--minor',
                    `Team ${localTeamId} commander: ${ownCommander}`
                );
            }
        } else {
            const commander1 = describeCommander(1);
            const commander2 = describeCommander(2);
            appendTextRow(
                'commander-line commander-line--minor',
                `Cmd R:${commander1} \u00b7 Cmd B:${commander2}`
            );
        }

        if (ctx.matchInfo.match_state !== ctx.GP.MatchStateType.Ended && ctx.postMatchSummaryVisible) {
            closePostMatchSummary();
        }

        switch (ctx.matchInfo.match_state) {
            case ctx.GP.MatchStateType.Waiting: {
                const waitingPlayerCount = Math.max(ctx.players.size, ctx.localPlayerState ? 1 : 0);
                if (waitingPlayerCount >= ctx.MIN_PLAYERS_TO_START) {
                    appendTextRow('text-yellow-300', 'Match is initializing...');
                } else {
                    appendTextRow(
                        'text-yellow-400',
                        `Waiting for players... (${toInt(waitingPlayerCount)}/${toInt(ctx.MIN_PLAYERS_TO_START)})`
                    );
                }
                break;
            }
            case ctx.GP.MatchStateType.Active: {
                const timeRemaining = Math.max(0, Number(ctx.matchInfo.time_remaining || 0));
                const minutes = Math.floor(timeRemaining / 60);
                const seconds = Math.floor(timeRemaining % 60);
                appendTextRow('text-white', `Time: ${minutes}:${seconds.toString().padStart(2, '0')}`);
                if (ctx.matchInfo.game_mode === ctx.GP.GameModeType.TeamDeathmatch || ctx.matchInfo.game_mode === ctx.GP.GameModeType.CaptureTheFlag) {
                    const teamScores = document.createElement('div');
                    teamScores.className = 'team-scores';
                    let redScore = 0;
                    let blueScore = 0;
                    if (ctx.matchInfo.team_scores) {
                        ctx.matchInfo.team_scores.forEach(ts => {
                            if (ts.team_id === 1) redScore = toInt(ts.score, 0);
                            if (ts.team_id === 2) blueScore = toInt(ts.score, 0);
                        });
                    }
                    const redSpan = document.createElement('span');
                    redSpan.className = 'team-score team-red';
                    redSpan.textContent = `Red: ${redScore}`;
                    teamScores.appendChild(redSpan);
                    const blueSpan = document.createElement('span');
                    blueSpan.className = 'team-score team-blue';
                    blueSpan.textContent = `Blue: ${blueScore}`;
                    teamScores.appendChild(blueSpan);
                    fragment.appendChild(teamScores);
                }
                break;
            }
            case ctx.GP.MatchStateType.Ended: {
                const statusRow = document.createElement('div');
                statusRow.className = 'text-green-400';
                statusRow.appendChild(document.createTextNode('Match Ended! '));
                const winnerNameRaw = String(ctx.matchInfo.winner_name || '').trim();
                const winnerTeamId = toInt(ctx.matchInfo.winner_id, 0);
                if (winnerNameRaw && winnerNameRaw !== "null") {
                    statusRow.appendChild(document.createTextNode(`Winner: ${winnerNameRaw}`));
                } else if (winnerTeamId === 1 || winnerTeamId === 2) {
                    const teamColorClass = winnerTeamId === 1 ? "team-red" : "team-blue";
                    statusRow.appendChild(document.createTextNode('Winner: '));
                    const winnerTeamSpan = document.createElement('span');
                    winnerTeamSpan.className = teamColorClass;
                    winnerTeamSpan.textContent = `Team ${winnerTeamId}`;
                    statusRow.appendChild(winnerTeamSpan);
                } else {
                    statusRow.appendChild(document.createTextNode("It's a Draw!"));
                }
                fragment.appendChild(statusRow);
                break;
            }
        }
        ctx.matchInfoDiv.replaceChildren(fragment);
    }

    function updateNetworkProfilerUi(currentTimeMs) {
        const ctx = getCtx();
        if (!ctx.networkProfilerDiv) return;

        const elapsedMs = Math.max(1, currentTimeMs - ctx.networkProfilerWindowStartMs);
        if (elapsedMs >= ctx.NETWORK_PROFILER_WINDOW_MS) {
            const elapsedSec = elapsedMs / 1000;
            ctx.networkProfilerBps = ctx.networkProfilerIncomingBytes / elapsedSec;
            ctx.networkProfilerPps = ctx.networkProfilerIncomingPackets / elapsedSec;
            ctx.networkProfilerMps = ctx.networkProfilerIncomingMessages / elapsedSec;
            ctx.networkProfilerUpdateHz = ctx.networkProfilerStateUpdates / elapsedSec;
            ctx.networkProfilerWindowStartMs = currentTimeMs;
            ctx.networkProfilerIncomingBytes = 0;
            ctx.networkProfilerIncomingMessages = 0;
            ctx.networkProfilerIncomingPackets = 0;
            ctx.networkProfilerStateUpdates = 0;
            ctx.networkProfilerFastDeltaPackets = 0;
            ctx.networkProfilerFullParsePackets = 0;
        }

        const shouldShow = !!ctx.gameSettings.showNetworkProfiler;
        ctx.networkProfilerDiv.classList.toggle('hidden', !shouldShow);
        if (!shouldShow) return;

        const snapshotHz = ctx.snapshotIntervalEma > 0 ? (1000 / ctx.snapshotIntervalEma) : 0;
        const channelBuffered = (ctx.dataChannel && ctx.dataChannel.readyState === 'open')
            ? ctx.dataChannel.bufferedAmount
            : 0;
        const text = [
            `rx ${(ctx.networkProfilerBps / 1024).toFixed(1)} KB/s  pkts ${ctx.networkProfilerPps.toFixed(1)}/s  msgs ${ctx.networkProfilerMps.toFixed(1)}/s`,
            `state ${ctx.networkProfilerUpdateHz.toFixed(1)}/s  fast\u0394 ${ctx.networkProfilerFastDeltaPackets}  parse\u0394 ${ctx.networkProfilerFullParsePackets}`,
            `snapshot ${snapshotHz.toFixed(1)}Hz  jitter ${ctx.snapshotJitterEma.toFixed(1)}ms  interp ${ctx.adaptiveInterpolationDelayMs.toFixed(1)}ms`,
            `dc buffered ${channelBuffered}B`
        ].join('\n');

        if (window.__e2e) {
            window.__e2e.networkProfiler = {
                rxBytesPerSec: Number(ctx.networkProfilerBps.toFixed(2)),
                packetsPerSec: Number(ctx.networkProfilerPps.toFixed(2)),
                messagesPerSec: Number(ctx.networkProfilerMps.toFixed(2)),
                stateUpdatesPerSec: Number(ctx.networkProfilerUpdateHz.toFixed(2)),
                snapshotHz: Number(snapshotHz.toFixed(2)),
                snapshotJitterMs: Number(ctx.snapshotJitterEma.toFixed(2)),
                interpolationDelayMs: Number(ctx.adaptiveInterpolationDelayMs.toFixed(2)),
                dataChannelBufferedBytes: channelBuffered,
                fastDeltaPackets: ctx.networkProfilerFastDeltaPackets,
                fullParsePackets: ctx.networkProfilerFullParsePackets
            };
        }

        if (ctx.uiCache.networkProfilerText !== text) {
            ctx.uiCache.networkProfilerText = text;
            ctx.networkProfilerDiv.textContent = text;
        }
    }

    function updateGameStatsUI() {
        const ctx = getCtx();
        if (ctx.myPlayerId && ctx.localPlayerState) {
            ctx.setTextIfChanged(ctx.myPlayerIdSpan, ctx.myPlayerId.substring(0, 8), 'myPlayerId');
            const teamText = ctx.localPlayerState.team_id === 1 ? 'Red' :
                ctx.localPlayerState.team_id === 2 ? 'Blue' : (ctx.localPlayerState.team_id === 0 ? 'FFA' : 'None');
            ctx.setTextIfChanged(ctx.playerTeamSpan, teamText, 'teamText');
            const teamClass = ctx.localPlayerState.team_id === 1 ? 'team-red' :
                ctx.localPlayerState.team_id === 2 ? 'team-blue' : (ctx.localPlayerState.team_id === 0 ? 'team-ffa' : '');
            if (ctx.uiCache.teamClass !== teamClass) {
                ctx.uiCache.teamClass = teamClass;
                ctx.playerTeamSpan.className = teamClass;
            }
            const commanderRoleText = ctx.isLocalTeamCommander() ? 'Commander' : 'Member';
            ctx.setTextIfChanged(ctx.commanderRoleSpan, commanderRoleText, 'commanderRole');
            ctx.setTextIfChanged(ctx.playerHealthSpan, ctx.localPlayerState.health, 'health');
            ctx.setTextIfChanged(ctx.playerShieldSpan, ctx.localPlayerState.shield_current, 'shield');
            ctx.setTextIfChanged(ctx.playerAmmoSpan, ctx.localPlayerState.ammo, 'ammo');

            if (ctx.localPlayerState.weapon !== ctx.GP.WeaponType.Melee && ctx.localPlayerState.ammo === 0 && ctx.localPlayerState.reload_progress === -1) {
                ctx.setTextIfChanged(ctx.reloadPromptSpan, ' (Press R to Reload!)', 'reloadText');
            } else if (ctx.localPlayerState.reload_progress !== -1 && ctx.localPlayerState.reload_progress < 1.0) {
                ctx.setTextIfChanged(ctx.reloadPromptSpan, ` (Reloading ${Math.round(ctx.localPlayerState.reload_progress * 100)}%)`, 'reloadText');
            } else {
                ctx.setTextIfChanged(ctx.reloadPromptSpan, '', 'reloadText');
            }

            ctx.setTextIfChanged(ctx.playerWeaponSpan, ctx.weaponNames[ctx.localPlayerState.weapon] || 'Unknown', 'weapon');
            ctx.setTextIfChanged(ctx.playerScoreSpan, ctx.localPlayerState.score, 'score');
            ctx.setTextIfChanged(ctx.playerKillsSpan, ctx.localPlayerState.kills, 'kills');
            ctx.setTextIfChanged(ctx.playerDeathsSpan, ctx.localPlayerState.deaths, 'deaths');

            const speedBoostSeconds = Math.max(0, Math.ceil(Number(ctx.localPlayerState.speed_boost_remaining) || 0));
            const damageBoostSeconds = Math.max(0, Math.ceil(Number(ctx.localPlayerState.damage_boost_remaining) || 0));
            const powerupSignature = `${speedBoostSeconds}:${damageBoostSeconds}`;
            if (ctx.uiCache.powerupsSignature !== powerupSignature) {
                ctx.uiCache.powerupsSignature = powerupSignature;
                ctx.powerupStatusDiv.replaceChildren();

                const appendPowerupIndicator = (iconText, label) => {
                    const indicator = document.createElement('div');
                    indicator.className = 'powerup-indicator';
                    const icon = document.createElement('span');
                    icon.className = 'icon';
                    icon.textContent = iconText;
                    indicator.appendChild(icon);
                    indicator.appendChild(document.createTextNode(` ${label}`));
                    ctx.powerupStatusDiv.appendChild(indicator);
                };

                if (speedBoostSeconds > 0) {
                    appendPowerupIndicator('\uD83C\uDFC3', `Speed: ${speedBoostSeconds}s`);
                }
                if (damageBoostSeconds > 0) {
                    appendPowerupIndicator('\uD83D\uDCAA', `Damage: ${damageBoostSeconds}s`);
                }
            }
        }
        ctx.setTextIfChanged(ctx.playerCountSpan, ctx.players.size, 'playerCount');
        ctx.setTextIfChanged(ctx.pingDisplay, Math.round(ctx.ping), 'ping');
        if (ctx.networkIndicator) ctx.networkIndicator.update(ctx.ping);
        updateNetworkProfilerUi(performance.now());

        if (ctx.healthVignette && ctx.localPlayerState) {
            const healthPercent = ctx.localPlayerState.health / ctx.localPlayerState.max_health;
            ctx.updateHealthVignette(ctx.healthVignette, healthPercent, ctx.frameNowMs);
        }
    }

    function toggleScoreboard(forceShow = null) {
        const ctx = getCtx();
        if (forceShow === true) {
            ctx.scoreboardDiv.classList.remove('hidden');
        } else if (forceShow === false) {
            ctx.scoreboardDiv.classList.add('hidden');
        } else {
            ctx.scoreboardDiv.classList.toggle('hidden');
        }
        if (!ctx.scoreboardDiv.classList.contains('hidden')) {
            updateScoreboard();
        }
    }

    function updateScoreboard() {
        const ctx = getCtx();
        if (!ctx.matchInfo || ctx.scoreboardDiv.classList.contains('hidden')) return;

        const sortedPlayers = Array.from(ctx.players.values()).sort((a, b) => b.score - a.score);
        const scoreboardContentDiv = document.getElementById('scoreboardContent');

        const ffaScoreboardSection = document.getElementById('ffaScoreboardSection');
        const teamScoreboardSection = document.getElementById('teamScoreboardSection');
        const ffaPlayersTableBody = document.getElementById('ffaPlayersTable').getElementsByTagName('tbody')[0];
        const redTeamPlayersTableBody = document.getElementById('redTeamPlayers').getElementsByTagName('tbody')[0];
        const blueTeamPlayersTableBody = document.getElementById('blueTeamPlayers').getElementsByTagName('tbody')[0];

        if (ctx.matchInfo.game_mode === ctx.GP.GameModeType.FreeForAll) {
            ffaScoreboardSection.classList.remove('hidden');
            teamScoreboardSection.classList.add('hidden');
            scoreboardContentDiv.classList.remove('two-columns');
            ffaPlayersTableBody.replaceChildren();
            sortedPlayers.forEach((p, index) => {
                const row = ffaPlayersTableBody.insertRow();
                row.insertCell().textContent = index + 1;
                row.insertCell().textContent = p.username;
                row.insertCell().textContent = p.score;
                row.insertCell().textContent = p.kills;
                row.insertCell().textContent = p.deaths;
            });
        } else {
            ffaScoreboardSection.classList.add('hidden');
            teamScoreboardSection.classList.remove('hidden');
            scoreboardContentDiv.classList.add('two-columns');
            redTeamPlayersTableBody.replaceChildren();
            blueTeamPlayersTableBody.replaceChildren();

            let redScore = 0, blueScore = 0;
            if (ctx.matchInfo.team_scores) {
                ctx.matchInfo.team_scores.forEach(ts => {
                    if (ts.team_id === 1) redScore = ts.score;
                    if (ts.team_id === 2) blueScore = ts.score;
                });
            }
            document.getElementById('scoreboardTeamRedScore').textContent = redScore;
            document.getElementById('scoreboardTeamBlueScore').textContent = blueScore;

            sortedPlayers.forEach(p => {
                const tableBody = p.team_id === 1 ? redTeamPlayersTableBody : (p.team_id === 2 ? blueTeamPlayersTableBody : null);
                if (tableBody) {
                    const row = tableBody.insertRow();
                    row.insertCell().textContent = p.username;
                    row.insertCell().textContent = p.score;
                    row.insertCell().textContent = p.kills;
                    row.insertCell().textContent = p.deaths;
                }
            });
        }
    }

    function saveAndApplySettings() {
        const ctx = getCtx();
        ctx.gameSettings.soundEnabled = document.getElementById('soundEnabled').checked;
        ctx.gameSettings.soundVolume = document.getElementById('soundVolume').value / 100;
        ctx.gameSettings.musicEnabled = document.getElementById('musicEnabled').checked;
        ctx.gameSettings.musicVolume = document.getElementById('musicVolume').value / 100;
        ctx.gameSettings.graphicsQuality = document.getElementById('graphicsQuality').value;
        ctx.gameSettings.particleEffects = document.getElementById('particleEffects').checked;
        ctx.gameSettings.screenShake = document.getElementById('screenShake').checked;
        ctx.gameSettings.showFPS = document.getElementById('showFPS').checked;
        ctx.gameSettings.showNetworkProfiler = !!ctx.showNetworkProfilerCheckbox?.checked;
        ctx.gameSettings.combatUiQuality = ctx.normalizeCombatUiQuality(ctx.combatUiQualitySelect?.value);
        ctx.gameSettings.sensitivity = parseFloat(document.getElementById('sensitivity').value);
        ctx.gameSettings.mobileStickyFire = !!ctx.mobileStickyFireCheckbox?.checked;
        ctx.gameSettings.mobileAutoFireAim = !!ctx.mobileAutoFireAimCheckbox?.checked;
        ctx.gameSettings.mobileHaptics = !!ctx.mobileHapticsCheckbox?.checked;
        ctx.gameSettings.showDestroyedWallDebug = false;
        ctx.applyTournamentPresetSettings();
        ctx.applyEffectsProfile();

        if (ctx.audioManager) {
            ctx.audioManager.setGlobalVolume(ctx.gameSettings.soundVolume);
            ctx.audioManager.setMuted(!ctx.gameSettings.soundEnabled);
        }
        if (ctx.effectsManager) {
            ctx.effectsManager.setPerformanceProfile(ctx.activeEffectsProfileName);
        }
        ctx.syncParticlesBudget();
        if (window.__e2e) {
            window.__e2e.effectsProfile = ctx.activeEffectsProfileName;
        }

        ctx.fpsCounterDiv.classList.toggle('hidden', !ctx.gameSettings.showFPS);
        if (!ctx.gameSettings.mobileStickyFire && ctx.mobileStickyFireArmed) {
            ctx.mobileStickyFireArmed = false;
            if (!ctx.mobileFireTouchActive && !ctx.mobileAimActive) {
                ctx.inputState.shooting = false;
            }
        }
        ctx.syncMobileFireButtonState();
        ctx.updateMobileButtonSizing();
        ctx.combatUiState.radialHudCache.lastPaintAt = 0;

        localStorage.setItem('gameSettings', JSON.stringify(ctx.gameSettings));
        ctx.log('Settings saved.', 'success');
        ctx.settingsMenuDiv.classList.add('hidden');
    }

    function loadSettings() {
        const ctx = getCtx();
        const storedSettings = localStorage.getItem('gameSettings');
        if (storedSettings) {
            Object.assign(ctx.gameSettings, JSON.parse(storedSettings));
            ctx.log('Settings loaded from localStorage.', 'info');
        }
        ctx.gameSettings.showDestroyedWallDebug = false;
        ctx.gameSettings.combatUiQuality = ctx.normalizeCombatUiQuality(ctx.gameSettings.combatUiQuality);
        ctx.applyMobilePresetSettings();
        ctx.applyTournamentPresetSettings();
        ctx.applyStablePresetSettings();
        ctx.applyBenchPresetSettings();
        if (ctx.COMBAT_UI_QUALITY_OVERRIDE) {
            ctx.gameSettings.combatUiQuality = ctx.COMBAT_UI_QUALITY_OVERRIDE;
        }
        ctx.applyEffectsProfile();

        // Apply loaded settings to DOM
        document.getElementById('soundEnabled').checked = ctx.gameSettings.soundEnabled;
        document.getElementById('soundVolume').value = ctx.gameSettings.soundVolume * 100;
        document.getElementById('soundVolumeValue').textContent = ctx.gameSettings.soundVolume * 100 + '%';
        document.getElementById('musicEnabled').checked = ctx.gameSettings.musicEnabled;
        document.getElementById('musicVolume').value = ctx.gameSettings.musicVolume * 100;
        document.getElementById('musicVolumeValue').textContent = ctx.gameSettings.musicVolume * 100 + '%';
        document.getElementById('graphicsQuality').value = ctx.gameSettings.graphicsQuality;
        document.getElementById('particleEffects').checked = ctx.gameSettings.particleEffects;
        document.getElementById('screenShake').checked = ctx.gameSettings.screenShake;
        document.getElementById('showFPS').checked = ctx.gameSettings.showFPS;
        if (ctx.showNetworkProfilerCheckbox) {
            ctx.showNetworkProfilerCheckbox.checked = !!ctx.gameSettings.showNetworkProfiler;
        }
        if (ctx.combatUiQualitySelect) ctx.combatUiQualitySelect.value = ctx.normalizeCombatUiQuality(ctx.gameSettings.combatUiQuality);
        document.getElementById('sensitivity').value = ctx.gameSettings.sensitivity;
        document.getElementById('sensitivityValue').textContent = ctx.gameSettings.sensitivity.toFixed(1);
        if (ctx.mobileStickyFireCheckbox) ctx.mobileStickyFireCheckbox.checked = !!ctx.gameSettings.mobileStickyFire;
        if (ctx.mobileAutoFireAimCheckbox) ctx.mobileAutoFireAimCheckbox.checked = !!ctx.gameSettings.mobileAutoFireAim;
        if (ctx.mobileHapticsCheckbox) ctx.mobileHapticsCheckbox.checked = !!ctx.gameSettings.mobileHaptics;

        if (ctx.audioManager) {
            ctx.audioManager.setGlobalVolume(ctx.gameSettings.soundVolume);
            ctx.audioManager.setMuted(!ctx.gameSettings.soundEnabled);
        }
        if (ctx.effectsManager) {
            ctx.effectsManager.setPerformanceProfile(ctx.activeEffectsProfileName);
        }
        ctx.syncParticlesBudget();
        if (window.__e2e) {
            window.__e2e.effectsProfile = ctx.activeEffectsProfileName;
        }
        ctx.fpsCounterDiv.classList.toggle('hidden', !ctx.gameSettings.showFPS);
        if (!ctx.gameSettings.mobileStickyFire) {
            ctx.mobileStickyFireArmed = false;
        }
        ctx.syncMobileFireButtonState();
        ctx.updateMobileButtonSizing();
        ctx.combatUiState.radialHudCache.lastPaintAt = 0;
    }

    function toggleSettings() {
        const ctx = getCtx();
        ctx.settingsMenuDiv.classList.toggle('hidden');
    }

    return {
        escapeHtml,
        formatModeName,
        closePostMatchSummary,
        renderPostMatchSummary,
        clearKillcamPlayback,
        startKillcamPlayback,
        renderKillcamCanvas,
        updateKillcamReplayUi,
        tryExtractSystemEvent,
        handleSystemEvent,
        updateKillFeed,
        updateChatDisplay,
        updateMatchInfo,
        updateNetworkProfilerUi,
        updateGameStatsUI,
        toggleScoreboard,
        updateScoreboard,
        saveAndApplySettings,
        loadSettings,
        toggleSettings,
    };
}
