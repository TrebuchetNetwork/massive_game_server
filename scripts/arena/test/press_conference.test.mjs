// Tests for press_conference.mjs — event selection, quote sanitation, and the
// no-file-on-failure guarantee. LLM traffic is always an injected fetchImpl;
// these tests never touch the network.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import {
  buildPressPrompt,
  providerModelFor,
  readOpenRouterKey,
  runPressConference,
  sanitizeQuote,
  selectEvent,
  speakerStats,
} from '../press_conference.mjs';
import { trackPolicy } from '../continuous/league.mjs';

const NOW = Date.parse('2026-09-16T12:00:00.000Z');
const HOUR = 3_600_000;
const HEX = (c) => c.repeat(64);

const mascot = (emoji, title, color) => ({ key: null, emoji, title, color });

function entry(over = {}) {
  return {
    model_id: 'vendor/model-a', slug: 'vendor/model-a-20260101',
    mascot: mascot('🦊', 'Alpha', '#caff00'),
    joined_at: '2026-09-01T00:00:00.000Z', submissions_used: 1,
    artifact: {
      wasm_sha256: HEX('a'), source_sha256: HEX('b'), prompt_sha256: HEX('c'),
      version: 1, parent_version: null,
    },
    rating: 50, wins: 4, losses: 4, draws: 2, matches: 10,
    days_in_league: 5, status: 'active',
    ...over,
  };
}

function trackSlice(trackId, over = {}) {
  const policy = trackPolicy(trackId);
  return {
    day_index: 3,
    policy: {
      max_submissions: policy.maxSubmissions,
      compile_attempts: policy.compileAttempts,
      feedback_interval_ms: policy.feedbackIntervalMs,
      max_revisions: policy.maxRevisions,
    },
    roster: [], retired: [], announcements: [], last_feedback_at: null,
    ...over,
  };
}

function makeState(trackOverrides = {}) {
  const tracks = {};
  for (const t of ['L0', 'L1', 'L2', 'L3']) tracks[t] = trackSlice(t, trackOverrides[t]);
  return {
    schema_version: 2,
    league_id: 'cml-test',
    tracks,
    created_at: '2026-09-01T00:00:00.000Z',
    updated_at: new Date(NOW - HOUR).toISOString(),
    recruit_failures: {},
  };
}

const snap = (at, roster) => ({ at, league_id: 'cml-test', track: 'L0', day_index: 1, season_id: 's', roster });
const snapEntry = (modelId, rating, over = {}) => ({
  model_id: modelId, slug: `${modelId}-20260101`, rating, wins: 1, losses: 1, draws: 0, matches: 2, ...over,
});

// ---------------------------------------------------------------------------
// selectEvent
// ---------------------------------------------------------------------------

test('selectEvent returns null for a quiet league', () => {
  const state = makeState();
  const snapshots = {
    L0: [
      snap(new Date(NOW - 25 * HOUR).toISOString(), [snapEntry('vendor/a', 60), snapEntry('vendor/b', 50)]),
      snap(new Date(NOW - HOUR).toISOString(), [snapEntry('vendor/a', 61), snapEntry('vendor/b', 49)]),
    ],
    L1: [], L2: [], L3: [],
  };
  assert.equal(selectEvent({ state, snapshots, nowMs: NOW }), null);
});

test('selectEvent picks a leader change from the last two snapshots', () => {
  const state = makeState();
  const snapshots = {
    L0: [
      snap(new Date(NOW - 25 * HOUR).toISOString(), [snapEntry('vendor/a', 60), snapEntry('vendor/b', 50)]),
      snap(new Date(NOW - HOUR).toISOString(), [snapEntry('vendor/b', 62), snapEntry('vendor/a', 60)]),
    ],
    L1: [], L2: [], L3: [],
  };
  const event = selectEvent({ state, snapshots, nowMs: NOW });
  assert.equal(event.type, 'leader_change');
  assert.equal(event.model_id, 'vendor/b');
  assert.match(event.caption, /new leader of track L0/);
  assert.match(event.caption, /rating 62/);
  assert.match(event.caption, /from a \(rating 60\)/);
});

test('selectEvent ignores a stale leader change (latest snapshot outside the window)', () => {
  const state = makeState();
  const snapshots = {
    L0: [
      snap(new Date(NOW - 49 * HOUR).toISOString(), [snapEntry('vendor/a', 60), snapEntry('vendor/b', 50)]),
      snap(new Date(NOW - 26 * HOUR).toISOString(), [snapEntry('vendor/b', 62), snapEntry('vendor/a', 60)]),
    ],
    L1: [], L2: [], L3: [],
  };
  assert.equal(selectEvent({ state, snapshots, nowMs: NOW }), null);
});

