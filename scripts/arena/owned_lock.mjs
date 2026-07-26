import { randomBytes } from 'node:crypto';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const DEFAULT_SETTLE_MS = 40;
const DEFAULT_INITIALIZATION_GRACE_MS = 2_000;
const DEFAULT_MAX_ATTEMPTS = 32;

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

const ownerPath = (lockPath) => path.join(lockPath, 'owner.json');

function newToken() {
  return `${process.pid}-${Date.now()}-${randomBytes(12).toString('hex')}`;
}

async function processStartId(pid) {
  if (process.platform !== 'linux') return null;
  try {
    const stat = await fs.readFile(`/proc/${pid}/stat`, 'utf8');
    const closingParen = stat.lastIndexOf(')');
    if (closingParen < 0) return null;
    // Fields after the command name start at proc field 3. Start time is field 22.
    const fields = stat.slice(closingParen + 1).trim().split(/\s+/);
    return fields[19] ? `linux-proc-start:${fields[19]}` : null;
  } catch {
    return null;
  }
}

async function readJsonFile(targetPath) {
  try {
    return JSON.parse(await fs.readFile(targetPath, 'utf8'));
  } catch {
    return null;
  }
}

function validProcessOwner(value) {
  return value
    && Number.isSafeInteger(Number(value.pid))
    && Number(value.pid) > 0;
}

async function observeLock(lockPath) {
  let stat;
  try {
    stat = await fs.lstat(lockPath);
  } catch (error) {
    if (error?.code === 'ENOENT') return null;
    throw error;
  }
  const owner = stat.isDirectory()
    ? await readJsonFile(ownerPath(lockPath))
    : await readJsonFile(lockPath);
  return {
    dev: stat.dev,
    ino: stat.ino,
    isDirectory: stat.isDirectory(),
    mtimeMs: stat.mtimeMs,
    owner,
  };
}

function sameIdentity(left, right) {
  return Boolean(left && right && left.dev === right.dev && left.ino === right.ino);
}

async function ownerIsAlive(owner) {
  if (!validProcessOwner(owner)) return false;
  const pid = Number(owner.pid);
  try {
    process.kill(pid, 0);
  } catch (error) {
    if (error?.code === 'ESRCH') return false;
    // EPERM proves a process exists even when it belongs to another account.
    if (error?.code === 'EPERM') return true;
    throw error;
  }
  if (typeof owner.process_start_id === 'string' && owner.process_start_id) {
    const currentStartId = await processStartId(pid);
    if (currentStartId && currentStartId !== owner.process_start_id) return false;
  }
  return true;
}

function activeError(activeMessage, owner) {
  const message = typeof activeMessage === 'function'
    ? activeMessage(owner)
    : activeMessage;
  return new Error(message || `lock is already owned by PID ${owner.pid}`);
}

async function restoreMovedLock(quarantinePath, lockPath, settleMs) {
  try {
    await fs.rename(quarantinePath, lockPath);
    return;
  } catch (error) {
    if (!['EEXIST', 'ENOTEMPTY'].includes(error?.code)) throw error;
  }
  // A different claimant won the canonical pathname. Its settle check prevents
  // the displaced owner from proceeding without a lock, so the quarantine can
  // be removed after that check has had time to complete.
  await delay(settleMs * 2);
  await fs.rm(quarantinePath, { recursive: true, force: true });
}

async function reclaimObservedLock(lockPath, observed, settleMs) {
  // Narrow the stale-check/rename window. If the canonical inode changed, the
  // caller must observe and evaluate its new owner instead of deleting it.
  const current = await observeLock(lockPath);
  if (!sameIdentity(observed, current)) return false;
  if (validProcessOwner(current.owner) && await ownerIsAlive(current.owner)) return false;

  const quarantinePath = `${lockPath}.stale-${newToken()}`;
  try {
    await fs.rename(lockPath, quarantinePath);
  } catch (error) {
    if (['ENOENT', 'EEXIST', 'ENOTEMPTY'].includes(error?.code)) return false;
    throw error;
  }

  const moved = await observeLock(quarantinePath);
  if (!sameIdentity(observed, moved)) {
    await restoreMovedLock(quarantinePath, lockPath, settleMs);
    return false;
  }
  await fs.rm(quarantinePath, { recursive: true, force: true });
  return true;
}

/**
 * Acquire a process-owned filesystem lock.
 *
 * The canonical lock is a directory so a stale inode can be moved aside and
 * verified before deletion. A random ownership token and Linux process start ID
 * prevent PID reuse and pathname replacement from granting false ownership.
 */
export async function acquireOwnedLock(lockPath, options = {}) {
  const settleMs = options.settleMs ?? DEFAULT_SETTLE_MS;
  const initializationGraceMs = options.initializationGraceMs
    ?? DEFAULT_INITIALIZATION_GRACE_MS;
  const maxAttempts = options.maxAttempts ?? DEFAULT_MAX_ATTEMPTS;
  await fs.mkdir(path.dirname(lockPath), { recursive: true });

  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    const token = newToken();
    const candidatePath = `${lockPath}.candidate-${token}`;
    const owner = {
      pid: process.pid,
      token,
      started_at: new Date().toISOString(),
      process_start_id: await processStartId(process.pid),
    };
    await fs.mkdir(candidatePath, { mode: 0o700 });
    try {
      await fs.writeFile(ownerPath(candidatePath), `${JSON.stringify(owner)}\n`, {
        flag: 'wx',
        mode: 0o600,
      });
      await fs.rename(candidatePath, lockPath);

      // Let any stale reclaimer that observed the previous inode finish, then
      // prove this token still owns the canonical pathname before returning.
      await delay(settleMs);
      const confirmed = await observeLock(lockPath);
      if (confirmed?.isDirectory && confirmed.owner?.token === token) {
        return {
          lockPath,
          token,
          pid: process.pid,
          processStartId: owner.process_start_id,
        };
      }
      await delay(settleMs);
      continue;
    } catch (error) {
      await fs.rm(candidatePath, { recursive: true, force: true }).catch(() => {});
      if (!['EEXIST', 'ENOTEMPTY', 'ENOTDIR', 'EISDIR'].includes(error?.code)) throw error;
    }

    const observed = await observeLock(lockPath);
    if (!observed) continue;
    if (validProcessOwner(observed.owner) && await ownerIsAlive(observed.owner)) {
      throw activeError(options.activeMessage, observed.owner);
    }
    if (!observed.owner
        && Date.now() - observed.mtimeMs < initializationGraceMs) {
      await delay(settleMs);
      continue;
    }
    await reclaimObservedLock(lockPath, observed, settleMs);
  }
  throw new Error(`could not acquire stable ownership of lock '${lockPath}'`);
}

/** Remove only the canonical lock carrying this caller's ownership token. */
export async function releaseOwnedLock(lock, options = {}) {
  if (!lock?.lockPath || !lock?.token) return false;
  const settleMs = options.settleMs ?? DEFAULT_SETTLE_MS;
  const observed = await observeLock(lock.lockPath);
  if (!observed?.isDirectory || observed.owner?.token !== lock.token) return false;

  const quarantinePath = `${lock.lockPath}.release-${newToken()}`;
  try {
    await fs.rename(lock.lockPath, quarantinePath);
  } catch (error) {
    if (error?.code === 'ENOENT') return false;
    throw error;
  }
  const moved = await observeLock(quarantinePath);
  if (!sameIdentity(observed, moved) || moved?.owner?.token !== lock.token) {
    await restoreMovedLock(quarantinePath, lock.lockPath, settleMs);
    return false;
  }
  await fs.rm(quarantinePath, { recursive: true, force: true });
  return true;
}
