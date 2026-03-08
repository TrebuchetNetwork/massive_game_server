const { spawn } = require('child_process');
const path = require('path');
const http = require('http');
const https = require('https');

let serverProcess;
let expectedServerExit = false;

function resolveBaseUrl() {
  return process.env.E2E_BASE_URL || 'http://127.0.0.1:19080';
}

function resolveWsUrl(baseUrlOverride) {
  if (!baseUrlOverride && process.env.E2E_WS_URL) {
    return process.env.E2E_WS_URL;
  }
  const base = new URL(baseUrlOverride || resolveBaseUrl());
  const wsProtocol = base.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${wsProtocol}//${base.host}/ws`;
}

function waitForHttpReady(url) {
  const timeoutMs = Number.parseInt(process.env.E2E_SERVER_START_TIMEOUT_MS || '180000', 10) || 180000;
  const start = Date.now();
  const client = url.startsWith('https') ? https : http;

  return new Promise((resolve, reject) => {
    const attempt = () => {
      const req = client.get(url, (res) => {
        if (res.statusCode === 200) {
          res.resume();
          resolve();
          return;
        }
        res.resume();
        retry();
      });
      req.on('error', retry);
    };

    const retry = () => {
      if (Date.now() - start > timeoutMs) {
        reject(new Error(`Timed out waiting for ${url}`));
        return;
      }
      setTimeout(attempt, 500);
    };

    attempt();
  });
}

async function startServer(options = {}) {
  if (process.env.E2E_SERVER_SKIP === '1') return;
  if (serverProcess) return;
  expectedServerExit = false;

  const { env: envOverrides = {}, baseUrl: overrideBaseUrl } = options;

  const cwd = path.resolve(__dirname, '..', '..', '..', '..');
  const cmd = process.env.E2E_SERVER_CMD || 'cargo';
  const args = process.env.E2E_SERVER_CMD
    ? process.env.E2E_SERVER_CMD.split(' ').slice(1)
    : ['run', '-p', 'massive_game_server_core', '--bin', 'massive_game_server_core'];

  const baseUrl = overrideBaseUrl || resolveBaseUrl();
  const base = new URL(baseUrl);
  const childEnv = { ...process.env };
  for (const key of Object.keys(childEnv)) {
    if (key.startsWith('MGS_') || key.startsWith('E2E_')) {
      delete childEnv[key];
    }
  }
  Object.assign(childEnv, envOverrides);
  if (!childEnv.MGS_PORT) {
    childEnv.MGS_PORT = base.port || (base.protocol === 'https:' ? '443' : '80');
  }
  if (!childEnv.MGS_HOST) {
    childEnv.MGS_HOST = '0.0.0.0';
  }
  if (!childEnv.MGS_TARGET_BOT_COUNT) {
    childEnv.MGS_TARGET_BOT_COUNT = '0';
  }

  serverProcess = spawn(cmd, args, {
    cwd,
    env: childEnv,
    stdio: ['ignore', 'pipe', 'pipe']
  });

  serverProcess.stdout.on('data', (data) => process.stdout.write(data.toString()));
  serverProcess.stderr.on('data', (data) => process.stderr.write(data.toString()));

  let ready = false;
  const processRef = serverProcess;
  const exitPromise = new Promise((resolve, reject) => {
    processRef.once('exit', (code) => {
      if (serverProcess === processRef) {
        serverProcess = undefined;
      }
      if (ready || expectedServerExit) {
        resolve();
        return;
      }
      reject(new Error(`Server exited early with code ${code}`));
    });
  });

  await Promise.race([waitForHttpReady(`${baseUrl}/healthz`), exitPromise]);
  await Promise.race([waitForHttpReady(`${baseUrl}/readyz`), exitPromise]);
  await Promise.race([waitForHttpReady(`${baseUrl}/client.html`), exitPromise]);
  ready = true;
}

async function stopServer() {
  if (!serverProcess) return;
  const processToStop = serverProcess;
  serverProcess = undefined;
  expectedServerExit = true;

  const exited = new Promise((resolve) => {
    processToStop.once('exit', () => resolve());
  });

  processToStop.kill('SIGINT');
  await Promise.race([exited, new Promise((resolve) => setTimeout(resolve, 3000))]);

  if (processToStop.exitCode === null) {
    processToStop.kill('SIGKILL');
    await exited;
  }

  expectedServerExit = false;
}

function registerServerLifecycle(test, options = {}) {
  test.beforeAll(async () => {
    await startServer(options);
  });

  test.afterAll(async () => {
    await stopServer();
  });
}

module.exports = {
  registerServerLifecycle,
  resolveBaseUrl,
  resolveWsUrl,
  startServer,
  stopServer,
};
