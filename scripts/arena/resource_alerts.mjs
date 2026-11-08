// Evidence-based resource-retention alerts. No single-sample guessing: a warning
// requires a sustained condition or a full growth window on the same server PID.

export const UDP_RETENTION_MIN_SOCKETS = 100;
export const UDP_RETENTION_MIN_DURATION_MS = 15 * 60 * 1000;
export const FD_GROWTH_MIN_DELTA = 50;
export const FD_GROWTH_WINDOW_MS = 30 * 60 * 1000;

const peersOf = (sample) => Number(sample.game?.webrtcPeersConnected ?? 0);
const isIdle = (sample) => Number.isFinite(peersOf(sample)) && peersOf(sample) <= 1;

export function evaluateResourceAlert(history, { nowMs = Date.now() } = {}) {
  const samples = (Array.isArray(history) ? history : [])
    .filter((sample) => Number.isFinite(sample?.t) && sample.t <= nowMs)
    .sort((a, b) => a.t - b.t);
  const latest = samples.at(-1);
  const pid = latest?.process?.pid;
  if (!Number.isFinite(pid) || pid <= 0) return null;

  const recent = samples.filter((sample) => sample.process?.pid === pid
    && sample.t >= nowMs - FD_GROWTH_WINDOW_MS);
  if (recent.length < 2) return null;

  const first = recent[0];
  const durationMs = latest.t - first.t;
  const fdDelta = latest.process.fds - first.process.fds;
  const sustainedUdp = durationMs >= UDP_RETENTION_MIN_DURATION_MS
    && recent.every((sample) => sample.process.udpSockets >= UDP_RETENTION_MIN_SOCKETS && isIdle(sample));
  const fdGrowth = durationMs >= FD_GROWTH_WINDOW_MS
    && fdDelta >= FD_GROWTH_MIN_DELTA
    && recent.every(isIdle);
  if (!sustainedUdp && !fdGrowth) return null;

  return {
    code: sustainedUdp ? 'udp_retention' : 'fd_growth',
    severity: 'warning',
    since: new Date(first.t).toISOString(),
    message: sustainedUdp
      ? `UDP ${latest.process.udpSockets} for ${Math.round(durationMs / 60000)}m with ${peersOf(latest)} peer(s) connected`
      : `+${fdDelta} FDs in ${Math.round(durationMs / 60000)}m with ${peersOf(latest)} peer(s) connected`,
    metrics: {
      pid,
      udpSockets: latest.process.udpSockets,
      udpOutOfRange: latest.process.udpOutOfRange,
      fds: latest.process.fds,
      fdDelta,
      rssMb: latest.process.rssMb,
      peers: peersOf(latest),
      durationMs,
    },
  };
}
