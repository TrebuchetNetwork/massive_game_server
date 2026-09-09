#!/usr/bin/env node
// Generals worker: a commanding model per team.
//
// Every cycle this reads the live battlefield, renders it as a top-down
// tactical map, and asks an OpenRouter model for ONE mass call for its side.
// The order is posted to the server, which validates it and pushes it onto
// the commander channel the ships already follow.
//
// The model call happens here, outside the game tick, so a slow or failed
// completion can never stall the simulation — an unanswered cycle simply
// lets the standing order expire.
//
// Usage:
//   node general_worker.mjs [--once] [--dry-run] [--interval-ms 6000]
// Env:
//   MGS_BASE_URL            default http://127.0.0.1:8080
//   MGS_ADMIN_TOKEN_FILE    default ~/.config/massive-game-server/secrets/arena-admin-bearer-token
//   OPENROUTER_KEY_FILE     default ~/.secrets/openrouter-arena.key
//   GENERAL_MODEL_TEAM1 / GENERAL_MODEL_TEAM2
//   GENERAL_MAX_CALLS       stop after N model calls (cost guard, default 0 = unlimited)

import { readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import path from 'node:path';
import { createCanvas, drawText, encodePng } from '../media/lib/raster.mjs';

const args = process.argv.slice(2);
const flag = (name) => args.includes(`--${name}`);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};

const BASE = process.env.MGS_BASE_URL || 'http://127.0.0.1:8080';
const TOKEN_FILE = process.env.MGS_ADMIN_TOKEN_FILE
  || path.join(homedir(), '.config/massive-game-server/secrets/arena-admin-bearer-token');
const KEY_FILE = process.env.OPENROUTER_KEY_FILE || path.join(homedir(), '.secrets/openrouter-arena.key');
const MODELS = {
  1: process.env.GENERAL_MODEL_TEAM1 || 'anthropic/claude-opus-5',
  2: process.env.GENERAL_MODEL_TEAM2 || 'openai/gpt-6-astra',
};
// Reasoning + vision runs roughly 1.3k tokens per call; at two teams that is
// about $0.04 a cycle on frontier models. 30s keeps a commanded match near
// $5/hour. Lower it for a showcase, not for an all-day run.
const INTERVAL_MS = Number(opt('interval-ms', process.env.GENERAL_INTERVAL_MS || 30000));
const MAX_CALLS = Number(process.env.GENERAL_MAX_CALLS || 0);
const ONCE = flag('once');
const DRY_RUN = flag('dry-run');
const MODEL_TIMEOUT_MS = Number(process.env.GENERAL_MODEL_TIMEOUT_MS || 20000);

// World bounds, mirrored from server constants.
const WORLD = { minX: -800, maxX: 800, minY: -600, maxY: 600 };
const MAP_W = 640;
const MAP_H = 480;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);

async function readSecret(file) {
  return (await readFile(file, 'utf8')).trim();
}

