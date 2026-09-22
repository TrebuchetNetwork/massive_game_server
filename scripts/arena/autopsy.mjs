// Borrow-checker autopsy — data layer + diff computation for the model-page
// "Autopsy" section rendered by build_model_pages.mjs.
//
// Where revision sources actually live (continuous league):
//
//   submissions.jsonl            one record per REVISION attempt (v2+; the
//                                initial generation is "submission 1" and is
//                                not recorded there). Accepted records carry
//                                source_sha256 / wasm_sha256 / compile_attempts
//                                / outcome / at; failed attempts carry nulls.
//   tracks/<T>/revision-journal/ one JSON per attempt. Accepted journals keep
//                                the full source twice: top-level `source` and
//                                `request.response.source_code`, plus
//                                `checkpoint.source_sha256` and — crucially —
//                                `checkpoint.revision_of`, the PARENT source
//                                sha256, which is the only durable record of
//                                the v1 digest. Failed codegen/compile
//                                attempts keep the attempted source under
//                                `request.response.source_code`.
//   tracks/<T>/fighters/<key>/   the fighter record: source.rs is always the
//                                CURRENT (latest accepted) artifact only —
//                                older versions are overwritten in place.
//   <artifactsRoot>/seasons/continuous-*/sources/*.rs
//                                per-day season snapshots of every entrant's
//                                compiled source. This is the most complete
//                                archive: every artifact version that fought
//                                at least one season day appears here.
//
// Resolution strategy: match by sha256, not by filename (season sources are
// named by season-entrant id, not by league model_id). Index every candidate
// source once (fighter records, journals, season snapshots) and resolve each
// lineage version's `source_sha256` against the index. Measured coverage on
// the live artifacts (2026-09): all 50 models with ≥2 accepted versions
// resolve EVERY version — v1 included (via revision_of → season snapshot).
//
// No npm dependencies; the diff is a compact LCS over lines.

import crypto from 'node:crypto';
import path from 'node:path';

export const AUTOPSY_CONTEXT_LINES = 3;
export const AUTOPSY_MAX_CHANGED_LINES = 200;
// Safety valve for pathological inputs: full DP needs (n+1)*(m+1) cells.
// Source limit is 51 200 bytes (~1 500 lines), so 6M cells never triggers on
// real artifacts — it only guards against malformed fixture abuse.
const LCS_MAX_CELLS = 6_000_000;

export function sha256(text) {
  return crypto.createHash('sha256').update(String(text)).digest('hex');
}

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

function splitLines(text) {
  return String(text ?? '').split('\n');
}

const same = (text) => ({ type: 'same', text });
const add = (text) => ({ type: 'add', text });
const del = (text) => ({ type: 'del', text });

/** LCS over the differing middle of two line arrays. */
function diffMiddle(a, b) {
  if (!a.length) return b.map(add);
  if (!b.length) return a.map(del);
  if ((a.length + 1) * (b.length + 1) > LCS_MAX_CELLS) {
    // Give up alignment: whole middle reads as delete + insert.
    return [...a.map(del), ...b.map(add)];
  }
  const n = a.length;
  const m = b.length;
  const w = m + 1;
  const dp = new Uint32Array((n + 1) * w);
  for (let i = n - 1; i >= 0; i -= 1) {
    for (let j = m - 1; j >= 0; j -= 1) {
      dp[i * w + j] = a[i] === b[j]
        ? dp[(i + 1) * w + j + 1] + 1
        : Math.max(dp[(i + 1) * w + j], dp[i * w + j + 1]);
    }
  }
  const out = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push(same(a[i]));
      i += 1;
      j += 1;
    } else if (dp[(i + 1) * w + j] >= dp[i * w + j + 1]) {
      out.push(del(a[i]));
      i += 1;
    } else {
      out.push(add(b[j]));
      j += 1;
    }
  }
  while (i < n) { out.push(del(a[i])); i += 1; }
  while (j < m) { out.push(add(b[j])); j += 1; }
  return out;
}

/**
 * Line-level diff of two source texts as an op list:
 * [{type:'same'|'add'|'del', text}]. Common prefix/suffix are trimmed before
 * the LCS pass so small edits on large files stay fast.
 */
export function diffLines(oldText, newText) {
  const a = splitLines(oldText);
  const b = splitLines(newText);
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start += 1;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA -= 1;
    endB -= 1;
  }
  return [
    ...a.slice(0, start).map(same),
    ...diffMiddle(a.slice(start, endA), b.slice(start, endB)),
    ...a.slice(endA).map(same),
  ];
}

