# Arena Mid-Season Feedback Round Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give each of the 10 weekly arena models exactly one mid-season chance to revise its bot using its own performance stats, with full audit integrity.

**Architecture:** New admin-gated `/api/arena/code/revise` route on the Rust server (frozen revision prompt template, stats digest + previous source as data slots) → new `--revise-only` mode in the season runner (journaled single provider call per model, atomic checkpoint swap) → one-shot scheduling hook in the weekly supervisor at epoch 336, with epoch/roster validators taught to accept exactly one artifact swap per model at the revision boundary.

**Tech Stack:** Rust/warp server (`server/src/operational/code_generation.rs`), Node ESM scripts (`scripts/arena/run_top10_season.mjs`, `scripts/arena/weekly_supervisor.mjs`), node:test, cargo test.

**Spec:** `docs/superpowers/specs/2026-07-27-arena-feedback-round-design.md`

**Key anchors (read before editing):**
- Server: `code_generation.rs` — constants at :42-51, `GenerateBotCodeBody` :175-181, response structs :209-273, `status()` :1041-1059, `generate_bot_code` :1061-1177, `generate_via_openrouter` :1299-1374, `build_openrouter_request` :1469-1509, routes :2994-3107, tests from :3109.
- Runner: `run_top10_season.mjs` — `parseArgs` :60-106, constants :37-39, `validateV2CheckpointMetadata` :728-755, `generateEntrant` :1733, `directories` :2075, main modes :2159-2218.
- Supervisor: `weekly_supervisor.mjs` — `ensureGeneration` :1109-1208 (writes `state.artifact_bindings`), epoch roster wasm pin :1348-1357, `cumulativeRoster` wasm-change throw :1509-1511, `main()` loop :1794-1841, `runRunner` :279.

**Hard constraints discovered (do not violate):**
- `state.artifact_bindings` pins `{model_id, wasm_bytes, wasm_sha256}` per model at generation; epoch roster validation (:1352-1353) and `cumulativeRoster` (:1509-1511) throw if an epoch carries different wasm hashes. The revision must update `state.artifact_bindings` atomically with the checkpoint swap, or epochs after the revision die.
- The checkpoint contract validator (`validateV2CheckpointMetadata`) pins `prompt_version`/`prompt_sha256` to `codeStatus` values. Revised checkpoints must validate against a **revision** contract returned by `/api/arena/code/status` (new fields), never against values self-reported by a generation response.
- `compile_attempts` must stay ≤ 100 and is NOT incremented per epoch (epoch-96 fix); revision compile increments it once per model — at current ~100 for some W31 models, the swap path must tolerate `compile_attempts == 100` checkpoints and the increment must use the same `Math.min(100, +1)` semantics as `persistCompileFailure` (:854-864), i.e. effectively a no-op at the cap. Simpler: revision checkpoints carry `compile_attempts` forward unchanged and validation keeps passing.

---

### Task 1: Server — revision prompt template, contract constants, status fields

**Files:**
- Modify: `server/src/operational/code_generation.rs` (constants :42-51, `canonical/sha` fns :1730-1739, `CodeGenerationStatusResponse` :237-253, `status()` :1041-1059, tests :3207-3220)

- [ ] **Step 1: Failing test — status exposes the revision contract**

In `mod tests` (near the existing prompt-sha test at :3207), add:

```rust
#[test]
fn status_exposes_revision_contract() {
    let service = test_service(); // use the same helper the existing status test uses
    let status = service.status();
    assert_eq!(status.revision_prompt_version, ARENA_REVISION_PROMPT_VERSION);
    assert_eq!(status.revision_prompt_sha256, revision_prompt_sha256());
    assert_eq!(revision_prompt_sha256().len(), 64);
    // template is deterministic
    assert_eq!(revision_prompt_sha256(), revision_prompt_sha256());
    // revision contract differs from the generation contract
    assert_ne!(status.revision_prompt_sha256, status.prompt_sha256);
}
```

Run: `cargo test -p massive_game_server_core code_generation::tests::status_exposes_revision_contract`
Expected: FAIL (`revision_prompt_version` field does not exist)

- [ ] **Step 2: Add revision constants + template + hash fns**

After `ARENA_UNIFORM_COMPETITION_PROMPT` (:51), add:

```rust
pub const ARENA_REVISION_PROMPT_VERSION: &str = "arena-rust-revision-v1.0.0";
pub const ARENA_REVISION_SYSTEM_PROMPT: &str = "You are a contestant in a deterministic Rust/WASM fighter competition revising your previous fighter. Begin the Rust source immediately; do not analyze exhaustively. Return exactly one complete Rust 2021 source file as raw text and stop immediately after its final closing brace. Prefer a simple, complete file below 8 KiB and roughly 2,000 visible tokens. An incomplete file is a failed submission. Never return markdown fences, explanations, or anything before or after the source file.";
pub const ARENA_REVISION_USER_PROMPT_PREFIX: &str = "You submitted the fighter below earlier this season. Its mid-season performance digest follows. Return one improved complete Rust source file that keeps the exact same required exports and ABI, and addresses the weaknesses the digest shows. Do not change the function signature.\n\nPREVIOUS SOURCE\n";
pub const ARENA_REVISION_STATS_SEPARATOR: &str = "\n\nPERFORMANCE DIGEST\n";
```

