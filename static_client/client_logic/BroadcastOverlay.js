// DOM overlay for Arena TV: LIVE badge, team scores, match clock, kill
// feed, commentary line. Kill-feed/clock logic is pure and unit-tested;
// DOM assembly is a thin shell over it. All values come from the server
// (scoreboard endpoint, kill-feed events) — nothing is invented here.

export const KILL_FEED_TTL_MS = 8000;
export const KILL_FEED_LIMIT = 5;

export function pushKillEntry(feed, entry) {
    return [entry, ...feed].slice(0, KILL_FEED_LIMIT);
}

export function pruneKillFeed(feed, atMs, ttlMs = KILL_FEED_TTL_MS) {
    return feed.filter((e) => atMs - e.atMs <= ttlMs);
}

export function formatClock(seconds) {
    if (!Number.isFinite(seconds)) return '--:--';
    const s = Math.max(0, Math.floor(seconds));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

export function formatTeamScores(teamScores) {
    return teamScores.map((entry, i) => {
        const team = entry && typeof entry === 'object' ? (entry.team_id ?? i + 1) : i + 1;
        const score = entry && typeof entry === 'object' ? entry.score : entry;
        return `T${team} ${score}`;
    }).join(' · ');
}

const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
}[c]));

export function createBroadcastOverlay({ document: doc, parent }) {
    const element = doc.createElement('div');
    element.className = 'arena-tv-overlay';
    element.innerHTML =
        '<div class="arena-tv-top">' +
        '<span class="arena-tv-live">LIVE</span>' +
        '<span class="arena-tv-scores"></span>' +
        '<span class="arena-tv-clock">--:--</span>' +
        '</div>' +
        '<div class="arena-tv-killfeed"></div>' +
        '<div class="arena-tv-commentary"></div>';
    parent.appendChild(element);
    const scoresEl = element.querySelector('.arena-tv-scores');
    const clockEl = element.querySelector('.arena-tv-clock');
    const feedEl = element.querySelector('.arena-tv-killfeed');
    const commentaryEl = element.querySelector('.arena-tv-commentary');
    let feed = [];

    function renderFeed() {
        feedEl.innerHTML = feed.map((e) =>
            `<div class="arena-tv-kill">${esc(e.killerName)} ⟶ ${esc(e.victimName)}</div>`,
        ).join('');
    }

    return {
        element,
        pushKill(entry) {
            feed = pushKillEntry(feed, entry);
            renderFeed();
        },
        tick(atMs) {
            const before = feed.length;
            feed = pruneKillFeed(feed, atMs);
            if (feed.length !== before) renderFeed();
        },
        setCommentary(line) {
            commentaryEl.textContent = line || '';
        },
        setScoreboard(payload = {}) {
            const { teamScores, timeRemaining } = payload || {};
            if (Array.isArray(teamScores)) {
                scoresEl.textContent = formatTeamScores(teamScores);
            }
            clockEl.textContent = formatClock(timeRemaining);
        },
    };
}
