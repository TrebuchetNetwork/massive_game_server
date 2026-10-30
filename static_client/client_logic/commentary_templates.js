// Event -> broadcast commentary lines. Every field printed comes from the
// event payload; an event missing its fields yields no line (never
// "undefined"/"NaN" on air). Cooldowns per type, dedup per exact line,
// deterministic pool rotation.

export const COMMENTARY_POOLS = Object.freeze({
    streak: Object.freeze([
        (e) => `${e.name} is on a ${e.count}-kill streak`,
        (e) => `${e.count} straight for ${e.name} — who stops them?`,
        (e) => `${e.name} heating up: ${e.count} kills without dying`,
    ]),
    multi_kill: Object.freeze([
        (e) => `Multi-kill x${e.count} by ${e.name}!`,
        (e) => `${e.name} deletes ${e.count} fighters in one push`,
        (e) => `${e.count} down at once — ${e.name} on a rampage`,
    ]),
    low_hp_escape: Object.freeze([
        (e) => `${e.name} survives on a sliver of HP`,
        (e) => `One hit from death, ${e.name} gets out`,
        (e) => `${e.name} refuses to die`,
    ]),
    lead_change: Object.freeze([
        (e) => `Team ${e.team} takes the lead`,
        (e) => `Momentum swing — team ${e.team} in front now`,
        (e) => `Team ${e.team} flips the scoreboard`,
    ]),
    champion_kill: Object.freeze([
        (e) => `${e.killer} takes down the champion ${e.victim}`,
        (e) => `Champion falls: ${e.killer} gets ${e.victim}`,
        (e) => `${e.victim} dethroned by ${e.killer}`,
    ]),
    duel_start: Object.freeze([
        (e) => `${e.name} steps up to challenge the champion`,
        (e) => `A wild challenger appears: ${e.name}`,
        (e) => `${e.name} wants the crown`,
    ]),
});

const REQUIRED_FIELDS = {
    streak: ['name', 'count'],
    multi_kill: ['name', 'count'],
    low_hp_escape: ['name'],
    lead_change: ['team'],
    champion_kill: ['killer', 'victim'],
    duel_start: ['name'],
};

export function createCommentary({ cooldownMs = 20000, dedupMs = 60000 } = {}) {
    const lastTypeAt = new Map();
    const recentLines = new Map();
    const counters = new Map();

    function onEvent(ev, atMs) {
        if (!ev || !COMMENTARY_POOLS[ev.type]) return null;
        const fields = REQUIRED_FIELDS[ev.type] || [];
        for (const f of fields) {
            const v = ev[f];
            if (v === undefined || v === null || v === '' || (typeof v === 'number' && !Number.isFinite(v))) return null;
        }
        const last = lastTypeAt.get(ev.type);
        if (last !== undefined && atMs - last < cooldownMs) return null;
        const pool = COMMENTARY_POOLS[ev.type];
        // Rotation must not let a repeat of the same event back on air with
        // different phrasing while any of its exact lines is still recent.
        for (const render of pool) {
            const seenAt = recentLines.get(render(ev));
            if (seenAt !== undefined && atMs - seenAt < dedupMs) return null;
        }
        const i = (counters.get(ev.type) || 0) % pool.length;
        const line = pool[i](ev);
        counters.set(ev.type, (counters.get(ev.type) || 0) + 1);
        lastTypeAt.set(ev.type, atMs);
        recentLines.set(line, atMs);
        for (const [l, t] of recentLines) if (atMs - t > dedupMs) recentLines.delete(l);
        return line;
    }

    return { onEvent };
}
