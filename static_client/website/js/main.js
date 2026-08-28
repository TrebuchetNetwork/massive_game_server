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

  // Mirror of scripts/arena/build_model_pages.mjs baseSlug():
  // "deepseek/deepseek-v4-pro-20260423" -> "deepseek-v4-pro".
  const baseSlug = (rawId) =>
    String(rawId || '').split('/').pop().replace(/-\d{8}$/, '').replace(/:free$/, '');

  // Match a roster model to its /models/<slug>.html page via mascots.json.
  // Tries the most specific candidate first so dated twins like
  // deepseek-v4-flash vs deepseek-v4-flash-0731 resolve to distinct slugs.
  const mascotForModel = (mascots, model) => {
    if (!mascots || !model) return null;
    const candidates = [...new Set([
      baseSlug(model.provider_model),
      baseSlug(model.canonical_slug),
      baseSlug(model.model_id),
    ].filter(Boolean))].sort((a, b) => b.length - a.length);
    const slug = candidates.find((candidate) => mascots[candidate]);
    return slug ? { slug, ...mascots[slug] } : null;
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

  const renderRoster = (models, mascots) => {
    const roster = document.querySelector('[data-roster]');
    if (!roster || !Array.isArray(models) || models.length === 0) return;

    roster.replaceChildren();
    models.slice(0, 5).forEach((model, index) => {
      const mascot = mascotForModel(mascots, model);
      const row = document.createElement(mascot ? 'a' : 'article');
      row.className = 'roster__row';
      if (mascot) row.href = `/models/${mascot.slug}.html`;

      const number = document.createElement('span');
      number.className = 'roster__index';
      number.textContent = String(index + 1).padStart(2, '0');

      const identity = document.createElement('div');
      const name = document.createElement('strong');
      name.textContent = `${mascot ? `${mascot.emoji} ` : ''}${formatModelName(model.model_name || model.model_id)}`;
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

  const renderSeasonRoster = (ratings, mascots) => {
    const roster = document.querySelector('[data-roster]');
    if (!roster || ratings?.active !== true || !Array.isArray(ratings.roster) || ratings.roster.length === 0) {
      return false;
    }

    roster.replaceChildren();
    ratings.roster.forEach((model) => {
      const mascot = mascotForModel(mascots, model);
      const row = document.createElement(mascot ? 'a' : 'article');
      row.className = 'roster__row roster__row--rated';
      if (mascot) row.href = `/models/${mascot.slug}.html`;

      const number = document.createElement('span');
      number.className = 'roster__index';
      number.textContent = String(model.rank || 0).padStart(2, '0');

      const identity = document.createElement('div');
      const name = document.createElement('strong');
      name.textContent = `${mascot ? `${mascot.emoji} ` : ''}${formatModelName(model.model_name || model.provider_model)}`;
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
      const [overviewResponse, leaderboardResponse, ratingsResponse, mascotsResponse] = await Promise.all([
        fetch('/api/public/arena/overview', { headers: { Accept: 'application/json' } }),
        fetch('/api/public/arena/leaderboard?limit=5', { headers: { Accept: 'application/json' } }),
        fetch('/api/public/arena/ratings', { headers: { Accept: 'application/json' }, cache: 'no-store' })
          .catch(() => null),
        fetch('/models/mascots.json', { headers: { Accept: 'application/json' } })
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

      let mascots = null;
      if (mascotsResponse?.ok) {
        mascots = await mascotsResponse.json().catch(() => null);
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
      if (!renderSeasonRoster(ratings, mascots)) renderRoster(leaderboard.models, mascots);
    } catch (_) {
      if (status) status.textContent = 'Registry ready';
    }
  };

  const hydrateHighlights = async () => {
    const section = document.querySelector('[data-highlights]');
    const grid = document.querySelector('[data-highlights-grid]');
    if (!section || !grid) return;
    try {
      const response = await fetch('/media/highlights/index.json', { headers: { Accept: 'application/json' } });
      if (!response.ok) return;
      const payload = await response.json();
      if (!Array.isArray(payload) || payload.length === 0) return;

      const clips = payload
        .filter((clip) => clip && clip.webm)
        .sort((a, b) => String(b.webm).localeCompare(String(a.webm)))
        .slice(0, 3);
      if (clips.length === 0) return;

      grid.replaceChildren();
      clips.forEach((clip) => {
        const figure = document.createElement('figure');
        figure.className = 'highlight-card';

        const video = document.createElement('video');
        video.muted = true;
        video.loop = true;
        video.autoplay = true;
        video.playsInline = true;
        video.preload = 'metadata';
        if (clip.gif) video.poster = `/media/highlights/${clip.gif}`;
        const source = document.createElement('source');
        source.src = `/media/highlights/${clip.webm}`;
        source.type = 'video/webm';
        video.append(source);
        if (clip.gif) {
          const fallback = document.createElement('img');
          fallback.src = `/media/highlights/${clip.gif}`;
          fallback.alt = clip.reason || 'Arena highlight';
          video.append(fallback);
        }

        const caption = document.createElement('figcaption');
        const reason = document.createElement('strong');
        reason.textContent = clip.reason || 'Highlight';
        const meta = document.createElement('small');
        const players = [...new Set(Array.isArray(clip.players) ? clip.players : [])];
        meta.textContent = players.length > 0 ? players.join(' · ') : String(clip.date || '');
        caption.append(reason, meta);

        figure.append(video, caption);
        grid.append(figure);
      });

      section.hidden = false;
      grid.querySelectorAll('video').forEach((video) => {
        video.play().catch(() => {});
      });
    } catch (_) {
      // Highlights are optional: leave the section hidden on any failure.
    }
  };

  // Compact continuous-league ticker under the roster. Reads the static
  // payload emitted by build_model_pages.mjs; any failure leaves the ticker
  // hidden (fail-silent).
  const LEAGUE_TICKER_ICONS = { entrant: '🌱', revision: '🔧', retirement: '🪦' };

  const leagueTickerText = (a) => {
    const name = `${a?.mascot?.emoji ? `${a.mascot.emoji} ` : ''}${a?.mascot?.title || a?.slug || 'A model'}`;
    switch (a?.type) {
      case 'entrant':
        return `${name} enters the league${a.provider_rank ? ` · OpenRouter #${a.provider_rank}` : ''}`;
      case 'revision': {
        const outcome = { accepted: 'accepted', compile_failed: 'compile failed', codegen_failed: 'codegen failed', interrupted: 'interrupted' }[a.outcome] || a.outcome || 'revision';
        return `${name} v${a.version} ${outcome}`;
      }
      case 'retirement':
        return `${name} retires to the Hall of Fame`;
      default:
        return `${name} league update`;
    }
  };

  const leagueTickerAge = (iso) => {
    const ms = Date.parse(iso);
    if (!Number.isFinite(ms)) return '';
    const minutes = Math.max(0, Math.floor((Date.now() - ms) / 60000));
    if (minutes < 60) return `${minutes}m ago`;
    const hours = Math.floor(minutes / 60);
    if (hours < 48) return `${hours}h ago`;
    return `${Math.floor(hours / 24)}d ago`;
  };

  const hydrateLeagueTicker = async () => {
    const ticker = document.querySelector('[data-league-ticker]');
    if (!ticker) return;
    try {
      const response = await fetch('/models/league.json', { headers: { Accept: 'application/json' }, cache: 'no-store' });
      if (!response.ok) return;
      const payload = await response.json();
      const items = Array.isArray(payload?.announcements) ? payload.announcements.slice(0, 3) : [];
      if (items.length === 0) return;

      const label = document.createElement('a');
      label.className = 'league-ticker__label';
      label.href = '/models/';
      label.textContent = `Continuous league · day ${Number(payload.day_index) || 0}`;

      const children = [label];
      items.forEach((a) => {
        const item = document.createElement('span');
        item.className = 'league-ticker__item';
        const icon = document.createElement('i');
        icon.textContent = LEAGUE_TICKER_ICONS[a?.type] || '📣';
        const text = document.createElement('span');
        text.textContent = leagueTickerText(a);
        item.append(icon, text);
        const age = leagueTickerAge(a?.at);
        if (age) {
          const time = document.createElement('time');
          time.dateTime = String(a.at);
          time.textContent = age;
          item.append(time);
        }
        children.push(item);
      });
      ticker.replaceChildren(...children);
      ticker.hidden = false;
    } catch (_) {
      // League ticker is optional: stay hidden on any failure.
    }
  };

  // Featured fight footage in the hero's arena card. Picks the newest clip
  // from the highlights index, keeps it muted/looped/playsinline, and
  // lazy-swaps the source in after load so the video never blocks first
  // paint. Any failure (no clips, fetch error, decode error, reduced
  // motion) leaves the CSS simulation untouched.
  const hydrateHeroFootage = async () => {
    const field = document.querySelector('.arena-field');
    if (!field) return;
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    try {
      const response = await fetch('/media/highlights/index.json', { headers: { Accept: 'application/json' } });
      if (!response.ok) return;
      const payload = await response.json();
      if (!Array.isArray(payload) || payload.length === 0) return;

      const clip = payload
        .filter((entry) => entry && entry.webm)
        .sort((a, b) => String(b.webm).localeCompare(String(a.webm)))[0];
      if (!clip) return;

      const video = document.createElement('video');
      video.className = 'arena-field__video';
      video.muted = true;
      video.loop = true;
      video.autoplay = true;
      video.playsInline = true;
      video.preload = 'none';
      video.setAttribute('aria-hidden', 'true');
      video.tabIndex = -1;
      const still = clip.poster || clip.gif;
      if (still) video.poster = `/media/highlights/${still}`;

      const teardown = () => {
        video.remove();
        field.classList.remove('arena-field--footage');
      };
      video.addEventListener('error', teardown);
      video.addEventListener('stalled', () => {
        if (video.readyState < 2) teardown();
      }, { once: true });

      field.classList.add('arena-field--footage');
      field.prepend(video);

      const topline = document.querySelector('.arena-visual__topline span');
      if (topline) {
        topline.textContent = `Latest highlight / ${clip.reason || 'fight replay'}`;
      }

      // Lazy source swap: only start fetching once the page has painted.
      const startPlayback = () => {
        const source = document.createElement('source');
        source.src = `/media/highlights/${clip.webm}`;
        source.type = 'video/webm';
        source.addEventListener('error', teardown);
        video.append(source);
        video.preload = 'auto';
        video.load();
        video.play().catch(() => {});
      };
      if (document.readyState === 'complete') {
        startPlayback();
      } else {
        window.addEventListener('load', startPlayback, { once: true });
      }
    } catch (_) {
      // Hero footage is decorative: keep the CSS simulation on any failure.
    }
  };

  // Compact live telemetry strip under the hero. Reads the public overview
  // plus the league state, refreshes every 30s, and hides on any error.
  const hydrateLiveStrip = async () => {
    const strip = document.querySelector('[data-live-strip]');
    if (!strip) return;

    const pulse = document.createElement('span');
    const pulseDot = document.createElement('span');
    const pulseLabel = document.createElement('b');
    pulse.append(pulseDot, pulseLabel);

    const modelsItem = document.createElement('span');
    modelsItem.className = 'live-strip__item';
    const modelsValue = document.createElement('b');
    modelsItem.append(modelsValue, document.createTextNode(' models active'));

    const flightItem = document.createElement('span');
    flightItem.className = 'live-strip__item';
    const flightValue = document.createElement('b');
    flightItem.append(flightValue, document.createTextNode(' in flight'));

    const dayItem = document.createElement('span');
    dayItem.className = 'live-strip__item';
    dayItem.append(document.createTextNode('League day '));
    const dayValue = document.createElement('b');
    dayItem.append(dayValue);

    strip.replaceChildren(pulse, modelsItem, flightItem, dayItem);

    const refresh = async () => {
      try {
        const [overviewResponse, leagueResponse] = await Promise.all([
          fetch('/api/public/arena/overview', { headers: { Accept: 'application/json' }, cache: 'no-store' }),
          fetch('/models/league.json', { headers: { Accept: 'application/json' }, cache: 'no-store' }),
        ]);
        if (!overviewResponse.ok || !leagueResponse.ok) throw new Error('live strip unavailable');

        const overviewPayload = await overviewResponse.json();
        const leaguePayload = await leagueResponse.json();
        const overview = overviewPayload?.data;
        if (!overview) throw new Error('live strip payload invalid');

        const inFlight = Number(overview.in_flight_matches) || 0;
        pulse.className = inFlight > 0 ? 'live-strip__item live-strip__item--live' : 'live-strip__item';
        pulseDot.className = inFlight > 0 ? 'live-strip__dot' : 'live-strip__dot live-strip__dot--idle';
        pulseLabel.textContent = inFlight > 0 ? 'Live' : 'Standby';
        modelsValue.textContent = String(Number(overview.active_models) || 0);
        flightValue.textContent = String(inFlight);
        dayValue.textContent = String(Number(leaguePayload?.day_index) || 0).padStart(2, '0');

        strip.hidden = false;
      } catch (_) {
        strip.hidden = true;
      }
    };

    await refresh();
    window.setInterval(refresh, 30000);
  };

  hydrateArenaTelemetry();
  hydrateHighlights();
  hydrateLeagueTicker();
  hydrateHeroFootage();
  hydrateLiveStrip();
});
