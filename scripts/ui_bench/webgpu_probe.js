#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const { chromium } = require("playwright");

function parseArgs(argv) {
  const args = {
    url: "http://127.0.0.1:18080/client.html?mode=webgpu&profile=1",
    durationMs: 5000,
    width: 1280,
    height: 720,
    minFps: 0,
    sampleIntervalMs: 200,
    powerPreference: "high-performance",
    framePacing: "uncapped",
    yieldEveryFrames: 200,
    waitForGpuEveryFrames: 0,
    headless: false,
    timeoutMs: 90000,
    outPath: path.resolve(process.cwd(), "artifacts", "ui_bench", "webgpu_probe.json")
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--url") args.url = argv[++i];
    else if (arg === "--duration") args.durationMs = Number(argv[++i]) * 1000;
    else if (arg === "--duration-ms") args.durationMs = Number(argv[++i]);
    else if (arg === "--width") args.width = Number(argv[++i]);
    else if (arg === "--height") args.height = Number(argv[++i]);
    else if (arg === "--min-fps") args.minFps = Number(argv[++i]);
    else if (arg === "--sample-interval-ms") args.sampleIntervalMs = Number(argv[++i]);
    else if (arg === "--power") args.powerPreference = argv[++i];
    else if (arg === "--frame-pacing") args.framePacing = argv[++i];
    else if (arg === "--yield-every-frames") args.yieldEveryFrames = Number(argv[++i]);
    else if (arg === "--wait-gpu-every-frames") args.waitForGpuEveryFrames = Number(argv[++i]);
    else if (arg === "--timeout-ms") args.timeoutMs = Number(argv[++i]);
    else if (arg === "--headed") args.headless = false;
    else if (arg === "--headless") args.headless = true;
    else if (arg === "--out") args.outPath = path.resolve(process.cwd(), argv[++i]);
    else if (arg === "--help") {
      printHelp();
      process.exit(0);
    }
  }

  args.durationMs = Math.max(500, Math.min(120000, Math.floor(args.durationMs)));
  args.width = Math.max(320, Math.min(4096, Math.floor(args.width)));
  args.height = Math.max(180, Math.min(2160, Math.floor(args.height)));
  args.minFps = Math.max(0, Math.min(360, Number(args.minFps) || 0));
  args.sampleIntervalMs = Math.max(50, Math.min(5000, Math.floor(args.sampleIntervalMs)));
  args.timeoutMs = Math.max(5000, Math.min(300000, Math.floor(args.timeoutMs)));
  args.powerPreference = args.powerPreference === "low-power" ? "low-power" : "high-performance";
  args.framePacing = args.framePacing === "raf" ? "raf" : "uncapped";
  args.yieldEveryFrames = Math.max(1, Math.min(5000, Math.floor(args.yieldEveryFrames)));
  args.waitForGpuEveryFrames = Math.max(0, Math.min(1000, Math.floor(args.waitForGpuEveryFrames)));

  return args;
}

function printHelp() {
  console.log(`WebGPU probe options:
  --url <url>                   Page URL (default: client.html?mode=webgpu&profile=1)
  --duration <seconds>          Probe duration in seconds (default: 5)
  --duration-ms <ms>            Probe duration in ms
  --width <px>                  Render width (default: 1280)
  --height <px>                 Render height (default: 720)
  --min-fps <fps>               FPS pass threshold (default: 0)
  --sample-interval-ms <ms>     Telemetry sample interval (default: 200)
  --power <mode>                high-performance or low-power
  --frame-pacing <mode>         uncapped (default) or raf
  --yield-every-frames <n>      uncapped mode cooperative yield interval (default: 200)
  --wait-gpu-every-frames <n>   uncapped mode queue sync cadence (default: 0/off)
  --timeout-ms <ms>             End-to-end timeout (default: 90000)
  --headed                      Run with visible browser (default)
  --headless                    Run headless
  --out <path>                  Output JSON path
  --help                        Show this help
`);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const browserArgs = [
    "--enable-unsafe-webgpu",
    "--ignore-gpu-blocklist",
    "--enable-features=Vulkan,UseSkiaRenderer,UnsafeWebGPU",
    "--disable-dawn-features=disallow_unsafe_apis",
    "--disable-features=BlockInsecurePrivateNetworkRequests",
    "--disable-background-timer-throttling",
    "--disable-backgrounding-occluded-windows",
    "--disable-renderer-backgrounding",
    "--disable-frame-rate-limit",
    "--disable-gpu-vsync"
  ];

  const browser = await chromium.launch({
    headless: args.headless,
    args: browserArgs
  });

  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  page.setDefaultTimeout(args.timeoutMs);

  const startedAt = new Date().toISOString();
  let result;
  let pageErrors = [];
  page.on("pageerror", (err) => pageErrors.push(String(err)));

  try {
    await page.goto(args.url, { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => Boolean(window.__e2e && typeof window.__e2e.runWebGPUTest === "function"));

    result = await page.evaluate(async (probeOptions) => {
      return window.__e2e.runWebGPUTest(probeOptions);
    }, {
      durationMs: args.durationMs,
      width: args.width,
      height: args.height,
      minFps: args.minFps,
      sampleIntervalMs: args.sampleIntervalMs,
      powerPreference: args.powerPreference,
      framePacing: args.framePacing,
      yieldEveryFrames: args.yieldEveryFrames,
      waitForGpuEveryFrames: args.waitForGpuEveryFrames
    });
  } finally {
    await context.close();
    await browser.close();
  }

  const payload = {
    startedAt,
    finishedAt: new Date().toISOString(),
    url: args.url,
    options: {
      durationMs: args.durationMs,
      width: args.width,
      height: args.height,
      minFps: args.minFps,
      sampleIntervalMs: args.sampleIntervalMs,
      powerPreference: args.powerPreference,
      framePacing: args.framePacing,
      yieldEveryFrames: args.yieldEveryFrames,
      waitForGpuEveryFrames: args.waitForGpuEveryFrames,
      headless: args.headless
    },
    pageErrors,
    result
  };

  fs.mkdirSync(path.dirname(args.outPath), { recursive: true });
  fs.writeFileSync(args.outPath, JSON.stringify(payload, null, 2));
  console.log(JSON.stringify(payload, null, 2));

  if (!result || !result.supported) {
    process.exit(2);
  }
  if (!result.passed) {
    process.exit(3);
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
