import test from 'node:test';
import assert from 'node:assert/strict';

import { evaluateResourceAlert } from './resource_alerts.mjs';
import { buildUsageDoc } from './usage_recorder.mjs';

const MIN = 60_000;
const NOW = Date.parse('2026-11-08T12:00:00Z');

function samples({ minutes, udp = 10, fds = 20, peers = 0, pid = 42 }) {
  return Array.from({ length: minutes + 1 }, (_, index) => ({
    t: NOW - (minutes - index) * MIN,
    process: { pid, udpSockets: udp, udpOutOfRange: 0, fds, rssMb: 512 },
    game: { webrtcPeersConnected: peers },
  }));
}

test('healthy and short-lived UDP samples do not alert', () => {
  assert.equal(evaluateResourceAlert(samples({ minutes: 31, udp: 20 }), { nowMs: NOW }), null);
  assert.equal(evaluateResourceAlert(samples({ minutes: 10, udp: 150 }), { nowMs: NOW }), null);
});

test('sustained UDP retention emits a resource alert', () => {
  const history = samples({ minutes: 31, udp: 120, fds: 180 });
  const alert = evaluateResourceAlert(history, { nowMs: NOW });
  assert.equal(alert.code, 'udp_retention');
  assert.equal(alert.severity, 'warning');
  assert.equal(alert.since, '2026-11-08T11:30:00.000Z');
  assert.equal(alert.metrics.udpSockets, 120);
  assert.equal(alert.metrics.fdDelta, 0);
});

test('idle FD growth over thirty minutes emits an alert', () => {
  const history = samples({ minutes: 31, udp: 20, fds: 20 });
  history.forEach((sample, index) => { sample.process.fds = 20 + index * 2; });
  const alert = evaluateResourceAlert(history, { nowMs: NOW });
  assert.equal(alert.code, 'fd_growth');
  assert.equal(alert.metrics.fdDelta, 60);
});

test('active peers suppress leak alerts and a new PID starts a fresh baseline', () => {
  assert.equal(evaluateResourceAlert(samples({ minutes: 31, udp: 150, peers: 2 }), { nowMs: NOW }), null);
  const history = samples({ minutes: 31, udp: 150 });
  history.push(...samples({ minutes: 5, udp: 5, pid: 99 }));
  assert.equal(evaluateResourceAlert(history, { nowMs: NOW }), null);
});

test('usage document includes only evidence-backed alerts', () => {
  const history = samples({ minutes: 31, udp: 120, fds: 180 });
  const doc = buildUsageDoc({ sample: history.at(-1), history, nowMs: NOW });
  assert.equal(doc.alerts.length, 1);
  assert.equal(doc.alerts[0].code, 'udp_retention');
  assert.equal(doc.alerts[0].metrics.udpSockets, 120);
});
