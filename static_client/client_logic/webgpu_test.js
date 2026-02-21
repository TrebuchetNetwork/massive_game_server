export function getRendererBackendSummary(currentApp, pixiNs) {
    const appRef = currentApp || null;
    const pixi = pixiNs || (typeof globalThis !== 'undefined' ? globalThis.PIXI : undefined);
    if (!appRef || !appRef.renderer) {
        return {
            backend: 'none',
            pixiRendererType: null,
            webGLVersion: 0,
        };
    }

    const renderer = appRef.renderer;
    const webGLVersion = Number(renderer.context?.webGLVersion || 0);
    let backend = 'unknown';

    if (webGLVersion >= 2) {
        backend = 'webgl2';
    } else if (webGLVersion === 1) {
        backend = 'webgl1';
    } else if (pixi && renderer.type === pixi.RENDERER_TYPE.CANVAS) {
        backend = 'canvas2d';
    } else if (pixi && renderer.type === pixi.RENDERER_TYPE.WEBGL) {
        backend = 'webgl';
    }

    return {
        backend,
        pixiRendererType: Number(renderer.type || 0),
        webGLVersion,
    };
}

export function readWebGpuAutorunConfigFromUrl(uiModeParams) {
    const durationMs = Math.max(
        500,
        Math.min(60000, Math.floor(Number(uiModeParams.get('webgpu_duration_ms')) || 4000))
    );
    const width = Math.max(
        320,
        Math.min(3840, Math.floor(Number(uiModeParams.get('webgpu_width')) || 1280))
    );
    const height = Math.max(
        180,
        Math.min(2160, Math.floor(Number(uiModeParams.get('webgpu_height')) || 720))
    );
    const minFps = Math.max(
        0,
        Math.min(360, Number(uiModeParams.get('webgpu_min_fps')) || 0)
    );
    const framePacingRaw = String(uiModeParams.get('webgpu_frame_pacing') || 'raf').toLowerCase();
    const framePacing = framePacingRaw === 'uncapped' ? 'uncapped' : 'raf';
    const yieldEveryFrames = Math.max(
        1,
        Math.min(5000, Math.floor(Number(uiModeParams.get('webgpu_yield_every_frames')) || 200))
    );
    const waitForGpuEveryFrames = Math.max(
        0,
        Math.min(1000, Math.floor(Number(uiModeParams.get('webgpu_wait_gpu_every_frames')) || 0))
    );

    return {
        durationMs,
        width,
        height,
        minFps,
        powerPreference: uiModeParams.get('webgpu_power') || 'high-performance',
        framePacing,
        yieldEveryFrames,
        waitForGpuEveryFrames,
    };
}