/**
 * Unified diff with `context` unchanged lines around each change cluster,
 * capped at the first ~`maxChanged` changed (add+del) lines. Returns
 * { unchanged, hunks, truncated, changedLines } where each hunk is
 * { oldStart, newStart, lines: [{type, text}] } and a truncated tail is
 * marked by a {type:'meta'} ellipsis line in the last hunk.
 */
export function unifiedDiff(oldText, newText, {
  context = AUTOPSY_CONTEXT_LINES,
  maxChanged = AUTOPSY_MAX_CHANGED_LINES,
} = {}) {
  const ops = diffLines(oldText, newText);
  const changedIdx = [];
  for (let i = 0; i < ops.length; i += 1) {
    if (ops[i].type !== 'same') changedIdx.push(i);
  }
  if (!changedIdx.length) return { unchanged: true, hunks: [], truncated: false, changedLines: 0 };

  // Cluster change indices; clusters separated by <= 2*context unchanged
  // lines merge into one hunk (their context windows would overlap).
  const clusters = [];
  let cur = [changedIdx[0], changedIdx[0]];
  for (let k = 1; k < changedIdx.length; k += 1) {
    const idx = changedIdx[k];
    if (idx - cur[1] <= 2 * context + 1) {
      cur[1] = idx;
    } else {
      clusters.push(cur);
      cur = [idx, idx];
    }
  }
  clusters.push(cur);

  // Line numbers at each op boundary.
  const oldLineAt = new Int32Array(ops.length + 1);
  const newLineAt = new Int32Array(ops.length + 1);
  let ol = 1;
  let nl = 1;
  for (let i = 0; i < ops.length; i += 1) {
    oldLineAt[i] = ol;
    newLineAt[i] = nl;
    if (ops[i].type !== 'add') ol += 1;
    if (ops[i].type !== 'del') nl += 1;
  }
  oldLineAt[ops.length] = ol;
  newLineAt[ops.length] = nl;

  const hunks = [];
  let budget = maxChanged;
  let changedLines = 0;
  let truncated = false;
  for (let c = 0; c < clusters.length; c += 1) {
    const [first, last] = clusters[c];
    const from = Math.max(0, first - context);
    const to = Math.min(ops.length - 1, last + context);
    const hunk = {
      oldStart: oldLineAt[from],
      newStart: newLineAt[from],
      lines: [],
    };
    let stop = false;
    for (let i = from; i <= to; i += 1) {
      const op = ops[i];
      if (op.type !== 'same') {
        if (budget <= 0) {
          truncated = true;
          stop = true;
          break;
        }
        budget -= 1;
        changedLines += 1;
      }
      hunk.lines.push(op);
    }
    if (!stop && budget <= 0 && c + 1 < clusters.length) {
      // Budget exactly exhausted with more clusters pending: truncate here.
      truncated = true;
      stop = true;
    }
    if (stop) {
      // Don't leave a dangling context tail on a truncated hunk.
      while (hunk.lines.length && hunk.lines[hunk.lines.length - 1].type === 'same') hunk.lines.pop();
      hunk.lines.push({ type: 'meta', text: `… diff truncated after ${maxChanged} changed lines …` });
      hunks.push(hunk);
      break;
    }
    hunks.push(hunk);
  }
  return { unchanged: false, hunks, truncated, changedLines };
}

// ---------------------------------------------------------------------------
// Lineage collection
// ---------------------------------------------------------------------------

const keyFor = (track, modelId, stint) => `${track}${modelId}${stint ?? ''}`;

/** Accepted revision records (v2+) for one league entry, version-ordered. */
export function acceptedRevisions(submissions, { track, modelId, stint }) {
  return (Array.isArray(submissions) ? submissions : [])
    .filter((s) => s
      && (s.track ?? null) === track
      && s.model_id === modelId
      && (s.stint == null || stint == null || s.stint === stint)
      && s.outcome === 'accepted'
      && Number.isSafeInteger(s.version_attempted)
      && typeof s.source_sha256 === 'string')
    .sort((a, b) => a.version_attempted - b.version_attempted);
}

