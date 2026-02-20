/**
 * Accelerated projectile/player overlay layers (WebGPU/WebGL2) extracted from client.html.
 */

export function createAcceleratedLayerRuntime({
  WEBGL2_SUPPORTED = false,
  WEBGPU_PROJECTILE_INSTANCE_STRIDE = 7,
  WEBGPU_PLAYER_INSTANCE_STRIDE = 8,
  EMPTY_INSTANCE_ARRAY = new Float32Array(0),
} = {}) {
class WebGPUProjectileLayer {
    constructor(hostElement) {
        this.hostElement = hostElement;
        this.backend = 'webgpu';
        this.canvas = null;
        this.context = null;
        this.adapter = null;
        this.device = null;
        this.format = null;
        this.pipeline = null;
        this.uniformBuffer = null;
        this.bindGroup = null;
        this.quadBuffer = null;
        this.instanceBuffer = null;
        this.instanceCapacity = 0;
        this.instanceCount = 0;
        this.ready = false;
        this.lastError = null;
        this.lastInstanceCount = 0;
    }

    async init(width, height) {
        if (!navigator.gpu) {
            throw new Error('WebGPU unavailable');
        }
        this.adapter = await navigator.gpu.requestAdapter({ powerPreference: 'high-performance' });
        if (!this.adapter) {
            throw new Error('No WebGPU adapter available');
        }
        this.device = await this.adapter.requestDevice();

        this.canvas = document.createElement('canvas');
        this.canvas.className = 'webgpu-projectile-layer';
        this.canvas.style.position = 'absolute';
        this.canvas.style.left = '0';
        this.canvas.style.top = '0';
        this.canvas.style.width = '100%';
        this.canvas.style.height = '100%';
        this.canvas.style.pointerEvents = 'none';
        this.canvas.style.zIndex = '3';
        this.hostElement.style.position = this.hostElement.style.position || 'relative';
        this.hostElement.appendChild(this.canvas);

        this.context = this.canvas.getContext('webgpu');
        if (!this.context) {
            throw new Error('Failed to acquire WebGPU canvas context');
        }
        this.format = navigator.gpu.getPreferredCanvasFormat
            ? navigator.gpu.getPreferredCanvasFormat()
            : 'bgra8unorm';
        this.context.configure({
            device: this.device,
            format: this.format,
            alphaMode: 'premultiplied'
        });

        this.resize(width, height);
        this.initPipeline();
        this.ensureInstanceCapacity(1024);
        this.ready = true;
    }

    initPipeline() {
        const shader = this.device.createShaderModule({
            code: `
struct ViewUniform {
  left: f32,
  top: f32,
  width: f32,
  height: f32,
};

@group(0) @binding(0) var<uniform> view: ViewUniform;

struct VertexInput {
  @location(0) corner: vec2<f32>,
  @location(1) worldPos: vec2<f32>,
  @location(2) size: f32,
  @location(3) color: vec4<f32>,
};

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
  let world = input.worldPos + input.corner * input.size;
  let nx = ((world.x - view.left) / max(view.width, 1.0)) * 2.0 - 1.0;
  let ny = 1.0 - ((world.y - view.top) / max(view.height, 1.0)) * 2.0;
  var out: VertexOutput;
  out.position = vec4<f32>(nx, ny, 0.0, 1.0);
  out.color = input.color;
  return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
  return input.color;
}
            `
        });

        this.uniformBuffer = this.device.createBuffer({
            size: 16,
            usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST
        });

        const bindGroupLayout = this.device.createBindGroupLayout({
            entries: [
                {
                    binding: 0,
                    visibility: GPUShaderStage.VERTEX,
                    buffer: { type: 'uniform' }
                }
            ]
        });
        const pipelineLayout = this.device.createPipelineLayout({
            bindGroupLayouts: [bindGroupLayout]
        });

        this.pipeline = this.device.createRenderPipeline({
            layout: pipelineLayout,
            vertex: {
                module: shader,
                entryPoint: 'vs_main',
                buffers: [
                    {
                        arrayStride: 8,
                        stepMode: 'vertex',
                        attributes: [
                            { shaderLocation: 0, offset: 0, format: 'float32x2' }
                        ]
                    },
                    {
                        arrayStride: 28,
                        stepMode: 'instance',
                        attributes: [
                            { shaderLocation: 1, offset: 0, format: 'float32x2' },
                            { shaderLocation: 2, offset: 8, format: 'float32' },
                            { shaderLocation: 3, offset: 12, format: 'float32x4' }
                        ]
                    }
                ]
            },
            fragment: {
                module: shader,
                entryPoint: 'fs_main',
                targets: [
                    {
                        format: this.format,
                        blend: {
                            color: {
                                srcFactor: 'src-alpha',
                                dstFactor: 'one',
                                operation: 'add'
                            },
                            alpha: {
                                srcFactor: 'one',
                                dstFactor: 'one-minus-src-alpha',
                                operation: 'add'
                            }
                        }
                    }
                ]
            },
            primitive: {
                topology: 'triangle-list'
            }
        });

        this.bindGroup = this.device.createBindGroup({
            layout: bindGroupLayout,
            entries: [
                {
                    binding: 0,
                    resource: { buffer: this.uniformBuffer }
                }
            ]
        });

        const quadVertices = new Float32Array([
            -1, -1,
             1, -1,
            -1,  1,
            -1,  1,
             1, -1,
             1,  1
        ]);
        this.quadBuffer = this.device.createBuffer({
            size: quadVertices.byteLength,
            usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST
        });
        this.device.queue.writeBuffer(this.quadBuffer, 0, quadVertices);
    }

    ensureInstanceCapacity(count) {
        if (count <= this.instanceCapacity) return;
        let nextCapacity = Math.max(1024, this.instanceCapacity);
        while (nextCapacity < count) {
            nextCapacity *= 2;
        }
        this.instanceBuffer = this.device.createBuffer({
            size: nextCapacity * 28,
            usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST
        });
        this.instanceCapacity = nextCapacity;
    }

    resize(width, height) {
        if (!this.canvas) return;
        const w = Math.max(1, Math.floor(width));
        const h = Math.max(1, Math.floor(height));
        this.canvas.width = w;
        this.canvas.height = h;
    }

    render(viewBounds, instanceArray) {
        if (!this.ready || !this.context || !this.device || !this.pipeline) return;
        const view = new Float32Array([
            Number(viewBounds.left) || 0,
            Number(viewBounds.top) || 0,
            Math.max(1, (Number(viewBounds.right) || 0) - (Number(viewBounds.left) || 0)),
            Math.max(1, (Number(viewBounds.bottom) || 0) - (Number(viewBounds.top) || 0))
        ]);
        this.device.queue.writeBuffer(this.uniformBuffer, 0, view);

        const instanceCount = Math.max(0, Math.floor((instanceArray?.length || 0) / 7));
        this.instanceCount = instanceCount;
        this.lastInstanceCount = instanceCount;
        if (instanceCount > 0) {
            this.ensureInstanceCapacity(instanceCount);
            this.device.queue.writeBuffer(this.instanceBuffer, 0, instanceArray);
        }

        const encoder = this.device.createCommandEncoder();
        const pass = encoder.beginRenderPass({
            colorAttachments: [
                {
                    view: this.context.getCurrentTexture().createView(),
                    clearValue: { r: 0, g: 0, b: 0, a: 0 },
                    loadOp: 'clear',
                    storeOp: 'store'
                }
            ]
        });
        pass.setPipeline(this.pipeline);
        pass.setBindGroup(0, this.bindGroup);
        pass.setVertexBuffer(0, this.quadBuffer);
        if (instanceCount > 0) {
            pass.setVertexBuffer(1, this.instanceBuffer);
            pass.draw(6, instanceCount, 0, 0);
        }
        pass.end();
        this.device.queue.submit([encoder.finish()]);
    }

    clear(viewBounds) {
        this.render(viewBounds, EMPTY_INSTANCE_ARRAY);
    }

    destroy() {
        this.ready = false;
        try {
            if (this.context && typeof this.context.unconfigure === 'function') {
                this.context.unconfigure();
            }
        } catch (_) {}
        try {
            if (this.device && typeof this.device.destroy === 'function') {
                this.device.destroy();
            }
        } catch (_) {}
        if (this.canvas && this.canvas.parentNode) {
            this.canvas.parentNode.removeChild(this.canvas);
        }
        this.canvas = null;
        this.context = null;
        this.adapter = null;
        this.device = null;
        this.pipeline = null;
        this.bindGroup = null;
        this.uniformBuffer = null;
        this.quadBuffer = null;
        this.instanceBuffer = null;
        this.instanceCapacity = 0;
        this.instanceCount = 0;
    }
}

class WebGPUPlayerLayer {
    constructor(hostElement) {
        this.hostElement = hostElement;
        this.backend = 'webgpu';
        this.canvas = null;
        this.context = null;
        this.adapter = null;
        this.device = null;
        this.format = null;
        this.pipeline = null;
        this.uniformBuffer = null;
        this.bindGroup = null;
        this.shipVertexBuffer = null;
        this.instanceBuffer = null;
        this.instanceCapacity = 0;
        this.instanceCount = 0;
        this.ready = false;
        this.lastError = null;
        this.lastInstanceCount = 0;
    }

    async init(width, height) {
        if (!navigator.gpu) {
            throw new Error('WebGPU unavailable');
        }
        this.adapter = await navigator.gpu.requestAdapter({ powerPreference: 'high-performance' });
        if (!this.adapter) {
            throw new Error('No WebGPU adapter available');
        }
        this.device = await this.adapter.requestDevice();

        this.canvas = document.createElement('canvas');
        this.canvas.className = 'webgpu-player-layer';
        this.canvas.style.position = 'absolute';
        this.canvas.style.left = '0';
        this.canvas.style.top = '0';
        this.canvas.style.width = '100%';
        this.canvas.style.height = '100%';
        this.canvas.style.pointerEvents = 'none';
        this.canvas.style.zIndex = '4';
        this.hostElement.style.position = this.hostElement.style.position || 'relative';
        this.hostElement.appendChild(this.canvas);

        this.context = this.canvas.getContext('webgpu');
        if (!this.context) {
            throw new Error('Failed to acquire WebGPU canvas context');
        }
        this.format = navigator.gpu.getPreferredCanvasFormat
            ? navigator.gpu.getPreferredCanvasFormat()
            : 'bgra8unorm';
        this.context.configure({
            device: this.device,
            format: this.format,
            alphaMode: 'premultiplied'
        });

        this.resize(width, height);
        this.initPipeline();
        this.ensureInstanceCapacity(512);
        this.ready = true;
    }

    initPipeline() {
        const shader = this.device.createShaderModule({
            code: `
struct ViewUniform {
  left: f32,
  top: f32,
  width: f32,
  height: f32,
};

@group(0) @binding(0) var<uniform> view: ViewUniform;

struct VertexInput {
  @location(0) localPos: vec2<f32>,
  @location(1) worldPos: vec2<f32>,
  @location(2) rotation: f32,
  @location(3) size: f32,
  @location(4) color: vec4<f32>,
};

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
  let c = cos(input.rotation);
  let s = sin(input.rotation);
  let scaled = input.localPos * input.size;
  let rotated = vec2<f32>(
    scaled.x * c - scaled.y * s,
    scaled.x * s + scaled.y * c
  );
  let world = input.worldPos + rotated;
  let nx = ((world.x - view.left) / max(view.width, 1.0)) * 2.0 - 1.0;
  let ny = 1.0 - ((world.y - view.top) / max(view.height, 1.0)) * 2.0;
  var out: VertexOutput;
  out.position = vec4<f32>(nx, ny, 0.0, 1.0);
  out.color = input.color;
  return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
  return input.color;
}
            `
        });

        this.uniformBuffer = this.device.createBuffer({
            size: 16,
            usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST
        });

        const bindGroupLayout = this.device.createBindGroupLayout({
            entries: [
                {
                    binding: 0,
                    visibility: GPUShaderStage.VERTEX,
                    buffer: { type: 'uniform' }
                }
            ]
        });
        const pipelineLayout = this.device.createPipelineLayout({
            bindGroupLayouts: [bindGroupLayout]
        });

        this.pipeline = this.device.createRenderPipeline({
            layout: pipelineLayout,
            vertex: {
                module: shader,
                entryPoint: 'vs_main',
                buffers: [
                    {
                        arrayStride: 8,
                        stepMode: 'vertex',
                        attributes: [
                            { shaderLocation: 0, offset: 0, format: 'float32x2' }
                        ]
                    },
                    {
                        arrayStride: 32,
                        stepMode: 'instance',
                        attributes: [
                            { shaderLocation: 1, offset: 0, format: 'float32x2' },
                            { shaderLocation: 2, offset: 8, format: 'float32' },
                            { shaderLocation: 3, offset: 12, format: 'float32' },
                            { shaderLocation: 4, offset: 16, format: 'float32x4' }
                        ]
                    }
                ]
            },
            fragment: {
                module: shader,
                entryPoint: 'fs_main',
                targets: [
                    {
                        format: this.format,
                        blend: {
                            color: {
                                srcFactor: 'src-alpha',
                                dstFactor: 'one-minus-src-alpha',
                                operation: 'add'
                            },
                            alpha: {
                                srcFactor: 'one',
                                dstFactor: 'one-minus-src-alpha',
                                operation: 'add'
                            }
                        }
                    }
                ]
            },
            primitive: {
                topology: 'triangle-list'
            }
        });

        this.bindGroup = this.device.createBindGroup({
            layout: bindGroupLayout,
            entries: [
                {
                    binding: 0,
                    resource: { buffer: this.uniformBuffer }
                }
            ]
        });

        // Arrow-like ship triangle (nose points +X in local space).
        const shipVertices = new Float32Array([
            1.0, 0.0,
            -0.8, 0.68,
            -0.8, -0.68
        ]);
        this.shipVertexBuffer = this.device.createBuffer({
            size: shipVertices.byteLength,
            usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST
        });
        this.device.queue.writeBuffer(this.shipVertexBuffer, 0, shipVertices);
    }

    ensureInstanceCapacity(count) {
        if (count <= this.instanceCapacity) return;
        let nextCapacity = Math.max(512, this.instanceCapacity);
        while (nextCapacity < count) {
            nextCapacity *= 2;
        }
        this.instanceBuffer = this.device.createBuffer({
            size: nextCapacity * 32,
            usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST
        });
        this.instanceCapacity = nextCapacity;
    }

    resize(width, height) {
        if (!this.canvas) return;
        const w = Math.max(1, Math.floor(width));
        const h = Math.max(1, Math.floor(height));
        this.canvas.width = w;
        this.canvas.height = h;
    }

    render(viewBounds, instanceArray) {
        if (!this.ready || !this.context || !this.device || !this.pipeline) return;
        const view = new Float32Array([
            Number(viewBounds.left) || 0,
            Number(viewBounds.top) || 0,
            Math.max(1, (Number(viewBounds.right) || 0) - (Number(viewBounds.left) || 0)),
            Math.max(1, (Number(viewBounds.bottom) || 0) - (Number(viewBounds.top) || 0))
        ]);
        this.device.queue.writeBuffer(this.uniformBuffer, 0, view);

        const instanceCount = Math.max(0, Math.floor((instanceArray?.length || 0) / WEBGPU_PLAYER_INSTANCE_STRIDE));
        this.instanceCount = instanceCount;
        this.lastInstanceCount = instanceCount;
        if (instanceCount > 0) {
            this.ensureInstanceCapacity(instanceCount);
            this.device.queue.writeBuffer(this.instanceBuffer, 0, instanceArray);
        }

        const encoder = this.device.createCommandEncoder();
        const pass = encoder.beginRenderPass({
            colorAttachments: [
                {
                    view: this.context.getCurrentTexture().createView(),
                    clearValue: { r: 0, g: 0, b: 0, a: 0 },
                    loadOp: 'clear',
                    storeOp: 'store'
                }
            ]
        });
        pass.setPipeline(this.pipeline);
        pass.setBindGroup(0, this.bindGroup);
        pass.setVertexBuffer(0, this.shipVertexBuffer);
        if (instanceCount > 0) {
            pass.setVertexBuffer(1, this.instanceBuffer);
            pass.draw(3, instanceCount, 0, 0);
        }
        pass.end();
        this.device.queue.submit([encoder.finish()]);
    }

    clear(viewBounds) {
        this.render(viewBounds, EMPTY_INSTANCE_ARRAY);
    }

    destroy() {
        this.ready = false;
        try {
            if (this.context && typeof this.context.unconfigure === 'function') {
                this.context.unconfigure();
            }
        } catch (_) {}
        try {
            if (this.device && typeof this.device.destroy === 'function') {
                this.device.destroy();
            }
        } catch (_) {}
        if (this.canvas && this.canvas.parentNode) {
            this.canvas.parentNode.removeChild(this.canvas);
        }
        this.canvas = null;
        this.context = null;
        this.adapter = null;
        this.device = null;
        this.pipeline = null;
        this.bindGroup = null;
        this.uniformBuffer = null;
        this.shipVertexBuffer = null;
        this.instanceBuffer = null;
        this.instanceCapacity = 0;
        this.instanceCount = 0;
    }
}

function getAcceleratedLayerBackend(layer) {
    if (!layer || typeof layer !== 'object') return 'none';
    const backend = String(layer.backend || '').toLowerCase();
    if (backend === 'webgpu' || backend === 'webgl2') {
        return backend;
    }
    return 'unknown';
}

function formatAcceleratedBackendLabel(backend) {
    const normalized = String(backend || 'unknown').toLowerCase();
    if (normalized === 'webgpu') return 'WebGPU';
    if (normalized === 'webgl2') return 'WebGL2';
    if (normalized === 'none') return 'disabled';
    return normalized.toUpperCase();
}

class WebGL2ProjectileLayer {
    constructor(hostElement) {
        this.hostElement = hostElement;
        this.backend = 'webgl2';
        this.canvas = null;
        this.gl = null;
        this.program = null;
        this.uniformViewLocation = null;
        this.vao = null;
        this.quadBuffer = null;
        this.instanceBuffer = null;
        this.instanceCapacity = 0;
        this.instanceCount = 0;
        this.ready = false;
        this.lastError = null;
        this.lastInstanceCount = 0;
    }

    init(width, height) {
        if (!WEBGL2_SUPPORTED) {
            throw new Error('WebGL2 unavailable');
        }

        this.canvas = document.createElement('canvas');
        this.canvas.className = 'webgl2-projectile-layer';
        this.canvas.style.position = 'absolute';
        this.canvas.style.left = '0';
        this.canvas.style.top = '0';
        this.canvas.style.width = '100%';
        this.canvas.style.height = '100%';
        this.canvas.style.pointerEvents = 'none';
        this.canvas.style.zIndex = '3';
        const hostPosition = this.hostElement.style.position;
        if (!hostPosition || hostPosition === 'static') {
            this.hostElement.style.position = 'relative';
        }
        this.hostElement.appendChild(this.canvas);

        this.gl = this.canvas.getContext('webgl2', {
            alpha: true,
            antialias: false,
            depth: false,
            stencil: false,
            premultipliedAlpha: true,
            preserveDrawingBuffer: false,
            powerPreference: 'high-performance'
        });
        if (!this.gl) {
            throw new Error('Failed to acquire WebGL2 canvas context');
        }

        this.resize(width, height);
        this.initPipeline();
        this.ensureInstanceCapacity(1024);
        this.ready = true;
    }

    compileShader(type, source) {
        const gl = this.gl;
        const shader = gl.createShader(type);
        if (!shader) {
            throw new Error('Failed to allocate shader');
        }
        gl.shaderSource(shader, source);
        gl.compileShader(shader);
        if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
            const shaderError = gl.getShaderInfoLog(shader) || 'unknown compile error';
            gl.deleteShader(shader);
            throw new Error(`Projectile shader compile failed: ${shaderError}`);
        }
        return shader;
    }

    createProgram(vertexSource, fragmentSource) {
        const gl = this.gl;
        const vertexShader = this.compileShader(gl.VERTEX_SHADER, vertexSource);
        const fragmentShader = this.compileShader(gl.FRAGMENT_SHADER, fragmentSource);
        const program = gl.createProgram();
        if (!program) {
            gl.deleteShader(vertexShader);
            gl.deleteShader(fragmentShader);
            throw new Error('Failed to create projectile WebGL2 program');
        }
        gl.attachShader(program, vertexShader);
        gl.attachShader(program, fragmentShader);
        gl.linkProgram(program);
        gl.deleteShader(vertexShader);
        gl.deleteShader(fragmentShader);
        if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
            const linkError = gl.getProgramInfoLog(program) || 'unknown link error';
            gl.deleteProgram(program);
            throw new Error(`Projectile shader link failed: ${linkError}`);
        }
        return program;
    }

    initPipeline() {
        const gl = this.gl;
        const vertexSource = `#version 300 es
precision highp float;
layout(location = 0) in vec2 aCorner;
layout(location = 1) in vec2 aWorldPos;
layout(location = 2) in float aSize;
layout(location = 3) in vec4 aColor;
uniform vec4 uView;
out vec4 vColor;
void main() {
  vec2 world = aWorldPos + aCorner * aSize;
  float nx = ((world.x - uView.x) / max(uView.z, 1.0)) * 2.0 - 1.0;
  float ny = 1.0 - ((world.y - uView.y) / max(uView.w, 1.0)) * 2.0;
  gl_Position = vec4(nx, ny, 0.0, 1.0);
  vColor = aColor;
}
`;
        const fragmentSource = `#version 300 es
precision mediump float;
in vec4 vColor;
out vec4 outColor;
void main() {
  outColor = vColor;
}
`;

        this.program = this.createProgram(vertexSource, fragmentSource);
        this.uniformViewLocation = gl.getUniformLocation(this.program, 'uView');
        this.vao = gl.createVertexArray();
        this.quadBuffer = gl.createBuffer();
        this.instanceBuffer = gl.createBuffer();

        if (!this.vao || !this.quadBuffer || !this.instanceBuffer) {
            throw new Error('Failed to allocate projectile WebGL2 buffers');
        }

        gl.bindVertexArray(this.vao);

        const quadVertices = new Float32Array([
            -1, -1,
             1, -1,
            -1,  1,
            -1,  1,
             1, -1,
             1,  1
        ]);
        gl.bindBuffer(gl.ARRAY_BUFFER, this.quadBuffer);
        gl.bufferData(gl.ARRAY_BUFFER, quadVertices, gl.STATIC_DRAW);
        gl.enableVertexAttribArray(0);
        gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 8, 0);
        gl.vertexAttribDivisor(0, 0);

        gl.bindBuffer(gl.ARRAY_BUFFER, this.instanceBuffer);
        gl.bufferData(gl.ARRAY_BUFFER, 1024 * 28, gl.DYNAMIC_DRAW);
        gl.enableVertexAttribArray(1);
        gl.vertexAttribPointer(1, 2, gl.FLOAT, false, 28, 0);
        gl.vertexAttribDivisor(1, 1);
        gl.enableVertexAttribArray(2);
        gl.vertexAttribPointer(2, 1, gl.FLOAT, false, 28, 8);
        gl.vertexAttribDivisor(2, 1);
        gl.enableVertexAttribArray(3);
        gl.vertexAttribPointer(3, 4, gl.FLOAT, false, 28, 12);
        gl.vertexAttribDivisor(3, 1);

        gl.bindVertexArray(null);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);
        this.instanceCapacity = 1024;
    }

    ensureInstanceCapacity(count) {
        if (!this.gl || !this.instanceBuffer) return;
        if (count <= this.instanceCapacity) return;
        let nextCapacity = Math.max(1024, this.instanceCapacity || 0);
        while (nextCapacity < count) {
            nextCapacity *= 2;
        }
        this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.instanceBuffer);
        this.gl.bufferData(this.gl.ARRAY_BUFFER, nextCapacity * 28, this.gl.DYNAMIC_DRAW);
        this.gl.bindBuffer(this.gl.ARRAY_BUFFER, null);
        this.instanceCapacity = nextCapacity;
    }

    resize(width, height) {
        if (!this.canvas) return;
        const w = Math.max(1, Math.floor(width));
        const h = Math.max(1, Math.floor(height));
        this.canvas.width = w;
        this.canvas.height = h;
    }

    render(viewBounds, instanceArray) {
        if (!this.ready || !this.gl || !this.program || !this.vao || !this.canvas) return;
        const gl = this.gl;
        const left = Number(viewBounds?.left) || 0;
        const top = Number(viewBounds?.top) || 0;
        const right = Number(viewBounds?.right) || left;
        const bottom = Number(viewBounds?.bottom) || top;
        const viewWidth = Math.max(1, right - left);
        const viewHeight = Math.max(1, bottom - top);

        const instanceCount = Math.max(0, Math.floor((instanceArray?.length || 0) / WEBGPU_PROJECTILE_INSTANCE_STRIDE));
        this.instanceCount = instanceCount;
        this.lastInstanceCount = instanceCount;

        gl.viewport(0, 0, this.canvas.width, this.canvas.height);
        gl.disable(gl.DEPTH_TEST);
        gl.enable(gl.BLEND);
        gl.blendEquationSeparate(gl.FUNC_ADD, gl.FUNC_ADD);
        gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE, gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
        gl.clearColor(0, 0, 0, 0);
        gl.clear(gl.COLOR_BUFFER_BIT);

        gl.useProgram(this.program);
        if (this.uniformViewLocation) {
            gl.uniform4f(this.uniformViewLocation, left, top, viewWidth, viewHeight);
        }
        gl.bindVertexArray(this.vao);

        if (instanceCount > 0) {
            this.ensureInstanceCapacity(instanceCount);
            gl.bindBuffer(gl.ARRAY_BUFFER, this.instanceBuffer);
            gl.bufferSubData(gl.ARRAY_BUFFER, 0, instanceArray);
            gl.drawArraysInstanced(gl.TRIANGLES, 0, 6, instanceCount);
        }

        gl.bindVertexArray(null);
        gl.useProgram(null);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);
    }

    clear(viewBounds) {
        this.render(viewBounds, EMPTY_INSTANCE_ARRAY);
    }

    destroy() {
        this.ready = false;
        if (this.gl) {
            if (this.program) this.gl.deleteProgram(this.program);
            if (this.quadBuffer) this.gl.deleteBuffer(this.quadBuffer);
            if (this.instanceBuffer) this.gl.deleteBuffer(this.instanceBuffer);
            if (this.vao) this.gl.deleteVertexArray(this.vao);
        }
        if (this.canvas && this.canvas.parentNode) {
            this.canvas.parentNode.removeChild(this.canvas);
        }
        this.canvas = null;
        this.gl = null;
        this.program = null;
        this.uniformViewLocation = null;
        this.vao = null;
        this.quadBuffer = null;
        this.instanceBuffer = null;
        this.instanceCapacity = 0;
        this.instanceCount = 0;
    }
}