After `competition_prompt_sha256()` (:1737-1739), add:

```rust
fn canonical_revision_prompt() -> String {
    format!(
        "SYSTEM\n{}\n\nUSER\n{}",
        ARENA_REVISION_SYSTEM_PROMPT, ARENA_REVISION_USER_PROMPT_PREFIX
    )
}

fn revision_prompt_sha256() -> String {
    sha256_hex(canonical_revision_prompt().as_bytes())
}
```

Note: the hashed template covers the fixed prefix only; `previous_source` and `stats_digest` are appended after it as data (see Task 2). This keeps one constant hash while allowing per-model data.

- [ ] **Step 3: Add status fields**

In `CodeGenerationStatusResponse` (:237-253) add two fields:

```rust
    revision_prompt_version: String,
    revision_prompt_sha256: String,
```

In `status()` (:1042-1058) populate them:

```rust
            revision_prompt_version: ARENA_REVISION_PROMPT_VERSION.to_owned(),
            revision_prompt_sha256: revision_prompt_sha256(),
```

- [ ] **Step 4: Verify**

Run: `cargo test -p massive_game_server_core code_generation`
Expected: all PASS including the new test

- [ ] **Step 5: Commit**

```bash
git add server/src/operational/code_generation.rs
git commit -m "feat(arena): revision prompt template and status contract fields"
```

---

### Task 2: Server — `/api/arena/code/revise` route

**Files:**
- Modify: `server/src/operational/code_generation.rs` (body structs :175-200, service impl after `generate_bot_code` :1177, `build_openrouter_request` :1469-1509, routes :3030-3052, tests)

- [ ] **Step 1: Failing tests**

Add to `mod tests`:

```rust
#[test]
fn revise_rejects_oversized_stats_digest() {
    // Build a ReviseBotCodeBody with stats_digest of MAX_ARENA_STATS_DIGEST_BYTES + 1
    // and assert service.revise_bot_code returns Err with code "stats_digest_too_large".
    // Mirror the setup style of the existing generate tests (:3280-3290).
}

#[tokio::test]
async fn revise_route_requires_admin_auth() {
    // POST /api/arena/code/revise without Authorization -> error code "admin_auth_required".
    // Mirror the existing route-auth test pattern for /api/arena/code/generate.
}
```

Run: `cargo test -p massive_game_server_core code_generation::tests::revise`
Expected: FAIL (`revise_bot_code` does not exist)

- [ ] **Step 2: Request/response structs**

After `GenerateBotCodeBody` (:175-181) add:

```rust
const MAX_ARENA_STATS_DIGEST_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Deserialize)]
struct ReviseBotCodeBody {
    model: String,
    previous_source: String,
    stats_digest: String,
    reasoning_mode: Option<String>,
    reasoning_effort: Option<String>,
}
```

Response: reuse `GenerateBotCodeResponse` unchanged (it already carries prompt_version/sha/audit fields).

- [ ] **Step 3: Parameterize the OpenRouter request builder**

Change `build_openrouter_request` (:1469) to take the two message contents:

```rust
fn build_openrouter_request(
    model: &str,
    max_completion_tokens: u32,
    reasoning_policy: &ArenaReasoningPolicy,
    system_content: &str,
    user_content: &str,
) -> OpenRouterChatRequest {
```

Replace the two `ARENA_UNIFORM_*` literals in the messages vec with `system_content.to_owned()` / `user_content.to_owned()`. Update the existing call in `generate_via_openrouter` (:1305-1306) to pass `ARENA_UNIFORM_SYSTEM_PROMPT` and `ARENA_UNIFORM_COMPETITION_PROMPT`.

- [ ] **Step 4: `revise_bot_code` service method**

Add next to `generate_bot_code` (:1177). It mirrors that function exactly except:

```rust
    async fn revise_bot_code(
        &self,
        body: ReviseBotCodeBody,
    ) -> Result<GenerateBotCodeResponse, ApiErrorBody> {
        let model = body.model.trim();
        if model.is_empty() {
            return Err(ApiErrorBody { code: "invalid_model", message: "model is required".to_owned() });
        }
        if body.previous_source.len() > self.inner.max_source_bytes {
            return Err(ApiErrorBody { code: "previous_source_too_large", message: format!("previous_source exceeds configured max ({} > {} bytes)", body.previous_source.len(), self.inner.max_source_bytes) });
        }
        if body.stats_digest.len() > MAX_ARENA_STATS_DIGEST_BYTES {
            return Err(ApiErrorBody { code: "stats_digest_too_large", message: format!("stats_digest exceeds configured max ({} > {} bytes)", body.stats_digest.len(), MAX_ARENA_STATS_DIGEST_BYTES) });
        }
        let reasoning_policy = normalize_arena_reasoning_policy(
            body.reasoning_mode.as_deref(),
            body.reasoning_effort.as_deref(),
        ).map_err(|message| ApiErrorBody { code: "invalid_reasoning_policy", message })?;

        let user_content = format!(
            "{}{}{}{}",
            ARENA_REVISION_USER_PROMPT_PREFIX, body.previous_source,
            ARENA_REVISION_STATS_SEPARATOR, body.stats_digest,
        );
        // From here: identical flow to generate_bot_code — call generate_via_openrouter
        // (passing ARENA_REVISION_SYSTEM_PROMPT and &user_content into the now-parameterized
        // builder; generate_via_openrouter takes the two contents as new params), validate the
        // returned source with validate_source_impl, and build GenerateBotCodeResponse with:
        //   prompt_style: ARENA_REVISION_PROMPT_VERSION
        //   objective: "improve the previous fighter using the mid-season performance digest"
        //   prompt_version: ARENA_REVISION_PROMPT_VERSION
        //   prompt_sha256: revision_prompt_sha256()
        //   prompt_text: canonical_revision_prompt()
        // and simulated fallback identical to generate_bot_code when no API key is set.
    }
```

