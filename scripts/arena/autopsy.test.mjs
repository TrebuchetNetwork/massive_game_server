// Tests for autopsy.mjs — lineage collection against a fixture artifact
// tree, diff correctness, identical-source handling. Real fs in tmp dirs,
// same pattern as the build_model_pages fixtures.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  acceptedRevisions,
  autopsyFor,
  buildSourceIndex,
  collectAutopsy,
  diffLines,
  failedAttempts,
  loadAutopsies,
  revisionDiffs,
  sha256,
  unifiedDiff,
} from './autopsy.mjs';
import { defaultIo } from './build_model_pages.mjs';

const STINT = '2026-08-10T12:00:00.000Z';
const V1 = 'pub fn tick() -> i32 {\n    1\n}\n';
const V2 = 'pub fn tick() -> i32 {\n    // revised\n    2\n}\n';
const V3 = 'pub fn tick() -> i32 {\n    // revised again\n    3\n}\n';

/**
 * Fixture tree: alpha-one in L2 with v1 -> v2 (accepted) -> v3 (one
 * compile_failed, then accepted). v1's source lives ONLY in a season day
 * snapshot (its sha comes from the v2 journal's checkpoint.revision_of);
 * v2's only in the revision journal; v3 is the current fighter record.
 */
function writeAutopsyFixture(root) {
  const continuousDir = path.join(root, 'continuous');
  const artifactsRoot = root;
  const trackDir = path.join(continuousDir, 'tracks', 'L2');

  const state = {
    schema_version: 2,
    league_id: 'cml-test-autopsy',
    tracks: {
      L2: {
        day_index: 5,
        policy: { max_submissions: 4, compile_attempts: 3, feedback_interval_ms: 0, max_revisions: 3 },
        roster: [{
          model_id: 'test/alpha-one', slug: 'test/alpha-one-20260101',
          mascot: { emoji: '🦊', title: 'Alpha', color: '#caff00' },
          joined_at: STINT, submissions_used: 3,
          artifact: {
            version: 3, parent_version: 2,
            wasm_sha256: sha256('wasm3'), source_sha256: sha256(V3), prompt_sha256: sha256('prompt'),
          },
          rating: 55, wins: 8, losses: 2, draws: 1, matches: 11,
          days_in_league: 5, status: 'active',
        }],
        retired: [],
        announcements: [],
        last_feedback_at: null,
      },
    },
    created_at: STINT,
    updated_at: STINT,
  };
  fs.mkdirSync(continuousDir, { recursive: true });
  fs.writeFileSync(path.join(continuousDir, 'state.json'), JSON.stringify(state));

  const submission = (over) => JSON.stringify({
    track: 'L2', model_id: 'test/alpha-one', slug: 'test/alpha-one-20260101', stint: STINT,
    version_attempted: 2, parent_version: 1,
    prompt_sha256: sha256('prompt'), brief_sha256: sha256('brief'),
    source_sha256: sha256(V2), wasm_sha256: sha256('wasm2'),
    compile_attempts: 1, outcome: 'accepted', at: '2026-08-15T12:00:00.000Z', ...over,
  });
  fs.writeFileSync(path.join(continuousDir, 'submissions.jsonl'), `${submission({})}\n${submission({
    version_attempted: 3, parent_version: 2, source_sha256: null, wasm_sha256: null,
    compile_attempts: 2, outcome: 'compile_failed', at: '2026-08-20T12:00:00.000Z',
  })}\n${submission({
    version_attempted: 3, parent_version: 2, source_sha256: sha256(V3), wasm_sha256: sha256('wasm3'),
    compile_attempts: 1, at: '2026-08-22T12:00:00.000Z',
  })}\n${submission({
    stint: '2026-07-01T00:00:00.000Z', at: '2026-07-05T12:00:00.000Z', // stale stint — ignored
  })}\n`);

  // Fighter record: current (v3) source only.
  const fighterDir = path.join(trackDir, 'fighters', 'test__alpha-one');
  fs.mkdirSync(fighterDir, { recursive: true });
  fs.writeFileSync(path.join(fighterDir, 'source.rs'), V3);

  // Revision journal for the accepted v2: carries the v2 source and the v1
  // digest (checkpoint.revision_of).
  const journalDir = path.join(trackDir, 'revision-journal');
  fs.mkdirSync(journalDir, { recursive: true });
  fs.writeFileSync(path.join(journalDir, `test__alpha-one-${Date.parse(STINT)}-v2-s2.json`), JSON.stringify({
    schema_version: 1,
    track: 'L2',
    model_id: 'test/alpha-one',
    stint: STINT,
    version_attempted: 2,
    parent_version: 1,
    outcome: 'accepted',
    source: V2,
    checkpoint: { source_sha256: sha256(V2), wasm_sha256: sha256('wasm2'), revision_of: sha256(V1) },
  }));
  // A torn journal file must be skipped, not fatal.
  fs.writeFileSync(path.join(journalDir, 'torn.json'), '{"track":"L2",');

  // Season day snapshot: the only place the v1 source survives.
  const seasonSources = path.join(artifactsRoot, 'seasons', 'continuous-cml-test-autopsy-L2-day1-premier', 'sources');
  fs.mkdirSync(seasonSources, { recursive: true });
  fs.writeFileSync(path.join(seasonSources, 'orw-test-01-deadbeef-test-alpha-one.rs'), V1);
  // A weekly season dir must be ignored even if it carries lookalike files.
  const weeklySources = path.join(artifactsRoot, 'seasons', 'weekly-2026-08-10-aaaa', 'sources');
  fs.mkdirSync(weeklySources, { recursive: true });
  fs.writeFileSync(path.join(weeklySources, 'decoy.rs'), 'pub fn decoy() {}\n');

  return { artifactsRoot, continuousDir, state };
}

