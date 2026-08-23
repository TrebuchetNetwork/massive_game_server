import { test } from 'node:test';
import assert from 'node:assert/strict';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { mascotFor } from '../../mascots.mjs';
import {
  MAX_ROSTER_SIZE,
  SCHEMA_VERSION,
  atomicWriteJson,
  createState,
  loadOrCreateState,
  statePathFor,
  validateState,
  writeState,
} from '../state.mjs';

const sha = (ch) => ch.repeat(64);

function rosterEntry(overrides = {}) {
  const modelId = overrides.model_id || 'vendor/model-a';
  return {
    model_id: modelId,
    slug: `${modelId}-20260801`,
    mascot: mascotFor(modelId),
    joined_at: '2026-08-20T00:00:00.000Z',
    submissions_used: 1,
    artifact: {
      wasm_sha256: sha('a'),
      source_sha256: sha('b'),
      prompt_sha256: sha('c'),
      version: 1,
      parent_version: null,
    },
    rating: 50,
    wins: 0,
    losses: 0,
    draws: 0,
    matches: 0,
    days_in_league: 0,
    status: 'active',
    ...overrides,
  };
}

async function tempDir() {
  return fs.mkdtemp(path.join(os.tmpdir(), 'cml-state-'));
}

test('createState produces a valid empty state', () => {
  const state = createState({ now: new Date('2026-08-23T12:00:00.000Z') });
  assert.equal(state.schema_version, SCHEMA_VERSION);
  assert.match(state.league_id, /^cml-20260823-[0-9a-f]{8}$/);
  assert.equal(state.day_index, 0);
  assert.deepEqual(state.roster, []);
  assert.deepEqual(state.retired, []);
  assert.deepEqual(state.announcements, []);
  assert.equal(state.last_feedback_at, null);
  validateState(state);
});

test('validateState accepts a populated valid state', () => {
  const state = createState({ now: new Date('2026-08-23T12:00:00.000Z') });
  state.roster.push(rosterEntry());
  state.roster.push(rosterEntry({
    model_id: 'vendor/model-b',
    rating: 34.5,
    wins: 2,
    losses: 8,
    draws: 0,
    matches: 10,
    days_in_league: 3,
    submissions_used: 3,
    artifact: {
      wasm_sha256: sha('d'),
      source_sha256: sha('e'),
      prompt_sha256: sha('f'),
      version: 3,
      parent_version: 2,
    },
  }));
  state.retired.push({
    ...rosterEntry({ model_id: 'vendor/model-c' }),
    retired_at: '2026-08-22T00:00:00.000Z',
    reason: 'rating_below_bar',
  });
  state.announcements.push({
    type: 'retirement',
    model: 'vendor/model-c',
    at: '2026-08-22T00:00:00.000Z',
  });
  state.last_feedback_at = '2026-08-21T00:00:00.000Z';
  assert.equal(validateState(state), state);
});

test('validateState rejects malformed documents', () => {
  const base = createState({ now: new Date('2026-08-23T12:00:00.000Z') });
  const cases = {
    'wrong schema version': { ...base, schema_version: 2 },
    'missing league id': { ...base, league_id: '' },
    'league id with slash': { ...base, league_id: 'cml/bad' },
    'negative day index': { ...base, day_index: -1 },
    'roster not array': { ...base, roster: {} },
    'retired not array': { ...base, retired: null },
    'announcements not array': { ...base, announcements: 'no' },
    'bad feedback timestamp': { ...base, last_feedback_at: 'yesterday' },
    'non-roundtrip timestamp': { ...base, created_at: '2026-08-23 12:00:00' },
    'missing updated_at': { ...base, updated_at: undefined },
  };
  for (const [name, doc] of Object.entries(cases)) {
    assert.throws(() => validateState(doc), Error, name);
  }
});