Because `generate_via_openrouter` now needs the message contents, change its signature to accept `system_content: &str, user_content: &str` and forward them to `build_openrouter_request`; update the `generate_bot_code` call site accordingly.

- [ ] **Step 5: Wire the route**

After the `generate` route (:3030-3052) add an identical `revise` route:

```rust
    let revise = warp::path!("api" / "arena" / "code" / "revise")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .and_then(
            |authorization: Option<String>,
             body: ReviseBotCodeBody,
             service: CodeGenerationService| async move {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return Ok::<_, warp::Rejection>(error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    ));
                }
                let reply = match service.revise_bot_code(body).await {
                    Ok(response) => ok_response(response),
                    Err(err) => error_response(err.code, err.message),
                };
                Ok::<_, warp::Rejection>(reply)
            },
        );
```

Add `.or(revise)` into the returned route composition (:3102-3106), and note the route is inside the existing `/api/arena` admin gate (`server/src/operational/admin_auth.rs`), so the bearer check is defense-in-depth.

- [ ] **Step 6: Verify**

Run: `cargo test -p massive_game_server_core`
Expected: all PASS (new + existing, including existing generate tests — proves the builder refactor broke nothing)

- [ ] **Step 7: Commit**

```bash
git add server/src/operational/code_generation.rs
git commit -m "feat(arena): admin-gated /api/arena/code/revise route"
```

---

### Task 3: Runner — `--revise-only` parsing + revision contract validation

**Files:**
- Modify: `scripts/arena/run_top10_season.mjs` (`parseArgs` :60-106, `usage()` :108-136, validators :728-781)
- Test: `scripts/arena/run_top10_season.test.mjs`

- [ ] **Step 1: Failing tests**

Add to the test file:

```js
test('revise-only parses and cannot combine with other runner modes', () => {
  const options = parseArgs([
    '--revise-only',
    '--ranking-file', 'ranking.json',
    '--season-id', 'weekly-test',
    '--stats-state', 'state.json',
  ]);
  assert.equal(options.reviseOnly, true);
  assert.equal(options.statsState, 'state.json');
  assert.throws(
    () => parseArgs(['--revise-only', '--evaluate-only', '--ranking-file', 'r', '--season-id', 's', '--stats-state', 'x']),
    /cannot be combined/,
  );
  assert.throws(
    () => parseArgs(['--revise-only', '--season-id', 's', '--stats-state', 'x']),
    /requires --ranking-file, --season-id and --stats-state/,
  );
});

test('revision checkpoint validates against the revision contract', () => {
  // Use compiledCheckpoint() with prompt_version/prompt_sha256 replaced by the
  // revision contract values from a rawCodeStatus() that includes them, then:
  // - validateGenerationCheckpoint accepts it when codeStatus carries the revision pair
  // - throws /prompt/ when checkpoint carries the generation pair but the contract is revision
});
```

Run: `node --test scripts/arena/run_top10_season.test.mjs`
Expected: FAIL (`options.reviseOnly` undefined; codeStatus has no revision fields)

- [ ] **Step 2: parseArgs + usage**

In `parseArgs` options object add `reviseOnly: false, statsState: null`; add cases:

```js
      case '--revise-only': options.reviseOnly = true; break;
      case '--stats-state': options.statsState = nextValue(); break;
```

Add combination guard mirroring the rehydrate one (:98-104):

```js
  if (options.reviseOnly
      && (options.dryRun || options.snapshotOnly || options.generateOnly
        || options.evaluateOnly || options.rehydrateOnly)) {
    throw new Error('--revise-only cannot be combined with another runner mode');
  }
  if (options.reviseOnly && (!options.rankingFile || !options.seasonId || !options.statsState)) {
    throw new Error('--revise-only requires --ranking-file, --season-id and --stats-state');
  }
```

Add `--revise-only` and `--stats-state PATH` lines to `usage()`.

- [ ] **Step 3: Revision contract in codeStatus + checkpoint validation**

- In `normalizeCodeStatus`, accept optional `revision_prompt_version` / `revision_prompt_sha256` from the status response; when present, require the version to be a non-empty string and the sha to match `/^[a-f0-9]{64}$/`.
- Add `export function revisionContractFromCodeStatus(codeStatus)` returning `{ prompt_version, prompt_sha256 }` or `null` when absent.
- In `validateV2CheckpointMetadata` (:728-755), the prompt fields are currently compared against `codeStatus.prompt_version`/`prompt_sha256` inside `validateCheckpointAudit` — change the comparison to accept **either** the generation pair **or** the revision pair from `revisionContractFromCodeStatus(codeStatus)`; mismatch with both throws `generation checkpoint prompt contract differs for ${entrant.provider_model}`. Keep every other rule identical.
- In `competitionContractFromCodeStatus` (:1023-1038) add `revision_prompt_version`/`revision_prompt_sha256` when present so `assertCodeStatusUnchanged` also pins the revision contract across an epoch.