class WebGL2PlayerLayer {
    constructor(hostElement) {
        this.hostElement = hostElement;
        this.backend = 'webgl2';
        this.canvas = null;
        this.gl = null;
        this.program = null;
        this.uniformViewLocation = null;
        this.vao = null;
        this.shipVertexBuffer = null;
        this.instanceBuffer = null;
        this.instanceCapacity = 0;
        this.instanceCount = 0;
        this.ready = false;
        this.lastError = null;
        this.lastInstanceCount = 0;
    }

    init(width, height) {
        if (!WEBGL2_SUPPORTED) {
            throw new Error('WebGL2 unavailable');
        }

        this.canvas = document.createElement('canvas');
        this.canvas.className = 'webgl2-player-layer';
        this.canvas.style.position = 'absolute';
        this.canvas.style.left = '0';
        this.canvas.style.top = '0';
        this.canvas.style.width = '100%';
        this.canvas.style.height = '100%';
        this.canvas.style.pointerEvents = 'none';
        this.canvas.style.zIndex = '4';
        const hostPosition = this.hostElement.style.position;
        if (!hostPosition || hostPosition === 'static') {
            this.hostElement.style.position = 'relative';
        }
        this.hostElement.appendChild(this.canvas);

        this.gl = this.canvas.getContext('webgl2', {
            alpha: true,
            antialias: false,
            depth: false,
            stencil: false,
            premultipliedAlpha: true,
            preserveDrawingBuffer: false,
            powerPreference: 'high-performance'
        });
        if (!this.gl) {
            throw new Error('Failed to acquire WebGL2 canvas context');
        }

        this.resize(width, height);
        this.initPipeline();
        this.ensureInstanceCapacity(512);
        this.ready = true;
    }

