"use strict";

const BASE_BROWSER_ARGS = [
  "--disable-background-timer-throttling",
  "--disable-backgrounding-occluded-windows",
  "--disable-renderer-backgrounding",
  "--disable-features=BlockInsecurePrivateNetworkRequests",
];

const WEBGPU_BROWSER_ARGS = [
  "--enable-unsafe-webgpu",
  "--ignore-gpu-blocklist",
  "--enable-features=Vulkan,UseSkiaRenderer,UnsafeWebGPU",
  "--disable-dawn-features=disallow_unsafe_apis",
  "--disable-frame-rate-limit",
  "--disable-gpu-vsync",
];

function urlRequestsWebGpu(rawUrl) {
  if (!rawUrl) return false;
  try {
    const parsed = new URL(rawUrl);
    const params = parsed.searchParams;
    return (
      params.get("webgpu") === "1" ||
      params.get("webgpu_test") === "1" ||
      params.get("require_webgpu") === "1" ||
      params.get("mode") === "webgpu" ||
      params.get("webgpu_projectiles") === "1" ||
      params.get("webgpu_players") === "1" ||
      params.get("webgpu_instances") === "1"
    );
  } catch (_) {
    return false;
  }
}

function uniqueArgs(args) {
  return Array.from(new Set(args.filter(Boolean)));
}

function buildLaunchOptions({ headless = true, url = "" } = {}) {
  const webgpuRequested = urlRequestsWebGpu(url);
  const args = webgpuRequested
    ? uniqueArgs([...BASE_BROWSER_ARGS, ...WEBGPU_BROWSER_ARGS])
    : [...BASE_BROWSER_ARGS];

  return {
    headless,
    args,
    webgpuRequested,
  };
}

module.exports = {
  BASE_BROWSER_ARGS,
  WEBGPU_BROWSER_ARGS,
  urlRequestsWebGpu,
  buildLaunchOptions,
};