- [ ] **Step 4: Verify**

Run: `node --test scripts/arena/run_top10_season.test.mjs`
Expected: all PASS (51 existing + new)

- [ ] **Step 5: Commit**

```bash
git add scripts/arena/run_top10_season.mjs scripts/arena/run_top10_season.test.mjs
git commit -m "feat(arena): revise-only args and revision checkpoint contract"
```

---

### Task 4: Runner — stats digest builder

**Files:**
- Modify: `scripts/arena/run_top10_season.mjs` (new exported function near `competitionContractFromCodeStatus` :1023)
- Test: `scripts/arena/run_top10_season.test.mjs`

- [ ] **Step 1: Failing test**

```js
test('stats digest is bounded, deterministic and model-scoped', () => {
  const season = /* minimal season.json shape: { roster: [ {model_id:'a', model_name:'A', personal_rating:50, team_rating:40, collaboration_rating:30, world_rating:20, strategy_rating:44, rank:2, wins:3, losses:5, draws:1, matches_played:9}, {model_id:'b', ...rank:1, strategy_rating:60} ] } */;
  const supervisorState = { epochs: Array.from({ length: 12 }, (_, index) => ({
    standings: [
      { model_id: 'a', epoch_rank: (index % 3) + 1 },
      { model_id: 'b', epoch_rank: ((index + 1) % 3) + 1 },
    ],
  })) };
  const digest = buildRevisionStatsDigest({ seasonSnapshot: season, supervisorState, modelId: 'a' });
  const parsed = JSON.parse(digest);
  assert.equal(parsed.model_id, 'a');
  assert.equal(parsed.current.strategy_rating, 44);
  assert.deepEqual(parsed.last_epoch_ranks.length, 10); // last 10 epochs only
  assert.equal(parsed.top_opponents[0].model_id, 'b');
  assert.ok(Buffer.byteLength(digest, 'utf8') <= 4096);
  assert.equal(digest, buildRevisionStatsDigest({ seasonSnapshot: season, supervisorState, modelId: 'a' }), 'deterministic');
  assert.throws(() => buildRevisionStatsDigest({ seasonSnapshot: season, supervisorState, modelId: 'missing' }), /no roster entry/);
});
```

Run: `node --test scripts/arena/run_top10_season.test.mjs`
Expected: FAIL (`buildRevisionStatsDigest` not exported)

- [ ] **Step 2: Implement**

```js
export function buildRevisionStatsDigest({ seasonSnapshot, supervisorState, modelId }) {
  const roster = Array.isArray(seasonSnapshot?.roster) ? seasonSnapshot.roster : [];
  const entry = roster.find((candidate) => candidate.model_id === modelId);
  if (!entry) throw new Error(`no roster entry for ${modelId} in season snapshot`);
  const epochs = Array.isArray(supervisorState?.epochs) ? supervisorState.epochs : [];
  const lastEpochRanks = epochs.slice(-10).map((epoch) => {
    const standing = (epoch.standings || []).find((candidate) => candidate.model_id === modelId);
    return standing?.epoch_rank ?? null;
  });
  const epochWins = epochs.reduce((count, epoch) => count
    + ((epoch.standings || []).some((s) => s.model_id === modelId && s.epoch_rank === 1) ? 1 : 0), 0);
  const topOpponents = roster
    .filter((candidate) => candidate.model_id !== modelId)
    .sort((a, b) => Number(b.strategy_rating) - Number(a.strategy_rating))
    .slice(0, 3)
    .map((candidate) => ({ model_id: candidate.model_id, strategy_rating: candidate.strategy_rating }));
  const digest = {
    schema_version: 1,
    model_id: modelId,
    season_id: seasonSnapshot.season_id ?? null,
    epochs_completed: epochs.length,
    epoch_wins: epochWins,
    current: {
      rank: entry.rank,
      personal_rating: entry.personal_rating,
      team_rating: entry.team_rating,
      collaboration_rating: entry.collaboration_rating,
      world_rating: entry.world_rating,
      strategy_rating: entry.strategy_rating,
      wins: entry.wins ?? 0,
      losses: entry.losses ?? 0,
      draws: entry.draws ?? 0,
      matches_played: entry.matches_played ?? 0,
    },
    last_epoch_ranks: lastEpochRanks,
    top_opponents: topOpponents,
  };
  const serialized = JSON.stringify(digest);
  if (Buffer.byteLength(serialized, 'utf8') > 4096) throw new Error(`stats digest exceeds 4096 bytes for ${modelId}`);
  return serialized;
}
```

- [ ] **Step 3: Verify + commit**

Run: `node --test scripts/arena/run_top10_season.test.mjs` → all PASS

```bash
git add scripts/arena/run_top10_season.mjs scripts/arena/run_top10_season.test.mjs
git commit -m "feat(arena): bounded per-model revision stats digest"
```

