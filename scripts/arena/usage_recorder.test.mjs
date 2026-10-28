// usage_recorder.test.mjs — pure-parser and series-shaping tests for the
// usage recorder. Collectors (proc/prom/ngrok/systemd) are not touched here;
// they are thin and exercised live by the service itself.

import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  buildUsageDoc,
  downsample,
  parseLoadavg,
  parseMeminfo,
  parseNgrokTunnels,
  parseProcStat,
  parseProcStatus,
  parsePromSample,
  readRecentSamples,
} from './usage_recorder.mjs';

test('parseProcStat reads utime/stime past a comm with spaces and parens', () => {
  // field order after comm: state ppid pgrp session tty_nr tpgid flags
  // minflt cminflt majflt cmajflt utime stime ...
  const text = '1234 (massive game (core)) S 1 1234 1234 0 -1 4194304 100 0 0 0 460 120 0 0 20 0 30 0';
  assert.deepEqual(parseProcStat(text), { utime: 460, stime: 120 });
});

test('parseProcStat returns null on garbage', () => {
  assert.equal(parseProcStat(''), null);
  assert.equal(parseProcStat('no parens here'), null);
  assert.equal(parseProcStat('1 (x) S a b c d e f g h i j k l m'), null);
});

test('parseProcStatus extracts VmRSS and Threads', () => {
  const text = 'Name:\tmassive_game_se\nVmRSS:\t  512340 kB\nThreads:\t42\n';
  assert.deepEqual(parseProcStatus(text), { rssKb: 512340, threads: 42 });
});

test('parseLoadavg parses three loads', () => {
  assert.deepEqual(parseLoadavg('1.25 0.75 0.50 3/456 7890\n'), { load1: 1.25, load5: 0.75, load15: 0.5 });
  assert.equal(parseLoadavg('junk'), null);
});

test('parseMeminfo extracts totals', () => {
  const text = 'MemTotal:       1056300736 kB\nMemFree:        900000000 kB\nMemAvailable:   990000000 kB\n';
  assert.deepEqual(parseMeminfo(text), { memTotalKb: 1056300736, memAvailableKb: 990000000 });
});

test('parsePromSample picks the tracked gauges and counters', () => {
  const text = [
    '# TYPE game_players_connected gauge',
    'game_players_connected 7',
    'game_ws_connections_active 3',
    'game_webrtc_peers_in_state{state="connected"} 5',
    'game_webrtc_peers_in_state{state="closed"} 9',
    'game_arena_exhibition_runtimes_active 24',
    'game_match_degraded 0',
    'game_frames_total 154256',
    'game_network_bytes_total{direction="ingress_ws"} 2821',
    'game_network_bytes_total{direction="egress_ws"} 13816',
    'game_network_bytes_total{direction="ingress_data_channel"} 42968',
    'game_bot_decision_seconds_count 27498',
  ].join('\n');
  assert.deepEqual(parsePromSample(text), {
    playersConnected: 7,
    wsConnections: 3,
    webrtcPeersConnected: 5,
    exhibitionRuntimes: 24,
    matchDegraded: 0,
    framesTotal: 154256,
    ingressBytesTotal: 2821,
    egressBytesTotal: 13816,
    dataChannelIngressTotal: 42968,
    botDecisionsTotal: 27498,
  });
});

test('parsePromSample degrades to nulls when metrics are absent', () => {
  const parsed = parsePromSample('# empty');
  assert.equal(parsed.playersConnected, null);
  assert.equal(parsed.framesTotal, null);
});

test('parseNgrokTunnels finds the space tunnel counters', () => {
  const doc = JSON.stringify({
    tunnels: [
      { public_url: 'https://selfware.design', metrics: { conns: { count: 1 }, http: { count: 2 } } },
      { public_url: 'https://space.selfware.design', metrics: { conns: { count: 40 }, http: { count: 900 } } },
    ],
  });
  assert.deepEqual(parseNgrokTunnels(doc), { connsTotal: 40, httpTotal: 900 });
});

test('parseNgrokTunnels returns null without the tunnel or on bad json', () => {
  assert.equal(parseNgrokTunnels('{"tunnels":[]}'), null);
  assert.equal(parseNgrokTunnels('not json'), null);
});

test('downsample keeps short series intact and bucket-averages long ones', () => {
  const short = [{ t: 1, value: 5 }, { t: 2, value: 7 }];
  assert.deepEqual(downsample(short, 10), short);

  const long = [];
  for (let i = 0; i < 1000; i += 1) long.push({ t: i * 1000, value: i });
  const out = downsample(long, 10);
  assert.equal(out.length, 10);
  for (const p of out) assert.ok(Number.isFinite(p.value));
  // monotonic mean for a ramp input
  for (let i = 1; i < out.length; i += 1) assert.ok(out[i].value > out[i - 1].value);
});

test('downsample drops points without finite t/value', () => {
  const out = downsample([{ t: 1, value: null }, { t: 2, value: 3 }, { value: 9 }], 10);
  assert.deepEqual(out, [{ t: 2, value: 3 }]);
});

test('readRecentSamples reads back the trailing window across day files', () => {
  const dir = mkdtempSync(join(tmpdir(), 'usage-test-'));
  try {
    const now = Date.parse('2026-10-28T12:00:00Z');
    const day = (ms) => new Date(ms).toISOString().slice(0, 10);
    const mk = (t) => `${JSON.stringify({ t, process: { cpuPct: 1 } })}\n`;
    writeFileSync(join(dir, `usage-${day(now)}.jsonl`), mk(now - 3600_000) + mk(now));
    writeFileSync(join(dir, `usage-${day(now - 24 * 3600_000)}.jsonl`), mk(now - 23 * 3600_000) + mk(now - 26 * 3600_000));
    const samples = readRecentSamples({ dir, nowMs: now, windowMs: 24 * 3600_000 });
    assert.deepEqual(samples.map((s) => s.t), [now - 23 * 3600_000, now - 3600_000, now]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('buildUsageDoc shapes latest + downsampled series', () => {
  const now = Date.parse('2026-10-28T12:00:00Z');
  const history = [];
  for (let i = 0; i < 100; i += 1) {
    history.push({
      t: now - (100 - i) * 60_000,
      process: { cpuPct: i, rssMb: 500 },
      host: { load: { load1: 0.5 } },
      game: { playersConnected: i % 5, wsConnections: i % 3 },
      ngrok: { connsTotal: 100 + i },
    });
  }
  const doc = buildUsageDoc({ sample: history[history.length - 1], history, nowMs: now });
  assert.equal(doc.generatedAt, '2026-10-28T12:00:00.000Z');
  assert.equal(doc.latest.process.cpuPct, 99);
  assert.ok(doc.series.cpuPct.length <= 288);
  assert.equal(doc.series.rssMb.at(-1).value, 500);
  assert.ok(doc.series.ngrokConns.length > 0);
});