    compileShader(type, source) {
        const gl = this.gl;
        const shader = gl.createShader(type);
        if (!shader) {
            throw new Error('Failed to allocate shader');
        }
        gl.shaderSource(shader, source);
        gl.compileShader(shader);
        if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
            const shaderError = gl.getShaderInfoLog(shader) || 'unknown compile error';
            gl.deleteShader(shader);
            throw new Error(`Player shader compile failed: ${shaderError}`);
        }
        return shader;
    }

    createProgram(vertexSource, fragmentSource) {
        const gl = this.gl;
        const vertexShader = this.compileShader(gl.VERTEX_SHADER, vertexSource);
        const fragmentShader = this.compileShader(gl.FRAGMENT_SHADER, fragmentSource);
        const program = gl.createProgram();
        if (!program) {
            gl.deleteShader(vertexShader);
            gl.deleteShader(fragmentShader);
            throw new Error('Failed to create player WebGL2 program');
        }
        gl.attachShader(program, vertexShader);
        gl.attachShader(program, fragmentShader);
        gl.linkProgram(program);
        gl.deleteShader(vertexShader);
        gl.deleteShader(fragmentShader);
        if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
            const linkError = gl.getProgramInfoLog(program) || 'unknown link error';
            gl.deleteProgram(program);
            throw new Error(`Player shader link failed: ${linkError}`);
        }
        return program;
    }

    initPipeline() {
        const gl = this.gl;
        const vertexSource = `#version 300 es
precision highp float;
layout(location = 0) in vec2 aLocalPos;
layout(location = 1) in vec2 aWorldPos;
layout(location = 2) in float aRotation;
layout(location = 3) in float aSize;
layout(location = 4) in vec4 aColor;
uniform vec4 uView;
out vec4 vColor;
void main() {
  float c = cos(aRotation);
  float s = sin(aRotation);
  vec2 scaled = aLocalPos * aSize;
  vec2 rotated = vec2(
    scaled.x * c - scaled.y * s,
    scaled.x * s + scaled.y * c
  );
  vec2 world = aWorldPos + rotated;
  float nx = ((world.x - uView.x) / max(uView.z, 1.0)) * 2.0 - 1.0;
  float ny = 1.0 - ((world.y - uView.y) / max(uView.w, 1.0)) * 2.0;
  gl_Position = vec4(nx, ny, 0.0, 1.0);
  vColor = aColor;
}
`;
        const fragmentSource = `#version 300 es
precision mediump float;
in vec4 vColor;
out vec4 outColor;
void main() {
  outColor = vColor;
}
`;

        this.program = this.createProgram(vertexSource, fragmentSource);
        this.uniformViewLocation = gl.getUniformLocation(this.program, 'uView');
        this.vao = gl.createVertexArray();
        this.shipVertexBuffer = gl.createBuffer();
        this.instanceBuffer = gl.createBuffer();

        if (!this.vao || !this.shipVertexBuffer || !this.instanceBuffer) {
            throw new Error('Failed to allocate player WebGL2 buffers');
        }

        gl.bindVertexArray(this.vao);

        const shipVertices = new Float32Array([
            1.0, 0.0,
            -0.8, 0.68,
            -0.8, -0.68
        ]);
        gl.bindBuffer(gl.ARRAY_BUFFER, this.shipVertexBuffer);
        gl.bufferData(gl.ARRAY_BUFFER, shipVertices, gl.STATIC_DRAW);
        gl.enableVertexAttribArray(0);
        gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 8, 0);
        gl.vertexAttribDivisor(0, 0);

        gl.bindBuffer(gl.ARRAY_BUFFER, this.instanceBuffer);
        gl.bufferData(gl.ARRAY_BUFFER, 512 * 32, gl.DYNAMIC_DRAW);
        gl.enableVertexAttribArray(1);
        gl.vertexAttribPointer(1, 2, gl.FLOAT, false, 32, 0);
        gl.vertexAttribDivisor(1, 1);
        gl.enableVertexAttribArray(2);
        gl.vertexAttribPointer(2, 1, gl.FLOAT, false, 32, 8);
        gl.vertexAttribDivisor(2, 1);
        gl.enableVertexAttribArray(3);
        gl.vertexAttribPointer(3, 1, gl.FLOAT, false, 32, 12);
        gl.vertexAttribDivisor(3, 1);
        gl.enableVertexAttribArray(4);
        gl.vertexAttribPointer(4, 4, gl.FLOAT, false, 32, 16);
        gl.vertexAttribDivisor(4, 1);

        gl.bindVertexArray(null);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);
        this.instanceCapacity = 512;
    }

    ensureInstanceCapacity(count) {
        if (!this.gl || !this.instanceBuffer) return;
        if (count <= this.instanceCapacity) return;
        let nextCapacity = Math.max(512, this.instanceCapacity || 0);
        while (nextCapacity < count) {
            nextCapacity *= 2;
        }
        this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.instanceBuffer);
        this.gl.bufferData(this.gl.ARRAY_BUFFER, nextCapacity * 32, this.gl.DYNAMIC_DRAW);
        this.gl.bindBuffer(this.gl.ARRAY_BUFFER, null);
        this.instanceCapacity = nextCapacity;
    }

    resize(width, height) {
        if (!this.canvas) return;
        const w = Math.max(1, Math.floor(width));
        const h = Math.max(1, Math.floor(height));
        this.canvas.width = w;
        this.canvas.height = h;
    }

    render(viewBounds, instanceArray) {
        if (!this.ready || !this.gl || !this.program || !this.vao || !this.canvas) return;
        const gl = this.gl;
        const left = Number(viewBounds?.left) || 0;
        const top = Number(viewBounds?.top) || 0;
        const right = Number(viewBounds?.right) || left;
        const bottom = Number(viewBounds?.bottom) || top;
        const viewWidth = Math.max(1, right - left);
        const viewHeight = Math.max(1, bottom - top);

        const instanceCount = Math.max(0, Math.floor((instanceArray?.length || 0) / WEBGPU_PLAYER_INSTANCE_STRIDE));
        this.instanceCount = instanceCount;
        this.lastInstanceCount = instanceCount;

        gl.viewport(0, 0, this.canvas.width, this.canvas.height);
        gl.disable(gl.DEPTH_TEST);
        gl.enable(gl.BLEND);
        gl.blendEquationSeparate(gl.FUNC_ADD, gl.FUNC_ADD);
        gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
        gl.clearColor(0, 0, 0, 0);
        gl.clear(gl.COLOR_BUFFER_BIT);

        gl.useProgram(this.program);
        if (this.uniformViewLocation) {
            gl.uniform4f(this.uniformViewLocation, left, top, viewWidth, viewHeight);
        }
        gl.bindVertexArray(this.vao);

        if (instanceCount > 0) {
            this.ensureInstanceCapacity(instanceCount);
            gl.bindBuffer(gl.ARRAY_BUFFER, this.instanceBuffer);
            gl.bufferSubData(gl.ARRAY_BUFFER, 0, instanceArray);
            gl.drawArraysInstanced(gl.TRIANGLES, 0, 3, instanceCount);
        }

        gl.bindVertexArray(null);
        gl.useProgram(null);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);
    }

    clear(viewBounds) {
        this.render(viewBounds, EMPTY_INSTANCE_ARRAY);
    }

    destroy() {
        this.ready = false;
        if (this.gl) {
            if (this.program) this.gl.deleteProgram(this.program);
            if (this.shipVertexBuffer) this.gl.deleteBuffer(this.shipVertexBuffer);
            if (this.instanceBuffer) this.gl.deleteBuffer(this.instanceBuffer);
            if (this.vao) this.gl.deleteVertexArray(this.vao);
        }
        if (this.canvas && this.canvas.parentNode) {
            this.canvas.parentNode.removeChild(this.canvas);
        }
        this.canvas = null;
        this.gl = null;
        this.program = null;
        this.uniformViewLocation = null;
        this.vao = null;
        this.shipVertexBuffer = null;
        this.instanceBuffer = null;
        this.instanceCapacity = 0;
        this.instanceCount = 0;
    }
}


  return {
    WebGPUProjectileLayer,
    WebGPUPlayerLayer,
    WebGL2ProjectileLayer,
    WebGL2PlayerLayer,
    getAcceleratedLayerBackend,
    formatAcceleratedBackendLabel,
  };
}