test('selectEvent ranks leader change above retirement', () => {
  const state = makeState({
    L0: {
      announcements: [{
        type: 'retirement', track: 'L0', model_id: 'vendor/dead', slug: 'vendor/dead-20260101',
        mascot: mascot('🪦', 'Dead', '#999999'), reason: 'rating 30 < 35',
        stats: { rating: 30, wins: 1, losses: 9, draws: 0, matches: 10, days_in_league: 6, submissions_used: 1 },
        at: new Date(NOW - 2 * HOUR).toISOString(),
      }],
    },
  });
  const snapshots = {
    L0: [
      snap(new Date(NOW - 25 * HOUR).toISOString(), [snapEntry('vendor/a', 60), snapEntry('vendor/b', 50)]),
      snap(new Date(NOW - HOUR).toISOString(), [snapEntry('vendor/b', 62), snapEntry('vendor/a', 60)]),
    ],
    L1: [], L2: [], L3: [],
  };
  const event = selectEvent({ state, snapshots, nowMs: NOW });
  assert.equal(event.type, 'leader_change');
});

test('selectEvent ranks retirement above debutant', () => {
  const at = new Date(NOW - 2 * HOUR).toISOString();
  const state = makeState({
    L0: {
      announcements: [
        {
          type: 'entrant', track: 'L0', model_id: 'vendor/newbie', slug: 'vendor/newbie-20260101',
          mascot: mascot('🌱', 'Newbie', '#00e0ff'), provider_rank: 7, at,
        },
        {
          type: 'retirement', track: 'L0', model_id: 'vendor/dead', slug: 'vendor/dead-20260101',
          mascot: mascot('🪦', 'Dead', '#999999'), reason: 'displaced by fresh challenger vendor/newbie',
          stats: { rating: 40, wins: 5, losses: 5, draws: 0, matches: 10, days_in_league: 9, submissions_used: 1 },
          at,
        },
      ],
    },
  });
  const event = selectEvent({ state, snapshots: { L0: [], L1: [], L2: [], L3: [] }, nowMs: NOW });
  assert.equal(event.type, 'retirement');
  assert.equal(event.model_id, 'vendor/dead');
  assert.match(event.caption, /retired from track L0/);
  assert.match(event.caption, /displaced by fresh challenger/);
});

test('selectEvent picks a debutant when nothing bigger happened', () => {
  const state = makeState({
    L2: {
      announcements: [{
        type: 'fresh_challenger', track: 'L2', model_id: 'vendor/newbie', slug: 'vendor/newbie-20260101',
        mascot: mascot('🌱', 'Newbie', '#00e0ff'),
        at: new Date(NOW - 3 * HOUR).toISOString(),
      }],
    },
  });
  const event = selectEvent({ state, snapshots: { L0: [], L1: [], L2: [], L3: [] }, nowMs: NOW });
  assert.equal(event.type, 'debutant');
  assert.equal(event.track, 'L2');
  assert.match(event.caption, /fresh challenger/);
});

test('selectEvent ignores announcements older than 24h', () => {
  const state = makeState({
    L0: {
      announcements: [{
        type: 'retirement', track: 'L0', model_id: 'vendor/dead', slug: 'vendor/dead-20260101',
        mascot: mascot('🪦', 'Dead', '#999999'), reason: 'old news',
        at: new Date(NOW - 26 * HOUR).toISOString(),
      }],
    },
  });
  assert.equal(selectEvent({ state, snapshots: { L0: [], L1: [], L2: [], L3: [] }, nowMs: NOW }), null);
});

test('selectEvent picks cross-track divergence >= 5, skips smaller spreads', () => {
  const big = makeState({
    L0: { roster: [entry({ model_id: 'vendor/a', rating: 60 })] },
    L3: { roster: [entry({ model_id: 'vendor/a', rating: 52 })] },
  });
  const event = selectEvent({ state: big, snapshots: { L0: [], L1: [], L2: [], L3: [] }, nowMs: NOW });
  assert.equal(event.type, 'divergence');
  assert.equal(event.model_id, 'vendor/a');
  assert.equal(event.track, 'L0', 'track is the one with the higher rating');
  assert.match(event.caption, /spread 8\.00/);

  const small = makeState({
    L0: { roster: [entry({ model_id: 'vendor/a', rating: 60 })] },
    L3: { roster: [entry({ model_id: 'vendor/a', rating: 56 })] },
  });
  assert.equal(selectEvent({ state: small, snapshots: { L0: [], L1: [], L2: [], L3: [] }, nowMs: NOW }), null);
});

