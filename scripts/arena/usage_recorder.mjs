// usage_recorder.mjs — continuous usage & resource recorder for the arena host.
//
// Runs as a long-lived user service (massive-game-usage-recorder.service).
// Every SAMPLE_INTERVAL_MS it writes one JSON line per sample:
//
//   artifacts/monitoring/usage-YYYY-MM-DD.jsonl   (append-only, UTC day files,
//                                                  files older than KEEP_DAYS
//                                                  pruned once per hour)
//   static_client/media/usage.json                (latest sample + last-24h
//                                                  series downsampled for the
//                                                  public System page)
//
// Per sample it collects, each collector failing soft to nulls:
//   - server process: CPU% (jiffies delta over wall delta, one core = 100%),
//     RSS, threads, open FD count — resolved fresh via pgrep every sample so
//     server restarts are picked up (and recorded as null gaps)
//   - host: load averages, memory total/available
//   - game: the Prometheus gauges/counters on METRICS_URL (players connected,
//     ws connections, exhibition runtimes, frames, network bytes)
//   - ngrok: tunnel connection/http counters for the space tunnel, probed on
//     the inspector ports (4040..4043); remote IPs are not exposed by the
//     inspector API so only counters are recorded
//
// Pure parsers are exported for tests; collectors are thin wrappers and the
// main loop never throws.

import { appendFileSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { execSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..');

export const METRICS_URL = process.env.MGS_METRICS_URL || 'http://127.0.0.1:9090/metrics';
export const MONITOR_DIR = process.env.MGS_MONITOR_DIR || join(REPO, 'artifacts', 'monitoring');
export const USAGE_JSON = process.env.MGS_USAGE_JSON || join(REPO, 'static_client', 'media', 'usage.json');
export const SAMPLE_INTERVAL_MS = Number(process.env.MGS_USAGE_INTERVAL_MS || 30_000);
export const KEEP_DAYS = Number(process.env.MGS_USAGE_KEEP_DAYS || 14);
export const SERIES_WINDOW_MS = 24 * 3600 * 1000;
export const SERIES_MAX_POINTS = 288; // 24h at 5-minute buckets
const NGROK_PORTS = [4040, 4041, 4042, 4043];
const NGROK_TUNNEL_SUFFIX = 'space.selfware.design';
const CLK_TCK = 100; // Linux USER_HZ on every platform we deploy to

// ── Pure parsers ──────────────────────────────────────────────────────────

/** /proc/<pid>/stat text -> { utime, stime } jiffies. Handles comm with spaces/parens. */
export function parseProcStat(text) {
  const s = String(text || '');
  const close = s.lastIndexOf(')');
  if (close < 0) return null;
  const fields = s.slice(close + 2).split(' ');
  // fields[0] is state (field 3); utime is field 14 -> index 11, stime field 15 -> index 12
  const utime = Number(fields[11]);
  const stime = Number(fields[12]);
  if (!Number.isFinite(utime) || !Number.isFinite(stime)) return null;
  return { utime, stime };
}

/** /proc/<pid>/status text -> { rssKb, threads }. */
export function parseProcStatus(text) {
  const out = { rssKb: null, threads: null };
  for (const line of String(text || '').split('\n')) {
    if (line.startsWith('VmRSS:')) {
      const n = Number(line.slice(6).trim().split(' ')[0]);
      if (Number.isFinite(n)) out.rssKb = n;
    } else if (line.startsWith('Threads:')) {
      const n = Number(line.slice(8).trim());
      if (Number.isFinite(n)) out.threads = n;
    }
  }
  return out;
}

/** /proc/loadavg text -> { load1, load5, load15 }. */
export function parseLoadavg(text) {
  const parts = String(text || '').trim().split(/\s+/);
  const [load1, load5, load15] = parts.map(Number);
  if (![load1, load5, load15].every(Number.isFinite)) return null;
  return { load1, load5, load15 };
}

/** /proc/meminfo text -> { memTotalKb, memAvailableKb }. */
export function parseMeminfo(text) {
  const out = { memTotalKb: null, memAvailableKb: null };
  for (const line of String(text || '').split('\n')) {
    if (line.startsWith('MemTotal:')) {
      const n = Number(line.slice(9).trim().split(' ')[0]);
      if (Number.isFinite(n)) out.memTotalKb = n;
    } else if (line.startsWith('MemAvailable:')) {
      const n = Number(line.slice(13).trim().split(' ')[0]);
      if (Number.isFinite(n)) out.memAvailableKb = n;
    }
  }
  return out;
}

/** Prometheus exposition -> the gauge/counter subset the recorder tracks. */
export function parsePromSample(text) {
  const t = String(text || '');
  const get = (name, labels = '') => {
    const re = new RegExp(`^${name}${labels} ([0-9.eE+-]+)$`, 'm');
    const m = re.exec(t);
    return m ? Number(m[1]) : null;
  };
  return {
    playersConnected: get('game_players_connected'),
    wsConnections: get('game_ws_connections_active'),
    webrtcPeersConnected: get('game_webrtc_peers_in_state', '\\{state="connected"\\}'),
    exhibitionRuntimes: get('game_arena_exhibition_runtimes_active'),
    matchDegraded: get('game_match_degraded'),
    framesTotal: get('game_frames_total'),
    ingressBytesTotal: get('game_network_bytes_total', '\\{direction="ingress_ws"\\}'),
    egressBytesTotal: get('game_network_bytes_total', '\\{direction="egress_ws"\\}'),
    dataChannelIngressTotal: get('game_network_bytes_total', '\\{direction="ingress_data_channel"\\}'),
    botDecisionsTotal: get('game_bot_decision_seconds_count'),
  };
}

/** ngrok /api/tunnels JSON -> { conns, httpRequests } for the space tunnel, or null. */
export function parseNgrokTunnels(jsonText, suffix = NGROK_TUNNEL_SUFFIX) {
  let doc;
  try {
    doc = JSON.parse(jsonText);
  } catch {
    return null;
  }
  const tunnels = Array.isArray(doc?.tunnels) ? doc.tunnels : [];
  const tunnel = tunnels.find((t) => typeof t?.public_url === 'string' && t.public_url.includes(suffix));
  if (!tunnel) return null;
  const conns = tunnel.metrics?.conns?.count;
  const http = tunnel.metrics?.http?.count;
  return {
    connsTotal: Number.isFinite(conns) ? conns : null,
    httpTotal: Number.isFinite(http) ? http : null,
  };
}

/** Bucket-average a series of { t, value } into at most maxPoints points. */
export function downsample(points, maxPoints = SERIES_MAX_POINTS) {
  const xs = (points || []).filter((p) => p && Number.isFinite(p.t) && Number.isFinite(p.value));
  if (xs.length <= maxPoints) return xs.map((p) => ({ t: p.t, value: p.value }));
  const span = xs[xs.length - 1].t - xs[0].t || 1;
  const buckets = new Map();
  for (const p of xs) {
    const b = Math.min(maxPoints - 1, Math.floor(((p.t - xs[0].t) / span) * maxPoints));
    const agg = buckets.get(b) || { sum: 0, n: 0, t: p.t };
    agg.sum += p.value;
    agg.n += 1;
    buckets.set(b, agg);
  }
  return [...buckets.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([, agg]) => ({ t: agg.t, value: agg.sum / agg.n }));
}

// ── Collectors (thin, never throw) ────────────────────────────────────────

const readText = (path) => {
  try {
    return readFileSync(path, 'utf8');
  } catch {
    return null;
  }
};

function serverPid() {
  try {
    const out = execSync('pgrep -f massive_game_server_core', {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
      timeout: 3000,
    }).trim();
    const pid = Number(out.split('\n')[0]);
    return Number.isFinite(pid) && pid > 0 ? pid : null;
  } catch {
    return null;
  }
}

function countFds(pid) {
  try {
    return readdirSync(`/proc/${pid}/fd`).length;
  } catch {
    return null;
  }
}

async function fetchText(url, timeoutMs) {
  const res = await fetch(url, { signal: AbortSignal.timeout(timeoutMs) });
  if (!res.ok) throw new Error(`http ${res.status}`);
  return res.text();
}

async function collectProm() {
  try {
    return parsePromSample(await fetchText(METRICS_URL, 3000));
  } catch {
    return null;
  }
}

async function collectNgrok() {
  for (const port of NGROK_PORTS) {
    try {
      const parsed = parseNgrokTunnels(await fetchText(`http://127.0.0.1:${port}/api/tunnels`, 1500));
      if (parsed) return parsed;
    } catch {
      // inspector not on this port — try the next one
    }
  }
  return null;
}

// ── Sample assembly ───────────────────────────────────────────────────────

let prevProc = null; // { pid, jiffies, atMs } for the CPU% delta

function collectProcess(nowMs) {
  const pid = serverPid();
  if (pid === null) {
    prevProc = null;
    return { pid: null, cpuPct: null, rssMb: null, threads: null, fds: null };
  }
  const stat = parseProcStat(readText(`/proc/${pid}/stat`) || '');
  const status = parseProcStatus(readText(`/proc/${pid}/status`) || '');
  let cpuPct = null;
  if (stat) {
    const jiffies = stat.utime + stat.stime;
    if (prevProc && prevProc.pid === pid) {
      const dj = jiffies - prevProc.jiffies;
      const dt = (nowMs - prevProc.atMs) / 1000;
      if (dj >= 0 && dt > 0) cpuPct = Math.round(((dj / CLK_TCK) / dt) * 1000) / 10;
    }
    prevProc = { pid, jiffies, atMs: nowMs };
  } else {
    prevProc = null;
  }
  return {
    pid,
    cpuPct,
    rssMb: status.rssKb !== null ? Math.round(status.rssKb / 1024) : null,
    threads: status.threads,
    fds: countFds(pid),
  };
}

function collectHost() {
  return {
    load: parseLoadavg(readText('/proc/loadavg') || ''),
    mem: parseMeminfo(readText('/proc/meminfo') || ''),
  };
}

// ── Persistence ───────────────────────────────────────────────────────────

const dayStamp = (ms) => new Date(ms).toISOString().slice(0, 10);
const dayFile = (ms) => join(MONITOR_DIR, `usage-${dayStamp(ms)}.jsonl`);

function appendSample(sample) {
  mkdirSync(MONITOR_DIR, { recursive: true });
  appendFileSync(dayFile(sample.t), JSON.stringify(sample) + '\n');
}

/** Read back the trailing window of samples across recent day files. */
export function readRecentSamples({ dir = MONITOR_DIR, windowMs = SERIES_WINDOW_MS, nowMs = Date.now() } = {}) {
  const out = [];
  for (let d = 0; d <= 2; d += 1) {
    const path = join(dir, `usage-${dayStamp(nowMs - d * 24 * 3600 * 1000)}.jsonl`);
    const text = readText(path);
    if (!text) continue;
    for (const line of text.split('\n')) {
      if (!line.trim()) continue;
      try {
        const s = JSON.parse(line);
        if (Number.isFinite(s?.t) && s.t >= nowMs - windowMs) out.push(s);
      } catch {
        // partial trailing line from a crash mid-append — skip
      }
    }
  }
  return out.sort((a, b) => a.t - b.t);
}

let lastPruneHour = -1;
function pruneOldFiles(nowMs) {
  const hour = Math.floor(nowMs / 3600_000);
  if (hour === lastPruneHour) return;
  lastPruneHour = hour;
  const cutoff = dayStamp(nowMs - KEEP_DAYS * 24 * 3600 * 1000);
  try {
    for (const name of readdirSync(MONITOR_DIR)) {
      const m = /^usage-(\d{4}-\d{2}-\d{2})\.jsonl$/.exec(name);
      if (m && m[1] < cutoff) rmSync(join(MONITOR_DIR, name), { force: true });
    }
  } catch {
    // keep recording; pruning retries next hour
  }
}

/** Shape of the public usage.json consumed by the System page. */
export function buildUsageDoc({ sample, history, nowMs }) {
  const seriesOf = (pick) => downsample(
    history.map((s) => ({ t: s.t, value: pick(s) })).filter((p) => p.value !== null && p.value !== undefined),
    SERIES_MAX_POINTS,
  );
  return {
    generatedAt: new Date(nowMs).toISOString(),
    intervalSec: Math.round(SAMPLE_INTERVAL_MS / 1000),
    latest: sample,
    series: {
      cpuPct: seriesOf((s) => s.process?.cpuPct),
      rssMb: seriesOf((s) => s.process?.rssMb),
      players: seriesOf((s) => s.game?.playersConnected),
      wsConnections: seriesOf((s) => s.game?.wsConnections),
      load1: seriesOf((s) => s.host?.load?.load1),
      ngrokConns: seriesOf((s) => s.ngrok?.connsTotal),
    },
  };
}

function writeUsageJson(sample, nowMs) {
  const history = readRecentSamples({ nowMs });
  const doc = buildUsageDoc({ sample, history, nowMs });
  mkdirSync(dirname(USAGE_JSON), { recursive: true });
  // Same-directory write + rename so the site never serves a partial file.
  const tmp = `${USAGE_JSON}.tmp`;
  writeFileSync(tmp, JSON.stringify(doc));
  renameSync(tmp, USAGE_JSON);
}

// ── Main loop ─────────────────────────────────────────────────────────────

async function sampleOnce() {
  const nowMs = Date.now();
  const [game, ngrok] = await Promise.all([collectProm(), collectNgrok()]);
  const sample = {
    t: nowMs,
    process: collectProcess(nowMs),
    host: collectHost(),
    game,
    ngrok,
  };
  try {
    appendSample(sample);
    writeUsageJson(sample, nowMs);
    pruneOldFiles(nowMs);
  } catch (error) {
    console.error('[usage_recorder] persist failed:', error?.message || error);
  }
  return sample;
}

export async function main() {
  console.log(`[usage_recorder] interval=${SAMPLE_INTERVAL_MS}ms dir=${MONITOR_DIR} out=${USAGE_JSON}`);
  // eslint-disable-next-line no-constant-condition
  while (true) {
    const started = Date.now();
    await sampleOnce();
    const elapsed = Date.now() - started;
    await new Promise((r) => setTimeout(r, Math.max(250, SAMPLE_INTERVAL_MS - elapsed)));
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error('[usage_recorder] fatal:', error);
    process.exit(1);
  });
}