---

### Task 5: Runner — `reviseEntrant` (journal → provider call → compile → atomic swap)

**Files:**
- Modify: `scripts/arena/run_top10_season.mjs` (after `generateEntrant` :1733; reuse `callArenaApi`, `validateCompileResponse` :636-656, `atomicWriteJson` :156, `readJson`)
- Test: `scripts/arena/run_top10_season.test.mjs`

- [ ] **Step 1: Failing tests**

```js
test('revision swaps the checkpoint only after a valid compile, consumes one provider call', async (t) => {
  // temp dirs {generations, sources, revisions}; gen-1 compiledCheckpoint() + source on disk;
  // apiClient mock counts calls:
  //   '/api/arena/code/revise'  -> revision generation response (revision prompt pair from rawCodeStatus)
  //   '/api/arena/code/compile' -> { compiled: true, bytes_written: 640, wasm_sha256: 'e'.repeat(64) }
  //   anything else -> throw
  // assert: exactly ONE revise call; returned checkpoint has wasm_sha256 'e'.repeat(64),
  //   revision_of === gen1 source_sha256, stats_digest_sha256 matches the digest,
  //   compile_attempts carried forward unchanged (no per-epoch increments, epoch-96 guard);
  // generations/<id>.json now holds the revised checkpoint; revision journal file exists.
});

test('failed revision keeps the gen-1 checkpoint and records the failure', async (t) => {
  // same setup; '/api/arena/code/compile' returns { compiled: false, compiler_stderr: 'boom' }
  // assert: rejects /boom/; generations/<id>.json is byte-identical to before;
  // a second reviseEntrant call rejects /revision already attempted/ WITHOUT any apiClient call.
});
```

Run: `node --test scripts/arena/run_top10_season.test.mjs` → FAIL

- [ ] **Step 2: Implement `reviseEntrant`**

```js
const REVISION_JOURNAL_FILE = 'revision-attempts.json';

export async function reviseEntrant(
  context,
  entrant,
  directories,
  { statsDigest, revisionEpoch, attemptAt = null },
) {
  const checkpointPath = path.join(directories.generations, `${entrant.model_id}.json`);
  const sourcePath = path.join(directories.sources, `${entrant.model_id}.rs`);
  const journalPath = path.join(directories.revisions, REVISION_JOURNAL_FILE);
  const previous = await readJson(checkpointPath);
  const previousSource = await fs.readFile(sourcePath, 'utf8');
  validateGenerationCheckpoint(previous, entrant, previousSource, context.codeStatus);

  const journal = await readJson(journalPath).catch((error) => {
    if (error?.code === 'ENOENT') return { attempts: {} };
    throw error;
  });
  if (journal.attempts?.[entrant.model_id]) {
    throw new Error(`revision already attempted for ${entrant.provider_model}`);
  }
  journal.attempts = journal.attempts || {};
  journal.attempts[entrant.model_id] = {
    started_at: attemptAt || new Date().toISOString(),
    stats_digest_sha256: sha256(statsDigest),
  };
  await fs.mkdir(directories.revisions, { recursive: true, mode: 0o700 });
  await atomicWriteJson(journalPath, journal);

  const response = await callArenaApi(context, {
    method: 'POST',
    route: '/api/arena/code/revise',
    timeoutMs: 180_000,
    body: {
      model: entrant.provider_model,
      previous_source: previousSource,
      stats_digest: statsDigest,
      reasoning_mode: entrant.reasoning_policy.mode,
      reasoning_effort: entrant.reasoning_policy.effort,
    },
  });
  // Reuse the generation-response validation but against the REVISION contract:
  // prompt_version/prompt_sha256 must equal revisionContractFromCodeStatus(context.codeStatus);
  // resolved model must equal entrant.provider_model; finish_reason 'stop'; terminal usage.
  const verified = validateRevisionResponse(response, context.codeStatus, entrant);

  const compile = await callArenaApi(context, {
    method: 'POST',
    route: '/api/arena/code/compile',
    timeoutMs: 180_000,
    body: { model_id: entrant.model_id, source_code: verified.source, overwrite: true },
  });
  const wasmArtifact = validateCompileResponse(compile, entrant.model_id);

  const completedAt = new Date().toISOString();
  const revised = {
    ...previous,
    source_bytes: Buffer.byteLength(verified.source, 'utf8'),
    source_sha256: sha256(verified.source),
    wasm_bytes: wasmArtifact.wasmBytes,
    wasm_sha256: wasmArtifact.wasmSha256,
    prompt_version: verified.promptVersion,
    prompt_sha256: verified.promptSha256,
    compiled_at: completedAt,
    last_compile_attempt_at: completedAt,
    last_compile_error_sha256: null,
    revision_of: previous.source_sha256,
    revision_epoch: revisionEpoch,
    stats_digest_sha256: sha256(statsDigest),
  };
  validateGenerationCheckpoint(revised, entrant, verified.source, context.codeStatus);
  // Persist source first, then checkpoint; both writes atomic. The epoch loop's
  // trustArchivedArtifact path validates the pair on next epoch.
  await atomicWriteBytes(sourcePath, Buffer.from(verified.source, 'utf8'));
  await atomicWriteJson(checkpointPath, revised);
  journal.attempts[entrant.model_id].completed_at = completedAt;
  journal.attempts[entrant.model_id].wasm_sha256 = wasmArtifact.wasmSha256;
  await atomicWriteJson(journalPath, journal);
  return revised;
}
```