test('speakerStats reads the retired ledger for a retirement event', () => {
  const retired = {
    ...entry({ model_id: 'vendor/dead', rating: 30, wins: 1, losses: 9, draws: 0, matches: 10 }),
    retired_at: new Date(NOW - HOUR).toISOString(), reason: 'rating 30 < 35',
  };
  const state = makeState({ L0: { retired: [retired] } });
  const event = { type: 'retirement', track: 'L0', model_id: 'vendor/dead', slug: 'vendor/dead-20260101' };
  const stats = speakerStats({ state, event });
  assert.equal(stats.rating, 30);
  assert.equal(stats.losses, 9);
  assert.equal(stats.mascot.title, 'Alpha');
});

// ---------------------------------------------------------------------------
// sanitizeQuote
// ---------------------------------------------------------------------------

test('sanitizeQuote strips markdown, quotes and newlines', () => {
  const raw = '  "**We** did _great_ today!"\n\n> Follow-up: `charge` more.  ';
  assert.equal(sanitizeQuote(raw), 'We did great today! Follow-up: charge more.');
});

test('sanitizeQuote keeps at most two sentences', () => {
  const raw = 'One sentence here. Another one follows! A third sneaks in? Fourth.';
  assert.equal(sanitizeQuote(raw), 'One sentence here. Another one follows!');
});

test('sanitizeQuote caps at 280 chars on a word boundary', () => {
  const raw = `${'word '.repeat(120)}end.`;
  const out = sanitizeQuote(raw);
  assert.ok(out.length <= 280, `length ${out.length}`);
  assert.ok(out.endsWith('…'));
  assert.ok(!out.endsWith(' …'), 'no dangling partial word');
});

test('sanitizeQuote converts markdown links to their label', () => {
  assert.equal(sanitizeQuote('See [the standings](https://example.com) now.'), 'See the standings now.');
});

test('sanitizeQuote drops a trailing fragment cut off mid-sentence', () => {
  // Provider hit the token cap after one complete sentence.
  assert.equal(
    sanitizeQuote('Four days was a short but fierce sprint. My record and rating show I wasn'),
    'Four days was a short but fierce sprint.',
  );
  // Nothing complete at all -> unusable, no quote is published.
  assert.equal(sanitizeQuote('My record and rating show I wasn'), null);
  // A complete quote ending in terminal punctuation is untouched.
  assert.equal(sanitizeQuote('Ready to fight.'), 'Ready to fight.');
  assert.equal(sanitizeQuote('What a match!'), 'What a match!');
});

test('sanitizeQuote returns null for unusable output', () => {
  assert.equal(sanitizeQuote(''), null);
  assert.equal(sanitizeQuote('   \n  '), null);
  assert.equal(sanitizeQuote('***`#>`'), null);
  assert.equal(sanitizeQuote(null), null);
  assert.equal(sanitizeQuote(42), null);
});

// ---------------------------------------------------------------------------
// buildPressPrompt
// ---------------------------------------------------------------------------

test('buildPressPrompt carries persona, event and real numbers only', () => {
  const event = { type: 'retirement', track: 'L0', caption: 'mimo-v2.5 retired from track L0 — rating 25.61 < 35.' };
  const stats = { rating: 25.61, wins: 240, losses: 802, draws: 110, matches: 1152, days_in_league: 4, slug: 'xiaomi/mimo-v2.5-20260422' };
  const prompt = buildPressPrompt({ event, stats, mascot: mascot('🦊', 'Kitsune', '#f97316') });
  assert.match(prompt, /mascot "Kitsune" 🦊/);
  assert.match(prompt, /mimo-v2\.5 retired from track L0/);
  assert.match(prompt, /rating 25\.61/);
  assert.match(prompt, /240W-802L-110D in 1152 matches/);
  assert.match(prompt, /4 days in the league/);
  // No coaching / strategy vocabulary in the press prompt either.
  assert.doesNotMatch(prompt.toLowerCase(), /you should|improve your|try to|increase|decrease/);
});