test('validateState enforces the roster size cap', () => {
  const state = createState({ now: new Date('2026-08-23T12:00:00.000Z') });
  for (let index = 0; index < MAX_ROSTER_SIZE; index += 1) {
    state.roster.push(rosterEntry({ model_id: `vendor/model-${index}` }));
  }
  validateState(state);
  state.roster.push(rosterEntry({ model_id: 'vendor/model-overflow' }));
  assert.throws(() => validateState(state), /at most 10/);
});

test('validateState rejects duplicate roster model IDs', () => {
  const state = createState({ now: new Date('2026-08-23T12:00:00.000Z') });
  state.roster.push(rosterEntry(), rosterEntry());
  assert.throws(() => validateState(state), /duplicate/);
});

test('validateState rejects malformed roster entries', () => {
  const mutations = {
    'submissions above cap': { submissions_used: 4 },
    'negative submissions': { submissions_used: -1 },
    'rating above 100': { rating: 100.01 },
    'negative rating': { rating: -0.01 },
    'non-integer wins': { wins: 1.5, matches: 1.5 },
    'matches inconsistent with W/L/D': { matches: 7 },
    'negative days': { days_in_league: -1 },
    'wrong status': { status: 'retired' },
    'bad joined_at': { joined_at: '2026-08-20' },
    'bad wasm sha': { artifact: {
      wasm_sha256: 'not-a-sha',
      source_sha256: sha('b'),
      prompt_sha256: sha('c'),
      version: 1,
      parent_version: null,
    } },
    'version below 1': { artifact: {
      wasm_sha256: sha('a'),
      source_sha256: sha('b'),
      prompt_sha256: sha('c'),
      version: 0,
      parent_version: null,
    } },
    'bad mascot': { mascot: { emoji: '', title: 'x', color: '#fff' } },
  };
  for (const [name, patch] of Object.entries(mutations)) {
    const state = createState({ now: new Date('2026-08-23T12:00:00.000Z') });
    state.roster.push(rosterEntry(patch));
    assert.throws(() => validateState(state), Error, name);
  }
});

test('validateState rejects malformed retired entries and announcements', () => {
  const state = createState({ now: new Date('2026-08-23T12:00:00.000Z') });
  state.retired.push({ ...rosterEntry(), reason: 'rating_below_bar' });
  assert.throws(() => validateState(state), /retired/);

  const state2 = createState({ now: new Date('2026-08-23T12:00:00.000Z') });
  state2.announcements.push({ type: '', at: '2026-08-22T00:00:00.000Z' });
  assert.throws(() => validateState(state2), /announcement/);
});

test('loadOrCreateState creates then reloads a valid state', async () => {
  const dir = await tempDir();
  const first = await loadOrCreateState(dir, { now: new Date('2026-08-23T12:00:00.000Z') });
  assert.equal(first.created, true);
  assert.equal(first.statePath, statePathFor(dir));

  const second = await loadOrCreateState(dir);
  assert.equal(second.created, false);
  assert.deepEqual(second.state, first.state);
});

test('loadOrCreateState refuses a corrupted state file', async () => {
  const dir = await tempDir();
  await fs.writeFile(statePathFor(dir), '{"schema_version":1,"league_id":"cml-x"}\n');
  await assert.rejects(() => loadOrCreateState(dir), Error);
});

test('writeState validates before persisting', async () => {
  const dir = await tempDir();
  const state = createState({ now: new Date('2026-08-23T12:00:00.000Z') });
  await writeState(dir, state);
  const onDisk = JSON.parse(await fs.readFile(statePathFor(dir), 'utf8'));
  assert.deepEqual(onDisk, state);

  await assert.rejects(() => writeState(dir, { ...state, roster: null }), Error);
});

test('atomicWriteJson leaves no temp files behind', async () => {
  const dir = await tempDir();
  const target = path.join(dir, 'nested', 'doc.json');
  await atomicWriteJson(target, { ok: true });
  assert.deepEqual(JSON.parse(await fs.readFile(target, 'utf8')), { ok: true });
  const leftovers = (await fs.readdir(path.dirname(target))).filter((name) => name.includes('.tmp-'));
  assert.deepEqual(leftovers, []);
});
