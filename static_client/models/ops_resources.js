// ops_resources.js — live "Resources" panel for the System page.
//
// Fetches /media/usage.json (published every ~30s by massive-game-
// usage-recorder.service) and renders freshness, latest values, and 24h
// sparklines. CSP-safe: no inline styles/scripts; SVG uses presentation
// attributes only. Degrades to an honest "recorder offline" mark.
(function () {
    'use strict';

    var USAGE_URL = '/media/usage.json';
    var REFRESH_MS = 60000;
    var STALE_AFTER_MS = 120000;
    var ACID = '#caff00';
    var DIM = '#5e6d65';

    var root = document.getElementById('ops-res');
    if (!root) return;

    function esc(s) {
        return String(s).replace(/[&<>"']/g, function (c) {
            return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c];
        });
    }

    function fmtAgo(ms) {
        var s = Math.max(0, Math.round(ms / 1000));
        if (s < 90) return s + 's ago';
        var m = Math.round(s / 60);
        if (m < 90) return m + 'm ago';
        return Math.round(m / 60) + 'h ago';
    }

    function fmtNum(v, digits) {
        if (v === null || v === undefined || !isFinite(v)) return '—';
        return Number(v).toLocaleString('en-US', {
            minimumFractionDigits: digits || 0,
            maximumFractionDigits: digits || 0,
        });
    }

    // SVG sparkline as a string. Points: [{t, value}]. Returns '' when <2 points.
    function sparkline(points, color) {
        var pts = (points || []).filter(function (p) { return p && isFinite(p.value); });
        if (pts.length < 2) return '';
        var w = 260, h = 56, pad = 2;
        var min = Infinity, max = -Infinity;
        pts.forEach(function (p) { if (p.value < min) min = p.value; if (p.value > max) max = p.value; });
        var span = (max - min) || 1;
        var step = (w - pad * 2) / (pts.length - 1);
        var d = pts.map(function (p, i) {
            var x = pad + i * step;
            var y = pad + (h - pad * 2) * (1 - (p.value - min) / span);
            return (i === 0 ? 'M' : 'L') + x.toFixed(1) + ' ' + y.toFixed(1);
        }).join(' ');
        return '<svg viewBox="0 0 ' + w + ' ' + h + '" width="' + w + '" height="' + h + '" role="img" aria-label="24 hour trend, min ' +
            esc(fmtNum(min, 1)) + ', max ' + esc(fmtNum(max, 1)) + '">' +
            '<path d="' + d + '" fill="none" stroke="' + color + '" stroke-width="1.5" stroke-linejoin="round" stroke-linecap="round"/></svg>';
    }

    function card(title, valueHtml, points, color, caption) {
        var chart = sparkline(points, color);
        return '<div class="ops-res__card">' +
            '<h3>' + esc(title) + '</h3>' +
            '<p class="ops-res__val">' + valueHtml + '</p>' +
            (chart || '<p class="ops-res__empty">no samples yet</p>') +
            (caption ? '<p class="ops-res__cap">' + esc(caption) + '</p>' : '') +
            '</div>';
    }

    function render(doc) {
        var now = Date.now();
        var bakedMs = Date.parse(doc.generatedAt || '');
        var ageMs = isFinite(bakedMs) ? now - bakedMs : Infinity;
        var live = ageMs < STALE_AFTER_MS;
        var badge = live
            ? '<span class="ops__badge ops__badge--ok">live · last sample ' + esc(fmtAgo(ageMs)) + '</span>'
            : '<span class="ops__badge ops__badge--fail">stale · last sample ' + (isFinite(ageMs) ? esc(fmtAgo(ageMs)) : 'unknown') + '</span>';

        var L = doc.latest || {};
        var proc = L.process || {};
        var host = L.host || {};
        var game = L.game || {};
        var ngrok = L.ngrok || {};
        var S = doc.series || {};

        var memPct = (host.mem && isFinite(host.mem.memTotalKb) && host.mem.memTotalKb > 0 && isFinite(host.mem.memAvailableKb))
            ? Math.round((1 - host.mem.memAvailableKb / host.mem.memTotalKb) * 100) : null;
        var procAlive = proc.pid !== null && proc.pid !== undefined;

        var html =
            '<p class="ops-res__meta">' + badge +
            ' <span class="ops-res__src">sampler: massive-game-usage-recorder.service · every ' + esc(fmtNum(doc.intervalSec)) + 's · 24h window · history kept 14 days</span></p>' +
            '<div class="ops-res__grid">' +
            card('Server CPU', procAlive ? esc(fmtNum(proc.cpuPct, 1)) + '<small>% of one core</small>' : '<small>server down</small>',
                S.cpuPct, ACID, 'game server process, 100% = one saturated core') +
            card('Server memory', procAlive ? esc(fmtNum(proc.rssMb)) + '<small> MB RSS · ' + esc(fmtNum(proc.threads)) + ' threads · ' + esc(fmtNum(proc.fds)) + ' fds</small>' : '<small>server down</small>',
                S.rssMb, ACID, null) +
            card('Host load', esc(fmtNum(host.load && host.load.load1, 2)) + '<small> load1 · mem ' + (memPct === null ? '—' : esc(fmtNum(memPct)) + '% used') + '</small>',
                S.load1, DIM, null) +
            card('Players connected', esc(fmtNum(game.playersConnected)) + '<small> humans online now</small>',
                S.players, ACID, null) +
            card('Signaling sockets', esc(fmtNum(game.wsConnections)) + '<small> active WebSockets</small>',
                S.wsConnections, DIM, null) +
            card('ngrok tunnel', esc(fmtNum(ngrok.connsTotal)) + '<small> connections since tunnel start</small>',
                S.ngrokConns, DIM, 'cumulative counter — slope is the interesting part') +
            '</div>' +
            '<p class="metric-note">Exhibition fighters active: ' + esc(fmtNum(game.exhibitionRuntimes)) +
            ' · match degraded: ' + (game.matchDegraded ? '<b>yes</b>' : 'no') +
            ' · frames processed: ' + esc(fmtNum(game.framesTotal)) + ' (cumulative)</p>';

        root.innerHTML = html;
    }

    function renderOffline(message) {
        root.innerHTML = '<p class="ops-res__meta"><span class="ops__badge ops__badge--unknown">recorder offline</span> ' +
            '<span class="ops-res__src">' + esc(message || 'usage.json unreachable') + '</span></p>';
    }

    function refresh() {
        fetch(USAGE_URL, { cache: 'no-store' })
            .then(function (res) {
                if (!res.ok) throw new Error('http ' + res.status);
                return res.json();
            })
            .then(render)
            .catch(function (err) { renderOffline(err && err.message); });
    }

    refresh();
    setInterval(refresh, REFRESH_MS);
})();