function fixtureAutopsy() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'autopsy-'));
  const { artifactsRoot, continuousDir, state } = writeAutopsyFixture(root);
  const submissions = fs.readFileSync(path.join(continuousDir, 'submissions.jsonl'), 'utf8')
    .split('\n').filter(Boolean).map(JSON.parse);
  const autopsies = loadAutopsies({
    artifactsRoot, continuousDir, state, submissions, io: defaultIo,
  });
  const entry = state.tracks.L2.roster[0];
  return { root, autopsies, entry, submissions };
}

test('loadAutopsies collects the full lineage with sources from every store', () => {
  const { autopsies, entry } = fixtureAutopsy();
  const found = autopsyFor(autopsies, 'L2', entry);
  assert.ok(found, 'autopsy exists for a model with accepted revisions');
  const { autopsy, failures } = found;
  assert.equal(autopsy.model, 'test/alpha-one');
  assert.equal(autopsy.track, 'L2');
  assert.deepEqual(autopsy.versions.map((v) => v.version), [1, 2, 3]);
  const [v1, v2, v3] = autopsy.versions;
  assert.equal(v1.outcome, 'entrant');
  assert.equal(v1.sha256, sha256(V1)); // via journal checkpoint.revision_of
  assert.equal(v1.source, V1); // resolved from the season day snapshot
  assert.equal(v2.source, V2); // resolved from the revision journal
  assert.equal(v3.source, V3); // resolved from the fighter record
  assert.equal(v2.compileAttempts, 1);
  assert.equal(v3.at, '2026-08-22T12:00:00.000Z');
  // Failed attempt surfaces as timeline context only, scoped to the stint.
  assert.deepEqual(failures, [{
    version: 3, outcome: 'compile_failed', compileAttempts: 2, at: '2026-08-20T12:00:00.000Z',
  }]);
});

test('loadAutopsies skips entries without accepted revisions', () => {
  const { autopsies } = fixtureAutopsy();
  assert.equal(autopsies.size, 1);
});

