import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { runSeasonRunner } from '../runner.mjs';

test('runSeasonRunner captures stdout from a successful runner invocation', async () => {
  const { stdout } = await runSeasonRunner(['--help']);
  assert.match(stdout, /run_top10_season/);
  assert.match(stdout, /--evaluate-only/);
});

test('runSeasonRunner rejects with a sanitized error on a non-zero exit', async () => {
  await assert.rejects(
    runSeasonRunner(['--definitely-not-a-flag']),
    (error) => {
      assert.match(error.message, /season runner exited with code 1/);
      assert.match(error.message, /unknown option/);
      return true;
    },
  );
});

test('runSeasonRunner kills the child after the timeout', async () => {
  // A FIFO with no writer blocks the child's ranking-file read forever, so
  // the timeout — not the child — decides the outcome.
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'cml-runner-'));
  const fifoPath = path.join(directory, 'ranking.fifo');
  execFileSync('mkfifo', [fifoPath]);
  await assert.rejects(
    runSeasonRunner(['--ranking-file', fifoPath], { timeoutMs: 250 }),
    /season runner timed out after 250ms and was killed/,
  );
});
