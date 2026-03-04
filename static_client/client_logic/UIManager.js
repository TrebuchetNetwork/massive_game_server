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

    let lastCountdownBeepSecond = null;
    let lastMatchOutcomeAudioSignature = '';
    const POST_MATCH_RECORDS_KEY = 'mgs_post_match_records_v1';
    const POST_MATCH_PERF_HISTORY_KEY = 'mgs_post_match_perf_history_v1';
    const POST_MATCH_PERF_HISTORY_LIMIT = 20;
    const CHAT_MUTED_IDS_KEY = 'mgs_chat_muted_ids_v1';
    const CHAT_MUTED_NAMES_KEY = 'mgs_chat_muted_names_v1';
    const CHAT_BLOCKED_IDS_KEY = 'mgs_chat_blocked_ids_v1';
    const CHAT_BLOCKED_NAMES_KEY = 'mgs_chat_blocked_names_v1';
    const CHAT_REPORT_LOG_KEY = 'mgs_chat_report_log_v1';
    const CHAT_REPORT_LOG_LIMIT = 80;
    let chatModerationRevision = 0;
    const mutedPlayerIds = new Set();
    const mutedNames = new Set();
    const blockedPlayerIds = new Set();
    const blockedNames = new Set();

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

    function parseMvpMetricNumber(rawValue) {
        if (typeof rawValue === 'number' && Number.isFinite(rawValue)) return rawValue;
        const match = String(rawValue ?? '').match(/-?\d+(?:\.\d+)?/);
        if (!match) return null;
        const parsed = Number(match[0]);
        return Number.isFinite(parsed) ? parsed : null;
    }

    function getMvpTier(metricKey, numericValue) {
        if (!Number.isFinite(numericValue)) {
            return { code: 'D', label: 'Entry' };
        }
        const value = Number(numericValue);
        if (metricKey === 'kills') {
            if (value >= 20) return { code: 'S', label: 'Elite' };
            if (value >= 14) return { code: 'A', label: 'Pro' };
            if (value >= 9) return { code: 'B', label: 'Strong' };
            if (value >= 5) return { code: 'C', label: 'Solid' };
            return { code: 'D', label: 'Entry' };
        }
        if (metricKey === 'damage') {
            if (value >= 4500) return { code: 'S', label: 'Elite' };
            if (value >= 3000) return { code: 'A', label: 'Pro' };
            if (value >= 1800) return { code: 'B', label: 'Strong' };
            if (value >= 900) return { code: 'C', label: 'Solid' };
            return { code: 'D', label: 'Entry' };
        }
        if (metricKey === 'objective') {
            if (value >= 6) return { code: 'S', label: 'Elite' };
            if (value >= 4) return { code: 'A', label: 'Pro' };
            if (value >= 2) return { code: 'B', label: 'Strong' };
            if (value >= 1) return { code: 'C', label: 'Solid' };
            return { code: 'D', label: 'Entry' };
        }
        return { code: 'D', label: 'Entry' };
    }

    function loadStoredJson(key, fallbackValue) {
        try {
            const raw = localStorage.getItem(key);
            if (!raw) return fallbackValue;
            const parsed = JSON.parse(raw);
            return parsed && typeof parsed === 'object' ? parsed : fallbackValue;
        } catch (_) {
            return fallbackValue;
        }
    }

    function normalizeChatIdentity(rawValue) {
        return String(rawValue ?? '').trim().toLowerCase();
    }

    function hydrateIdentitySet(targetSet, rawList, normalize = true) {
        targetSet.clear();
        if (!Array.isArray(rawList)) return;
        for (let i = 0; i < rawList.length; i += 1) {
            const rawItem = rawList[i];
            const item = normalize ? normalizeChatIdentity(rawItem) : String(rawItem ?? '').trim();
            if (!item) continue;
            targetSet.add(item);
        }
    }

    function persistIdentitySet(storageKey, targetSet) {
        try {
            localStorage.setItem(storageKey, JSON.stringify(Array.from(targetSet)));
        } catch (_) {}
    }

    function bumpChatModerationRevision() {
        chatModerationRevision = (chatModerationRevision + 1) % 1000000;
    }

    function loadChatModerationState() {
        hydrateIdentitySet(
            mutedPlayerIds,
            loadStoredJson(CHAT_MUTED_IDS_KEY, []),
            false
        );
        hydrateIdentitySet(
            mutedNames,
            loadStoredJson(CHAT_MUTED_NAMES_KEY, []),
            true
        );
        hydrateIdentitySet(
            blockedPlayerIds,
            loadStoredJson(CHAT_BLOCKED_IDS_KEY, []),
            false
        );
        hydrateIdentitySet(
            blockedNames,
            loadStoredJson(CHAT_BLOCKED_NAMES_KEY, []),
            true
        );
        bumpChatModerationRevision();
    }

    function formatChatTimestamp(rawTimestamp) {
        const timestamp = Number(rawTimestamp);
        if (!Number.isFinite(timestamp) || timestamp <= 0) return '';
        const date = new Date(timestamp);
        if (Number.isNaN(date.getTime())) return '';
        const hh = String(date.getHours()).padStart(2, '0');
        const mm = String(date.getMinutes()).padStart(2, '0');
        const ss = String(date.getSeconds()).padStart(2, '0');
        return `${hh}:${mm}:${ss}`;
    }

    function resolveChatMessageIdentity(msg) {
        const playerId = String(msg?.player_id ?? '').trim();
        const normalizedName = normalizeChatIdentity(msg?.username);
        return {
            playerId,
            normalizedName,
        };
    }

    function isChatMessageSuppressed(msg) {
        const { playerId, normalizedName } = resolveChatMessageIdentity(msg);
        if (playerId && (blockedPlayerIds.has(playerId) || mutedPlayerIds.has(playerId))) {
            return true;
        }
        if (normalizedName && (blockedNames.has(normalizedName) || mutedNames.has(normalizedName))) {
            return true;
        }
        return false;
    }

    function persistChatModerationState() {
        persistIdentitySet(CHAT_MUTED_IDS_KEY, mutedPlayerIds);
        persistIdentitySet(CHAT_MUTED_NAMES_KEY, mutedNames);
        persistIdentitySet(CHAT_BLOCKED_IDS_KEY, blockedPlayerIds);
        persistIdentitySet(CHAT_BLOCKED_NAMES_KEY, blockedNames);
    }

    loadChatModerationState();

    function playUiSound(soundName, volume = 0.24) {
        const ctx = getCtx();
        if (!ctx.audioManager || !ctx.gameSettings?.soundEnabled) return;
        ctx.audioManager.playSound(soundName, null, volume);
    }

    function maybePlayCountdownBeep(remainingSecs) {
        const seconds = Math.max(0, Math.ceil(Number(remainingSecs) || 0));
        if (seconds > 10 || seconds <= 0) {
            if (seconds > 10) {
                lastCountdownBeepSecond = null;
            }
            return;
        }
        if (seconds === lastCountdownBeepSecond) return;
        lastCountdownBeepSecond = seconds;
        playUiSound('countdownBeep', seconds <= 3 ? 0.3 : 0.22);
    }

    function maybePlayMatchOutcomeSound(winnerTeamRaw, winnerNameRaw) {
        const ctx = getCtx();
        const winnerTeam = toInt(winnerTeamRaw, 0);
        const winnerName = String(winnerNameRaw || '').trim();
        const signature = `${winnerTeam}:${winnerName || 'none'}`;
        if (signature === lastMatchOutcomeAudioSignature) return;
        lastMatchOutcomeAudioSignature = signature;

        const localTeamId = toInt(ctx.localPlayerState?.team_id, 0);
        const localName = String(ctx.localPlayerState?.username || '').trim().toLowerCase();
        const winnerNameNorm = winnerName.toLowerCase();
        let localWon = false;
        if ((winnerTeam === 1 || winnerTeam === 2) && (localTeamId === 1 || localTeamId === 2)) {
            localWon = winnerTeam === localTeamId;
        } else if (winnerNameNorm && localName) {
            localWon = winnerNameNorm === localName;
        }

        if (localWon) {
            playUiSound('victorySting', 0.42);
            return;
        }

        if (winnerTeam > 0 || winnerNameNorm) {
            playUiSound('defeatSting', 0.4);
        }
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
            { label: 'Most Kills:', value: mvpKills, metricKey: 'kills', shortCode: 'K' },
            { label: 'Most Damage:', value: mvpDamage, metricKey: 'damage', shortCode: 'DMG' },
            { label: 'Most Objective:', value: mvpObjectives, metricKey: 'objective', shortCode: 'OBJ' },
        ].forEach((entry, index) => {
            if (index > 0) ctx.postMatchMvpDiv.appendChild(document.createTextNode(' '));
            const award = document.createElement('span');
            const tier = getMvpTier(entry.metricKey, parseMvpMetricNumber(entry.value));
            award.className = `mvp-award mvp-award--tier-${tier.code.toLowerCase()}`;
            award.title = `${tier.label} tier`;
            const icon = document.createElement('span');
            icon.className = 'mvp-award__icon';
            icon.textContent = entry.shortCode;
            award.appendChild(icon);
            const boldLabel = document.createElement('b');
            boldLabel.textContent = String(entry.label);
            award.appendChild(boldLabel);
            award.appendChild(document.createTextNode(` ${String(entry.value)}`));
            const tierBadge = document.createElement('span');
            tierBadge.className = 'mvp-award__tier';
            tierBadge.textContent = tier.code;
            award.appendChild(tierBadge);
            ctx.postMatchMvpDiv.appendChild(award);
        });

        const playersRows = Array.isArray(summaryPayload.players) ? summaryPayload.players : [];
        const localRow = playersRows.find((row) => {
            const pid = row?.player_id || row?.playerId;
            return pid && pid === ctx.myPlayerId;
        }) || null;
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
            if (localRow) {
                const kills = toInt(localRow?.kills, 0);
                const score = toInt(localRow?.score, 0);
                const damage = toInt(localRow?.damage_dealt || localRow?.damageDealt, 0);
                const kd = Number(localRow?.kd_ratio || localRow?.kdRatio || 0);
                let history = [];
                try { history = JSON.parse(localStorage.getItem('kdTrend') || '[]'); } catch (_) {}
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

                const currentPerf = {
                    kills,
                    score,
                    damage,
                    kd: Number.isFinite(kd) ? Number(kd.toFixed(2)) : 0,
                    ts: Date.now(),
                };
                const storedPerfHistory = loadStoredJson(POST_MATCH_PERF_HISTORY_KEY, []);
                const perfHistory = Array.isArray(storedPerfHistory) ? storedPerfHistory : [];
                const baselineWindow = perfHistory.slice(-10);
                if (baselineWindow.length >= 3) {
                    const avgKills = baselineWindow.reduce((sum, row) => sum + (Number(row?.kills) || 0), 0) / baselineWindow.length;
                    const avgScore = baselineWindow.reduce((sum, row) => sum + (Number(row?.score) || 0), 0) / baselineWindow.length;
                    const avgDamage = baselineWindow.reduce((sum, row) => sum + (Number(row?.damage) || 0), 0) / baselineWindow.length;
                    const avgKd = baselineWindow.reduce((sum, row) => sum + (Number(row?.kd) || 0), 0) / baselineWindow.length;
                    const comparisons = [
                        { label: 'Kills', now: kills, avg: avgKills },
                        { label: 'Score', now: score, avg: avgScore },
                        { label: 'Damage', now: damage, avg: avgDamage },
                        { label: 'K/D', now: currentPerf.kd, avg: avgKd },
                    ];
                    const compareLabel = document.createElement('div');
                    compareLabel.className = 'trend-label';
                    compareLabel.textContent = 'Performance vs last 10';
                    trendDiv.appendChild(compareLabel);

                    const compareGrid = document.createElement('div');
                    compareGrid.className = 'post-match-compare';
                    comparisons.forEach((entry) => {
                        const avg = Number(entry.avg) || 0;
                        const deltaPct = avg > 0 ? ((Number(entry.now) - avg) / avg) * 100 : 0;
                        const item = document.createElement('div');
                        item.className = 'post-match-compare__item';
                        const stateClass = deltaPct >= 2 ? 'up' : (deltaPct <= -2 ? 'down' : 'flat');
                        item.classList.add(`post-match-compare__item--${stateClass}`);
                        const sign = deltaPct > 0 ? '+' : '';
                        item.textContent = `${entry.label} ${sign}${deltaPct.toFixed(0)}%`;
                        compareGrid.appendChild(item);
                    });
                    trendDiv.appendChild(compareGrid);
                }
                perfHistory.push(currentPerf);
                const trimmedHistory = perfHistory.slice(-POST_MATCH_PERF_HISTORY_LIMIT);
                try {
                    localStorage.setItem(POST_MATCH_PERF_HISTORY_KEY, JSON.stringify(trimmedHistory));
                } catch (_) {}

                const defaultRecords = {
                    matches: 0,
                    bestKills: 0,
                    bestScore: 0,
                    bestDamage: 0,
                    bestKd: 0,
                };
                const storedRecords = loadStoredJson(POST_MATCH_RECORDS_KEY, defaultRecords);
                const prevRecords = {
                    bestKills: Number(storedRecords?.bestKills) || 0,
                    bestScore: Number(storedRecords?.bestScore) || 0,
                    bestDamage: Number(storedRecords?.bestDamage) || 0,
                    bestKd: Number(storedRecords?.bestKd) || 0,
                };
                const nextRecords = {
                    matches: Math.max(0, toInt(storedRecords?.matches, 0)) + 1,
                    bestKills: Math.max(prevRecords.bestKills, kills),
                    bestScore: Math.max(prevRecords.bestScore, score),
                    bestDamage: Math.max(prevRecords.bestDamage, damage),
                    bestKd: Math.max(prevRecords.bestKd, currentPerf.kd),
                };
                try {
                    localStorage.setItem(POST_MATCH_RECORDS_KEY, JSON.stringify(nextRecords));
                } catch (_) {}

                const recordLabel = document.createElement('div');
                recordLabel.className = 'trend-label';
                recordLabel.textContent = `Personal Records (${nextRecords.matches} matches)`;
                trendDiv.appendChild(recordLabel);

                const recordGrid = document.createElement('div');
                recordGrid.className = 'post-match-records';
                [
                    {
                        label: 'Best Kills',
                        value: Math.trunc(nextRecords.bestKills),
                        isNew: kills > prevRecords.bestKills,
                    },
                    {
                        label: 'Best Score',
                        value: Math.trunc(nextRecords.bestScore),
                        isNew: score > prevRecords.bestScore,
                    },
                    {
                        label: 'Best Damage',
                        value: Math.trunc(nextRecords.bestDamage),
                        isNew: damage > prevRecords.bestDamage,
                    },
                    {
                        label: 'Best K/D',
                        value: nextRecords.bestKd.toFixed(2),
                        isNew: currentPerf.kd > prevRecords.bestKd,
                    },
                ].forEach((entry) => {
                    const card = document.createElement('div');
                    card.className = 'post-match-record';
                    if (entry.isNew) card.classList.add('post-match-record--new');
                    const title = document.createElement('div');
                    title.className = 'post-match-record__label';
                    title.textContent = entry.label;
                    const value = document.createElement('div');
                    value.className = 'post-match-record__value';
                    value.textContent = String(entry.value);
                    card.appendChild(title);
                    card.appendChild(value);
                    if (entry.isNew) {
                        const tag = document.createElement('div');
                        tag.className = 'post-match-record__tag';
                        tag.textContent = 'NEW';
                        card.appendChild(tag);
                    }
                    recordGrid.appendChild(card);
                });
                trendDiv.appendChild(recordGrid);
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

        if (eventName === 'wall_slam' && payload && typeof payload === 'object') {
            const stunSecs = Math.max(0, Number(payload.stun_secs ?? payload.stunSecs ?? 0));
            const impactSpeed = Math.max(0, Number(payload.impact_speed ?? payload.impactSpeed ?? 0));
            const stunMs = Math.round(stunSecs * 1000);
            if (stunMs > 0) {
                ctx.setObjectiveUrgency(`Wall slam! Stunned for ${stunMs}ms`, 'critical', 1400);
            } else {
                ctx.setObjectiveUrgency('Wall slam!', 'critical', 1100);
            }
            playUiSound('playerHit', 0.24);
            if (typeof navigator !== 'undefined' && typeof navigator.vibrate === 'function') {
                try { navigator.vibrate([14, 12, 16]); } catch (_) {}
            }
            ctx.log(`Wall slam impact registered (speed=${impactSpeed.toFixed(1)}).`, 'warn');
            return true;
        }

        if (eventName === 'match_summary' && payload && typeof payload === 'object') {
            ctx.latestMatchSummary = payload;
            renderPostMatchSummary(payload);
            const winner = Number(payload.winning_team || 0);
            const winnerName = String(payload.winning_name || payload.winner_name || '').trim();
            const reason = payload.reason || 'match_complete';
            if (winner > 0) {
                ctx.setObjectiveUrgency(`Match ended (${reason}). Winner: Team ${winner}`, 'info', 6000);
            } else {
                ctx.setObjectiveUrgency(`Match ended (${reason}).`, 'info', 6000);
            }
            maybePlayMatchOutcomeSound(winner, winnerName);
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
                maybePlayCountdownBeep(countdownSeconds);
            } else {
                ctx.setObjectiveUrgency(`Mode shifted: ${fromMode} -> ${toMode}`, 'positive', 2600);
            }
            ctx.log(`Mode transition event: ${fromMode} -> ${toMode} (${phase})`, 'info');
            return true;
        }

        if (eventName === 'ctf_overtime' && payload && typeof payload === 'object') {
            const overtimeRound = Math.max(1, Math.trunc(Number(payload.round || 1)));
            const durationSecs = Number(payload.duration_secs ?? payload.durationSecs ?? payload.time_remaining ?? 0);
            if (Number.isFinite(durationSecs) && durationSecs > 0) {
                ctx.setObjectiveUrgency(
                    `CTF Overtime (Round ${overtimeRound}) - ${Math.round(durationSecs)}s to win`,
                    'critical',
                    3200
                );
            } else {
                ctx.setObjectiveUrgency(
                    `CTF Overtime (Round ${overtimeRound}) started`,
                    'critical',
                    2800
                );
            }
            ctx.log(`CTF overtime triggered (round ${overtimeRound}).`, 'warn');
            return true;
        }

        if (eventName === 'fortress_phase' && payload && typeof payload === 'object') {
            const phase = String(payload.phase || 'round_start');
            const attackingTeam = Math.max(1, Math.trunc(Number(payload.attacking_team || payload.attackingTeam || 1)));
            const defendingTeam = Math.max(1, Math.trunc(Number(payload.defending_team || payload.defendingTeam || 2)));
            const outcome = String(payload.outcome || '');
            const timeRemaining = Number(payload.time_remaining ?? payload.timeRemaining ?? 0);
            if (phase === 'round_start') {
                if (Number.isFinite(timeRemaining) && timeRemaining > 0) {
                    ctx.setObjectiveUrgency(
                        `Fortress mode: Team ${attackingTeam} attack, Team ${defendingTeam} defend (${Math.round(timeRemaining)}s)`,
                        'critical',
                        3400
                    );
                } else {
                    ctx.setObjectiveUrgency(
                        `Fortress mode: Team ${attackingTeam} attack, Team ${defendingTeam} defend`,
                        'critical',
                        3000
                    );
                }
            } else if (phase === 'round_end' && outcome === 'attackers_captured') {
                ctx.setObjectiveUrgency(`Attackers (Team ${attackingTeam}) breached the fortress`, 'critical', 3200);
            } else if (phase === 'round_end' && outcome === 'defenders_hold') {
                ctx.setObjectiveUrgency(`Defenders (Team ${defendingTeam}) held the fortress`, 'positive', 3200);
            } else {
                ctx.setObjectiveUrgency('Fortress phase updated', 'info', 2200);
            }
            ctx.log(`Fortress phase: ${phase} (${outcome || 'none'}).`, phase === 'round_end' ? 'warn' : 'info');
            return true;
        }

        if (eventName === 'map_event' && payload && typeof payload === 'object') {
            const eventType = String(payload.event_type || payload.eventType || 'map_event');
            const eventIndex = Math.max(1, Math.trunc(Number(payload.event_index || payload.eventIndex || 1)));
            const spawnedPickups = Math.max(0, Math.trunc(Number(payload.spawned_pickups ?? payload.spawnedPickups ?? 0)));
            const nextEventSecs = Number(payload.next_event_secs ?? payload.nextEventSecs ?? 0);
            const bonusMultiplier = Number(payload.bonus_multiplier ?? payload.bonusMultiplier ?? 1);
            const hasHotZoneBonus = eventType === 'hot_zone' && Number.isFinite(bonusMultiplier) && bonusMultiplier > 1;
            const bonusPct = hasHotZoneBonus ? Math.round((bonusMultiplier - 1) * 100) : 0;
            if (eventType === 'hot_zone' && typeof ctx.setHotZoneEvent === 'function') {
                ctx.setHotZoneEvent({
                    event_index: eventIndex,
                    x: Number(payload.x),
                    y: Number(payload.y),
                    radius: Number(payload.radius),
                    bonus_multiplier: Number(payload.bonus_multiplier ?? payload.bonusMultiplier ?? 1),
                    next_event_secs: Number(payload.next_event_secs ?? payload.nextEventSecs ?? 0),
                });
            }
            if (eventType === 'hot_zone') {
                if (Number.isFinite(nextEventSecs) && nextEventSecs > 0) {
                    ctx.setObjectiveUrgency(
                        `Hot zone shifted (+${bonusPct}% points, ${spawnedPickups} pickups). Rotates in ${Math.round(nextEventSecs)}s`,
                        'critical',
                        3400
                    );
                } else {
                    ctx.setObjectiveUrgency(
                        `Hot zone shifted (+${bonusPct}% points, ${spawnedPickups} pickups)`,
                        'critical',
                        3000
                    );
                }
                playUiSound('countdownBeep', 0.24);
            } else {
                const label = eventType === 'center_supply_drop' ? 'Center supply drop' : 'Map event';
                if (Number.isFinite(nextEventSecs) && nextEventSecs > 0) {
                    ctx.setObjectiveUrgency(
                        `${label} active (${spawnedPickups} pickups). Next event in ${Math.round(nextEventSecs)}s`,
                        'critical',
                        3200
                    );
                } else {
                    ctx.setObjectiveUrgency(
                        `${label} active (${spawnedPickups} pickups)`,
                        'critical',
                        2800
                    );
                }
                if (eventType === 'center_supply_drop') {
                    playUiSound('flagFanfare', 0.3);
                } else {
                    playUiSound('countdownBeep', 0.2);
                }
            }

            const pingX = Number(payload.x);
            const pingY = Number(payload.y);
            if (Number.isFinite(pingX) && Number.isFinite(pingY) && Array.isArray(ctx.tacticalPings)) {
                const now = Date.now();
                const pingKindRaw = String(payload.ping_kind || payload.pingKind || 'defend').trim().toLowerCase();
                const pingKind = pingKindRaw === 'enemy' || pingKindRaw === 'defend' ? pingKindRaw : 'group';
                ctx.tacticalPings.push({
                    kind: pingKind,
                    x: pingX,
                    y: pingY,
                    createdAt: now,
                    expiresAt: now + Math.max(2200, Number(ctx.TACTICAL_PING_MS) || 6200),
                });
                if (ctx.tacticalPings.length > 18) {
                    ctx.tacticalPings.splice(0, ctx.tacticalPings.length - 18);
                }
            }

            ctx.log(`Map event #${eventIndex}: ${eventType} (${spawnedPickups} pickups).`, 'info');
            return true;
        }

        if (eventName === 'pickup_spawn_notice' && payload && typeof payload === 'object') {
            const phase = String(payload.phase || 'spawned');
            const pickupLabel = String(
                payload.pickup_label || payload.pickupLabel || payload.pickup_type || payload.pickupType || 'Power-up'
            );
            const secondsRemaining = Number(payload.seconds_remaining ?? payload.secondsRemaining ?? 0);
            const tone = phase === 'countdown' ? 'info' : 'positive';
            if (phase === 'countdown' && Number.isFinite(secondsRemaining) && secondsRemaining > 0) {
                ctx.setObjectiveUrgency(
                    `${pickupLabel} spawning in ${Math.round(secondsRemaining)}s`,
                    tone,
                    2200
                );
                maybePlayCountdownBeep(secondsRemaining);
            } else {
                ctx.setObjectiveUrgency(`${pickupLabel} available now`, tone, 2600);
                playUiSound('powerupCollect', 0.26);
            }

            const pingX = Number(payload.x);
            const pingY = Number(payload.y);
            if (Number.isFinite(pingX) && Number.isFinite(pingY) && Array.isArray(ctx.tacticalPings)) {
                const now = Date.now();
                const fallbackKind = pickupLabel.toLowerCase().includes('weapon') ? 'defend' : 'group';
                const pingKindRaw = String(payload.ping_kind || payload.pingKind || fallbackKind).trim().toLowerCase();
                const pingKind = pingKindRaw === 'enemy' || pingKindRaw === 'defend' ? pingKindRaw : 'group';
                ctx.tacticalPings.push({
                    kind: pingKind,
                    x: pingX,
                    y: pingY,
                    createdAt: now,
                    expiresAt: now + Math.max(1500, Number(ctx.TACTICAL_PING_MS) || 6200),
                });
                if (ctx.tacticalPings.length > 18) {
                    ctx.tacticalPings.splice(0, ctx.tacticalPings.length - 18);
                }
            }

            ctx.log(`Pickup event: ${pickupLabel} (${phase}).`, 'info');
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

    function appendChatReportEntry(msg) {
        const reportEntry = {
            timestamp: Date.now(),
            player_id: String(msg?.player_id ?? ''),
            username: String(msg?.username ?? ''),
            message: String(msg?.message ?? ''),
        };
        const reports = loadStoredJson(CHAT_REPORT_LOG_KEY, []);
        const rows = Array.isArray(reports) ? reports : [];
        rows.push(reportEntry);
        const trimmed = rows.length > CHAT_REPORT_LOG_LIMIT
            ? rows.slice(rows.length - CHAT_REPORT_LOG_LIMIT)
            : rows;
        try {
            localStorage.setItem(CHAT_REPORT_LOG_KEY, JSON.stringify(trimmed));
        } catch (_) {}
    }

    function setChatModerationStateForMessage(msg, mode, enabled) {
        const ctx = getCtx();
        const { playerId, normalizedName } = resolveChatMessageIdentity(msg);
        const actionMode = mode === 'block' ? 'block' : 'mute';
        const nextEnabled = !!enabled;
        if (actionMode === 'block') {
            if (playerId) {
                if (nextEnabled) blockedPlayerIds.add(playerId);
                else blockedPlayerIds.delete(playerId);
            }
            if (normalizedName) {
                if (nextEnabled) blockedNames.add(normalizedName);
                else blockedNames.delete(normalizedName);
            }
        } else {
            if (playerId) {
                if (nextEnabled) mutedPlayerIds.add(playerId);
                else mutedPlayerIds.delete(playerId);
            }
            if (normalizedName) {
                if (nextEnabled) mutedNames.add(normalizedName);
                else mutedNames.delete(normalizedName);
            }
        }
        persistChatModerationState();
        bumpChatModerationRevision();
        const actor = String(msg?.username || 'Player').trim() || 'Player';
        const verb = nextEnabled
            ? (actionMode === 'block' ? 'Blocked' : 'Muted')
            : (actionMode === 'block' ? 'Unblocked' : 'Unmuted');
        if (typeof ctx.setObjectiveUrgency === 'function') {
            ctx.setObjectiveUrgency(`${verb} ${actor} in chat`, 'info', 1100);
        }
        if (typeof ctx.log === 'function') {
            ctx.log(`${verb.toLowerCase()} chat messages from ${actor}.`, 'info');
        }
    }

    function createChatActionButton(label, className, onClick) {
        const button = document.createElement('button');
        button.type = 'button';
        button.className = `chat-entry__action ${className}`;
        button.textContent = label;
        button.addEventListener('click', (event) => {
            event.preventDefault();
            event.stopPropagation();
            onClick();
        });
        return button;
    }

    function updateChatDisplay() {
        const ctx = getCtx();
        const filteredMessages = ctx.chatMessages.filter((msg) => !isChatMessageSuppressed(msg));
        const visibleMessages = filteredMessages.slice(-10);
        const signature = `${chatModerationRevision}:${visibleMessages
            .map(msg => `${msg.seq || 0}:${msg.player_id}:${msg.username}:${msg.message}:${msg.timestamp || 0}`)
            .join('|')}`;
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
        visibleMessages.forEach((msg) => {
            const div = document.createElement('div');
            div.className = 'chat-entry';
            const normalizedName = normalizeChatIdentity(msg?.username);
            const isSystem = normalizedName === 'system';
            const isLocalMessage = String(msg?.player_id ?? '') === String(ctx.myPlayerId ?? '');
            if (isSystem) {
                div.classList.add('chat-entry--system');
            }

            const player = ctx.players.get(msg.player_id);
            const nameColor = player ? (ctx.teamColors[player.team_id] || ctx.teamColors[0]) : ctx.teamColors[0];
            const hexColor = '#' + nameColor.toString(16).padStart(6, '0');

            const timestamp = document.createElement('span');
            timestamp.className = 'chat-entry__ts';
            const timestampText = formatChatTimestamp(msg?.timestamp);
            timestamp.textContent = timestampText ? `[${timestampText}]` : '';

            const content = document.createElement('span');
            content.className = 'chat-entry__content';

            const username = document.createElement('span');
            username.className = 'username';
            username.style.color = hexColor;
            username.textContent = `${msg.username || 'System'}:`;

            const message = document.createElement('span');
            message.className = 'chat-entry__msg';
            message.textContent = String(msg.message || '');

            content.appendChild(username);
            content.appendChild(message);
            div.appendChild(timestamp);
            div.appendChild(content);

            const actionable = !isSystem && !isLocalMessage;
            if (actionable) {
                const actions = document.createElement('span');
                actions.className = 'chat-entry__actions';

                const { playerId } = resolveChatMessageIdentity(msg);
                const muted = (playerId && mutedPlayerIds.has(playerId)) || mutedNames.has(normalizedName);
                const blocked = (playerId && blockedPlayerIds.has(playerId)) || blockedNames.has(normalizedName);

                const muteBtn = createChatActionButton(
                    muted ? 'Unmute' : 'Mute',
                    'chat-entry__action--mute',
                    () => {
                        setChatModerationStateForMessage(msg, 'mute', !muted);
                        updateChatDisplay();
                    }
                );
                const blockBtn = createChatActionButton(
                    blocked ? 'Unblock' : 'Block',
                    'chat-entry__action--block',
                    () => {
                        setChatModerationStateForMessage(msg, 'block', !blocked);
                        updateChatDisplay();
                    }
                );
                const reportBtn = createChatActionButton(
                    'Report',
                    'chat-entry__action--report',
                    () => {
                        appendChatReportEntry(msg);
                        if (typeof ctx.setObjectiveUrgency === 'function') {
                            const actor = String(msg?.username || 'player').trim() || 'player';
                            ctx.setObjectiveUrgency(`Reported ${actor}`, 'critical', 1200);
                        }
                        if (typeof ctx.log === 'function') {
                            ctx.log(`Chat report logged for ${String(msg?.username || 'unknown')}.`, 'warn');
                        }
                    }
                );

                actions.appendChild(muteBtn);
                actions.appendChild(blockBtn);
                actions.appendChild(reportBtn);
                div.appendChild(actions);
            }

            ctx.chatDisplayDiv.appendChild(div);
        });
        ctx.chatDisplayDiv.scrollTop = ctx.chatDisplayDiv.scrollHeight;
    }

    function updateMatchInfo() {
        const ctx = getCtx();
        if (!ctx.matchInfo) {
            ctx.uiCache.matchInfoSignature = null;
            ctx.matchInfoDiv.classList.add('hidden');
            lastCountdownBeepSecond = null;
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
                lastCountdownBeepSecond = null;
                lastMatchOutcomeAudioSignature = '';
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
                maybePlayCountdownBeep(timeRemaining);
                lastMatchOutcomeAudioSignature = '';
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
                lastCountdownBeepSecond = null;
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
                maybePlayMatchOutcomeSound(winnerTeamId, winnerNameRaw);
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
        const ffaPlayersTable = document.getElementById('ffaPlayersTable');
        const redTeamPlayersTable = document.getElementById('redTeamPlayers');
        const blueTeamPlayersTable = document.getElementById('blueTeamPlayers');
        if (!scoreboardContentDiv || !ffaScoreboardSection || !teamScoreboardSection || !ffaPlayersTable || !redTeamPlayersTable || !blueTeamPlayersTable) {
            return;
        }
        const ffaPlayersTableBody = ffaPlayersTable.getElementsByTagName('tbody')[0];
        const redTeamPlayersTableBody = redTeamPlayersTable.getElementsByTagName('tbody')[0];
        const blueTeamPlayersTableBody = blueTeamPlayersTable.getElementsByTagName('tbody')[0];
        if (!ffaPlayersTableBody || !redTeamPlayersTableBody || !blueTeamPlayersTableBody) {
            return;
        }

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
            try {
                const parsed = JSON.parse(storedSettings);
                if (parsed && typeof parsed === 'object') {
                    for (const [key, value] of Object.entries(parsed)) {
                        if (!(key in ctx.gameSettings)) continue;
                        const currentValue = ctx.gameSettings[key];
                        if (typeof currentValue === 'boolean' && typeof value === 'boolean') {
                            ctx.gameSettings[key] = value;
                        } else if (typeof currentValue === 'string' && typeof value === 'string') {
                            ctx.gameSettings[key] = value;
                        } else if (typeof currentValue === 'number' && Number.isFinite(value)) {
                            ctx.gameSettings[key] = value;
                        }
                    }
                    ctx.log('Settings loaded from localStorage.', 'info');
                }
            } catch (_err) {
                ctx.log('Ignoring invalid settings payload from localStorage.', 'warning');
            }
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