/**
 * Scan the durable source stores once and build:
 *   sourcesBySha — Map sha256 -> source text, from (in priority order, first
 *                  write wins) fighter records, revision journals and season
 *                  day snapshots.
 *   parentSha    — Map keyFor(track, model_id, stint) -> Map(parentVersion ->
 *                  sha256), from journal checkpoints' `revision_of`. This is
 *                  the only place the v1 digest is recorded.
 *
 * All reads go through the injectable io (readdir/readJson/readText/exists)
 * so tests run against fixture trees. Unreadable files are skipped, never
 * fatal — publishing must not be blocked by a corrupt artifact.
 */
export function buildSourceIndex({ artifactsRoot, continuousDir, trackIds, io, log = () => {} }) {
  const sourcesBySha = new Map();
  const parentSha = new Map();
  const indexSource = (text) => {
    if (typeof text !== 'string' || !text) return;
    const sha = sha256(text);
    if (!sourcesBySha.has(sha)) sourcesBySha.set(sha, text);
  };

  for (const trackId of trackIds) {
    const trackDir = path.join(continuousDir, 'tracks', trackId);

    // Current fighter artifacts (latest accepted source per model).
    const fightersDir = path.join(trackDir, 'fighters');
    if (io.exists(fightersDir)) {
      for (const name of io.readdir(fightersDir)) {
        const sourcePath = path.join(fightersDir, name, 'source.rs');
        if (!io.exists(sourcePath)) continue;
        try {
          indexSource(io.readText(sourcePath));
        } catch (error) {
          log(`autopsy: skipping unreadable ${sourcePath} (${String(error?.message || error).slice(0, 120)})`);
        }
      }
    }

    // Revision journals: accepted sources + parent digests (revision_of).
    const journalDir = path.join(trackDir, 'revision-journal');
    if (io.exists(journalDir)) {
      for (const name of io.readdir(journalDir).filter((n) => n.endsWith('.json'))) {
        let journal;
        try {
          journal = io.readJson(path.join(journalDir, name));
        } catch {
          continue; // partial/corrupt journal — other stores may still cover it
        }
        if (!journal || typeof journal !== 'object') continue;
        indexSource(journal.source);
        indexSource(journal.request?.response?.source_code);
        const parent = journal.checkpoint?.revision_of;
        const version = Number(journal.version_attempted);
        if (typeof parent === 'string' && Number.isSafeInteger(version) && version >= 2) {
          const key = keyFor(journal.track ?? trackId, journal.model_id, journal.stint);
          if (!parentSha.has(key)) parentSha.set(key, new Map());
          const byVersion = parentSha.get(key);
          if (!byVersion.has(version - 1)) byVersion.set(version - 1, parent);
        }
      }
    }
  }

  // Season day snapshots — the complete historical archive. Continuous
  // seasons only; weekly seasons belong to a different league.
  const seasonsDir = path.join(artifactsRoot, 'seasons');
  if (io.exists(seasonsDir)) {
    for (const name of io.readdir(seasonsDir)) {
      if (!name.startsWith('continuous-')) continue;
      const sourcesDir = path.join(seasonsDir, name, 'sources');
      if (!io.exists(sourcesDir)) continue;
      for (const file of io.readdir(sourcesDir).filter((n) => n.endsWith('.rs'))) {
        const sourcePath = path.join(sourcesDir, file);
        try {
          indexSource(io.readText(sourcePath));
        } catch (error) {
          log(`autopsy: skipping unreadable ${sourcePath} (${String(error?.message || error).slice(0, 120)})`);
        }
      }
    }
  }
  return { sourcesBySha, parentSha };
}

/**
 * Collect the autopsy lineage for one league entry: v1 (the entrant) plus
 * every accepted revision, each with its source resolved against the index
 * (null when no store archived it). Returns null when the entry has no
 * accepted revision — a single-version artifact has nothing to diff — or
 * when not a single version's source could be resolved.
 *
 * Output: { model, track, versions: [{version, sha256, at, outcome,
 * compileAttempts, source}] }.
 */
