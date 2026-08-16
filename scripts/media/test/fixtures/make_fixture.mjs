// Generate a synthetic replay fixture: 2 teams x 5 bots, 90s at 20fps,
// sinusoidal movement, sparse kills early + a 4-kill cluster near 60s and a
// final low-hp showdown. Writes fixtures/synthetic_match.json and .json.zst.
import { writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const DIR = path.dirname(fileURLToPath(import.meta.url));
const FPS = 20;
const DURATION_S = 90;
const NAMES = [
  '#1 DeepSeek: DeepSeek V4 Pro', '#2 OpenAI: GPT-5.6 Luna',
  '#3 DeepSeek: DeepSeek V4 Flash 0731', '#4 Z.ai: GLM 5.2',
  '#5 Google: Gemini 3.6 Flash',
  '#6 NVIDIA: Nemotron 3 Ultra (free)', '#7 Tencent: Hy3',
  '#8 Poolside: Laguna S 2.1 (free)', '#9 DeepSeek: DeepSeek V4 Flash 0423',
  '#10 Xiaomi: MiMo-V2.5',
];
// Kill schedule: victim index -> death time (s). Cluster at 58-62s.
const DEATHS = new Map([
  [7, 12], [3, 25], [1, 58], [6, 59.5], [2, 61], [8, 62], [9, 78],
]);
const RESPAWN_S = 6;

const bots = NAMES.map((name, i) => ({
  id: `bot_${i}`, name, team: i < 5 ? 1 : 2,
  baseX: (i < 5 ? -1 : 1) * (300 + (i % 5) * 100),
  baseY: -400 + (i % 5) * 200,
  phase: i * 1.7,
}));
const deathTimes = new Map([...DEATHS.entries()].map(([i, t]) => [`bot_${i}`, t]));

const frames = [];
const startMs = 1_780_000_000_000;
for (let f = 0; f <= FPS * DURATION_S; f++) {
  const t = f / FPS;
  const players = bots.map((b, i) => {
    const deathT = deathTimes.get(b.id);
    let alive = true, hp = 100;
    if (deathT !== undefined) {
      const sinceDeath = t - deathT;
      if (sinceDeath >= 0 && sinceDeath < RESPAWN_S) alive = false;
      // Ramp hp down toward the death, back up after respawn.
      hp = sinceDeath < 0 ? Math.max(2, 100 - (100 / 8) * Math.max(0, 8 - (deathT - t))) : 100 - sinceDeath * 10;
      hp = Math.round(Math.max(2, Math.min(100, hp)));
    }
    const speed = 60 + 20 * Math.sin(b.phase + t * 0.5);
    const ang = b.phase + t * (0.3 + (i % 3) * 0.1);
    const x = Math.round((b.baseX + Math.cos(ang) * 220 + Math.sin(t * 0.9 + b.phase) * 60) * 100) / 100;
    const y = Math.round((b.baseY + Math.sin(ang) * 220 + Math.cos(t * 0.7 + b.phase) * 60) * 100) / 100;
    return {
      alive, health: hp, player_id: b.id, team_id: b.team, username: b.name,
      velocity_x: Math.round(Math.cos(ang) * speed * 100) / 100,
      velocity_y: Math.round(Math.sin(ang) * speed * 100) / 100,
      x, y,
    };
  });
  frames.push({
    events: 0, frame: f, kill_feed_size: 0, pickups: 0,
    players: players.length, projectiles: 0,
    sampled_players: players,
    timestamp_ms: startMs + f * (1000 / FPS),
  });
}

const doc = {
  frame_count: frames.length,
  frames,
  generated_at_ms: startMs + DURATION_S * 1000,
  map_name: 'synthetic-void',
  reason: 'time_expired',
};
const jsonPath = path.join(DIR, 'synthetic_match.json');
writeFileSync(jsonPath, JSON.stringify(doc));
execFileSync('/usr/bin/zstd', ['-f', '-q', jsonPath]);
console.log(`wrote ${jsonPath} + .zst (${frames.length} frames)`);
