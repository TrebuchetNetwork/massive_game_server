function withAdminHeaders(token, extraHeaders = {}) {
  const headers = { ...extraHeaders };
  if (token) headers.Authorization = `Bearer ${token}`;
  Object.keys(headers).forEach((key) => {
    if (headers[key] === undefined) delete headers[key];
  });
  return headers;
}

async function requestJson(baseUrl, path, { token, method = 'GET', body, headers = {} } = {}) {
  const response = await fetch(new URL(path, baseUrl), {
    method,
    headers: withAdminHeaders(token, {
      'Content-Type': body ? 'application/json' : undefined,
      ...headers,
    }),
    body: body ? JSON.stringify(body) : undefined,
  });
  const payload = await response.json().catch(() => null);
  return { response, payload };
}

function createAdminApi(baseUrl, token) {
  return {
    getMatchType: () => requestJson(baseUrl, '/api/ops/match-type', { token }),
    getJoinStages: () => requestJson(baseUrl, '/api/ops/join-stages', { token }),
    resetJoinStages: () =>
      requestJson(baseUrl, '/api/ops/join-stages/reset', { token, method: 'POST' }),
    getMatchSummaryLatest: () =>
      requestJson(baseUrl, '/api/ops/match-summary/latest', { token }),
    getKillcamLatest: () =>
      requestJson(baseUrl, '/api/ops/killcam/latest', { token }),
    getLiveReplayRecent: () =>
      requestJson(baseUrl, '/api/ops/live-replay/recent', { token }),
    getFeatureFlags: () => requestJson(baseUrl, '/api/ops/feature-flags', { token }),
    evaluateFeatureFlag: (body) =>
      requestJson(baseUrl, '/api/ops/feature-flags/evaluate', {
        token,
        method: 'POST',
        body,
      }),
  };
}

module.exports = {
  createAdminApi,
  requestJson,
  withAdminHeaders,
};