async function getJson(url, token) {
  const headers = { Accept: 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await fetch(url, { headers, signal: AbortSignal.timeout(8000) });
  if (!res.ok) throw new Error(`${url} -> ${res.status}`);
  return res.json();
}

// ── battlefield snapshot ──────────────────────────────────────────────────

async function battlefield(token) {
  const [replay, board] = await Promise.all([
    getJson(`${BASE}/api/ops/live-replay/recent?limit=1`, token),
    getJson(`${BASE}/api/public/match/scoreboard`, null),
  ]);
  const frame = (replay.frames || [])[0] || null;
  const ships = frame ? (frame.sampled_players || []) : [];
  return { ships, board };
}

function summarise(ships, board, teamId) {
  const enemyId = teamId === 1 ? 2 : 1;
  const mine = ships.filter((s) => s.team_id === teamId);
  const theirs = ships.filter((s) => s.team_id === enemyId);
  const stats = (list) => {
    if (!list.length) return { n: 0, alive: 0, hp: 0, cx: 0, cy: 0 };
    const alive = list.filter((s) => s.alive);
    const hp = Math.round(list.reduce((a, s) => a + (s.health || 0), 0) / list.length);
    const cx = Math.round(list.reduce((a, s) => a + s.x, 0) / list.length);
    const cy = Math.round(list.reduce((a, s) => a + s.y, 0) / list.length);
    return { n: list.length, alive: alive.length, hp, cx, cy };
  };
  const me = stats(mine);
  const foe = stats(theirs);
  const score = (id) => (board.team_scores || []).find((t) => t.team_id === id)?.score ?? 0;
  const roster = mine
    .slice(0, 12)
    .map((s) => `${s.username.replace(/\s*\[selfware\.design\]/, '')} hp=${s.health} at (${Math.round(s.x)},${Math.round(s.y)})`)
    .join('; ');
  return {
    text: [
      `Mode: ${board.game_mode}. Time remaining: ${Math.round(board.time_remaining)}s.`,
      `You command TEAM ${teamId}. Score ${score(teamId)} vs ${score(enemyId)}.`,
      `Your force: ${me.alive}/${me.n} alive, average health ${me.hp}, centre of mass (${me.cx}, ${me.cy}).`,
      `Enemy force: ${foe.alive}/${foe.n} alive, average health ${foe.hp}, centre of mass (${foe.cx}, ${foe.cy}).`,
      `Your base is at (${teamId === 1 ? -700 : 700}, 0); the enemy base is at (${teamId === 1 ? 700 : -700}, 0).`,
      `World bounds: x ${WORLD.minX}..${WORLD.maxX}, y ${WORLD.minY}..${WORLD.maxY}.`,
      `Your ships: ${roster || 'none visible'}.`,
    ].join('\n'),
    me,
    foe,
  };
}

// ── tactical map ──────────────────────────────────────────────────────────

function px(cv, x, y, rgb, a = 1) {
  x |= 0; y |= 0;
  if (x < 0 || y < 0 || x >= cv.width || y >= cv.height) return;
  const i = (y * cv.width + x) * 4;
  const ia = 1 - a;
  cv.data[i] = rgb[0] * a + cv.data[i] * ia;
  cv.data[i + 1] = rgb[1] * a + cv.data[i + 1] * ia;
  cv.data[i + 2] = rgb[2] * a + cv.data[i + 2] * ia;
  cv.data[i + 3] = 255;
}

function box(cv, cx, cy, r, rgb, a = 1) {
  for (let y = cy - r; y <= cy + r; y++) for (let x = cx - r; x <= cx + r; x++) px(cv, x, y, rgb, a);
}

function toMap(x, y) {
  return [
    Math.round(((x - WORLD.minX) / (WORLD.maxX - WORLD.minX)) * (MAP_W - 1)),
    Math.round(((y - WORLD.minY) / (WORLD.maxY - WORLD.minY)) * (MAP_H - 1)),
  ];
}

function renderBattlefield(ships, board, teamId) {
  const cv = createCanvas(MAP_W, MAP_H);
  for (let i = 0; i < cv.data.length; i += 4) {
    cv.data[i] = 14; cv.data[i + 1] = 17; cv.data[i + 2] = 23; cv.data[i + 3] = 255;
  }
  // grid every 200 world units
  for (let gx = WORLD.minX; gx <= WORLD.maxX; gx += 200) {
    const [mx] = toMap(gx, 0);
    for (let y = 0; y < MAP_H; y++) px(cv, mx, y, [40, 46, 58], 0.7);
  }
  for (let gy = WORLD.minY; gy <= WORLD.maxY; gy += 200) {
    const [, my] = toMap(0, gy);
    for (let x = 0; x < MAP_W; x++) px(cv, x, my, [40, 46, 58], 0.7);
  }
  // bases
  const friendly = [96, 200, 255];
  const hostile = [255, 110, 110];
  const [b1x, b1y] = toMap(-700, 0);
  const [b2x, b2y] = toMap(700, 0);
  box(cv, b1x, b1y, 6, teamId === 1 ? friendly : hostile, 0.55);
  box(cv, b2x, b2y, 6, teamId === 2 ? friendly : hostile, 0.55);
  drawText(cv, b1x - 18, b1y - 20, 'BASE 1', { scale: 1, color: [150, 160, 180] });
  drawText(cv, b2x - 18, b2y - 20, 'BASE 2', { scale: 1, color: [150, 160, 180] });

  for (const s of ships) {
    const [mx, my] = toMap(s.x, s.y);
    const own = s.team_id === teamId;
    const rgb = own ? friendly : hostile;
    const r = s.alive ? 3 : 1;
    box(cv, mx, my, r, rgb, s.alive ? 1 : 0.35);
    // health pip above the ship
    const hp = Math.max(0, Math.min(100, s.health || 0));
    for (let i = 0; i < Math.round(hp / 12); i++) px(cv, mx - 2 + i, my - 5, [120, 230, 140], 0.9);
  }

  drawText(cv, 8, 8, `YOU=BLUE TEAM ${teamId}  ENEMY=RED  ${board.game_mode}`, { scale: 1, color: [220, 226, 240] });
  drawText(cv, 8, 20, `t-${Math.round(board.time_remaining)}s`, { scale: 1, color: [160, 170, 190] });
  return Buffer.from(encodePng(cv));
}

// ── the general ───────────────────────────────────────────────────────────

const SYSTEM_BRIEF = `You are a GENERAL commanding one team of ships in a real-time arena.
You do not pilot ships. You issue ONE mass call that your whole team acts on.

Allowed orders:
  mass_attack       commit the team to a point (this overrides individual ship movement)
  retreat           break contact and regroup at a point (also overrides movement)
  hold              issue no repositioning; ships use their own judgement
  push_objective    press the enemy objective
  defend_objective  fall back onto your own objective

Answer with STRICT JSON only, no prose, no code fence:
{"order":"mass_attack","target_x":0,"target_y":0,"rationale":"under 15 words"}
target_x/target_y are world coordinates. Choose the point that wins the fight.`;

function parseOrder(raw) {
  if (!raw) return null;
  const match = raw.match(/\{[\s\S]*\}/);
  if (!match) return null;
  try {
    const parsed = JSON.parse(match[0]);
    if (!parsed || typeof parsed.order !== 'string') return null;
    return {
      order: parsed.order,
      target_x: Number(parsed.target_x),
      target_y: Number(parsed.target_y),
      rationale: typeof parsed.rationale === 'string' ? parsed.rationale : '',
    };
  } catch (_) {
    return null;
  }
}

async function askGeneral(key, model, briefing, png) {
  const body = {
    model,
    // Reasoning models spend most of their completion budget thinking; a
    // 200-token cap truncated the JSON mid-object on vision calls.
    max_tokens: 900,
    temperature: 0.4,
    messages: [
      { role: 'system', content: SYSTEM_BRIEF },
      {
        role: 'user',
        content: [
          { type: 'text', text: briefing },
          { type: 'image_url', image_url: { url: `data:image/png;base64,${png.toString('base64')}` } },
        ],
      },
    ],
  };
  const res = await fetch('https://openrouter.ai/api/v1/chat/completions', {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${key}`,
      'Content-Type': 'application/json',
      'HTTP-Referer': 'https://space.selfware.design',
      'X-Title': 'Model Arena Generals',
    },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(MODEL_TIMEOUT_MS),
  });
  if (!res.ok) throw new Error(`openrouter ${res.status}: ${(await res.text()).slice(0, 200)}`);
  const data = await res.json();
  const message = data?.choices?.[0]?.message || {};
  const text = message.content;
  let content = Array.isArray(text) ? text.map((c) => c.text || '').join(' ') : text;
  // Some models put everything in `reasoning` and leave `content` null.
  if (!content && typeof message.reasoning === 'string') content = message.reasoning;
  return {
    order: parseOrder(content),
    usage: data?.usage || null,
    raw: content,
    finish: data?.choices?.[0]?.finish_reason,
  };
}

async function postOrder(token, order) {
  const res = await fetch(`${BASE}/api/ops/general/order`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify(order),
    signal: AbortSignal.timeout(8000),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok || !data.ok) throw new Error(`order rejected: ${JSON.stringify(data).slice(0, 200)}`);
  return data.order;
}

async function cycle(token, key, calls) {
  const { ships, board } = await battlefield(token);
  if (!ships.length) { log('no live frame yet; skipping cycle'); return calls; }

  for (const teamId of [1, 2]) {
    if (MAX_CALLS && calls >= MAX_CALLS) { log(`call budget ${MAX_CALLS} reached`); return calls; }
    const model = MODELS[teamId];
    const { text } = summarise(ships, board, teamId);
    const png = renderBattlefield(ships, board, teamId);
    try {
      const started = Date.now();
      const { order, usage, raw, finish } = await askGeneral(key, model, text, png);
      calls += 1;
      if (!order) {
        log(`team ${teamId} ${model}: unusable reply (finish=${finish}): ${String(raw).slice(0, 160)}`);
        continue;
      }
      const payload = {
        team_id: teamId,
        order: order.order,
        target_x: Number.isFinite(order.target_x) ? order.target_x : 0,
        target_y: Number.isFinite(order.target_y) ? order.target_y : 0,
        model_id: model,
        model_name: model.split('/').pop(),
        rationale: order.rationale,
        // Stand until the next cycle has had a chance to land, then lapse.
        ttl_ms: INTERVAL_MS + 6000,
      };
      const tokens = usage ? `${usage.total_tokens ?? '?'}tok` : 'n/a';
      if (DRY_RUN) {
        log(`[dry-run] team ${teamId} ${model} -> ${payload.order} (${payload.target_x},${payload.target_y}) "${payload.rationale}" ${Date.now() - started}ms ${tokens}`);
      } else {
        const applied = await postOrder(token, payload);
        log(`team ${teamId} ${model} -> ${applied.posture} (${Math.round(applied.target_x)},${Math.round(applied.target_y)}) "${applied.rationale}" ${Date.now() - started}ms ${tokens}`);
      }
    } catch (err) {
      log(`team ${teamId} ${model} failed: ${String(err).slice(0, 200)}`);
    }
  }
  return calls;
}

(async () => {
  const [token, key] = await Promise.all([readSecret(TOKEN_FILE), readSecret(KEY_FILE)]);
  log(`generals worker: team1=${MODELS[1]} team2=${MODELS[2]} interval=${INTERVAL_MS}ms${DRY_RUN ? ' (dry-run)' : ''}`);
  let calls = 0;
  for (;;) {
    try {
      calls = await cycle(token, key, calls);
    } catch (err) {
      log(`cycle failed: ${String(err).slice(0, 200)}`);
    }
    if (ONCE || (MAX_CALLS && calls >= MAX_CALLS)) break;
    await sleep(INTERVAL_MS);
  }
})();