test('providerModelFor strips the fast-lane provisional marker', () => {
  assert.equal(providerModelFor('~deepseek/deepseek-flash-latest'), 'deepseek/deepseek-flash-latest');
  assert.equal(providerModelFor('xiaomi/mimo-v2.5'), 'xiaomi/mimo-v2.5');
});

// ---------------------------------------------------------------------------
// readOpenRouterKey
// ---------------------------------------------------------------------------

test('readOpenRouterKey resolves env, then env-pointed file, then null', async () => {
  assert.equal(await readOpenRouterKey({ env: { OPENROUTER_API_KEY: '  sk-live  ' } }), 'sk-live');
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'press-key-')), 'k');
  fs.writeFileSync(file, 'sk-from-file\n');
  assert.equal(await readOpenRouterKey({ env: { OPENROUTER_API_KEY_FILE: file } }), 'sk-from-file');
  assert.equal(await readOpenRouterKey({ env: {} }), null);
  assert.equal(await readOpenRouterKey({ env: { OPENROUTER_API_KEY_FILE: '/no/such/file' } }), null);
});

// ---------------------------------------------------------------------------
// runPressConference (temp-dir state, injected fetch)
// ---------------------------------------------------------------------------

function writeLeagueFixture(dir, trackOverrides = {}, snapshots = {}) {
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'state.json'), JSON.stringify(makeState(trackOverrides)));
  for (const [trackId, list] of Object.entries(snapshots)) {
    const historyDir = path.join(dir, 'tracks', trackId, 'history');
    fs.mkdirSync(historyDir, { recursive: true });
    fs.writeFileSync(path.join(historyDir, '2026-09-16.json'), JSON.stringify(list));
  }
}

function debutantFixture(dir) {
  const newcomer = entry({
    model_id: 'vendor/newbie', slug: 'vendor/newbie-20260101',
    mascot: mascot('🌱', 'Newbie', '#00e0ff'),
    rating: 50, wins: 0, losses: 0, draws: 0, matches: 0, days_in_league: 0,
  });
  writeLeagueFixture(dir, {
    L0: {
      roster: [newcomer],
      announcements: [{
        type: 'entrant', track: 'L0', model_id: 'vendor/newbie', slug: 'vendor/newbie-20260101',
        mascot: mascot('🌱', 'Newbie', '#00e0ff'), provider_rank: 3,
        at: new Date(NOW - HOUR).toISOString(),
      }],
    },
  });
}

const okFetch = (content) => async () => ({
  ok: true,
  json: async () => ({ choices: [{ message: { content } }] }),
});

const now = () => new Date(NOW);

test('runPressConference writes one sanitized quote for the day event', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'press-run-'));
  debutantFixture(dir);
  const result = await runPressConference({
    continuousDir: dir,
    now,
    env: { OPENROUTER_API_KEY: 'sk-test' },
    fetchImpl: okFetch('  "**I** just got here,\nand I already love the cage!" said the model. Extra sentence dropped.  '),
  });
  assert.equal(result.status, 'written');
  const record = JSON.parse(fs.readFileSync(result.file, 'utf8'));
  assert.equal(record.event.type, 'debutant');
  assert.equal(record.event.track, 'L0');
  assert.match(record.event.caption, /league debut/);
  assert.equal(record.model_id, 'vendor/newbie');
  assert.equal(record.slug, 'vendor/newbie-20260101');
  assert.deepEqual(record.mascot, { key: null, emoji: '🌱', title: 'Newbie', color: '#00e0ff' });
  assert.equal(record.quote, 'I just got here, and I already love the cage! said the model.');
  assert.equal(record.generated_at, new Date(NOW).toISOString());
  assert.match(record.prompt_sha256, /^[a-f0-9]{64}$/);
  // The file lands in <continuousDir>/press/<utc-date>.json.
  assert.equal(path.basename(result.file), '2026-09-16.json');
  assert.equal(path.basename(path.dirname(result.file)), 'press');
});

test('runPressConference passes the model own provider id to the chat call', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'press-run-'));
  debutantFixture(dir);
  let seenBody = null;
  const fetchImpl = async (url, init) => {
    seenBody = JSON.parse(init.body);
    return { ok: true, json: async () => ({ choices: [{ message: { content: 'Ready to fight.' } }] }) };
  };
  await runPressConference({
    continuousDir: dir, now, env: { OPENROUTER_API_KEY: 'sk-test' }, fetchImpl,
  });
  assert.equal(seenBody.model, 'vendor/newbie');
  assert.equal(seenBody.messages.length, 1);
  assert.match(seenBody.messages[0].content, /league debut/);
});