`validateRevisionResponse(response, codeStatus, entrant)` — new small validator: checks `prompt_version`/`prompt_sha256` against `revisionContractFromCodeStatus`, delegates the rest to the existing `validateGeneratedResponse({ ...response.generated ?? response, source_code: undefined }, ...)` pattern used at :714 (mirror how `validateArchivedProviderResponse` builds a response-shaped object), returns `{ source, sourceBytes, finishReason, resolvedModel, providerName, providerResponseId, usage, promptVersion, promptSha256 }`.

`atomicWriteBytes` — if not already exported in this file, add next to `atomicWriteJson` (:156-160): same tmp+rename pattern with `Buffer`.

- [ ] **Step 3: Verify + commit**

Run: `node --test scripts/arena/run_top10_season.test.mjs` → all PASS

```bash
git add scripts/arena/run_top10_season.mjs scripts/arena/run_top10_season.test.mjs
git commit -m "feat(arena): journaled single-shot reviseEntrant with atomic swap"
```

---

### Task 6: Runner — wire `--revise-only` into `main()`

**Files:**
- Modify: `scripts/arena/run_top10_season.mjs` (`directories` :2075, mode branch :2159-2218)

- [ ] **Step 1: Add `revisions` directory**

In the `directories` object (:2075) add:

```js
    revisions: path.join(seasonDirectory, 'revisions'),
```

- [ ] **Step 2: Mode branch**

Add before the `if (options.generateOnly)` early-return block (:2207), after the evaluateOnly branch:

```js
  } else if (options.reviseOnly) {
    const supervisorState = await readJson(path.resolve(options.statsState));
    const seasonSnapshot = await readJson(path.join(seasonDirectory, 'season.json'));
    const revisionEpoch = Array.isArray(supervisorState.epochs) ? supervisorState.epochs.length : 0;
    process.stdout.write(`[arena] revising ${entrants.length} fighters for ${seasonId} at epoch ${revisionEpoch}\n`);
    const results = await mapLimit(entrants, config.generationConcurrency, async (entrant) => {
      const statsDigest = buildRevisionStatsDigest({
        seasonSnapshot, supervisorState, modelId: entrant.model_id,
      });
      try {
        const checkpoint = await reviseEntrant(
          context, entrant, directories, { statsDigest, revisionEpoch },
        );
        process.stdout.write(`[arena] revised ${entrant.provider_model}\n`);
        return { model_id: entrant.model_id, status: 'improved', checkpoint };
      } catch (error) {
        process.stdout.write(`[arena] kept gen-1 for ${entrant.provider_model}: ${String(error?.message || error).slice(0, 200)}\n`);
        return { model_id: entrant.model_id, status: 'kept_gen1', error: String(error?.message || error).slice(0, 500) };
      }
    });
    await atomicWriteJson(path.join(seasonDirectory, 'revision-results.json'), {
      season_id: seasonId, revision_epoch: revisionEpoch, completed_at: new Date().toISOString(),
      entries: results.map(({ checkpoint, ...rest }) => ({
        ...rest,
        source_sha256_after: checkpoint?.source_sha256 ?? null,
        wasm_sha256_after: checkpoint?.wasm_sha256 ?? null,
      })),
    });
    await releaseLeagueLock();
    return;
  }
```

Also update the `generatedEntrants` declaration flow: this branch is terminal (returns), so place it as a sibling of the other mode branches and ensure the existing `if (options.generateOnly)` early-return still works. If a model already attempted (journal), `reviseEntrant` throws → recorded as `kept_gen1`, so re-running `--revise-only` after a crash is safe and never re-calls providers.

- [ ] **Step 3: Test**

```js
test('revise-only run produces revision-results.json and never calls providers twice', async () => {
  // mirror the runner-level test setup used for evaluate-only (:200-215 area),
  // with 2 entrants; assert results file shape and idempotent re-run.
});
```

Run: `node --test scripts/arena/run_top10_season.test.mjs` → all PASS

- [ ] **Step 4: Commit**

```bash
git add scripts/arena/run_top10_season.mjs scripts/arena/run_top10_season.test.mjs
git commit -m "feat(arena): wire --revise-only season mode"
```

---

### Task 7: Supervisor — accept one artifact swap at the revision boundary

**Files:**
- Modify: `scripts/arena/weekly_supervisor.mjs` (epoch roster pin :1348-1357, `cumulativeRoster` :1509-1511, plus wherever `frozenArtifact` is looked up for validation)
- Test: `scripts/arena/weekly_supervisor.test.mjs`

- [ ] **Step 1: Failing test**