test('collectAutopsy returns null without accepted revisions', () => {
  const out = collectAutopsy({
    track: 'L2',
    entry: { model_id: 'test/nobody', joined_at: STINT },
    submissions: [],
    sourcesBySha: new Map(),
    parentSha: new Map(),
  });
  assert.equal(out, null);
});

test('acceptedRevisions filters by stint and requires source_sha256', () => {
  const submissions = [
    { track: 'L2', model_id: 'm', stint: 's1', version_attempted: 2, outcome: 'accepted', source_sha256: 'x', at: '1' },
    { track: 'L2', model_id: 'm', stint: 's0', version_attempted: 2, outcome: 'accepted', source_sha256: 'y', at: '0' },
    { track: 'L2', model_id: 'm', stint: 's1', version_attempted: 3, outcome: 'compile_failed', source_sha256: null, at: '2' },
    { track: 'L3', model_id: 'm', stint: 's1', version_attempted: 2, outcome: 'accepted', source_sha256: 'z', at: '3' },
  ];
  const out = acceptedRevisions(submissions, { track: 'L2', modelId: 'm', stint: 's1' });
  assert.equal(out.length, 1);
  assert.equal(out[0].source_sha256, 'x');
});

test('buildSourceIndex tolerates missing dirs and corrupt journals', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'autopsy-empty-'));
  const { sourcesBySha, parentSha } = buildSourceIndex({
    artifactsRoot: root, continuousDir: path.join(root, 'nope'), trackIds: ['L2'], io: defaultIo,
  });
  assert.equal(sourcesBySha.size, 0);
  assert.equal(parentSha.size, 0);
});

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

test('diffLines: pure insertion', () => {
  const ops = diffLines('a\nb\nc', 'a\nb\nx\nc');
  assert.deepEqual(ops, [
    { type: 'same', text: 'a' },
    { type: 'same', text: 'b' },
    { type: 'add', text: 'x' },
    { type: 'same', text: 'c' },
  ]);
});

test('diffLines: pure deletion', () => {
  const ops = diffLines('a\nx\nb', 'a\nb');
  assert.deepEqual(ops, [
    { type: 'same', text: 'a' },
    { type: 'del', text: 'x' },
    { type: 'same', text: 'b' },
  ]);
});

test('diffLines: replacement keeps maximal context', () => {
  const ops = diffLines('a\nb\nc\nd', 'a\nx\nc\nd');
  assert.deepEqual(ops, [
    { type: 'same', text: 'a' },
    { type: 'del', text: 'b' },
    { type: 'add', text: 'x' },
    { type: 'same', text: 'c' },
    { type: 'same', text: 'd' },
  ]);
});

test('diffLines: identical inputs are all context', () => {
  assert.ok(diffLines('a\nb', 'a\nb').every((op) => op.type === 'same'));
  assert.ok(diffLines('', '').every((op) => op.type === 'same'));
});

test('unifiedDiff: hunk headers, context window, +/- lines', () => {
  const before = ['l1', 'l2', 'l3', 'l4', 'l5', 'l6', 'l7', 'l8', 'l9', 'l10'].join('\n');
  const after = ['l1', 'l2', 'CHANGED', 'l4', 'l5', 'l6', 'l7', 'l8', 'NEW', 'l9', 'l10'].join('\n');
  const diff = unifiedDiff(before, after, { context: 2 });
  assert.equal(diff.unchanged, false);
  assert.equal(diff.truncated, false);
  assert.equal(diff.hunks.length, 2); // 5 unchanged lines between the edits > 2*context
  const [h1, h2] = diff.hunks;
  assert.equal(h1.oldStart, 1); // l1, l2 context
  assert.deepEqual(h1.lines.map((l) => l.type), ['same', 'same', 'del', 'add', 'same', 'same']);
  assert.deepEqual(h1.lines[2], { type: 'del', text: 'l3' });
  assert.deepEqual(h1.lines[3], { type: 'add', text: 'CHANGED' });
  assert.equal(h2.oldStart, 7); // l7, l8 context
  assert.ok(h2.lines.some((l) => l.type === 'add' && l.text === 'NEW'));
  assert.equal(diff.changedLines, 3); // del l3 + add CHANGED + add NEW
});

