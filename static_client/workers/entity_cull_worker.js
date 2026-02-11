"use strict";

let wasmKernel = null;
let wasmKernelLabel = "js";

function asNumber(value, fallback = 0) {
  const n = Number(value);
  return Number.isFinite(n) ? n : fallback;
}

function inView(x, y, bounds, margin) {
  return (
    x >= bounds.left - margin &&
    x <= bounds.right + margin &&
    y >= bounds.top - margin &&
    y <= bounds.bottom + margin
  );
}

function getDistanceSqCandidate(x, y, bounds, margin, anchorX, anchorY) {
  if (wasmKernel && typeof wasmKernel.cullDistanceSq === "function") {
    try {
      const value = Number(
        wasmKernel.cullDistanceSq(
          x,
          y,
          bounds.left,
          bounds.right,
          bounds.top,
          bounds.bottom,
          margin,
          anchorX,
          anchorY
        )
      );
      if (Number.isFinite(value)) {
        return value;
      }
    } catch (_) {
      // Fallback to JS path when WASM invocation fails.
    }
  }

  if (!inView(x, y, bounds, margin)) {
    return -1;
  }
  const dx = x - anchorX;
  const dy = y - anchorY;
  return dx * dx + dy * dy;
}

function computeCull(payload) {
  const startedAtMs = performance.now();
  const config = payload?.config || {};
  const bounds = payload?.viewBounds || {};
  const players = Array.isArray(payload?.players) ? payload.players : [];
  const projectiles = Array.isArray(payload?.projectiles) ? payload.projectiles : [];

  const playerCullMargin = asNumber(config.playerCullMargin, 240);
  const projectileCullMargin = asNumber(config.projectileCullMargin, 220);
  const priorityDistanceSq = asNumber(config.playerPriorityDistanceSq, 900 * 900);
  const remoteRenderCap = Math.max(0, Math.floor(asNumber(config.remoteRenderCap, 150)));
  const remotePriorityOverflowCap = Math.max(0, Math.floor(asNumber(config.remotePriorityOverflowCap, 12)));
  const projectileRenderCap = Math.max(0, Math.floor(asNumber(config.projectileRenderCap, 900)));
  const localAnchorX = asNumber(config.localAnchorX, 0);
  const localAnchorY = asNumber(config.localAnchorY, 0);

  const localPlayerIds = [];
  const priorityCandidates = [];
  const remoteCandidates = [];

  for (let i = 0; i < players.length; i += 1) {
    const row = players[i];
    if (!Array.isArray(row) || row.length < 4) continue;
    const id = row[0];
    const x = asNumber(row[1], 0);
    const y = asNumber(row[2], 0);
    const isLocal = row[3] === 1;
    if (isLocal) {
      localPlayerIds.push(id);
      continue;
    }
    const distSq = getDistanceSqCandidate(
      x,
      y,
      bounds,
      playerCullMargin,
      localAnchorX,
      localAnchorY
    );
    if (distSq < 0) {
      continue;
    }
    if (distSq <= priorityDistanceSq) {
      priorityCandidates.push([id, distSq]);
    } else {
      remoteCandidates.push([id, distSq]);
    }
  }

  priorityCandidates.sort((a, b) => a[1] - b[1]);
  remoteCandidates.sort((a, b) => a[1] - b[1]);

  const selectedRemoteIds = [];
  const priorityCap = remoteRenderCap + remotePriorityOverflowCap;
  const priorityLimit = Math.min(priorityCap, priorityCandidates.length);
  for (let i = 0; i < priorityLimit; i += 1) {
    selectedRemoteIds.push(priorityCandidates[i][0]);
  }
  const remainingNormalCap = Math.max(0, remoteRenderCap - Math.min(remoteRenderCap, priorityLimit));
  const normalLimit = Math.min(remainingNormalCap, remoteCandidates.length);
  for (let i = 0; i < normalLimit; i += 1) {
    selectedRemoteIds.push(remoteCandidates[i][0]);
  }

  const selectedProjectileIds = [];
  if (projectileRenderCap > 0) {
    const projectileCandidates = [];
    for (let i = 0; i < projectiles.length; i += 1) {
      const row = projectiles[i];
      if (!Array.isArray(row) || row.length < 3) continue;
      const id = row[0];
      const x = asNumber(row[1], 0);
      const y = asNumber(row[2], 0);
      const distSq = getDistanceSqCandidate(
        x,
        y,
        bounds,
        projectileCullMargin,
        localAnchorX,
        localAnchorY
      );
      if (distSq < 0) {
        continue;
      }
      projectileCandidates.push([id, distSq]);
    }

    projectileCandidates.sort((a, b) => a[1] - b[1]);
    const projectileLimit = Math.min(projectileRenderCap, projectileCandidates.length);
    for (let i = 0; i < projectileLimit; i += 1) {
      selectedProjectileIds.push(projectileCandidates[i][0]);
    }
  }

  const finishedAtMs = performance.now();
  return {
    seq: Number(payload?.seq) || 0,
    playerIds: localPlayerIds.concat(selectedRemoteIds),
    projectileIds: selectedProjectileIds,
    computeMs: Number((finishedAtMs - startedAtMs).toFixed(3)),
    roundTripMs: Number((finishedAtMs - asNumber(payload?.requestedAtMs, finishedAtMs)).toFixed(3)),
    generatedAtMs: Number(finishedAtMs.toFixed(3))
  };
}

async function initWorker(payload) {
  const wasmUrl = typeof payload?.wasmUrl === "string" ? payload.wasmUrl.trim() : "";
  if (wasmUrl) {
    try {
      const response = await fetch(wasmUrl);
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      const bytes = await response.arrayBuffer();
      const result = await WebAssembly.instantiate(bytes, {});
      const exports = result?.instance?.exports || {};
      const cullDistanceSq = exports?.cull_distance_sq || exports?.cullDistanceSq || null;
      const cullVisibility = exports?.cull_visibility || exports?.cullVisibility || null;
      if (
        (cullDistanceSq && typeof cullDistanceSq === "function") ||
        (cullVisibility && typeof cullVisibility === "function")
      ) {
        wasmKernel = {
          cullDistanceSq,
          cullVisibility
        };
        wasmKernelLabel = "wasm";
      }
    } catch (_) {
      wasmKernel = null;
      wasmKernelLabel = "js";
    }
  }

  self.postMessage({
    type: "ready",
    wasmKernelActive: !!wasmKernel,
    kernel: wasmKernelLabel
  });
}

self.onmessage = async (event) => {
  const message = event?.data || {};
  try {
    if (message.type === "init") {
      await initWorker(message);
      return;
    }
    if (message.type === "compute") {
      const result = computeCull(message);
      self.postMessage({
        type: "result",
        ...result
      });
      return;
    }
  } catch (error) {
    self.postMessage({
      type: "error",
      error: error?.message || String(error)
    });
  }
};