```js
test('epoch validation and cumulative roster accept a single artifact swap at the recorded revision', () => {
  // Build a state with artifact_bindings (gen-1 hashes) and a revision record:
  //   revision: { epoch_index: 2, entries: [{ model_id: 'm1', status: 'improved',
  //     wasm_sha256_before: 'a'.repeat(64), wasm_sha256_after: 'b'.repeat(64), ... }] }
  // - an epoch snapshot at index < 2 with gen-1 hashes validates
  // - an epoch snapshot at index >= 2 with revised hashes validates
  // - an epoch snapshot at index >= 2 with a THIRD hash throws /epoch roster integrity/
  // - cumulativeRoster over mixed pre/post snapshots does not throw for 'm1'
  //   but still throws for an unrecorded swap on 'm2'
});
```

Run: `node --test scripts/arena/weekly_supervisor.test.mjs` → FAIL

- [ ] **Step 2: Implement `expectedArtifactForEpoch(state, modelId, epochIndex)`**

```js
function expectedArtifactForEpoch(state, modelId, epochIndex) {
  const frozen = (state.artifact_bindings || []).find((binding) => binding.model_id === modelId);
  const revision = state.revision;
  if (revision && Number.isSafeInteger(revision.epoch_index) && epochIndex >= revision.epoch_index) {
    const entry = (revision.entries || []).find((candidate) => candidate.model_id === modelId);
    if (entry?.status === 'improved'
        && /^\d+$/.test(String(entry.wasm_bytes_after))
        && /^[a-f0-9]{64}$/.test(String(entry.wasm_sha256_after || ''))) {
      return { wasm_bytes: Number(entry.wasm_bytes_after), wasm_sha256: entry.wasm_sha256_after };
    }
  }
  return frozen;
}
```

