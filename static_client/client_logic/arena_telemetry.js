(() => {
    const overviewRefreshMs = 15_000;
    const ratingsRefreshMs = 60_000;
    let lastGeneration = null;
    let lastSeasonId = null;
    let lastRatingsGeneratedAt = null;
    let latestRatings = null;
    let focusBeforeDialog = null;

    function setText(selector, value) {
        document.querySelectorAll(selector).forEach((element) => {
            element.textContent = value;
        });
    }

    function animateChip() {
        const chip = document.getElementById('arenaEvolutionChip');
        if (!chip) return;
        chip.classList.remove('arena-evolution-chip--advanced');
        requestAnimationFrame(() => chip.classList.add('arena-evolution-chip--advanced'));
    }

    function formatModelName(rawName) {
        const text = String(rawName || 'Unknown model').trim();
        const withoutProvider = text.includes(': ') ? text.split(': ').slice(1).join(': ') : text;
        return withoutProvider
            .split('/').pop()
            .replace(/[-_]+/g, ' ')
            .replace(/\b[a-z]/g, (letter) => letter.toUpperCase());
    }

    function formatRating(rawValue) {
        const value = Math.max(0, Math.min(100, Number(rawValue) || 0));
        return Number.isInteger(value) ? String(value) : value.toFixed(1);
    }

    function createMetric(label, value, rawDetail = '') {
        const metric = document.createElement('span');
        metric.className = 'arena-rating-metric';
        metric.setAttribute('aria-label', `${label} rating ${formatRating(value)} out of 100`);
        if (rawDetail) metric.title = rawDetail;

        const number = document.createElement('b');
        number.textContent = formatRating(value);
        const track = document.createElement('i');
        track.setAttribute('aria-hidden', 'true');
        const fill = document.createElement('span');
        fill.style.setProperty('--arena-rating', String(Math.max(0, Math.min(100, Number(value) || 0))));
        track.append(fill);
        metric.append(number, track);
        return metric;
    }

    function createRatingRow(entry) {
        const row = document.createElement('article');
        row.className = 'arena-rating-row';

        const rank = document.createElement('span');
        rank.className = 'arena-rating-row__rank';
        rank.textContent = String(entry.rank || 0).padStart(2, '0');

        const identity = document.createElement('div');
        identity.className = 'arena-rating-row__identity';
        const name = document.createElement('strong');
        name.textContent = formatModelName(entry.model_name || entry.provider_model);
        const meta = document.createElement('span');
        const tour = Number(entry.epochs_played) > 0
            ? `${Number(entry.season_points) || 0} pts · ${entry.epochs_played} epochs`
            : `${entry.evaluation_engagements || 0} fights`;
        meta.textContent = `OR #${entry.provider_rank || '–'} · ${tour} · ${entry.integrity_status || 'verified'}`;
        identity.append(name, meta);

        const metrics = document.createElement('div');
        metrics.className = 'arena-rating-row__metrics';
        metrics.append(
            createMetric('Strategy', entry.strategy_rating ?? entry.overall_rating, `${entry.season_points || 0} accumulated tour points`),
            createMetric('Personal', entry.personal_rating, `${entry.personal_score_for || 0} raw personal points`),
            createMetric('Team', entry.team_rating, `${entry.team_objective_for || 0} raw objective points`),
            createMetric('Collaboration', entry.collaboration_rating, `${entry.collaboration_score_for || 0} ally damage prevented / assist points`),
            createMetric('World', entry.world_rating, `${entry.world_points || 0} world placement points`),
        );
        row.append(rank, identity, metrics);
        return row;
    }

    function renderEmptyRatings(message) {
        const list = document.querySelector('[data-client-arena-ratings-list]');
        if (!list) return;
        const empty = document.createElement('div');
        empty.className = 'arena-ratings-empty';
        const title = document.createElement('strong');
        title.textContent = 'Season is staging.';
        const detail = document.createElement('span');
        detail.textContent = message;
        empty.append(title, detail);
        list.replaceChildren(empty);
    }

    function renderRatings(ratings) {
        latestRatings = ratings;
        const active = ratings?.active === true && Array.isArray(ratings.roster) && ratings.roster.length > 0;
        const seasonId = active ? String(ratings.season_id || 'active') : null;
        const list = document.querySelector('[data-client-arena-ratings-list]');

        if (!active) {
            setText('[data-client-arena-season]', 'PENDING');
            setText('[data-client-arena-chip-label]', 'Open ratings');
            setText('[data-client-arena-season-name]', 'No active verified season');
            setText('[data-client-arena-season-status]', 'All ten fighters must pass integrity checks');
            renderEmptyRatings('The board activates only after all ten real model responses compile and finish the same fixed, side-swapped matches.');
            return;
        }

        const champion = ratings.roster[0];
        setText('[data-client-arena-models]', String(ratings.roster.length));
        setText('[data-client-arena-season]', 'LIVE');
        setText(
            '[data-client-arena-chip-label]',
            Number(champion.epochs_played) > 0
                ? `#1 ${formatModelName(champion.model_name || champion.provider_model)} · ${champion.season_points} pts`
                : `#1 ${formatModelName(champion.model_name || champion.provider_model)} · ${formatRating(champion.overall_rating)}`,
        );
        setText('[data-client-arena-season-name]', seasonId.replaceAll('-', ' / '));
        setText(
            '[data-client-arena-season-status]',
            ratings.league?.epochs_completed
                ? `${ratings.league.epochs_completed} balanced epochs · tour points live`
                : `${ratings.roster.length} verified WASM fighters · P/T/C/W rated`,
        );

        if (list) {
            list.replaceChildren(...ratings.roster.map(createRatingRow));
        }
        const generatedAt = String(ratings.generated_at || '');
        if (
            (lastSeasonId !== null && lastSeasonId !== seasonId)
            || (lastRatingsGeneratedAt !== null && generatedAt && lastRatingsGeneratedAt !== generatedAt)
        ) animateChip();
        lastSeasonId = seasonId;
        lastRatingsGeneratedAt = generatedAt;
    }

    async function refreshArenaOverview() {
        try {
            const response = await fetch('/api/public/arena/overview', {
                headers: { Accept: 'application/json' },
                cache: 'no-store',
            });
            if (!response.ok) return;
            const payload = await response.json();
            const overview = payload?.data;
            if (!overview) return;

            const generation = Number(overview.total_completed_matches || 0);
            const modelCount = Number(overview.active_models || 0);
            if (!latestRatings?.active) setText('[data-client-arena-models]', String(modelCount));
            const visibleGeneration = Number(latestRatings?.league?.epochs_completed || generation);
            setText('[data-client-arena-generation]', String(visibleGeneration).padStart(3, '0'));

            if (lastGeneration !== null && generation > lastGeneration) animateChip();
            lastGeneration = generation;
        } catch (_) {
            // The game remains playable when optional public telemetry is unavailable.
        }
    }

    async function refreshArenaRatings() {
        try {
            const response = await fetch('/api/public/arena/ratings', {
                headers: { Accept: 'application/json' },
                cache: 'no-store',
            });
            if (!response.ok) return;
            const payload = await response.json();
            if (payload?.ok !== true || !payload?.data) return;
            renderRatings(payload.data);
        } catch (_) {
            if (!latestRatings) {
                renderEmptyRatings('Ratings telemetry is temporarily unavailable. Human controls and live combat continue normally.');
            }
        }
    }

    function setDialogOpen(open) {
        const dialog = document.getElementById('arenaRatingsDialog');
        const chip = document.getElementById('arenaEvolutionChip');
        if (!dialog) return;
        if (open) {
            focusBeforeDialog = document.activeElement;
            dialog.hidden = false;
            dialog.setAttribute('aria-hidden', 'false');
            document.body.classList.add('arena-ratings-open');
            dialog.querySelector('.arena-ratings-dialog__close')?.focus({ preventScroll: true });
            refreshArenaRatings();
        } else {
            dialog.hidden = true;
            dialog.setAttribute('aria-hidden', 'true');
            document.body.classList.remove('arena-ratings-open');
            const returnTarget = focusBeforeDialog instanceof HTMLElement ? focusBeforeDialog : chip;
            returnTarget?.focus({ preventScroll: true });
            focusBeforeDialog = null;
        }
    }

    function initialize() {
        const chip = document.getElementById('arenaEvolutionChip');
        const dialog = document.getElementById('arenaRatingsDialog');
        chip?.addEventListener('click', (event) => {
            event.stopPropagation();
            setDialogOpen(true);
        });
        dialog?.querySelectorAll('[data-arena-ratings-close]').forEach((button) => {
            button.addEventListener('click', () => setDialogOpen(false));
        });
        dialog?.addEventListener('pointerdown', (event) => event.stopPropagation());
        document.addEventListener('keydown', (event) => {
            if (event.key === 'Escape' && dialog && !dialog.hidden) {
                event.preventDefault();
                setDialogOpen(false);
                return;
            }
            if (event.key === 'Tab' && dialog && !dialog.hidden) {
                const focusable = [...dialog.querySelectorAll(
                    '.arena-ratings-dialog__panel button:not([disabled]), .arena-ratings-dialog__panel a[href]',
                )].filter((element) => element.getClientRects().length > 0);
                if (focusable.length === 0) return;
                const first = focusable[0];
                const last = focusable[focusable.length - 1];
                if (event.shiftKey && document.activeElement === first) {
                    event.preventDefault();
                    last.focus();
                } else if (!event.shiftKey && document.activeElement === last) {
                    event.preventDefault();
                    first.focus();
                }
            }
        });

        refreshArenaOverview();
        refreshArenaRatings();
        window.setInterval(refreshArenaOverview, overviewRefreshMs);
        window.setInterval(refreshArenaRatings, ratingsRefreshMs);
    }

    document.addEventListener('DOMContentLoaded', initialize);
})();
