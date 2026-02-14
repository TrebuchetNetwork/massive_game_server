"use strict";

let wasmKernel = null;
let wasmKernelLabel = "js";

const CULL_MODE_AUTO = "auto";
const CULL_MODE_LINEAR = "linear";
const CULL_MODE_QUADTREE = "quadtree";
const DEFAULT_QUADTREE_THRESHOLD = 96;
const MIN_QUADTREE_THRESHOLD = 24;
const MAX_QUADTREE_DEPTH = 7;
const QUADTREE_NODE_CAPACITY = 24;
const QUADTREE_MIN_SPAN = 1e-3;

const pooledArrays = [];
const candidatePool = [];
const quadtreeNodePool = [];

function borrowArray() {
  return pooledArrays.pop() || [];
}

function releaseArray(arrayRef) {
  if (!arrayRef) return;
  arrayRef.length = 0;
  pooledArrays.push(arrayRef);
}

function borrowCandidate(id, distSq) {
  const candidate = candidatePool.pop() || { id: null, distSq: 0 };
  candidate.id = id;
  candidate.distSq = distSq;
  return candidate;
}

function releaseCandidateArray(arrayRef) {
  if (!arrayRef) return;
  for (let i = 0; i < arrayRef.length; i += 1) {
    const candidate = arrayRef[i];
    if (!candidate) continue;
    candidate.id = null;
    candidate.distSq = 0;
    candidatePool.push(candidate);
  }
  releaseArray(arrayRef);
}

function borrowQuadtreeNode(minX, maxX, minY, maxY, depth) {
  const node = quadtreeNodePool.pop() || {
    minX: 0,
    maxX: 0,
    minY: 0,
    maxY: 0,
    depth: 0,
    points: null,
    children: null
  };
  node.minX = minX;
  node.maxX = maxX;
  node.minY = minY;
  node.maxY = maxY;
  node.depth = depth;
  node.points = borrowArray();
  node.children = null;
  return node;
}

function releaseQuadtree(rootNode) {
  if (!rootNode) return;
  const stack = borrowArray();
  stack.push(rootNode);
  while (stack.length > 0) {
    const node = stack.pop();
    if (!node) continue;
    if (Array.isArray(node.children)) {
      for (let i = 0; i < node.children.length; i += 1) {
        const child = node.children[i];
        if (child) {
          stack.push(child);
        }
      }
    }
    releaseArray(node.points);
    node.points = null;
    node.children = null;
    quadtreeNodePool.push(node);
  }
  releaseArray(stack);
}

function asNumber(value, fallback = 0) {
  const n = Number(value);
  return Number.isFinite(n) ? n : fallback;
}

function normalizeCullMode(value) {
  const mode = String(value || "").trim().toLowerCase();
  if (mode === CULL_MODE_LINEAR || mode === CULL_MODE_QUADTREE) {
    return mode;
  }
  return CULL_MODE_AUTO;
}

function normalizeQuadtreeThreshold(value) {
  const parsed = Math.floor(Number(value));
  if (!Number.isFinite(parsed)) {
    return DEFAULT_QUADTREE_THRESHOLD;
  }
  return Math.max(MIN_QUADTREE_THRESHOLD, parsed);
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
      // Fall through to JS path.
    }
  }

  if (!inView(x, y, bounds, margin)) {
    return -1;
  }
  const dx = x - anchorX;
  const dy = y - anchorY;
  return dx * dx + dy * dy;
}

function quadtreeContains(node, x, y) {
  return x >= node.minX && x <= node.maxX && y >= node.minY && y <= node.maxY;
}

function quadtreeIntersectsRect(node, minX, maxX, minY, maxY) {
  return !(maxX < node.minX || minX > node.maxX || maxY < node.minY || minY > node.maxY);
}

function quadtreeSubdivide(node) {
  const midX = (node.minX + node.maxX) * 0.5;
  const midY = (node.minY + node.maxY) * 0.5;
  const childDepth = node.depth + 1;
  node.children = [
    borrowQuadtreeNode(node.minX, midX, node.minY, midY, childDepth),
    borrowQuadtreeNode(midX, node.maxX, node.minY, midY, childDepth),
    borrowQuadtreeNode(node.minX, midX, midY, node.maxY, childDepth),
    borrowQuadtreeNode(midX, node.maxX, midY, node.maxY, childDepth)
  ];

  const existingPoints = node.points;
  node.points = borrowArray();
  for (let i = 0; i < existingPoints.length; i += 1) {
    const row = existingPoints[i];
    if (!Array.isArray(row) || row.length < 3) continue;
    const x = asNumber(row[1], 0);
    const y = asNumber(row[2], 0);
    quadtreeInsert(node, row, x, y);
  }
  releaseArray(existingPoints);
}

