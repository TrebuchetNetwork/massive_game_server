// Continuous Model League — child-process wrapper for run_top10_season.mjs.
//
// Modeled on weekly_supervisor.mjs's runRunner: spawn the season runner with
// the supervisor's environment plus ARENA_TOP_MODELS=10, capture stdout and
// stderr with byte caps, redact credentials from anything that leaves this
// process, and kill the child after a 30-minute timeout (SIGTERM, then
// SIGKILL after a short grace period).

import { spawn } from 'node:child_process';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT_DIR = path.resolve(SCRIPT_DIR, '../../..');
const RUNNER_PATH = path.join(SCRIPT_DIR, '..', 'run_top10_season.mjs');
const MAX_CAPTURE_BYTES = 8 * 1024 * 1024;
const MAX_ERROR_CHARS = 2_000;
const RUNNER_TIMEOUT_MS = 30 * 60 * 1000;
const KILL_GRACE_MS = 10_000;
const SECRET_ENV_NAMES = Object.freeze([
  'ARENA_ADMIN_BEARER_TOKEN',
  'OPENROUTER_API_KEY',
]);

async function secretRedactor() {
  const values = new Set();
  for (const name of SECRET_ENV_NAMES) {
    const direct = String(process.env[name] || '').trim();
    if (direct) values.add(direct);
    const secretPath = String(process.env[`${name}_FILE`] || '').trim();
    if (!secretPath) continue;
    values.add(secretPath);
    try {
      const fromFile = (await fs.readFile(secretPath, 'utf8')).trim();
      if (fromFile) values.add(fromFile);
    } catch {
      // The runner reports an unusable credential without its contents.
    }
  }
  const ordered = [...values].filter(Boolean).sort((left, right) => right.length - left.length);
  return (value) => {
    let sanitized = String(value ?? '');
    for (const secret of ordered) sanitized = sanitized.split(secret).join('[REDACTED]');
    return sanitized;
  };
}

const collectCapped = (current, chunk) => {
  const combined = current + chunk;
  return combined.length > MAX_CAPTURE_BYTES
    ? combined.slice(combined.length - MAX_CAPTURE_BYTES)
    : combined;
};

/**
 * Run the top-10 season runner as a child process.
 * `args` are CLI flags (e.g. ['--dry-run']); `env` overrides the inherited
 * environment (ARENA_TOP_MODELS defaults to 10). Resolves with the captured
 * { stdout, stderr } (stderr redacted); rejects with a sanitized error on a
 * non-zero exit or after the 30-minute timeout kills the child.
 */
export async function runSeasonRunner(args, { env = {}, timeoutMs = RUNNER_TIMEOUT_MS } = {}) {
  const redact = await secretRedactor();
  const child = spawn(process.execPath, [RUNNER_PATH, ...args], {
    cwd: ROOT_DIR,
    env: { ...process.env, ARENA_TOP_MODELS: '10', ...env },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let stdout = '';
  let stderr = '';
  child.stdout.on('data', (chunk) => {
    stdout = collectCapped(stdout, chunk.toString('utf8'));
  });
  child.stderr.on('data', (chunk) => {
    stderr = collectCapped(stderr, chunk.toString('utf8'));
  });

  let timedOut = false;
  const result = await new Promise((resolve, reject) => {
    let killTimer = null;
    const timeout = setTimeout(() => {
      timedOut = true;
      child.kill('SIGTERM');
      killTimer = setTimeout(() => {
        if (child.exitCode === null) child.kill('SIGKILL');
      }, KILL_GRACE_MS);
      killTimer.unref();
    }, timeoutMs);
    timeout.unref();
    const settle = (callback, value) => {
      clearTimeout(timeout);
      if (killTimer) clearTimeout(killTimer);
      callback(value);
    };
    child.once('error', (error) => settle(reject, error));
    child.once('close', (code, signal) => settle(resolve, { code, signal }));
  });

  if (timedOut) {
    throw new Error(
      `season runner timed out after ${timeoutMs}ms and was killed`,
    );
  }
  if (result.code !== 0) {
    const detail = redact(stderr).trim().slice(-MAX_ERROR_CHARS);
    throw new Error(
      `season runner exited ${result.signal ? `on ${result.signal}` : `with code ${result.code}`}`
      + (detail ? `: ${detail}` : ''),
    );
  }
  return { stdout, stderr: redact(stderr) };
}
