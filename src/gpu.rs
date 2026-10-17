// SPDX-License-Identifier: MIT OR Apache-2.0
//! GPU compute engine (optional `gpu` feature, built on wgpu compute shaders).
//!
//! The CPU code in `ops.rs` / `noise.rs` is the reference. A node is only sent to the GPU when this module has a
//! kernel for it, the parameters are inside what the kernel supports, the texture is big enough to be worth the
//! upload and read-back, and a usable adapter exists. In every other case the caller gets `None` and runs the CPU
//! code, so results never depend on whether a GPU is present (apart from float rounding, which the parity tests
//! bound). Without the feature every function here returns `None`.
//!
//! Environment: `MC_GPU=0` turns the engine off, `MC_GPU=force` also accepts software adapters (used by the tests),
//! `MC_GPU_MIN` sets the smallest pixel count that is sent to the GPU (default 262144 = 512 x 512).

use crate::tex::Tex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(true);
static MIN_PIXELS: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Turn GPU evaluation on or off at run time (the editor's switch).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Smallest texture (in pixels) worth sending to the GPU. 0 sends everything (tests).
pub fn set_min_pixels(n: usize) {
    MIN_PIXELS.store(n, Ordering::Relaxed);
}

fn min_pixels() -> usize {
    let v = MIN_PIXELS.load(Ordering::Relaxed);
    if v != usize::MAX {
        return v;
    }
    std::env::var("MC_GPU_MIN").ok().and_then(|s| s.parse().ok()).unwrap_or(512 * 512)
}

/// True when evaluation may use the GPU right now (part of the node cache key: the two paths differ by rounding).
pub fn active() -> bool {
    ENABLED.load(Ordering::Relaxed) && imp::global().is_some()
}

/// Name of the adapter in use, if any.
pub fn adapter_name() -> Option<String> {
    imp::global().map(|g| g.name())
}

fn usable(t_w: usize, t_h: usize) -> bool {
    ENABLED.load(Ordering::Relaxed) && t_w * t_h >= min_pixels() && imp::global().is_some()
}

/// Perlin (`fractal == false`) or fractal noise on the GPU. Returns a grayscale texture, or `None` for the CPU path.
#[allow(clippy::too_many_arguments)]
pub fn noise(w: usize, h: usize, fractal: bool, px: i32, py: i32, octaves: u32, gain: f32, seed: u32, mode: u32) -> Option<Tex> {
    if !usable(w, h) {
        return None;
    }
    imp::global()?.noise(w, h, fractal, px, py, octaves, gain, seed, mode)
}

/// Gaussian blur with wrap-around edges (the three-box approximation used for sigma >= 2.5).
pub fn blur(t: &Tex, sigma: f32) -> Option<Tex> {
    if !usable(t.w, t.h) || sigma < 2.5 {
        return None;
    }
    imp::global()?.blur(t, sigma)
}

/// Normal map from a height map (3x3 Scharr, wrap-around edges).
pub fn normal_from_height(t: &Tex, strength: f32, directx: bool) -> Option<Tex> {
    if !usable(t.w, t.h) {
        return None;
    }
    imp::global()?.normal(t, strength, directx)
}

#[cfg(not(feature = "gpu"))]
mod imp {
    use crate::tex::Tex;
    pub struct Gpu;
    pub fn global() -> Option<&'static Gpu> {
        None
    }
    impl Gpu {
        pub fn name(&self) -> String {
            String::new()
        }
        #[allow(clippy::too_many_arguments)]
        pub fn noise(&self, _: usize, _: usize, _: bool, _: i32, _: i32, _: u32, _: f32, _: u32, _: u32) -> Option<Tex> {
            None
        }
        pub fn blur(&self, _: &Tex, _: f32) -> Option<Tex> {
            None
        }
        pub fn normal(&self, _: &Tex, _: f32, _: bool) -> Option<Tex> {
            None
        }
    }
}