function quadtreeInsert(node, row, x, y) {
  if (!quadtreeContains(node, x, y)) {
    return false;
  }

  if (!node.children) {
    const reachedCapacity = node.points.length >= QUADTREE_NODE_CAPACITY;
    const reachedDepthLimit = node.depth >= MAX_QUADTREE_DEPTH;
    if (!reachedCapacity || reachedDepthLimit) {
      node.points.push(row);
      return true;
    }
    quadtreeSubdivide(node);
  }

  for (let i = 0; i < node.children.length; i += 1) {
    const child = node.children[i];
    if (quadtreeContains(child, x, y)) {
      return quadtreeInsert(child, row, x, y);
    }
  }

  node.points.push(row);
  return true;
}

function buildQuadtree(rows, bounds, margin) {
  if (!Array.isArray(rows) || rows.length === 0) {
    return null;
  }

  const expandedMargin = Math.max(24, Number(margin) || 0);
  let minX = asNumber(bounds.left, 0) - expandedMargin;
  let maxX = asNumber(bounds.right, 0) + expandedMargin;
  let minY = asNumber(bounds.top, 0) - expandedMargin;
  let maxY = asNumber(bounds.bottom, 0) + expandedMargin;

  for (let i = 0; i < rows.length; i += 1) {
    const row = rows[i];
    if (!Array.isArray(row) || row.length < 3) continue;
    const x = asNumber(row[1], 0);
    const y = asNumber(row[2], 0);
    if (x < minX) minX = x;
    if (x > maxX) maxX = x;
    if (y < minY) minY = y;
    if (y > maxY) maxY = y;
  }

  if ((maxX - minX) < QUADTREE_MIN_SPAN) {
    const midX = (maxX + minX) * 0.5;
    minX = midX - 1;
    maxX = midX + 1;
  }
  if ((maxY - minY) < QUADTREE_MIN_SPAN) {
    const midY = (maxY + minY) * 0.5;
    minY = midY - 1;
    maxY = midY + 1;
  }

  const root = borrowQuadtreeNode(minX, maxX, minY, maxY, 0);
  for (let i = 0; i < rows.length; i += 1) {
    const row = rows[i];
    if (!Array.isArray(row) || row.length < 3) continue;
    const x = asNumber(row[1], 0);
    const y = asNumber(row[2], 0);
    quadtreeInsert(root, row, x, y);
  }
  return root;
}

function queryQuadtree(rootNode, bounds, margin, outRows) {
  if (!rootNode || !outRows) return;
  const minX = asNumber(bounds.left, 0) - margin;
  const maxX = asNumber(bounds.right, 0) + margin;
  const minY = asNumber(bounds.top, 0) - margin;
  const maxY = asNumber(bounds.bottom, 0) + margin;

  const stack = borrowArray();
  stack.push(rootNode);
  while (stack.length > 0) {
    const node = stack.pop();
    if (!node || !quadtreeIntersectsRect(node, minX, maxX, minY, maxY)) {
      continue;
    }

    for (let i = 0; i < node.points.length; i += 1) {
      const row = node.points[i];
      if (!Array.isArray(row) || row.length < 3) continue;
      const x = asNumber(row[1], 0);
      const y = asNumber(row[2], 0);
      if (x >= minX && x <= maxX && y >= minY && y <= maxY) {
        outRows.push(row);
      }
    }

    if (!node.children) continue;
    for (let i = 0; i < node.children.length; i += 1) {
      const child = node.children[i];
      if (child) {
        stack.push(child);
      }
    }
  }
  releaseArray(stack);
}

