import http from 'node:http';
import https from 'node:https';

export const DEFAULT_MAX_RESPONSE_BYTES = 8 * 1024 * 1024;

const TRANSPORT_ERROR_CODES = new Map([
  ['ECONNREFUSED', 'arena_api_connection_refused'],
  ['ECONNRESET', 'arena_api_connection_reset'],
  ['EPIPE', 'arena_api_connection_reset'],
  ['ENOTFOUND', 'arena_api_dns_failure'],
  ['EAI_AGAIN', 'arena_api_dns_failure'],
  ['ETIMEDOUT', 'arena_api_timeout'],
  ['ERR_TLS_CERT_ALTNAME_INVALID', 'arena_api_tls_failure'],
  ['CERT_HAS_EXPIRED', 'arena_api_tls_failure'],
  ['DEPTH_ZERO_SELF_SIGNED_CERT', 'arena_api_tls_failure'],
  ['SELF_SIGNED_CERT_IN_CHAIN', 'arena_api_tls_failure'],
  ['UNABLE_TO_VERIFY_LEAF_SIGNATURE', 'arena_api_tls_failure'],
]);

export class ArenaApiTransportError extends Error {
  constructor(code, method, route) {
    super(`${method} ${route} transport failed: ${code}`);
    this.name = 'ArenaApiTransportError';
    this.code = code;
  }
}

function safeTransportError(error, method, route, fallbackCode = 'arena_api_transport_error') {
  const code = TRANSPORT_ERROR_CODES.get(String(error?.code || '')) || fallbackCode;
  return new ArenaApiTransportError(code, method, route);
}

function requestTarget(apiBase, route) {
  if (typeof route !== 'string' || !/^\/(?!\/)/.test(route)) {
    throw new Error('arena API route must be an absolute path');
  }
  let base;
  try {
    base = new URL(apiBase);
  } catch {
    throw new Error('arena API base URL is invalid');
  }
  if (base.protocol !== 'http:' && base.protocol !== 'https:') {
    throw new Error('arena API base URL must use HTTP or HTTPS');
  }
  if (base.username || base.password) {
    throw new Error('arena API base URL must not contain credentials');
  }
  const target = new URL(`${String(apiBase).replace(/\/$/, '')}${route}`);
  if (target.origin !== base.origin) {
    throw new Error('arena API route must remain on the configured origin');
  }
  return target;
}

function positiveInteger(value, label) {
  if (!Number.isSafeInteger(value) || value < 1) throw new Error(`${label} must be a positive integer`);
  return value;
}

/**
 * Call one arena JSON endpoint through Node's core HTTP stack.
 *
 * Transport errors deliberately expose only stable codes. Request headers,
 * credentials, bodies, response fragments, and low-level error messages are
 * never copied into an error or written to a log by this client.
 */
export async function arenaApiJson({
  apiBase,
  adminToken,
  method = 'GET',
  route,
  body,
  timeoutMs = 120_000,
  maxResponseBytes = DEFAULT_MAX_RESPONSE_BYTES,
}) {
  positiveInteger(timeoutMs, 'arena API timeout');
  positiveInteger(maxResponseBytes, 'arena API response limit');
  const normalizedMethod = String(method).toUpperCase();
  if (!/^[A-Z]+$/.test(normalizedMethod)) throw new Error('arena API method is invalid');
  const target = requestTarget(apiBase, route);
  const serializedBody = body === undefined ? null : JSON.stringify(body);
  const headers = { Accept: 'application/json' };
  if (adminToken) headers.Authorization = `Bearer ${adminToken}`;
  if (serializedBody !== null) {
    headers['Content-Type'] = 'application/json';
    headers['Content-Length'] = Buffer.byteLength(serializedBody);
  }
  const transport = target.protocol === 'https:' ? https : http;

  return new Promise((resolve, reject) => {
    let request;
    let response;
    let settled = false;
    let timer;

    const cleanUp = () => {
      if (timer) clearTimeout(timer);
      timer = undefined;
    };
    const succeed = (value) => {
      if (settled) return;
      settled = true;
      cleanUp();
      resolve(value);
    };
    const fail = (error) => {
      if (settled) return;
      settled = true;
      cleanUp();
      if (response && !response.destroyed) response.destroy();
      if (request && !request.destroyed) request.destroy();
      reject(error);
    };

    timer = setTimeout(() => {
      fail(new ArenaApiTransportError('arena_api_timeout', normalizedMethod, route));
    }, timeoutMs);

    try {
      request = transport.request({
        protocol: target.protocol,
        hostname: target.hostname,
        port: target.port || undefined,
        method: normalizedMethod,
        path: `${target.pathname}${target.search}`,
        headers,
      }, (incoming) => {
        response = incoming;
        const declaredLength = Number.parseInt(incoming.headers['content-length'] || '', 10);
        if (Number.isFinite(declaredLength) && declaredLength > maxResponseBytes) {
          fail(new ArenaApiTransportError(
            'arena_api_response_too_large',
            normalizedMethod,
            route,
          ));
          return;
        }

        const chunks = [];
        let receivedBytes = 0;
        incoming.on('data', (chunk) => {
          if (settled) return;
          const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
          receivedBytes += bytes.length;
          if (receivedBytes > maxResponseBytes) {
            fail(new ArenaApiTransportError(
              'arena_api_response_too_large',
              normalizedMethod,
              route,
            ));
            return;
          }
          chunks.push(bytes);
        });
        incoming.once('aborted', () => {
          fail(new ArenaApiTransportError(
            'arena_api_response_aborted',
            normalizedMethod,
            route,
          ));
        });
        incoming.once('error', (error) => {
          fail(safeTransportError(
            error,
            normalizedMethod,
            route,
            'arena_api_response_error',
          ));
        });
        incoming.once('end', () => {
          if (settled) return;
          const status = Number(incoming.statusCode) || 0;
          const raw = Buffer.concat(chunks, receivedBytes).toString('utf8');
          let payload;
          try {
            payload = raw ? JSON.parse(raw) : null;
          } catch {
            fail(new Error(`${normalizedMethod} ${route} returned non-JSON HTTP ${status}`));
            return;
          }
          if (status < 200 || status >= 300) {
            fail(new Error(`${normalizedMethod} ${route} failed with HTTP ${status}`));
            return;
          }
          if (payload?.ok !== true) {
            const code = payload?.error?.code || 'api_error';
            const message = payload?.error?.message || 'unknown arena API error';
            fail(new Error(`${normalizedMethod} ${route} failed: ${code}: ${message}`));
            return;
          }
          succeed(payload.data);
        });
      });
      request.once('error', (error) => {
        fail(safeTransportError(error, normalizedMethod, route));
      });
      request.end(serializedBody ?? undefined);
    } catch (error) {
      fail(safeTransportError(error, normalizedMethod, route));
    }
  });
}
