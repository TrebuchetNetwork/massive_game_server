import assert from 'node:assert/strict';
import http from 'node:http';
import test from 'node:test';
import {
  ArenaApiTransportError,
  arenaApiJson,
} from './arena_api_client.mjs';

async function withServer(handler, callback) {
  const server = http.createServer(handler);
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address();
  const apiBase = `http://127.0.0.1:${address.port}`;
  try {
    return await callback(apiBase, server);
  } finally {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

function jsonResponse(response, status, value) {
  const encoded = JSON.stringify(value);
  response.writeHead(status, {
    'Content-Type': 'application/json',
    'Content-Length': Buffer.byteLength(encoded),
  });
  response.end(encoded);
}

test('returns data after response headers are delayed within the total timeout', async () => {
  await withServer((_request, response) => {
    setTimeout(() => jsonResponse(response, 200, { ok: true, data: { ready: true } }), 75);
  }, async (apiBase) => {
    const result = await arenaApiJson({
      apiBase,
      route: '/delayed',
      timeoutMs: 500,
    });
    assert.deepEqual(result, { ready: true });
  });
});

test('enforces a total timeout and does not expose authorization or request body', async () => {
  const adminToken = 'secret-admin-token-for-test';
  const bodySecret = 'secret-request-body-for-test';
  await withServer((_request, response) => {
    setTimeout(() => jsonResponse(response, 200, { ok: true, data: null }), 200);
  }, async (apiBase) => {
    await assert.rejects(
      arenaApiJson({
        apiBase,
        adminToken,
        method: 'POST',
        route: '/slow',
        body: { value: bodySecret },
        timeoutMs: 30,
      }),
      (error) => {
        assert.ok(error instanceof ArenaApiTransportError);
        assert.equal(error.code, 'arena_api_timeout');
        assert.equal(error.message, 'POST /slow transport failed: arena_api_timeout');
        assert.ok(!error.message.includes(adminToken));
        assert.ok(!error.message.includes(bodySecret));
        return true;
      },
    );
  });
});

test('bounds streamed response bytes without including response fragments', async () => {
  const responseSecret = 'secret-response-fragment-for-test';
  await withServer((_request, response) => {
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.write(`{"ok":true,"data":"${responseSecret}`);
    response.end('xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"}');
  }, async (apiBase) => {
    await assert.rejects(
      arenaApiJson({
        apiBase,
        route: '/oversized',
        maxResponseBytes: 32,
      }),
      (error) => {
        assert.ok(error instanceof ArenaApiTransportError);
        assert.equal(error.code, 'arena_api_response_too_large');
        assert.ok(!error.message.includes(responseSecret));
        return true;
      },
    );
  });
});

test('preserves JSON envelope and HTTP error semantics', async () => {
  await withServer((request, response) => {
    if (request.url === '/api-error') {
      jsonResponse(response, 200, {
        ok: false,
        error: { code: 'fighter_invalid', message: 'fighter was rejected' },
      });
    } else if (request.url === '/http-error') {
      jsonResponse(response, 503, { ok: false });
    } else {
      response.writeHead(502, { 'Content-Type': 'text/plain' });
      response.end('upstream unavailable');
    }
  }, async (apiBase) => {
    await assert.rejects(
      arenaApiJson({ apiBase, method: 'POST', route: '/api-error' }),
      { message: 'POST /api-error failed: fighter_invalid: fighter was rejected' },
    );
    await assert.rejects(
      arenaApiJson({ apiBase, route: '/http-error' }),
      { message: 'GET /http-error failed with HTTP 503' },
    );
    await assert.rejects(
      arenaApiJson({ apiBase, route: '/non-json' }),
      { message: 'GET /non-json returned non-JSON HTTP 502' },
    );
  });
});

test('maps a refused connection to a stable safe transport code', async () => {
  await withServer((_request, response) => response.end(), async (apiBase, server) => {
    await new Promise((resolve) => server.close(resolve));
    await assert.rejects(
      arenaApiJson({ apiBase, route: '/offline', timeoutMs: 500 }),
      (error) => {
        assert.ok(error instanceof ArenaApiTransportError);
        assert.equal(error.code, 'arena_api_connection_refused');
        assert.equal(
          error.message,
          'GET /offline transport failed: arena_api_connection_refused',
        );
        return true;
      },
    );
  });
});