test('unifiedDiff: nearby changes merge into one hunk', () => {
  const before = ['a', 'b', 'c', 'd', 'e'].join('\n');
  const after = ['A', 'b', 'c', 'd', 'E'].join('\n');
  const diff = unifiedDiff(before, after, { context: 3 });
  assert.equal(diff.hunks.length, 1);
  assert.equal(diff.hunks[0].lines.filter((l) => l.type !== 'same').length, 4);
});

test('unifiedDiff: identical sources are flagged unchanged', () => {
  const diff = unifiedDiff('a\nb\nc', 'a\nb\nc');
  assert.deepEqual(diff, { unchanged: true, hunks: [], truncated: false, changedLines: 0 });
});

test('unifiedDiff: changed-line budget truncates with a marker', () => {
  const before = Array.from({ length: 40 }, (_, i) => `line${i}`).join('\n');
  const after = Array.from({ length: 40 }, (_, i) => (i % 10 === 0 ? `edited${i}` : `line${i}`)).join('\n');
  const diff = unifiedDiff(before, after, { context: 1, maxChanged: 4 });
  assert.equal(diff.truncated, true);
  assert.ok(diff.changedLines <= 4);
  const last = diff.hunks[diff.hunks.length - 1];
  assert.equal(last.lines[last.lines.length - 1].type, 'meta');
  assert.match(last.lines[last.lines.length - 1].text, /truncated/);
});

test('unifiedDiff: whole-file rewrite falls back to delete+insert', () => {
  const before = Array.from({ length: 100 }, (_, i) => `old${i}`).join('\n');
  const after = Array.from({ length: 100 }, (_, i) => `new${i}`).join('\n');
  const diff = unifiedDiff(before, after, { maxChanged: 10 });
  assert.equal(diff.truncated, true);
  assert.ok(diff.hunks[0].lines.some((l) => l.type === 'del'));
});

// ---------------------------------------------------------------------------
// Revision diffs over a lineage
// ---------------------------------------------------------------------------

test('revisionDiffs diffs consecutive versions and marks identical sources', () => {
  const { autopsies, entry } = fixtureAutopsy();
  const { autopsy } = autopsyFor(autopsies, 'L2', entry);
  const diffs = revisionDiffs(autopsy);
  assert.equal(diffs.length, 2);
  assert.deepEqual(diffs.map((d) => [d.from, d.to]), [[1, 2], [2, 3]]);
  assert.equal(diffs[0].status, 'diff');
  assert.ok(diffs[0].diff.hunks.length >= 1);
  assert.ok(diffs[0].diff.hunks[0].lines.some((l) => l.type === 'del' && l.text.includes('1')));
  assert.equal(diffs[0].outcome, 'accepted');

  // Identical sha across versions -> 'unchanged', no diff computed.
  const sameSha = sha256(V1);
  const identical = revisionDiffs({
    model: 'm', track: 'L2',
    versions: [
      { version: 1, sha256: sameSha, source: V1, outcome: 'entrant' },
      { version: 2, sha256: sameSha, source: V1, outcome: 'accepted' },
    ],
  });
  assert.deepEqual(identical.map((d) => d.status), ['unchanged']);
  assert.equal(identical[0].diff, null);
});

test('revisionDiffs marks unarchived sources as missing', () => {
  const diffs = revisionDiffs({
    model: 'm', track: 'L2',
    versions: [
      { version: 1, sha256: 'a', source: null, outcome: 'entrant' },
      { version: 2, sha256: 'b', source: V2, outcome: 'accepted' },
    ],
  });
  assert.equal(diffs[0].status, 'missing');
});