#[cfg(feature = "gpu")]
mod imp {
    use crate::tex::Tex;
    use std::borrow::Cow;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    pub struct Gpu {
        dev: wgpu::Device,
        queue: wgpu::Queue,
        info: wgpu::AdapterInfo,
        max_bytes: u64,
        pipes: Mutex<HashMap<&'static str, std::sync::Arc<wgpu::ComputePipeline>>>,
        grad: wgpu::Buffer,
    }

    pub fn global() -> Option<&'static Gpu> {
        static G: OnceLock<Option<Gpu>> = OnceLock::new();
        G.get_or_init(Gpu::create).as_ref()
    }

    /// 32-bit words of a uniform block.
    #[derive(Default)]
    struct Uni(Vec<u8>);
    impl Uni {
        fn u(mut self, v: u32) -> Uni {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        fn i(mut self, v: i32) -> Uni {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        fn f(mut self, v: f32) -> Uni {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        fn done(mut self) -> Vec<u8> {
            while self.0.len() % 16 != 0 || self.0.is_empty() {
                self.0.push(0);
            }
            self.0
        }
    }

    fn px_bytes(p: &[[f32; 4]]) -> &[u8] {
        // SAFETY: [f32; 4] is plain old data with no padding; the slice covers exactly the same memory
        unsafe { std::slice::from_raw_parts(p.as_ptr() as *const u8, std::mem::size_of_val(p)) }
    }

    impl Gpu {
        fn create() -> Option<Gpu> {
            let mode = std::env::var("MC_GPU").unwrap_or_default().to_lowercase();
            if mode == "0" || mode == "off" {
                return None;
            }
            let force = mode == "force";
            let inst = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::all(), ..Default::default() });
            let mut best: Option<wgpu::Adapter> = None;
            let mut rank = 0;
            for a in inst.enumerate_adapters(wgpu::Backends::all()) {
                let info = a.get_info();
                let r = match info.device_type {
                    wgpu::DeviceType::DiscreteGpu => 4,
                    wgpu::DeviceType::IntegratedGpu => 3,
                    wgpu::DeviceType::VirtualGpu => 2,
                    _ if force => 1,
                    _ => 0, // software rendering is never faster than the CPU code
                };
                // compute shaders need storage buffers in the compute stage; skip adapters without them
                if r > rank && a.limits().max_storage_buffers_per_shader_stage >= 4 {
                    rank = r;
                    best = Some(a);
                }
            }
            let adapter = best?;
            let info = adapter.get_info();
            let lim = adapter.limits();
            let (dev, queue) = pollster::block_on(adapter.request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("materialcraft"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits {
                        max_storage_buffer_binding_size: lim.max_storage_buffer_binding_size,
                        max_buffer_size: lim.max_buffer_size,
                        ..wgpu::Limits::downlevel_defaults()
                    },
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            ))
            .ok()?;
            let max_bytes = lim.max_storage_buffer_binding_size.min(lim.max_buffer_size as u32) as u64;
            // the same 4096-angle gradient table the CPU noise uses, so both paths pick identical gradients
            let tab: Vec<[f32; 2]> = (0..4096).map(|i| {
                let a = i as f32 / 4096.0 * std::f32::consts::TAU;
                [a.cos(), a.sin()]
            }).collect();
            let grad = dev.create_buffer(&wgpu::BufferDescriptor { label: Some("grad"), size: 4096 * 8, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            // SAFETY: [f32; 2] is plain old data
            let bytes = unsafe { std::slice::from_raw_parts(tab.as_ptr() as *const u8, tab.len() * 8) };
            queue.write_buffer(&grad, 0, bytes);
            Some(Gpu { dev, queue, info, max_bytes, pipes: Mutex::new(HashMap::new()), grad })
        }

        pub fn name(&self) -> String {
            format!("{} ({:?})", self.info.name, self.info.backend)
        }

        fn pipeline(&self, name: &'static str, src: &str) -> std::sync::Arc<wgpu::ComputePipeline> {
            let mut m = self.pipes.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(p) = m.get(name) {
                return p.clone();
            }
            let module = self.dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(name), source: wgpu::ShaderSource::Wgsl(Cow::Owned(src.to_string())) });
            let p = std::sync::Arc::new(self.dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(name),
                layout: None,
                module: &module,
                entry_point: "main",
                compilation_options: Default::default(),
                cache: None,
            }));
            m.insert(name, p.clone());
            p
        }

        fn buf(&self, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
            self.dev.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage, mapped_at_creation: false })
        }

        fn fits(&self, w: usize, h: usize) -> bool {
            (w * h * 16) as u64 <= self.max_bytes && w <= 65535 * 8 && h <= 65535 * 8
        }

        fn read(&self, out: &wgpu::Buffer, w: usize, h: usize, enc: wgpu::CommandEncoder) -> Option<Tex> {
            let bytes = (w * h * 16) as u64;
            let stage = self.buf(bytes, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
            let mut enc = enc;
            enc.copy_buffer_to_buffer(out, 0, &stage, 0, bytes);
            self.queue.submit(Some(enc.finish()));
            let slice = stage.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            self.dev.poll(wgpu::Maintain::Wait);
            rx.recv().ok()?.ok()?;
            let data = slice.get_mapped_range();
            let mut px = vec![[0.0f32; 4]; w * h];
            // SAFETY: the destination has exactly `bytes` bytes of plain f32 data
            unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), px.as_mut_ptr() as *mut u8, bytes as usize) };
            drop(data);
            stage.unmap();
            for p in px.iter_mut() {
                for c in p.iter_mut() {
                    if !c.is_finite() {
                        *c = 0.0;
                    }
                }
            }
            Some(Tex { w, h, px })
        }

        fn dispatch(&self, enc: &mut wgpu::CommandEncoder, pipe: &wgpu::ComputePipeline, entries: &[wgpu::BindGroupEntry], w: usize, h: usize) {
            let bg = self.dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &pipe.get_bind_group_layout(0), entries });
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(((w + 7) / 8) as u32, ((h + 7) / 8) as u32, 1);
        }

        fn uniform(&self, data: &[u8]) -> wgpu::Buffer {
            let b = self.buf(data.len() as u64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
            self.queue.write_buffer(&b, 0, data);
            b
        }

        #[allow(clippy::too_many_arguments)]
        pub fn noise(&self, w: usize, h: usize, fractal: bool, px: i32, py: i32, octaves: u32, gain: f32, seed: u32, mode: u32) -> Option<Tex> {
            if !self.fits(w, h) {
                return None;
            }
            let pipe = self.pipeline("noise", NOISE_WGSL);
            let uni = self.uniform(&Uni::default().u(w as u32).u(h as u32).i(px.max(1)).i(py.max(1)).u(octaves).u(seed).u(mode).u(fractal as u32).f(gain).done());
            let out = self.buf((w * h * 16) as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
            let mut enc = self.dev.create_command_encoder(&Default::default());
            self.dispatch(&mut enc, &pipe, &[
                wgpu::BindGroupEntry { binding: 0, resource: uni.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.grad.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: out.as_entire_binding() },
            ], w, h);
            self.read(&out, w, h, enc)
        }

        pub fn blur(&self, t: &Tex, sigma: f32) -> Option<Tex> {
            let (w, h) = (t.w, t.h);
            if !self.fits(w, h) {
                return None;
            }
            let pipe = self.pipeline("box", BOX_WGSL);
            let bytes = (w * h * 16) as u64;
            let a = self.buf(bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC);
            let b = self.buf(bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
            self.queue.write_buffer(&a, 0, px_bytes(&t.px));
            let mut enc = self.dev.create_command_encoder(&Default::default());
            let mut unis = Vec::new();
            let (mut src, mut dst) = (&a, &b);
            // three box blurs, each a horizontal then a vertical pass (same radii as the CPU path)
            for bw in crate::ops::gauss_boxes(sigma) {
                let r0 = (bw / 2).max(0);
                for dir in 0..2u32 {
                    let r = if dir == 0 { r0.min((w as i64 - 1) / 2) } else { r0.min((h as i64 - 1) / 2) };
                    let uni = self.uniform(&Uni::default().u(w as u32).u(h as u32).i(r as i32).u(dir).done());
                    self.dispatch(&mut enc, &pipe, &[
                        wgpu::BindGroupEntry { binding: 0, resource: uni.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: src.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: dst.as_entire_binding() },
                    ], w, h);
                    unis.push(uni);
                    std::mem::swap(&mut src, &mut dst);
                }
            }
            // after the last swap the newest result is in `src`
            let res = self.read(src, w, h, enc);
            drop(unis);
            res
        }

        pub fn normal(&self, t: &Tex, strength: f32, directx: bool) -> Option<Tex> {
            let (w, h) = (t.w, t.h);
            if !self.fits(w, h) {
                return None;
            }
            let pipe = self.pipeline("normal", NORMAL_WGSL);
            let inb = self.buf((w * h * 16) as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
            self.queue.write_buffer(&inb, 0, px_bytes(&t.px));
            let out = self.buf((w * h * 16) as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
            let k = strength * w as f32 * 0.05;
            let uni = self.uniform(&Uni::default().u(w as u32).u(h as u32).f(k).u(directx as u32).done());
            let mut enc = self.dev.create_command_encoder(&Default::default());
            self.dispatch(&mut enc, &pipe, &[
                wgpu::BindGroupEntry { binding: 0, resource: uni.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: inb.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: out.as_entire_binding() },
            ], w, h);
            self.read(&out, w, h, enc)
        }
    }

    const NOISE_WGSL: &str = r#"
struct U { w: u32, h: u32, px: i32, py: i32, octaves: u32, seed: u32, mode: u32, fractal: u32, gain: f32, a: f32, b: f32, c: f32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> grad: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read_write> outp: array<vec4<f32>>;

fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    var h = (bitcast<u32>(x) * 0x27d4eb2du) ^ (bitcast<u32>(y) * 0x165667b1u) ^ (seed * 0x9e3779b9u);
    h = h ^ (h >> 15u);
    h = h * 0x85ebca6bu;
    h = h ^ (h >> 13u);
    h = h * 0xc2b2ae35u;
    h = h ^ (h >> 16u);
    return h;
}
fn wrapi(a: i32, m: i32) -> i32 { return ((a % m) + m) % m; }
fn fade(t: f32) -> f32 { return t * t * t * (t * (t * 6.0 - 15.0) + 10.0); }
fn lerp(a: f32, b: f32, t: f32) -> f32 { return a + (b - a) * t; }
fn gdot(cx: i32, cy: i32, dx: f32, dy: f32, seed: u32) -> f32 {
    let c = grad[hash2(wrapi(cx, u.px), wrapi(cy, u.py), seed) >> 20u];
    return c.x * dx + c.y * dy;
}
fn perlin(uu: f32, vv: f32, seed: u32) -> f32 {
    let x = uu * f32(u.px);
    let y = vv * f32(u.py);
    let fxl = floor(x);
    let fyl = floor(y);
    let ix = i32(fxl);
    let iy = i32(fyl);
    let fx = x - fxl;
    let fy = y - fyl;
    let n00 = gdot(ix, iy, fx, fy, seed);
    let n10 = gdot(ix + 1, iy, fx - 1.0, fy, seed);
    let n01 = gdot(ix, iy + 1, fx, fy - 1.0, seed);
    let n11 = gdot(ix + 1, iy + 1, fx - 1.0, fy - 1.0, seed);
    let sx = fade(fx);
    let sy = fade(fy);
    let n = lerp(lerp(n00, n10, sx), lerp(n01, n11, sx), sy);
    return clamp(n * 0.70710678 + 0.5, 0.0, 1.0);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.w || id.y >= u.h) { return; }
    let uu = (f32(id.x) + 0.5) / f32(u.w);
    let vv = (f32(id.y) + 0.5) / f32(u.h);
    var g: f32;
    if (u.fractal == 0u) {
        g = perlin(uu, vv, u.seed);
    } else {
        var a = uu;
        var b = vv;
        var amp = 1.0;
        var sum = 0.0;
        var norm = 0.0;
        let oct = clamp(u.octaves, 1u, 12u);
        for (var o = 0u; o < oct; o = o + 1u) {
            let n = perlin(a, b, u.seed + o * 131u);
            var m = n;
            if (u.mode == 1u) { m = 1.0 - abs(2.0 * n - 1.0); }
            else if (u.mode == 2u) { m = abs(2.0 * n - 1.0); }
            sum = sum + m * amp;
            norm = norm + amp;
            amp = amp * u.gain;
            let na = 2.0 * a - b;
            let nb = a + 2.0 * b;
            a = na - floor(na);
            b = nb - floor(nb);
        }
        if (norm > 0.0) {
            let m = sum / norm;
            var k = 1.35;
            if (u.mode == 0u) { k = 1.7; }
            g = clamp(0.5 + (m - 0.5) * k, 0.0, 1.0);
        } else {
            g = 0.0;
        }
    }
    outp[id.y * u.w + id.x] = vec4<f32>(g, g, g, 1.0);
}
"#;

    const BOX_WGSL: &str = r#"
struct U { w: u32, h: u32, r: i32, dir: u32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> dst: array<vec4<f32>>;
fn wrapi(a: i32, m: i32) -> i32 {
    // the offset never exceeds half the size, so one correction is enough (and avoids signed remainders)
    if (a < 0) { return a + m; }
    if (a >= m) { return a - m; }
    return a;
}
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.w || id.y >= u.h) { return; }
    let x = i32(id.x);
    let y = i32(id.y);
    let w = i32(u.w);
    let h = i32(u.h);
    var acc = vec4<f32>(0.0);
    for (var k = -u.r; k <= u.r; k = k + 1) {
        if (u.dir == 0u) {
            acc = acc + src[u32(y * w + wrapi(x + k, w))];
        } else {
            acc = acc + src[u32(wrapi(y + k, h) * w + x)];
        }
    }
    dst[id.y * u.w + id.x] = acc * (1.0 / f32(2 * u.r + 1));
}
"#;

    const NORMAL_WGSL: &str = r#"
struct U { w: u32, h: u32, k: f32, directx: u32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> dst: array<vec4<f32>>;
fn lum(p: vec4<f32>) -> f32 { return 0.2126 * p.x + 0.7152 * p.y + 0.0722 * p.z; }
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.w || id.y >= u.h) { return; }
    let x = id.x;
    let y = id.y;
    var xm = x - 1u; if (x == 0u) { xm = u.w - 1u; }
    var xp = x + 1u; if (x + 1u == u.w) { xp = 0u; }
    var ym = y - 1u; if (y == 0u) { ym = u.h - 1u; }
    var yp = y + 1u; if (y + 1u == u.h) { yp = 0u; }
    let g = array<f32, 9>(
        lum(src[ym * u.w + xm]), lum(src[ym * u.w + x]), lum(src[ym * u.w + xp]),
        lum(src[y * u.w + xm]),  lum(src[y * u.w + x]),  lum(src[y * u.w + xp]),
        lum(src[yp * u.w + xm]), lum(src[yp * u.w + x]), lum(src[yp * u.w + xp]));
    // g indices: row 0 = ym, row 1 = y, row 2 = yp; column 0 = xm, 1 = x, 2 = xp
    let dx = (3.0 * (g[2] - g[0]) + 10.0 * (g[5] - g[3]) + 3.0 * (g[8] - g[6])) / 32.0 * u.k;
    var dy = (3.0 * (g[6] - g[0]) + 10.0 * (g[7] - g[1]) + 3.0 * (g[8] - g[2])) / 32.0 * u.k;
    if (u.directx != 0u) { dy = -dy; }
    let n = vec3<f32>(-dx, dy, 1.0);
    let l = length(n);
    let o = n / l * 0.5 + vec3<f32>(0.5);
    dst[id.y * u.w + id.x] = vec4<f32>(o.x, o.y, o.z, 1.0);
}
"#;
}