export function collectAutopsy({ track, entry, submissions, sourcesBySha, parentSha }) {
  const revisions = acceptedRevisions(submissions, {
    track,
    modelId: entry.model_id,
    stint: entry.joined_at,
  });
  if (!revisions.length) return null;
  const byVersion = parentSha.get(keyFor(track, entry.model_id, entry.joined_at)) || new Map();
  const resolve = (sha) => (typeof sha === 'string' && sha ? (sourcesBySha.get(sha) ?? null) : null);
  const v1sha = byVersion.get(1) ?? null;
  const versions = [{
    version: 1,
    sha256: v1sha,
    at: entry.joined_at,
    outcome: 'entrant',
    compileAttempts: 1,
    source: resolve(v1sha),
  }];
  for (const r of revisions) {
    versions.push({
      version: r.version_attempted,
      sha256: r.source_sha256,
      at: r.at,
      outcome: 'accepted',
      compileAttempts: Number(r.compile_attempts) || 0,
      source: resolve(r.source_sha256),
    });
  }
  // No source store archived ANY version of this lineage — the section would
  // carry no diff at all, so hide it (keeps source-less builds byte-identical
  // to a build without the autopsy feature).
  if (!versions.some((v) => v.source)) return null;
  return { model: entry.model_id, track, versions };
}

/**
 * Failed attempts (compile_failed / codegen_failed / interrupted) for the
 * timeline badges — they never go live, so they carry no diff, only the
 * stats context (compile attempts, outcome, at).
 */
export function failedAttempts(submissions, { track, modelId, stint }) {
  return (Array.isArray(submissions) ? submissions : [])
    .filter((s) => s
      && (s.track ?? null) === track
      && s.model_id === modelId
      && (s.stint == null || stint == null || s.stint === stint)
      && s.outcome && s.outcome !== 'accepted'
      && Number.isSafeInteger(s.version_attempted))
    .sort((a, b) => a.version_attempted - b.version_attempted
      || String(a.at || '').localeCompare(String(b.at || '')))
    .map((s) => ({
      version: s.version_attempted,
      outcome: s.outcome,
      compileAttempts: Number(s.compile_attempts) || 0,
      at: s.at,
    }));
}

/**
 * Load autopsies for every roster/retired entry in every track of a
 * continuous league state. Returns a Map keyFor(track, model_id, stint) ->
 * { autopsy, failures } containing only entries with ≥1 accepted revision.
 */
export function loadAutopsies({ artifactsRoot, continuousDir, state, submissions, trackIds, io, log = () => {} }) {
  const out = new Map();
  if (!state?.tracks) return out;
  const tracks = trackIds || Object.keys(state.tracks);
  const { sourcesBySha, parentSha } = buildSourceIndex({
    artifactsRoot, continuousDir, trackIds: tracks, io, log,
  });
  for (const trackId of tracks) {
    const slice = state.tracks[trackId];
    if (!slice) continue;
    for (const entry of [...(slice.roster || []), ...(slice.retired || [])]) {
      const autopsy = collectAutopsy({
        track: trackId, entry, submissions, sourcesBySha, parentSha,
      });
      if (!autopsy) continue;
      out.set(keyFor(trackId, entry.model_id, entry.joined_at), {
        autopsy,
        failures: failedAttempts(submissions, {
          track: trackId, modelId: entry.model_id, stint: entry.joined_at,
        }),
      });
    }
  }
  return out;
}

/** Lookup helper mirroring keyFor for callers holding an entry object. */
export function autopsyFor(autopsies, track, entry) {
  if (!autopsies || !entry) return null;
  return autopsies.get(keyFor(track, entry.model_id, entry.joined_at)) ?? null;
}

/**
 * Diff each consecutive pair of lineage versions. Returns one entry per
 * adjacent pair: { from, to, outcome, compileAttempts, at, status, diff }
 * where status is 'diff' (both sources present and differing), 'unchanged'
 * (identical source — common for recompiled failures later accepted), or
 * 'missing' (a source was not archived); diff is the unifiedDiff result for
 * status 'diff', else null.
 */
export function revisionDiffs(autopsy, options) {
  const out = [];
  const versions = autopsy?.versions || [];
  for (let i = 1; i < versions.length; i += 1) {
    const from = versions[i - 1];
    const to = versions[i];
    const base = {
      from: from.version,
      to: to.version,
      outcome: to.outcome,
      compileAttempts: to.compileAttempts,
      at: to.at,
    };
    if (!from.source || !to.source) {
      out.push({ ...base, status: 'missing', diff: null });
    } else if (from.sha256 && from.sha256 === to.sha256) {
      out.push({ ...base, status: 'unchanged', diff: null });
    } else {
      const diff = unifiedDiff(from.source, to.source, options);
      out.push({ ...base, status: diff.unchanged ? 'unchanged' : 'diff', diff });
    }
  }
  return out;
}