function compareCandidatesByDistance(a, b) {
  return a.distSq - b.distSq;
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

  const cullMode = normalizeCullMode(config.cullMode);
  const quadtreeThreshold = normalizeQuadtreeThreshold(config.quadtreeThreshold);
  const totalEntityCount = players.length + projectiles.length;
  const useQuadtree =
    totalEntityCount > 0 &&
    (cullMode === CULL_MODE_QUADTREE ||
      (cullMode === CULL_MODE_AUTO && totalEntityCount >= quadtreeThreshold));

  let playerTree = null;
  let projectileTree = null;
  const playerRows = borrowArray();
  const projectileRows = borrowArray();
  const priorityCandidates = borrowArray();
  const remoteCandidates = borrowArray();
  const projectileCandidates = borrowArray();
  const localPlayerIds = [];

  for (let i = 0; i < players.length; i += 1) {
    const row = players[i];
    if (!Array.isArray(row) || row.length < 4) continue;
    if (row[3] === 1) {
      localPlayerIds.push(row[0]);
    }
  }

  try {
    let playerRowsForEval = players;
    let projectileRowsForEval = projectiles;
    if (useQuadtree) {
      playerTree = buildQuadtree(players, bounds, playerCullMargin);
      projectileTree = buildQuadtree(projectiles, bounds, projectileCullMargin);
      queryQuadtree(playerTree, bounds, playerCullMargin, playerRows);
      queryQuadtree(projectileTree, bounds, projectileCullMargin, projectileRows);
      playerRowsForEval = playerRows;
      projectileRowsForEval = projectileRows;
    }

    for (let i = 0; i < playerRowsForEval.length; i += 1) {
      const row = playerRowsForEval[i];
      if (!Array.isArray(row) || row.length < 4) continue;
      const id = row[0];
      const x = asNumber(row[1], 0);
      const y = asNumber(row[2], 0);
      const isLocal = row[3] === 1;
      if (isLocal) {
        continue;
      }

      const distSq = useQuadtree
        ? ((x - localAnchorX) * (x - localAnchorX) + (y - localAnchorY) * (y - localAnchorY))
        : getDistanceSqCandidate(x, y, bounds, playerCullMargin, localAnchorX, localAnchorY);
      if (distSq < 0) {
        continue;
      }

      if (distSq <= priorityDistanceSq) {
        priorityCandidates.push(borrowCandidate(id, distSq));
      } else {
        remoteCandidates.push(borrowCandidate(id, distSq));
      }
    }

    priorityCandidates.sort(compareCandidatesByDistance);
    remoteCandidates.sort(compareCandidatesByDistance);

    const selectedRemoteIds = [];
    const priorityCap = remoteRenderCap + remotePriorityOverflowCap;
    const priorityLimit = Math.min(priorityCap, priorityCandidates.length);
    for (let i = 0; i < priorityLimit; i += 1) {
      selectedRemoteIds.push(priorityCandidates[i].id);
    }
    const remainingNormalCap = Math.max(0, remoteRenderCap - Math.min(remoteRenderCap, priorityLimit));
    const normalLimit = Math.min(remainingNormalCap, remoteCandidates.length);
    for (let i = 0; i < normalLimit; i += 1) {
      selectedRemoteIds.push(remoteCandidates[i].id);
    }

    const selectedProjectileIds = [];
    if (projectileRenderCap > 0) {
      for (let i = 0; i < projectileRowsForEval.length; i += 1) {
        const row = projectileRowsForEval[i];
        if (!Array.isArray(row) || row.length < 3) continue;
        const id = row[0];
        const x = asNumber(row[1], 0);
        const y = asNumber(row[2], 0);
        const distSq = useQuadtree
          ? ((x - localAnchorX) * (x - localAnchorX) + (y - localAnchorY) * (y - localAnchorY))
          : getDistanceSqCandidate(x, y, bounds, projectileCullMargin, localAnchorX, localAnchorY);
        if (distSq < 0) {
          continue;
        }
        projectileCandidates.push(borrowCandidate(id, distSq));
      }

      projectileCandidates.sort(compareCandidatesByDistance);
      const projectileLimit = Math.min(projectileRenderCap, projectileCandidates.length);
      for (let i = 0; i < projectileLimit; i += 1) {
        selectedProjectileIds.push(projectileCandidates[i].id);
      }
    }

    const finishedAtMs = performance.now();
    const effectiveMode = useQuadtree ? CULL_MODE_QUADTREE : CULL_MODE_LINEAR;
    return {
      seq: Number(payload?.seq) || 0,
      playerIds: localPlayerIds.concat(selectedRemoteIds),
      projectileIds: selectedProjectileIds,
      cullMode: effectiveMode,
      computeMs: Number((finishedAtMs - startedAtMs).toFixed(3)),
      roundTripMs: Number((finishedAtMs - asNumber(payload?.requestedAtMs, finishedAtMs)).toFixed(3)),
      generatedAtMs: Number(finishedAtMs.toFixed(3))
    };
  } finally {
    releaseCandidateArray(priorityCandidates);
    releaseCandidateArray(remoteCandidates);
    releaseCandidateArray(projectileCandidates);
    releaseArray(playerRows);
    releaseArray(projectileRows);
    releaseQuadtree(playerTree);
    releaseQuadtree(projectileTree);
  }
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