export async function runWebGPUTest(options = {}) {
    const startedAtMs = performance.now();
    const durationMs = Math.max(500, Math.min(120000, Math.floor(Number(options.durationMs) || 4000)));
    const width = Math.max(320, Math.min(4096, Math.floor(Number(options.width) || 1280)));
    const height = Math.max(180, Math.min(2160, Math.floor(Number(options.height) || 720)));
    const minFps = Math.max(0, Math.min(360, Number(options.minFps) || 0));
    const sampleIntervalMs = Math.max(50, Math.min(2000, Math.floor(Number(options.sampleIntervalMs) || 200)));
    const powerPreference = options.powerPreference === 'low-power' ? 'low-power' : 'high-performance';
    const framePacing = options.framePacing === 'uncapped' ? 'uncapped' : 'raf';
    const yieldEveryFrames = Math.max(1, Math.min(5000, Math.floor(Number(options.yieldEveryFrames) || 200)));
    const waitForGpuEveryFrames = Math.max(0, Math.min(1000, Math.floor(Number(options.waitForGpuEveryFrames) || 0)));
    const rendererBackend = options.rendererBackend || {
        backend: 'unknown',
        pixiRendererType: null,
        webGLVersion: 0,
    };
    const resultBase = {
        startedAt: new Date().toISOString(),
        durationMs,
        width,
        height,
        minFps,
        powerPreference,
        framePacing,
        yieldEveryFrames,
        waitForGpuEveryFrames,
        rendererBackend,
    };

    if (!navigator.gpu) {
        const unsupported = {
            ...resultBase,
            supported: false,
            passed: false,
            error: 'WebGPU is not available in this browser runtime',
        };
        if (window.__e2e) {
            window.__e2e.webgpuSupported = false;
            window.__e2e.webgpuLastResult = unsupported;
        }
        return unsupported;
    }

    let canvas = null;
    let context = null;
    let device = null;

    try {
        const adapter = await navigator.gpu.requestAdapter({ powerPreference });
        if (!adapter) {
            const noAdapter = {
                ...resultBase,
                supported: false,
                passed: false,
                error: 'No WebGPU adapter available',
            };
            if (window.__e2e) {
                window.__e2e.webgpuSupported = false;
                window.__e2e.webgpuLastResult = noAdapter;
            }
            return noAdapter;
        }

        let adapterInfo = null;
        if (typeof adapter.requestAdapterInfo === 'function') {
            try {
                adapterInfo = await adapter.requestAdapterInfo();
            } catch (_) {
                adapterInfo = null;
            }
        }

        const requiredLimits = {};
        if (options.requiredLimits && typeof options.requiredLimits === 'object') {
            Object.keys(options.requiredLimits).forEach((key) => {
                const value = Number(options.requiredLimits[key]);
                if (Number.isFinite(value) && value > 0) {
                    requiredLimits[key] = Math.floor(value);
                }
            });
        }

        const requestedFeatures = Array.isArray(options.requiredFeatures)
            ? options.requiredFeatures.filter((feature) => typeof feature === 'string')
            : [];
        const deviceDescriptor = {};
        if (requestedFeatures.length > 0) {
            deviceDescriptor.requiredFeatures = requestedFeatures;
        }
        if (Object.keys(requiredLimits).length > 0) {
            deviceDescriptor.requiredLimits = requiredLimits;
        }

        device = await adapter.requestDevice(deviceDescriptor);
        canvas = document.createElement('canvas');
        canvas.width = width;
        canvas.height = height;
        context = canvas.getContext('webgpu');
        if (!context) {
            throw new Error('Unable to create WebGPU context from canvas');
        }

        const format = navigator.gpu.getPreferredCanvasFormat
            ? navigator.gpu.getPreferredCanvasFormat()
            : 'bgra8unorm';
        context.configure({
            device,
            format,
            alphaMode: 'opaque',
        });

        const frameTimes = [];
        const samples = [];
        let frames = 0;
        let lastSampleAt = startedAtMs;
        let previousFrameAt = startedAtMs;
        const loopStart = performance.now();
        const maxFrames = framePacing === 'uncapped'
            ? Math.max(50000, Math.floor((durationMs / 1000) * 500000))
            : Math.max(1, Math.floor((durationMs / 1000) * 1000));

        while ((performance.now() - loopStart) < durationMs && frames < maxFrames) {
            if (framePacing === 'raf') {
                await new Promise((resolve) => requestAnimationFrame(resolve));
            } else if (frames > 0 && (frames % yieldEveryFrames) === 0) {
                await Promise.resolve();
            }
            const frameStart = performance.now();
            const elapsedSec = (frameStart - loopStart) / 1000;
            const encoder = device.createCommandEncoder();
            const textureView = context.getCurrentTexture().createView();
            const phase = (elapsedSec * Math.PI * 2) / 2.5;
            const pass = encoder.beginRenderPass({
                colorAttachments: [
                    {
                        view: textureView,
                        clearValue: {
                            r: 0.08 + 0.02 * Math.sin(phase),
                            g: 0.1 + 0.02 * Math.cos(phase * 0.7),
                            b: 0.14 + 0.03 * Math.sin(phase * 1.3),
                            a: 1,
                        },
                        loadOp: 'clear',
                        storeOp: 'store',
                    },
                ],
            });
            pass.end();
            device.queue.submit([encoder.finish()]);
            if (
                framePacing === 'uncapped' &&
                waitForGpuEveryFrames > 0 &&
                (frames + 1) % waitForGpuEveryFrames === 0 &&
                device.queue &&
                typeof device.queue.onSubmittedWorkDone === 'function'
            ) {
                await device.queue.onSubmittedWorkDone();
            }

            const frameEnd = performance.now();
            const frameMs = frameEnd - previousFrameAt;
            previousFrameAt = frameEnd;
            frameTimes.push(frameMs);
            frames += 1;

            if ((frameEnd - lastSampleAt) >= sampleIntervalMs) {
                lastSampleAt = frameEnd;
                samples.push({
                    atMs: Number((frameEnd - loopStart).toFixed(1)),
                    frameMs: Number(frameMs.toFixed(3)),
                    submittedFrames: frames,
                });
            }
        }

        const submitLoopFinishedAtMs = performance.now();
        let queueDrainMs = 0;
        if (device.queue && typeof device.queue.onSubmittedWorkDone === 'function') {
            const queueDrainStartedAtMs = performance.now();
            await device.queue.onSubmittedWorkDone();
            queueDrainMs = Math.max(0, performance.now() - queueDrainStartedAtMs);
        }

        const finishedAtMs = performance.now();
        const submitElapsedMs = Math.max(1, submitLoopFinishedAtMs - loopStart);
        const elapsedMs = Math.max(1, finishedAtMs - loopStart);
        const submitFps = frames / (submitElapsedMs / 1000);
        const completedFps = frames / (elapsedMs / 1000);
        const sorted = frameTimes.slice().sort((a, b) => a - b);
        const p95FrameMs = sorted.length > 0
            ? sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95))]
            : 0;
        const avgFrameMs = frameTimes.length > 0
            ? frameTimes.reduce((sum, value) => sum + value, 0) / frameTimes.length
            : 0;

        const webgpuResult = {
            ...resultBase,
            supported: true,
            passed: minFps <= 0 ? true : completedFps >= minFps,
            adapterInfo,
            adapterFeatureCount: adapter.features ? adapter.features.size : 0,
            adapterLimitMaxTexture2D: Number(adapter.limits?.maxTextureDimension2D || 0),
            format,
            frames,
            submitElapsedMs: Number(submitElapsedMs.toFixed(1)),
            elapsedMs: Number(elapsedMs.toFixed(1)),
            queueDrainMs: Number(queueDrainMs.toFixed(1)),
            submitFps: Number(submitFps.toFixed(2)),
            fps: Number(completedFps.toFixed(2)),
            avgFrameMs: Number(avgFrameMs.toFixed(3)),
            p95FrameMs: Number(p95FrameMs.toFixed(3)),
            samples,
        };

        if (window.__e2e) {
            window.__e2e.webgpuSupported = true;
            window.__e2e.webgpuLastResult = webgpuResult;
        }
        return webgpuResult;
    } catch (error) {
        const failed = {
            ...resultBase,
            supported: false,
            passed: false,
            error: error?.message || String(error),
        };
        if (window.__e2e) {
            window.__e2e.webgpuSupported = false;
            window.__e2e.webgpuLastResult = failed;
        }
        return failed;
    } finally {
        try {
            if (context && typeof context.unconfigure === 'function') {
                context.unconfigure();
            }
        } catch (_) {}
        try {
            if (device && typeof device.destroy === 'function') {
                device.destroy();
            }
        } catch (_) {}
    }
}
