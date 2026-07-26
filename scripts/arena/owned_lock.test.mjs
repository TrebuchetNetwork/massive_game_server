import assert from 'node:assert/strict';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import process from 'node:process';
import test from 'node:test';
import { acquireOwnedLock, releaseOwnedLock } from './owned_lock.mjs';

const fastOptions = {
  settleMs: 5,
  initializationGraceMs: 0,
  maxAttempts: 64,
  activeMessage: (owner) => `active pid ${owner.pid}`,
};

async function tempLock() {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'arena-owned-lock-'));
  return {
    directory,
    lockPath: path.join(directory, 'runner.lock'),
  };
}

test('only one contender can reclaim a stale lock', async () => {
  const { directory, lockPath } = await tempLock();
  let winner;
  try {
    await fs.mkdir(lockPath, { mode: 0o700 });
    await fs.writeFile(path.join(lockPath, 'owner.json'), `${JSON.stringify({
      pid: 2_147_483_647,
      token: 'stale-owner-token-0001',
      started_at: '2000-01-01T00:00:00.000Z',
    })}\n`);

    const contenders = await Promise.allSettled([
      acquireOwnedLock(lockPath, fastOptions),
      acquireOwnedLock(lockPath, fastOptions),
    ]);
    const fulfilled = contenders.filter((result) => result.status === 'fulfilled');
    const rejected = contenders.filter((result) => result.status === 'rejected');
    assert.equal(fulfilled.length, 1);
    assert.equal(rejected.length, 1);
    assert.match(String(rejected[0].reason?.message), /active pid/);
    winner = fulfilled[0].value;

    const owner = JSON.parse(await fs.readFile(path.join(lockPath, 'owner.json'), 'utf8'));
    assert.equal(owner.token, winner.token);
  } finally {
    if (winner) await releaseOwnedLock(winner, fastOptions);
    await fs.rm(directory, { recursive: true, force: true });
  }
});

test('release never removes a replacement owner lock', async () => {
  const { directory, lockPath } = await tempLock();
  const displacedPath = `${lockPath}.displaced`;
  let first;
  let second;
  try {
    first = await acquireOwnedLock(lockPath, fastOptions);
    await fs.rename(lockPath, displacedPath);
    second = await acquireOwnedLock(lockPath, fastOptions);

    assert.equal(await releaseOwnedLock(first, fastOptions), false);
    const current = JSON.parse(await fs.readFile(path.join(lockPath, 'owner.json'), 'utf8'));
    assert.equal(current.token, second.token);
    assert.equal(await releaseOwnedLock(second, fastOptions), true);
    second = null;
  } finally {
    if (second) await releaseOwnedLock(second, fastOptions);
    await fs.rm(directory, { recursive: true, force: true });
  }
});

test('legacy PID-only lock files retain live ownership and stale files migrate safely', async () => {
  const { directory, lockPath } = await tempLock();
  let acquired;
  try {
    await fs.writeFile(lockPath, `${JSON.stringify({
      pid: process.pid,
      started_at: new Date().toISOString(),
    })}\n`, { mode: 0o600 });
    await assert.rejects(acquireOwnedLock(lockPath, fastOptions), /active pid/);

    await fs.writeFile(lockPath, `${JSON.stringify({
      pid: 2_147_483_647,
      started_at: '2000-01-01T00:00:00.000Z',
    })}\n`, { mode: 0o600 });
    acquired = await acquireOwnedLock(lockPath, fastOptions);
    assert.equal((await fs.lstat(lockPath)).isDirectory(), true);
    assert.equal(await releaseOwnedLock(acquired, fastOptions), true);
    acquired = null;
  } finally {
    if (acquired) await releaseOwnedLock(acquired, fastOptions);
    await fs.rm(directory, { recursive: true, force: true });
  }
});
