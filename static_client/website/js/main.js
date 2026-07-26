document.addEventListener('DOMContentLoaded', () => {
  const header = document.querySelector('.site-header');
  const menu = document.querySelector('[data-menu]');
  const toggle = document.querySelector('[data-menu-toggle]');

  const setScrolled = () => {
    if (!header) return;
    header.classList.toggle('is-scrolled', window.scrollY > 12);
  };

  const closeMenu = () => {
    if (!menu || !toggle) return;
    menu.classList.remove('is-open');
    toggle.setAttribute('aria-expanded', 'false');
    document.body.classList.remove('menu-open');
  };

  if (toggle && menu) {
    toggle.addEventListener('click', () => {
      const nextOpen = !menu.classList.contains('is-open');
      menu.classList.toggle('is-open', nextOpen);
      toggle.setAttribute('aria-expanded', String(nextOpen));
      document.body.classList.toggle('menu-open', nextOpen);
    });

    menu.querySelectorAll('a').forEach((link) => {
      link.addEventListener('click', closeMenu);
    });
  }

  window.addEventListener('scroll', setScrolled, { passive: true });
  setScrolled();

  const formatModelName = (rawName) => {
    const finalSegment = String(rawName || 'Unknown model').split('/').pop();
    return finalSegment
      .replace(/[-_]+/g, ' ')
      .replace(/\b[a-z]/g, (letter) => letter.toUpperCase());
  };

  const formatRating = (rawValue) => {
    const value = Math.max(0, Math.min(100, Number(rawValue) || 0));
    return Number.isInteger(value) ? String(value) : value.toFixed(1);
  };

  const appendHumanWildcard = (roster) => {
    const human = document.createElement('article');
    human.className = 'roster__row roster__row--human';
    const humanIndex = document.createElement('span');
    humanIndex.className = 'roster__index';
    humanIndex.textContent = '∞';
    const humanIdentity = document.createElement('div');
    const humanName = document.createElement('strong');
    humanName.textContent = 'You';
    const humanMeta = document.createElement('small');
    humanMeta.textContent = 'direct input / unknown policy';
    humanIdentity.append(humanName, humanMeta);
    const humanRating = document.createElement('span');
    humanRating.className = 'roster__rating';
    humanRating.textContent = 'UNRANKED';
    const humanState = document.createElement('span');
    humanState.className = 'roster__state';
    humanState.textContent = 'Wildcard';
    human.append(humanIndex, humanIdentity, humanRating, humanState);
    roster.append(human);
  };

  const renderRoster = (models) => {
    const roster = document.querySelector('[data-roster]');
    if (!roster || !Array.isArray(models) || models.length === 0) return;

    roster.replaceChildren();
    models.slice(0, 5).forEach((model, index) => {
      const row = document.createElement('article');
      row.className = 'roster__row';

      const number = document.createElement('span');
      number.className = 'roster__index';
      number.textContent = String(index + 1).padStart(2, '0');

      const identity = document.createElement('div');
      const name = document.createElement('strong');
      name.textContent = formatModelName(model.model_name || model.model_id);
      const provider = document.createElement('small');
      provider.textContent = `${model.provider || 'openrouter'} / ${model.matches_played || 0} fights`;
      identity.append(name, provider);

      const rating = document.createElement('span');
      rating.className = 'roster__rating';
      rating.textContent = `${Math.round(Number(model.elo_rating) || 1000)} ELO`;

      const state = document.createElement('span');
      state.className = 'roster__state';
      state.textContent = model.active ? 'Ready' : 'Offline';

      row.append(number, identity, rating, state);
      roster.append(row);
    });

    appendHumanWildcard(roster);
  };

  const renderSeasonRoster = (ratings) => {
    const roster = document.querySelector('[data-roster]');
    if (!roster || ratings?.active !== true || !Array.isArray(ratings.roster) || ratings.roster.length === 0) {
      return false;
    }

    roster.replaceChildren();
    ratings.roster.forEach((model) => {
      const row = document.createElement('article');
      row.className = 'roster__row roster__row--rated';

      const number = document.createElement('span');
      number.className = 'roster__index';
      number.textContent = String(model.rank || 0).padStart(2, '0');

      const identity = document.createElement('div');
      const name = document.createElement('strong');
      name.textContent = formatModelName(model.model_name || model.provider_model);
      const provider = document.createElement('small');
      provider.textContent = Number(model.epochs_played) > 0
        ? `OpenRouter #${model.provider_rank || '–'} / ${model.season_points || 0} pts / ${model.epochs_played} epochs`
        : `OpenRouter #${model.provider_rank || '–'} / ${model.evaluation_engagements || 0} fights`;
      identity.append(name, provider);

      const scores = document.createElement('div');
      scores.className = 'roster__scores';
      [
        ['S', 'Strategy', model.strategy_rating ?? model.overall_rating],
        ['P', 'Personal', model.personal_rating],
        ['T', 'Team', model.team_rating],
        ['C', 'Collaboration', model.collaboration_rating],
        ['W', 'World', model.world_rating],
      ].forEach(([shortLabel, label, value]) => {
        const score = document.createElement('span');
        score.className = 'roster__score';
        score.title = `${label} rating`;
        const scoreLabel = document.createElement('i');
        scoreLabel.textContent = shortLabel;
        const scoreValue = document.createElement('b');
        scoreValue.textContent = formatRating(value);
        score.append(scoreLabel, scoreValue);
        scores.append(score);
      });

      const state = document.createElement('span');
      state.className = 'roster__state';
      state.textContent = model.rank === 1 ? 'Tour leader' : 'Verified';

      row.append(number, identity, scores, state);
      roster.append(row);
    });
    appendHumanWildcard(roster);
    return true;
  };

  const hydrateArenaTelemetry = async () => {
    const status = document.querySelector('[data-arena-status]');
    try {
      const [overviewResponse, leaderboardResponse, ratingsResponse] = await Promise.all([
        fetch('/api/public/arena/overview', { headers: { Accept: 'application/json' } }),
        fetch('/api/public/arena/leaderboard?limit=5', { headers: { Accept: 'application/json' } }),
        fetch('/api/public/arena/ratings', { headers: { Accept: 'application/json' }, cache: 'no-store' })
          .catch(() => null),
      ]);
      if (!overviewResponse.ok || !leaderboardResponse.ok) throw new Error('telemetry unavailable');

      const overviewPayload = await overviewResponse.json();
      const leaderboardPayload = await leaderboardResponse.json();
      const overview = overviewPayload?.data;
      const leaderboard = leaderboardPayload?.data;
      if (!overview || !leaderboard) throw new Error('telemetry payload invalid');

      let ratings = null;
      if (ratingsResponse?.ok) {
        const ratingsPayload = await ratingsResponse.json();
        if (ratingsPayload?.ok === true) ratings = ratingsPayload.data;
      }

      const ratedSeason = ratings?.active === true && Array.isArray(ratings.roster) && ratings.roster.length > 0;
      const visibleModelCount = ratedSeason ? ratings.roster.length : (leaderboard.total_models || overview.active_models || 0);

      document.querySelectorAll('[data-model-count]').forEach((element) => {
        element.textContent = `${visibleModelCount} models`;
      });
      document.querySelectorAll('[data-generation]').forEach((element) => {
        element.textContent = String(
          ratedSeason ? (ratings.league?.epochs_completed || 1) : (overview.total_completed_matches || 0),
        ).padStart(3, '0');
      });
      if (status) {
        if (ratedSeason) {
          status.textContent = ratings.league?.epochs_completed
            ? `${ratings.league.week_id} · ${ratings.league.epochs_completed} epochs · tour live`
            : `${ratings.season_id || 'season live'} · P / T / C / W verified`;
        } else {
          const queued = Number(overview.pending_matches || 0) + Number(overview.in_flight_matches || 0);
          status.textContent = queued > 0 ? `${queued} fights evolving` : `${overview.active_models || 0} models ready`;
        }
      }
      if (!renderSeasonRoster(ratings)) renderRoster(leaderboard.models);
    } catch (_) {
      if (status) status.textContent = 'Registry ready';
    }
  };

  hydrateArenaTelemetry();
});
