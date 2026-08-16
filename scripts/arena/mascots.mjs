// Mascot lookup for arena models. Exact/prefix match against mascots.json,
// deterministic fallback for unknown models (e.g. daily league entrants).
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REGISTRY_PATH = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  'mascots.json',
);

let cached = null;
function registry() {
  if (!cached) {
    cached = JSON.parse(readFileSync(REGISTRY_PATH, 'utf8'));
  }
  return cached;
}

/** Strip a trailing version/date suffix: "deepseek/deepseek-v4-pro-20260423" → "deepseek/deepseek-v4-pro". */
function baseSlug(modelId) {
  return String(modelId || '').replace(/-\d{6,}(-|$).*$/, '$1').replace(/:free$/, '');
}

export function mascotFor(modelId) {
  const { mascots, fallbacks } = registry();
  const id = String(modelId || '');
  const base = baseSlug(id);
  // Longest-prefix match so "deepseek/deepseek-v4-flash-20260731" prefers
  // the flash entry over the pro entry regardless of key order.
  let best = null;
  for (const key of Object.keys(mascots)) {
    if ((id.startsWith(key) || base.startsWith(key)) && (!best || key.length > best.length)) {
      best = key;
    }
  }
  if (best) return { key: best, ...mascots[best] };
  const digest = createHash('sha256').update(id).digest();
  const index = digest[0] % fallbacks.length;
  return { key: null, ...fallbacks[index] };
}

/** Test hook: drop the cached registry (e.g. after rewriting mascots.json). */
export function _resetMascotCache() {
  cached = null;
}