- At :1352-1353 replace `frozenArtifact?.wasm_bytes !== entry.wasm_bytes || frozenArtifact?.wasm_sha256 !== entry.wasm_sha256` with a lookup through `expectedArtifactForEpoch(state, entry.model_id, epochIndex)` (thread `epochIndex` into the validator — it already receives `state`; find the snapshot's index via `state.epochs.length` at call time in `archiveOrRunEpoch`, i.e. the index being committed).
- In `cumulativeRoster` (:1509-1511) replace the throw: when hashes differ, verify the change matches the recorded revision boundary for that model (`expectedArtifactForEpoch` pre/post), update `aggregate.template` to the newer entry, and continue; otherwise keep throwing.
- The validator must also accept `entry.compile_attempts` staying flat post-revision (already true: range check only, :1354-1356).

- [ ] **Step 3: Verify + commit**

Run: `node --test scripts/arena/weekly_supervisor.test.mjs` → all PASS

```bash
git add scripts/arena/weekly_supervisor.mjs scripts/arena/weekly_supervisor.test.mjs
git commit -m "feat(arena): tolerate the single recorded mid-season artifact swap"
```

---

### Task 8: Supervisor — schedule the revision round at epoch 336

**Files:**
- Modify: `scripts/arena/weekly_supervisor.mjs` (constant near top, `main()` loop :1814-1825, `validateState`)
- Test: `scripts/arena/weekly_supervisor.test.mjs`

- [ ] **Step 1: Failing test**

```js
test('revision round runs exactly once at the revision epoch and epochs resume after', async () => {
  // state with 336 epochs recorded and no revision record;
  // runner mock records argv; assert:
  // - child invoked with ['--ranking-file', ..., '--season-id', ..., '--revise-only', '--stats-state', statePath]
  // - state.revision written with epoch_index 336 and per-model entries from revision-results.json
  // - a second pass does not re-invoke the runner
});
```

Run: `node --test scripts/arena/weekly_supervisor.test.mjs` → FAIL

- [ ] **Step 2: Implement**

Add near the other constants:

```js
const REVISION_EPOCH_INDEX = integerEnv('ARENA_WEEKLY_REVISION_EPOCH', 336, 1, 1000);
```

(Keep it env-overridable for testing; default 336.)

Add `runRevisionIfDue({ config, state, statePath, weekDirectory, redact })`:

```js
async function runRevisionIfDue({ config, state, statePath, weekDirectory, redact }) {
  if (state.revision?.completed === true) return state;
  if (!Array.isArray(state.epochs) || state.epochs.length < REVISION_EPOCH_INDEX) return state;
  const rankingPath = await rankingPathFor(weekDirectory, state);
  process.stdout.write(`[arena-weekly] running mid-season revision round for ${state.week_id} at epoch ${state.epochs.length}\n`);
  await runRunner([
    '--ranking-file', rankingPath,
    '--season-id', state.season_id,
    '--revise-only',
    '--stats-state', statePath,
  ], { env: { ARENA_TEAM_SIZE: String(state.team_size) }, redact });
  const results = await readJson(path.join(
    ROOT_DIR, 'artifacts/arena/seasons', state.season_id, 'revision-results.json',
  ));
  const nextState = {
    ...state,
    revision: {
      completed: true,
      epoch_index: state.epochs.length,
      completed_at: results.completed_at,
      entries: results.entries,
    },
    updated_at: nowIso(),
  };
  validateState(nextState, state.week_id, state.seed_pack_size);
  await atomicWriteJson(statePath, nextState);
  process.stdout.write(`[arena-weekly] revision round complete: ${results.entries.filter((e) => e.status === 'improved').length}/${results.entries.length} improved\n`);
  return nextState;
}
```

- In `main()` between `publishIfNeeded` and `recordEpoch` (:1814-1816):

```js
        state = await runRevisionIfDue({
          config, state, statePath, weekDirectory: loaded.weekDirectory, redact,
        });
```

- In `validateState`, allow the optional `revision` key: when present require `completed === true`, safe `epoch_index >= 1`, and an `entries` array whose items have `model_id` in `state.entrant_model_ids`, `status` in `['improved', 'kept_gen1']`, and optional 64-hex `*_sha256` fields. (Mirror the strictness of neighboring validators.)
- Failure semantics: if the child exits non-zero, `runRunner` throws → the epoch loop's existing catch records a failure and backs off; `state.revision` is never written, so the round retries (provider calls are journal-protected against duplicates). This matches the league's existing failure model.

- [ ] **Step 3: Verify + commit**

Run: `node --test scripts/arena/*.test.mjs` → all PASS

```bash
git add scripts/arena/weekly_supervisor.mjs scripts/arena/weekly_supervisor.test.mjs
git commit -m "feat(arena): schedule mid-season revision round at epoch 336"
```

---

### Task 9: Rollout — rebuild, rotate admin token, restart, verify

**Files:**
- Modify: `run-server-with-turn.js:73` (hardcoded `MGS_ADMIN_BEARER_TOKEN = 'arena-test-token-2026'`)
- Modify: the supervisor/runner secret source (`~/.config/massive-game-server/arena-weekly.env` — check `ARENA_ADMIN_BEARER_TOKEN` / `_FILE` there)

- [ ] **Step 1: Rotate the admin token (same restart window)**

```bash
NEW_TOKEN=$(openssl rand -hex 32)
# Write to the env file the supervisor reads (verify exact key name first):
#   ~/.config/massive-game-server/arena-weekly.env  -> ARENA_ADMIN_BEARER_TOKEN=<NEW_TOKEN>
# Update run-server-with-turn.js:73 to read process.env.MGS_ADMIN_BEARER_TOKEN
# (sourced from the same env file by the systemd unit or the wrapper) instead of the literal.
```

Verify exact consumption: `grep -rn "MGS_ADMIN_BEARER_TOKEN\|ARENA_ADMIN_BEARER_TOKEN" run-server-with-turn.js scripts/arena/arena_api_client.mjs ~/.config/massive-game-server/`. The runner's client (`arena_api_client.mjs`) already supports `ARENA_ADMIN_BEARER_TOKEN_FILE` per `usage()` — prefer the `_FILE` variant with a 0600 file over env literals.

- [ ] **Step 2: Build + full test gates**

```bash
cd /home/habitat/massive-game-server-deployment/massive_game_server
cargo test --release -p massive_game_server_core   # full suite
node --test scripts/arena/*.test.mjs               # all arena scripts
cargo build --release                              # ~4.5 min
```

- [ ] **Step 3: Restart services**

```bash
systemctl --user restart massive-game-server.service
sleep 5
curl -s localhost:8080/healthz
systemctl --user restart massive-game-arena-weekly.service
```

- [ ] **Step 4: Verify live**

```bash
# Revision contract visible:
curl -s -H "Authorization: Bearer $NEW_TOKEN" localhost:8080/api/arena/code/status | python3 -m json.tool | grep revision
# Old token dead, new token works:
curl -s -o /dev/null -w '%{http_code}\n' -H "Authorization: Bearer arena-test-token-2026" localhost:8080/api/arena/models   # expect 401/503
curl -s -o /dev/null -w '%{http_code}\n' -H "Authorization: Bearer $NEW_TOKEN" localhost:8080/api/arena/models             # expect 200
# Game still healthy end-to-end:
cd e2e && npm test
# League resumes epochs:
journalctl --user -u massive-game-arena-weekly.service -n 20 --no-pager
```

- [ ] **Step 5: Watch the first revision round (Wednesday, epoch 336)**

```bash
journalctl --user -u massive-game-arena-weekly.service -f | grep -i revision
python3 -c "import json; s=json.load(open('artifacts/arena/weekly-supervisor/2026-W31/state.json')); print(json.dumps(s.get('revision'), indent=1))"
```

- [ ] **Step 6: Commit rollout changes**

```bash
git add run-server-with-turn.js
git commit -m "chore(arena): read admin bearer from env file, drop hardcoded token"
```

---

## Self-review notes (completed)

- Spec coverage: server route (T1-2), runner mode + digest + journal + swap (T3-6), supervisor scheduling + swap tolerance (T7-8), rollout + token rotation (T9). Digest deviates from the spec's "per-mode ratings" — season roster carries personal/team/collaboration/world/strategy aggregates, not per-mode values; v1 digest uses the available aggregates (spec updated verbally here; per-mode can be v2).
- `compile_attempts` cap: revision checkpoints carry the counter forward unchanged (epoch-96 guard stays intact); documented in hard constraints.
- Type consistency: `reviseEntrant` options `{statsDigest, revisionEpoch, attemptAt}`; `validateRevisionResponse` returns `{source, sourceBytes, finishReason, resolvedModel, providerName, providerResponseId, usage, promptVersion, promptSha256}`; `expectedArtifactForEpoch(state, modelId, epochIndex)`; `buildRevisionStatsDigest({seasonSnapshot, supervisorState, modelId})` — used consistently across tasks.