test('runPressConference writes NO file on provider failure', async () => {
  for (const status of [402, 403, 429, 500]) {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'press-run-'));
    debutantFixture(dir);
    const result = await runPressConference({
      continuousDir: dir,
      now,
      env: { OPENROUTER_API_KEY: 'sk-test' },
      fetchImpl: async () => ({ ok: false, status }),
    });
    assert.equal(result.status, 'failed', `HTTP ${status}`);
    assert.match(result.reason, new RegExp(`HTTP ${status}`));
    assert.ok(!fs.existsSync(path.join(dir, 'press')), 'no press dir created');
  }
});

test('runPressConference writes NO file on timeout or unusable output', async () => {
  const dirTimeout = fs.mkdtempSync(path.join(os.tmpdir(), 'press-run-'));
  debutantFixture(dirTimeout);
  const timedOut = await runPressConference({
    continuousDir: dirTimeout,
    now,
    env: { OPENROUTER_API_KEY: 'sk-test' },
    fetchImpl: async () => { throw new Error('The operation timed out'); },
  });
  assert.equal(timedOut.status, 'failed');
  assert.ok(!fs.existsSync(path.join(dirTimeout, 'press')));

  const dirBad = fs.mkdtempSync(path.join(os.tmpdir(), 'press-run-'));
  debutantFixture(dirBad);
  const bad = await runPressConference({
    continuousDir: dirBad,
    now,
    env: { OPENROUTER_API_KEY: 'sk-test' },
    fetchImpl: okFetch('***`#>`'), // passes transport, dies in sanitation
  });
  assert.equal(bad.status, 'failed');
  assert.match(bad.reason, /sanitation/);
  assert.ok(!fs.existsSync(path.join(dirBad, 'press')));

  const dirEmpty = fs.mkdtempSync(path.join(os.tmpdir(), 'press-run-'));
  debutantFixture(dirEmpty);
  const empty = await runPressConference({
    continuousDir: dirEmpty,
    now,
    env: { OPENROUTER_API_KEY: 'sk-test' },
    fetchImpl: okFetch('   \n  '), // no usable content at all
  });
  assert.equal(empty.status, 'failed');
  assert.ok(!fs.existsSync(path.join(dirEmpty, 'press')));
});

test('runPressConference writes NO file when no API key is configured', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'press-run-'));
  debutantFixture(dir);
  let called = false;
  const result = await runPressConference({
    continuousDir: dir,
    now,
    env: {},
    fetchImpl: async () => { called = true; throw new Error('must not be called'); },
  });
  assert.equal(result.status, 'failed');
  assert.equal(called, false);
  assert.ok(!fs.existsSync(path.join(dir, 'press')));
});

test('runPressConference skips a quiet day without any provider call', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'press-run-'));
  writeLeagueFixture(dir);
  let called = false;
  const result = await runPressConference({
    continuousDir: dir,
    now,
    env: { OPENROUTER_API_KEY: 'sk-test' },
    fetchImpl: async () => { called = true; throw new Error('must not be called'); },
  });
  assert.equal(result.status, 'quiet');
  assert.equal(called, false);
});

test('runPressConference never makes a second call when today file exists', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'press-run-'));
  debutantFixture(dir);
  const pressDir = path.join(dir, 'press');
  fs.mkdirSync(pressDir, { recursive: true });
  fs.writeFileSync(path.join(pressDir, '2026-09-16.json'), '{"already":"here"}');
  let called = false;
  const result = await runPressConference({
    continuousDir: dir,
    now,
    env: { OPENROUTER_API_KEY: 'sk-test' },
    fetchImpl: async () => { called = true; throw new Error('must not be called'); },
  });
  assert.equal(result.status, 'exists');
  assert.equal(called, false);
  // --force bypasses the once-per-day guard (manual regeneration only).
  const forced = await runPressConference({
    continuousDir: dir,
    now,
    env: { OPENROUTER_API_KEY: 'sk-test' },
    fetchImpl: okFetch('Back for one more question.'),
    force: true,
  });
  assert.equal(forced.status, 'written');
});

test('runPressConference reports unavailable when the league state is invalid', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'press-run-'));
  fs.writeFileSync(path.join(dir, 'state.json'), '{"schema_version":1}');
  const result = await runPressConference({ continuousDir: dir, now, env: {} });
  assert.equal(result.status, 'unavailable');
});
