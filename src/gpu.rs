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

/// Levels (input range, gamma, output range) on the GPU.
pub fn levels(t: &Tex, in_lo: f32, in_hi: f32, gamma: f32, out_lo: f32, out_hi: f32) -> Option<Tex> {
    if !usable(t.w, t.h) {
        return None;
    }
    imp::global()?.levels(t, in_lo, in_hi, gamma, out_lo, out_hi)
}

/// Directional blur (24 taps along a line, wrap-around, bilinear).
pub fn directional_blur(t: &Tex, angle: f32, length: f32) -> Option<Tex> {
    if !usable(t.w, t.h) {
        return None;
    }
    imp::global()?.directional_blur(t, angle, length)
}

/// Blend of `fg` over `bg` for the colour blend modes (not Dissolve), with the "ignore alpha" and "use source alpha"
/// alpha modes and no cropping; every input must have the same size. `None` leaves the other cases to the CPU.
pub fn blend(fg: &Tex, bg: &Tex, mask: Option<&Tex>, mode: u32, opacity: f32, alpha_mode: u32) -> Option<Tex> {
    if !usable(bg.w, bg.h) || mode == 19 || alpha_mode > 3 {
        return None;
    }
    if (fg.w, fg.h) != (bg.w, bg.h) || mask.map(|m| (m.w, m.h) != (bg.w, bg.h)).unwrap_or(false) {
        return None;
    }
    imp::global()?.blend(fg, bg, mask, mode, opacity, alpha_mode)
}

/// Spin (`mode` 0, `amt` = arc in radians) or zoom (`mode` 1, `amt` = amount) blur around the centre (`cx`, `cy`).
#[allow(clippy::too_many_arguments)]
pub fn radial_blur(t: &Tex, mode: u32, taps: u32, cx: f32, cy: f32, amt: f32, rad: f32) -> Option<Tex> {
    if !usable(t.w, t.h) {
        return None;
    }
    imp::global()?.radial_blur(t, mode, taps, cx, cy, amt, rad)
}

/// Horizon-based bent normal (`want_ao` false) or occlusion (`want_ao` true) of a height map.
#[allow(clippy::too_many_arguments)]
pub fn bent_normal(t: &Tex, dirs: u32, steps: u32, rad: f32, relief: f32, directx: bool, want_ao: bool) -> Option<Tex> {
    if !usable(t.w, t.h) {
        return None;
    }
    imp::global()?.bent_normal(t, dirs, steps, rad, relief, directx, want_ao)
}

/// Edge-preserving blur of `t` guided by `guide` (same size).
pub fn bilateral_blur(t: &Tex, guide: &Tex, rad: i32, range: f32) -> Option<Tex> {
    if !usable(t.w, t.h) || (guide.w, guide.h) != (t.w, t.h) || rad < 1 {
        return None;
    }
    imp::global()?.bilateral_blur(t, guide, rad, range)
}

/// Blur of `t` along the gradient of `slope` (same size); mode 0 average, 1 minimum, 2 maximum.
pub fn slope_blur(t: &Tex, slope: &Tex, samples: u32, intensity: f32, mode: u32) -> Option<Tex> {
    if !usable(t.w, t.h) || (slope.w, slope.h) != (t.w, t.h) {
        return None;
    }
    imp::global()?.slope_blur(t, slope, samples, intensity, mode)
}

/// Iterated extrusion: `steps` passes of `cur = max(m, sample(cur, p - (dx, dy)) - drop)` starting from the luminance of
/// `t`; returns the final plane as a grey texture.
pub fn extrude(t: &Tex, steps: u32, dx: f32, dy: f32, drop: f32) -> Option<Tex> {
    if !usable(t.w, t.h) || steps == 0 {
        return None;
    }
    imp::global()?.extrude(t, steps, dx, dy, drop)
}

/// How a warp reads its displacement from the map `w`.
#[derive(Clone, Copy)]
pub enum WarpKind {
    /// gradient of the map's luminance (central differences) times the intensity
    Slope,
    /// `(lum - offset) * intensity` along the direction (`c`, `s`)
    Directional { c: f32, s: f32, offset: f32 },
    /// red and green as a vector; `gs` flips the green axis
    Vector { gs: f32 },
}

/// Displace `t` by the map `w` (same size). `filtering`: 0 bilinear, 1 nearest, 2 bicubic.
pub fn warp(t: &Tex, w: &Tex, kind: WarpKind, intensity: f32, filtering: u32) -> Option<Tex> {
    if !usable(t.w, t.h) || (w.w, w.h) != (t.w, t.h) {
        return None;
    }
    imp::global()?.warp(t, w, kind, intensity, filtering)
}

/// Streak noise (see the directional noise node): `taps` samples along the direction (`ca`, `sa`), streak length `len`.
#[allow(clippy::too_many_arguments)]
pub fn directional_noise(w: usize, h: usize, p1: i32, taps: u32, ca: f32, sa: f32, len: f32, gain: f32, seed: u32) -> Option<Tex> {
    if !usable(w, h) {
        return None;
    }
    imp::global()?.dir_noise(w, h, p1, taps, ca, sa, len, gain, seed)
}

/// A mesh uploaded for ray tracing on the GPU (the baker's scene): the BVH words from `Bvh::export`, one allow-mask per
/// low-poly object (bit set = the high-poly object may be hit) and the back-face switch for primary rays.
pub struct SceneData {
    pub nodes: Vec<[u32; 4]>,
    pub tris: Vec<[u32; 4]>,
    /// `words` u32 per mask; empty when no object has a mask
    pub masks: Vec<u32>,
    pub words: u32,
    pub cull_back: bool,
}

/// Parameters of the hemisphere kernels.
#[derive(Clone, Copy)]
pub struct HemiParams {
    /// rays per sample
    pub rays: u32,
    pub dist: f32,
    /// ambient occlusion distance falloff exponent (0 = none)
    pub falloff: f32,
    /// false: ambient occlusion and bent normal (occlusion sum, direction sum); true: thickness (distance sum)
    pub thickness: bool,
    /// first of two hash seeds (the second is `seed + 1`); sub-sample index is added by the kernel
    pub seed: u32,
}

/// A scene on the GPU. `None` from `BakeScene::new` means the baker should use the CPU.
pub struct BakeScene(imp::Scene);

impl BakeScene {
    pub fn new(d: &SceneData) -> Option<BakeScene> {
        if !ENABLED.load(Ordering::Relaxed) {
            return None;
        }
        imp::global()?.scene(d).map(BakeScene)
    }
    pub fn adapter() -> Option<String> {
        if ENABLED.load(Ordering::Relaxed) { adapter_name() } else { None }
    }
    /// Closest hits. A ray is three `[u32; 4]`: `o.xyz, tmin` / `d.xyz, tmax` / `mask index (u32::MAX = any), 0, 0, 0`
    /// (floats as bits). A hit is `[t bits, triangle, u bits, v bits]`; a miss has t < 0.
    pub fn trace(&self, rays: &[[u32; 4]]) -> Option<Vec<[u32; 4]>> {
        imp::global()?.trace(&self.0, rays)
    }
    /// Cosine-weighted hemisphere rays per sample. A sample is two `[u32; 4]`: `origin.xyz, x` / `normal.xyz, y | sub << 16`.
    /// The result per sample is `[sum, dir.x, dir.y, dir.z]` as floats (occlusion) or `[sum, 0, 0, 0]` (thickness).
    pub fn hemi(&self, samples: &[[u32; 4]], p: HemiParams) -> Option<Vec<[f32; 4]>> {
        imp::global()?.hemi(&self.0, samples, p)
    }
}

#[cfg(not(feature = "gpu"))]
mod imp {
    use crate::tex::Tex;
    pub struct Gpu;
    pub struct Scene;
    pub fn global() -> Option<&'static Gpu> {
        None
    }
    impl Gpu {
        pub fn scene(&self, _: &super::SceneData) -> Option<Scene> {
            None
        }
        pub fn trace(&self, _: &Scene, _: &[[u32; 4]]) -> Option<Vec<[u32; 4]>> {
            None
        }
        pub fn hemi(&self, _: &Scene, _: &[[u32; 4]], _: super::HemiParams) -> Option<Vec<[f32; 4]>> {
            None
        }
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
        pub fn levels(&self, _: &Tex, _: f32, _: f32, _: f32, _: f32, _: f32) -> Option<Tex> {
            None
        }
        pub fn directional_blur(&self, _: &Tex, _: f32, _: f32) -> Option<Tex> {
            None
        }
        pub fn blend(&self, _: &Tex, _: &Tex, _: Option<&Tex>, _: u32, _: f32, _: u32) -> Option<Tex> {
            None
        }
        pub fn radial_blur(&self, _: &Tex, _: u32, _: u32, _: f32, _: f32, _: f32, _: f32) -> Option<Tex> {
            None
        }
        pub fn bent_normal(&self, _: &Tex, _: u32, _: u32, _: f32, _: f32, _: bool, _: bool) -> Option<Tex> {
            None
        }
        pub fn bilateral_blur(&self, _: &Tex, _: &Tex, _: i32, _: f32) -> Option<Tex> {
            None
        }
        pub fn extrude(&self, _: &Tex, _: u32, _: f32, _: f32, _: f32) -> Option<Tex> {
            None
        }
        #[allow(clippy::too_many_arguments)]
        pub fn dir_noise(&self, _: usize, _: usize, _: i32, _: u32, _: f32, _: f32, _: f32, _: f32, _: u32) -> Option<Tex> {
            None
        }
        pub fn warp(&self, _: &Tex, _: &Tex, _: super::WarpKind, _: f32, _: u32) -> Option<Tex> {
            None
        }
        pub fn slope_blur(&self, _: &Tex, _: &Tex, _: u32, _: f32, _: u32) -> Option<Tex> {
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
                        // the baker kernels bind five storage buffers (mesh nodes, triangles, masks, input, output)
                        max_storage_buffers_per_shader_stage: lim.max_storage_buffers_per_shader_stage.min(8),
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

        #[allow(clippy::too_many_arguments)]
        pub fn dir_noise(&self, w: usize, h: usize, p1: i32, taps: u32, ca: f32, sa: f32, len: f32, gain: f32, seed: u32) -> Option<Tex> {
            if !self.fits(w, h) {
                return None;
            }
            let helpers = NOISE_WGSL.split("@compute").next().unwrap_or("");
            let src = format!("{}{}", helpers, DIRNOISE_MAIN_WGSL);
            let pipe = self.pipeline("dirnoise", &src);
            let uni = self.uniform(&Uni::default().u(w as u32).u(h as u32).i(p1.max(1)).i(p1.max(1)).u(taps).u(seed).u(0).u(0).f(gain).f(ca).f(sa).f(len).done());
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

    impl Gpu {
        /// Run `src` once per pixel over `inputs` (bindings 1..=n, storage read) and read the result back.
        fn map_pixels(&self, name: &'static str, src: &str, uni: Vec<u8>, inputs: &[&Tex]) -> Option<Tex> {
            let (w, h) = (inputs.first()?.w, inputs.first()?.h);
            if !self.fits(w, h) || inputs.iter().any(|t| (t.w, t.h) != (w, h)) {
                return None;
            }
            let pipe = self.pipeline(name, src);
            let bytes = (w * h * 16) as u64;
            let ubuf = self.uniform(&uni);
            let bufs: Vec<wgpu::Buffer> = inputs
                .iter()
                .map(|t| {
                    let b = self.buf(bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
                    self.queue.write_buffer(&b, 0, px_bytes(&t.px));
                    b
                })
                .collect();
            let out = self.buf(bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
            let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }];
            for (i, b) in bufs.iter().enumerate() {
                entries.push(wgpu::BindGroupEntry { binding: 1 + i as u32, resource: b.as_entire_binding() });
            }
            entries.push(wgpu::BindGroupEntry { binding: 1 + bufs.len() as u32, resource: out.as_entire_binding() });
            let mut enc = self.dev.create_command_encoder(&Default::default());
            self.dispatch(&mut enc, &pipe, &entries, w, h);
            self.read(&out, w, h, enc)
        }

        pub fn levels(&self, t: &Tex, in_lo: f32, in_hi: f32, gamma: f32, out_lo: f32, out_hi: f32) -> Option<Tex> {
            let d = if in_hi >= in_lo { (in_hi - in_lo).max(1e-6) } else { (in_hi - in_lo).min(-1e-6) };
            let g = 1.0 / gamma.max(1e-3);
            let uni = Uni::default().u(t.w as u32).u(t.h as u32).f(in_lo).f(d).f(g).f(out_lo).f(out_hi).done();
            self.map_pixels("levels", LEVELS_WGSL, uni, &[t])
        }

        pub fn directional_blur(&self, t: &Tex, angle: f32, length: f32) -> Option<Tex> {
            let (s, c) = (angle * std::f32::consts::TAU).sin_cos();
            let uni = Uni::default().u(t.w as u32).u(t.h as u32).f(c).f(s).f(length).done();
            self.map_pixels("dirblur", DIRBLUR_WGSL, uni, &[t])
        }

        pub fn blend(&self, fg: &Tex, bg: &Tex, mask: Option<&Tex>, mode: u32, opacity: f32, alpha_mode: u32) -> Option<Tex> {
            let uni = Uni::default().u(bg.w as u32).u(bg.h as u32).u(mode).u(alpha_mode).f(opacity).u(mask.is_some() as u32).done();
            self.map_pixels("blend", BLEND_WGSL, uni, &[fg, bg, mask.unwrap_or(fg)])
        }

        #[allow(clippy::too_many_arguments)]
        pub fn radial_blur(&self, t: &Tex, mode: u32, taps: u32, cx: f32, cy: f32, amt: f32, rad: f32) -> Option<Tex> {
            let uni = Uni::default().u(t.w as u32).u(t.h as u32).u(mode).u(taps).f(cx).f(cy).f(amt).f(rad).done();
            let src = format!("{}{}", RADIAL_WGSL, samp_fn("samp", "src"));
            self.map_pixels("radial", &src, uni, &[t])
        }

        #[allow(clippy::too_many_arguments)]
        pub fn bent_normal(&self, t: &Tex, dirs: u32, steps: u32, rad: f32, relief: f32, directx: bool, want_ao: bool) -> Option<Tex> {
            let aspect = t.w as f32 / t.h as f32;
            let ysign = if directx { 1.0 } else { -1.0 };
            let uni = Uni::default().u(t.w as u32).u(t.h as u32).u(dirs).u(steps).f(rad).f(relief).f(ysign).u(want_ao as u32).f(aspect).done();
            let src = format!("{}{}", BENT_WGSL, samp_fn("samp", "src"));
            self.map_pixels("bent", &src, uni, &[t])
        }

        pub fn bilateral_blur(&self, t: &Tex, guide: &Tex, rad: i32, range: f32) -> Option<Tex> {
            let ss = (rad as f32 * 0.5).max(0.5);
            let uni = Uni::default().u(t.w as u32).u(t.h as u32).i(rad).f(ss).f(range).done();
            self.map_pixels("bilateral", BILATERAL_WGSL, uni, &[t, guide])
        }

        pub fn warp(&self, t: &Tex, w: &Tex, kind: super::WarpKind, intensity: f32, filtering: u32) -> Option<Tex> {
            use super::WarpKind::*;
            let (k, a, b, c) = match kind {
                Slope => (0u32, 0.0, 0.0, 0.0),
                Directional { c, s, offset } => (1, c, s, offset),
                Vector { gs } => (2, gs, 0.0, 0.0),
            };
            let uni = Uni::default().u(t.w as u32).u(t.h as u32).u(k).u(filtering.min(2)).f(intensity).f(a).f(b).f(c).done();
            let src = format!("{}{}", WARP_WGSL, samp_fn("samp", "src"));
            self.map_pixels("warp", &src, uni, &[t, w])
        }

        /// Many dependent passes on ping-pong buffers inside one submission (no readback between the passes).
        pub fn extrude(&self, t: &Tex, steps: u32, dx: f32, dy: f32, drop: f32) -> Option<Tex> {
            let (w, h) = (t.w, t.h);
            if !self.fits(w, h) || !self.fits(w, h * 3) {
                return None;
            }
            let pipe = self.pipeline("extrude", EXTRUDE_WGSL);
            let bytes = (w * h * 16) as u64;
            let src = self.buf(bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
            self.queue.write_buffer(&src, 0, px_bytes(&t.px));
            let ping = [self.buf(bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC), self.buf(bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC)];
            let mk = |first: u32| self.uniform(&Uni::default().u(w as u32).u(h as u32).u(first).f(dx).f(dy).f(drop).done());
            let (u_first, u_rest) = (mk(1), mk(0));
            let mut enc = self.dev.create_command_encoder(&Default::default());
            for i in 0..steps as usize {
                let (prev, out) = (&ping[(i + 1) % 2], &ping[i % 2]);
                let ub = if i == 0 { &u_first } else { &u_rest };
                self.dispatch(&mut enc, &pipe, &[
                    wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: src.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: prev.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: out.as_entire_binding() },
                ], w, h);
            }
            self.read(&ping[(steps as usize - 1) % 2], w, h, enc)
        }

        pub fn slope_blur(&self, t: &Tex, slope: &Tex, samples: u32, intensity: f32, mode: u32) -> Option<Tex> {
            let gu = Uni::default().u(slope.w as u32).u(slope.h as u32).done();
            let gv = self.map_pixels("slope_grad", SLOPE_GRAD_WGSL, gu, &[slope])?;
            let n = samples.clamp(1, 64);
            let uni = Uni::default().u(t.w as u32).u(t.h as u32).u(n).u(mode).f(intensity / n as f32).f(slope.w as f32 * 0.5).done();
            let src = format!("{}{}{}", SLOPE_WGSL, samp_fn("samp_t", "src"), samp_fn("samp_g", "gv"));
            self.map_pixels("slope_blur", &src, uni, &[t, &gv])
        }
    }

    pub struct Scene {
        nodes: wgpu::Buffer,
        tris: wgpu::Buffer,
        masks: wgpu::Buffer,
        words: u32,
        cull: bool,
    }

    fn u32_bytes(p: &[[u32; 4]]) -> &[u8] {
        // SAFETY: [u32; 4] is plain old data with no padding
        unsafe { std::slice::from_raw_parts(p.as_ptr() as *const u8, std::mem::size_of_val(p)) }
    }

    impl Gpu {
        pub fn scene(&self, d: &super::SceneData) -> Option<Scene> {
            let (nb, tb) = ((d.nodes.len() * 16) as u64, (d.tris.len() * 16) as u64);
            if self.dev.limits().max_storage_buffers_per_shader_stage < 5 || nb == 0 || tb == 0 || nb > self.max_bytes || tb > self.max_bytes {
                return None;
            }
            let mk = |data: &[u8], size: u64| {
                let b = self.buf(size.max(16), wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
                self.queue.write_buffer(&b, 0, data);
                b
            };
            let nodes = mk(u32_bytes(&d.nodes), nb);
            let tris = mk(u32_bytes(&d.tris), tb);
            let mut mb: Vec<u8> = d.masks.iter().flat_map(|w| w.to_le_bytes()).collect();
            while mb.len() < 16 {
                mb.push(0);
            }
            let mlen = mb.len() as u64;
            let masks = mk(&mb, mlen);
            Some(Scene { nodes, tris, masks, words: d.words.max(1), cull: d.cull_back })
        }

        /// Run one ray kernel over `n` items in slices; `inp` words per item `iw`, `out` words per item 1 (a `vec4<u32>`).
        fn run_rays(&self, name: &'static str, src: &str, sc: &Scene, input: &[[u32; 4]], iw: usize, per_item_rays: usize, uni: impl Fn(u32) -> Vec<u8>) -> Option<Vec<[u32; 4]>> {
            let n = input.len() / iw;
            let pipe = self.pipeline(name, src);
            let slice = (4_000_000 / per_item_rays.max(1)).clamp(4096, 1 << 20);
            if (slice * iw * 16) as u64 > self.max_bytes {
                return None;
            }
            let inb = self.buf((slice * iw * 16) as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
            let outb = self.buf((slice * 16) as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
            let stage = self.buf((slice * 16) as u64, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
            let mut result: Vec<[u32; 4]> = Vec::with_capacity(n);
            let mut at = 0;
            while at < n {
                let cnt = (n - at).min(slice);
                self.queue.write_buffer(&inb, 0, u32_bytes(&input[at * iw..(at + cnt) * iw]));
                let ub = self.uniform(&uni(cnt as u32));
                let bg = self.dev.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipe.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: sc.nodes.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: sc.tris.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: sc.masks.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 4, resource: inb.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 5, resource: outb.as_entire_binding() },
                    ],
                });
                let mut enc = self.dev.create_command_encoder(&Default::default());
                {
                    let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
                    pass.set_pipeline(&pipe);
                    pass.set_bind_group(0, &bg, &[]);
                    pass.dispatch_workgroups(((cnt + 63) / 64) as u32, 1, 1);
                }
                enc.copy_buffer_to_buffer(&outb, 0, &stage, 0, (cnt * 16) as u64);
                self.queue.submit(Some(enc.finish()));
                let sl = stage.slice(..(cnt * 16) as u64);
                let (tx, rx) = std::sync::mpsc::channel();
                sl.map_async(wgpu::MapMode::Read, move |r| {
                    let _ = tx.send(r);
                });
                self.dev.poll(wgpu::Maintain::Wait);
                rx.recv().ok()?.ok()?;
                let data = sl.get_mapped_range();
                let start = result.len();
                result.resize(start + cnt, [0; 4]);
                // SAFETY: the destination holds exactly cnt * 16 bytes of plain data
                unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), result[start..].as_mut_ptr() as *mut u8, cnt * 16) };
                drop(data);
                stage.unmap();
                at += cnt;
            }
            Some(result)
        }

        pub fn trace(&self, sc: &Scene, rays: &[[u32; 4]]) -> Option<Vec<[u32; 4]>> {
            let cull = sc.cull as u32;
            let words = sc.words;
            self.run_rays("bake_trace", &format!("{RAY_COMMON_WGSL}{TRACE_MAIN_WGSL}"), sc, rays, 3, 1, move |n| Uni::default().u(n).u(words).u(cull).u(0).f(0.0).f(0.0).u(0).u(0).done())
        }

        pub fn hemi(&self, sc: &Scene, samples: &[[u32; 4]], p: super::HemiParams) -> Option<Vec<[f32; 4]>> {
            let words = sc.words;
            let out = self.run_rays("bake_hemi", &format!("{RAY_COMMON_WGSL}{HEMI_MAIN_WGSL}"), sc, samples, 2, p.rays as usize, move |n| {
                Uni::default().u(n).u(words).u(0).u(p.rays).f(p.dist).f(p.falloff).u(p.thickness as u32).u(p.seed).done()
            })?;
            Some(out.into_iter().map(|w| [f32::from_bits(w[0]), f32::from_bits(w[1]), f32::from_bits(w[2]), f32::from_bits(w[3])]).collect())
        }
    }

    /// BVH traversal shared by the baker kernels. Words are read as `u32` and converted with `bitcast`, so triangle and
    /// node data keep their exact bits.
    const RAY_COMMON_WGSL: &str = r#"
struct U { n: u32, words: u32, cull: u32, m: u32, dist: f32, falloff: f32, mode: u32, seed: u32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> nodes: array<vec4<u32>>;
@group(0) @binding(2) var<storage, read> tris: array<vec4<u32>>;
@group(0) @binding(3) var<storage, read> masks: array<u32>;
@group(0) @binding(4) var<storage, read> inp: array<vec4<u32>>;
@group(0) @binding(5) var<storage, read_write> outp: array<vec4<u32>>;

fn fv(x: vec3<u32>) -> vec3<f32> { return bitcast<vec3<f32>>(x); }

struct Hit { t: f32, tri: u32, u: f32, v: f32 };

fn slab(bmin: vec3<f32>, bmax: vec3<f32>, o: vec3<f32>, inv: vec3<f32>, tm: f32) -> f32 {
    let a = (bmin - o) * inv;
    let b = (bmax - o) * inv;
    let lo = min(a, b);
    let hi = max(a, b);
    let t0 = max(max(max(lo.x, lo.y), lo.z), 0.0);
    let t1 = min(min(min(hi.x, hi.y), hi.z), tm);
    if (t0 <= t1) { return t0; }
    return -1.0;
}

fn fixd(x: f32) -> f32 {
    if (abs(x) < 1e-12) {
        if (x < 0.0) { return -1e-12; }
        return 1e-12;
    }
    return x;
}

fn closest(o: vec3<f32>, d: vec3<f32>, tmin: f32, tmax: f32, flt: u32, cull: bool) -> Hit {
    var best: Hit;
    best.t = -1.0;
    best.tri = 0u;
    best.u = 0.0;
    best.v = 0.0;
    let inv = vec3<f32>(1.0 / fixd(d.x), 1.0 / fixd(d.y), 1.0 / fixd(d.z));
    var tm = tmax;
    let r0 = nodes[0];
    let r1 = nodes[1];
    if (slab(fv(r0.xyz), fv(r1.xyz), o, inv, tm) < 0.0) { return best; }
    var stack: array<u32, 64>;
    var sp = 1u;
    stack[0] = 0u;
    loop {
        if (sp == 0u) { break; }
        sp = sp - 1u;
        let ni = stack[sp];
        let n0 = nodes[2u * ni];
        let n1 = nodes[2u * ni + 1u];
        if (n1.w > 0u) {
            for (var k = 0u; k < n1.w; k = k + 1u) {
                let s = n0.w + k;
                let t0 = tris[3u * s];
                let t1 = tris[3u * s + 1u];
                let t2 = tris[3u * s + 2u];
                if (flt != 0xffffffffu) {
                    let obj = t1.w;
                    let word = masks[flt * u.words + (obj >> 5u)];
                    if (((word >> (obj & 31u)) & 1u) == 0u) { continue; }
                }
                let p0 = fv(t0.xyz);
                let e1 = fv(t1.xyz);
                let e2 = fv(t2.xyz);
                let pv = cross(d, e2);
                let det = dot(e1, pv);
                if (abs(det) < 1e-20 || (cull && det < 0.0)) { continue; }
                let idet = 1.0 / det;
                let tv = o - p0;
                let uu = dot(tv, pv) * idet;
                if (uu < -1e-6 || uu > 1.000001) { continue; }
                let q = cross(tv, e1);
                let vv = dot(d, q) * idet;
                if (vv < -1e-6 || uu + vv > 1.000001) { continue; }
                let t = dot(e2, q) * idet;
                if (t > tmin && t < tm) {
                    tm = t;
                    best.t = t;
                    best.tri = t0.w;
                    best.u = clamp(uu, 0.0, 1.0);
                    best.v = clamp(vv, 0.0, 1.0);
                }
            }
        } else {
            let l = n0.w;
            let a0 = nodes[2u * l];
            let a1 = nodes[2u * l + 1u];
            let b0 = nodes[2u * l + 2u];
            let b1 = nodes[2u * l + 3u];
            let ta = slab(fv(a0.xyz), fv(a1.xyz), o, inv, tm);
            let tb = slab(fv(b0.xyz), fv(b1.xyz), o, inv, tm);
            if (ta >= 0.0 && tb >= 0.0) {
                if (sp + 2u <= 64u) {
                    if (ta <= tb) {
                        stack[sp] = l + 1u;
                        stack[sp + 1u] = l;
                    } else {
                        stack[sp] = l;
                        stack[sp + 1u] = l + 1u;
                    }
                    sp = sp + 2u;
                }
            } else if (ta >= 0.0) {
                stack[sp] = l;
                sp = sp + 1u;
            } else if (tb >= 0.0) {
                stack[sp] = l + 1u;
                sp = sp + 1u;
            }
        }
    }
    return best;
}
"#;

    const TRACE_MAIN_WGSL: &str = r#"
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= u.n) { return; }
    let a = inp[3u * i];
    let b = inp[3u * i + 1u];
    let c = inp[3u * i + 2u];
    let h = closest(fv(a.xyz), fv(b.xyz), bitcast<f32>(a.w), bitcast<f32>(b.w), c.x, u.cull == 1u);
    outp[i] = vec4<u32>(bitcast<u32>(h.t), h.tri, bitcast<u32>(h.u), bitcast<u32>(h.v));
}
"#;

    const HEMI_MAIN_WGSL: &str = r#"
fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    var h = (bitcast<u32>(x) * 0x27d4eb2du) ^ (bitcast<u32>(y) * 0x165667b1u) ^ (seed * 0x9e3779b9u);
    h = h ^ (h >> 15u);
    h = h * 0x85ebca6bu;
    h = h ^ (h >> 13u);
    h = h * 0xc2b2ae35u;
    h = h ^ (h >> 16u);
    return h;
}
fn rnd(x: i32, y: i32, seed: u32) -> f32 { return f32(hash2(x, y, seed) >> 8u) / 16777216.0; }

fn hemi_dir(n: vec3<f32>, a: f32, b: f32) -> vec3<f32> {
    var up = vec3<f32>(1.0, 0.0, 0.0);
    if (abs(n.y) < 0.99) { up = vec3<f32>(0.0, 1.0, 0.0); }
    let tg = normalize(cross(up, n));
    let bt = cross(n, tg);
    let phi = a * 6.2831855;
    let ct = sqrt(1.0 - b);
    let st = sqrt(b);
    return normalize(tg * (st * cos(phi)) + bt * (st * sin(phi)) + n * ct);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= u.n) { return; }
    let a = inp[2u * i];
    let b = inp[2u * i + 1u];
    let org = fv(a.xyz);
    let nrm = fv(b.xyz);
    let px = i32(a.w);
    let py = i32(b.w & 0xffffu);
    let sub = b.w >> 16u;
    let r1 = rnd(px, py, u.seed + sub * 3u);
    let r2 = rnd(py, px, u.seed + 1u + sub * 3u);
    var occ = 0.0;
    var dsum = vec3<f32>(0.0);
    for (var k = 0u; k < u.m; k = k + 1u) {
        let ha = f32(k) / f32(u.m);
        let hb = f32(reverseBits(k)) * 2.3283064e-10;
        let dd = hemi_dir(nrm, fract(ha + r1), fract(hb + r2));
        let h = closest(org, dd, 1e-7, u.dist, 0xffffffffu, false);
        if (u.mode == 1u) {
            if (h.t >= 0.0) { occ = occ + clamp(h.t / u.dist, 0.0, 1.0); } else { occ = occ + 1.0; }
        } else {
            if (h.t >= 0.0) {
                var f = 1.0;
                if (u.falloff > 0.0) { f = pow(clamp(1.0 - h.t / u.dist, 0.0, 1.0), max(u.falloff, 0.01)); }
                occ = occ + f;
                dsum = dsum + dd * (1.0 - f);
            } else {
                dsum = dsum + dd;
            }
        }
    }
    outp[i] = vec4<u32>(bitcast<u32>(occ), bitcast<u32>(dsum.x), bitcast<u32>(dsum.y), bitcast<u32>(dsum.z));
}
"#;

    /// Bilinear wrapped lookup in the storage buffer `buf` (same maths as `Tex::sample`); needs `u.w` and `u.h`.
    fn samp_fn(name: &str, buf: &str) -> String {
        format!(
            r#"
fn wrapi_{name}(a: i32, m: i32) -> i32 {{
    let q = i32(floor(f32(a) / f32(m)));
    return a - q * m;
}}
fn texel_{name}(x: i32, y: i32) -> vec4<f32> {{
    return {buf}[u32(wrapi_{name}(y, i32(u.h))) * u.w + u32(wrapi_{name}(x, i32(u.w)))];
}}
fn {name}(uu: f32, vv: f32) -> vec4<f32> {{
    let x = uu * f32(u.w) - 0.5;
    let y = vv * f32(u.h) - 0.5;
    let x0 = floor(x);
    let y0 = floor(y);
    let fx = x - x0;
    let fy = y - y0;
    let xa = i32(x0);
    let ya = i32(y0);
    let a = texel_{name}(xa, ya);
    let b = texel_{name}(xa + 1, ya);
    let c = texel_{name}(xa, ya + 1);
    let d = texel_{name}(xa + 1, ya + 1);
    let top = a + (b - a) * fx;
    let bot = c + (d - c) * fx;
    return top + (bot - top) * fy;
}}
"#
        )
    }

    const RADIAL_WGSL: &str = r#"
struct U { w: u32, h: u32, mode: u32, n: u32, cx: f32, cy: f32, amt: f32, rad: f32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> dst: array<vec4<f32>>;
fn wd(d: f32) -> f32 { return d - sign(d) * floor(abs(d) + 0.5); }
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.w || id.y >= u.h) { return; }
    let uu = (f32(id.x) + 0.5) / f32(u.w);
    let vv = (f32(id.y) + 0.5) / f32(u.h);
    let dx = wd(uu - u.cx);
    let dy = wd(vv - u.cy);
    let r = sqrt(dx * dx + dy * dy);
    var acc = vec4<f32>(0.0);
    if (u.mode == 1u) {
        let t = clamp(1.0 - r / u.rad, 0.0, 1.0);
        let amt = u.amt * t * t * (3.0 - 2.0 * t);
        for (var k = 0u; k < 24u; k = k + 1u) {
            let sc = 1.0 - amt * (f32(k) / 23.0);
            acc = acc + samp(u.cx + dx * sc, u.cy + dy * sc) / 24.0;
        }
    } else {
        let fade = clamp((0.5 - r) / 0.15, 0.0, 1.0);
        for (var k = 0u; k < u.n; k = k + 1u) {
            var th = 0.0;
            if (u.n > 1u) { th = (f32(k) / f32(u.n - 1u) - 0.5) * u.amt * fade; }
            let sn = sin(th);
            let cs = cos(th);
            acc = acc + samp(u.cx + dx * cs - dy * sn, u.cy + dx * sn + dy * cs) / f32(u.n);
        }
    }
    dst[id.y * u.w + id.x] = acc;
}
"#;

    const BENT_WGSL: &str = r#"
struct U { w: u32, h: u32, dirs: u32, steps: u32, rad: f32, relief: f32, ysign: f32, want_ao: u32, aspect: f32, a: f32, b: f32, c: f32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> dst: array<vec4<f32>>;
fn lum(p: vec4<f32>) -> f32 { return 0.2126 * p.x + 0.7152 * p.y + 0.0722 * p.z; }
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.w || id.y >= u.h) { return; }
    let uu = (f32(id.x) + 0.5) / f32(u.w);
    let vv = (f32(id.y) + 0.5) / f32(u.h);
    let h0 = lum(src[id.y * u.w + id.x]);
    var acc = vec3<f32>(0.0);
    var ao = 0.0;
    let pi = 3.14159265358979;
    for (var i = 0u; i < u.dirs; i = i + 1u) {
        let ang = (f32(i) + 0.5) / f32(u.dirs) * 6.28318530717959;
        let sn = sin(ang);
        let cn = cos(ang);
        var tanmax = 0.0;
        for (var k = 1u; k <= u.steps; k = k + 1u) {
            let d = u.rad * f32(k) / f32(u.steps);
            let hs = lum(samp(uu + cn * d, vv + sn * d * u.aspect));
            tanmax = max(tanmax, (hs - h0) * u.relief / d);
        }
        let hz = atan(tanmax);
        let theta = (hz + pi * 0.5) * 0.5;
        let vis = (pi * 0.5 - hz) / (pi * 0.5);
        acc = acc + vec3<f32>(vis * cn * cos(theta), vis * u.ysign * sn * cos(theta), vis * sin(theta));
        ao = ao + cos(hz) * cos(hz);
    }
    var o = vec4<f32>(0.5, 0.5, 1.0, 1.0);
    if (u.want_ao != 0u) {
        let a = clamp(ao / f32(u.dirs), 0.0, 1.0);
        o = vec4<f32>(a, a, a, 1.0);
    } else {
        let l = length(acc);
        if (l >= 1e-8) {
            let n = acc / l * 0.5 + vec3<f32>(0.5);
            o = vec4<f32>(n.x, n.y, n.z, 1.0);
        }
    }
    dst[id.y * u.w + id.x] = o;
}
"#;

    const BILATERAL_WGSL: &str = r#"
struct U { w: u32, h: u32, rad: i32, ss: f32, sr: f32, a: f32, b: f32, c: f32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> guide: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> dst: array<vec4<f32>>;
fn wrapi(a: i32, m: i32) -> i32 {
    let q = i32(floor(f32(a) / f32(m)));
    return a - q * m;
}
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.w || id.y >= u.h) { return; }
    let x = i32(id.x);
    let y = i32(id.y);
    let c = guide[id.y * u.w + id.x];
    var acc = vec4<f32>(0.0);
    var wsum = 0.0;
    let inv_s = 1.0 / (2.0 * u.ss * u.ss);
    let inv_r = 1.0 / (2.0 * u.sr * u.sr);
    for (var dy = -u.rad; dy <= u.rad; dy = dy + 1) {
        let yy = u32(wrapi(y + dy, i32(u.h)));
        for (var dx = -u.rad; dx <= u.rad; dx = dx + 1) {
            let i = yy * u.w + u32(wrapi(x + dx, i32(u.w)));
            let gp = guide[i];
            let dc = (gp.x - c.x) * (gp.x - c.x) + (gp.y - c.y) * (gp.y - c.y) + (gp.z - c.z) * (gp.z - c.z);
            let wgt = exp(-f32(dx * dx + dy * dy) * inv_s - dc * inv_r);
            acc = acc + src[i] * wgt;
            wsum = wsum + wgt;
        }
    }
    dst[id.y * u.w + id.x] = acc / wsum;
}
"#;

    const SLOPE_GRAD_WGSL: &str = r#"
struct U { w: u32, h: u32, a: u32, b: u32 };
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
    let gx = lum(src[y * u.w + xp]) - lum(src[y * u.w + xm]);
    let gy = lum(src[yp * u.w + x]) - lum(src[ym * u.w + x]);
    dst[y * u.w + x] = vec4<f32>(gx, gy, 0.0, 0.0);
}
"#;

    const SLOPE_WGSL: &str = r#"
struct U { w: u32, h: u32, n: u32, mode: u32, step: f32, gain: f32, a: f32, b: f32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> gv: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> dst: array<vec4<f32>>;
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.w || id.y >= u.h) { return; }
    var pu = (f32(id.x) + 0.5) / f32(u.w);
    var pv = (f32(id.y) + 0.5) / f32(u.h);
    var acc = vec4<f32>(0.0);
    if (u.mode == 1u) { acc = vec4<f32>(1.0); }
    for (var k = 0u; k < u.n; k = k + 1u) {
        let p = samp_t(pu, pv);
        if (u.mode == 1u) { acc = min(acc, p); }
        else if (u.mode == 2u) { acc = max(acc, p); }
        else { acc = acc + p / f32(u.n); }
        let g = samp_g(pu, pv);
        let l = max(sqrt(g.x * g.x + g.y * g.y), 1e-6);
        let kk = min(l * u.gain, 1.0) * u.step / l;
        pu = pu + g.x * kk;
        pv = pv + g.y * kk;
    }
    dst[id.y * u.w + id.x] = acc;
}
"#;

    const EXTRUDE_WGSL: &str = r#"
struct U { w: u32, h: u32, first: u32, dx: f32, dy: f32, drop: f32, a: f32, b: f32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> prev: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> dst: array<vec4<f32>>;
fn lum(p: vec4<f32>) -> f32 { return 0.2126 * p.x + 0.7152 * p.y + 0.0722 * p.z; }
fn wrapi(a: i32, m: i32) -> i32 {
    let q = i32(floor(f32(a) / f32(m)));
    return a - q * m;
}
fn pv(x: i32, y: i32) -> f32 {
    let i = u32(wrapi(y, i32(u.h))) * u.w + u32(wrapi(x, i32(u.w)));
    if (u.first != 0u) { return lum(src[i]); }
    return prev[i].x;
}
fn psamp(uu: f32, vv: f32) -> f32 {
    let x = uu * f32(u.w) - 0.5;
    let y = vv * f32(u.h) - 0.5;
    let x0 = floor(x);
    let y0 = floor(y);
    let fx = x - x0;
    let fy = y - y0;
    let xa = i32(x0);
    let ya = i32(y0);
    let top = mix(pv(xa, ya), pv(xa + 1, ya), fx);
    let bot = mix(pv(xa, ya + 1), pv(xa + 1, ya + 1), fx);
    return mix(top, bot, fy);
}
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.w || id.y >= u.h) { return; }
    let uu = (f32(id.x) + 0.5) / f32(u.w);
    let vv = (f32(id.y) + 0.5) / f32(u.h);
    let back = psamp(uu - u.dx, vv - u.dy) - u.drop;
    let g = max(lum(src[id.y * u.w + id.x]), max(back, 0.0));
    dst[id.y * u.w + id.x] = vec4<f32>(g, g, g, 1.0);
}
"#;

    const WARP_WGSL: &str = r#"
struct U { w: u32, h: u32, kind: u32, filt: u32, intensity: f32, a: f32, b: f32, c: f32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> wmap: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> dst: array<vec4<f32>>;
fn lum(p: vec4<f32>) -> f32 { return 0.2126 * p.x + 0.7152 * p.y + 0.0722 * p.z; }
fn wl(x: i32, y: i32) -> f32 {
    let xx = u32(wrapi_samp(x, i32(u.w)));
    let yy = u32(wrapi_samp(y, i32(u.h)));
    return lum(wmap[yy * u.w + xx]);
}
fn cw(t: f32) -> vec4<f32> {
    return vec4<f32>(
        0.5 * (-t * t * t + 2.0 * t * t - t),
        0.5 * (3.0 * t * t * t - 5.0 * t * t + 2.0),
        0.5 * (-3.0 * t * t * t + 4.0 * t * t + t),
        0.5 * (t * t * t - t * t));
}
fn cubic(uu: f32, vv: f32) -> vec4<f32> {
    let x = uu * f32(u.w) - 0.5;
    let y = vv * f32(u.h) - 0.5;
    let x0 = floor(x);
    let y0 = floor(y);
    let wx = cw(x - x0);
    let wy = cw(y - y0);
    let xi = i32(x0);
    let yi = i32(y0);
    var o = vec4<f32>(0.0);
    for (var j = 0; j < 4; j = j + 1) {
        for (var i = 0; i < 4; i = i + 1) {
            o = o + texel_samp(xi + i - 1, yi + j - 1) * (wx[i] * wy[j]);
        }
    }
    let n0 = texel_samp(xi, yi);
    let n1 = texel_samp(xi + 1, yi);
    let n2 = texel_samp(xi, yi + 1);
    let n3 = texel_samp(xi + 1, yi + 1);
    let lo = min(min(n0, n1), min(n2, n3));
    let hi = max(max(n0, n1), max(n2, n3));
    return clamp(o, lo, hi);
}
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.w || id.y >= u.h) { return; }
    let uu = (f32(id.x) + 0.5) / f32(u.w);
    let vv = (f32(id.y) + 0.5) / f32(u.h);
    let x = i32(id.x);
    let y = i32(id.y);
    var d = vec2<f32>(0.0);
    if (u.kind == 0u) {
        d = vec2<f32>(wl(x + 1, y) - wl(x - 1, y), wl(x, y + 1) - wl(x, y - 1)) * u.intensity;
    } else if (u.kind == 1u) {
        let k = (lum(wmap[id.y * u.w + id.x]) - u.c) * u.intensity;
        d = vec2<f32>(u.a * k, u.b * k);
    } else {
        let m = wmap[id.y * u.w + id.x];
        d = vec2<f32>((m.x - 0.5) * 2.0 * u.intensity, u.a * (m.y - 0.5) * 2.0 * u.intensity);
    }
    let pu = uu + d.x;
    let pv = vv + d.y;
    var o = vec4<f32>(0.0);
    if (u.filt == 0u) {
        o = samp(pu, pv);
    } else if (u.filt == 1u) {
        let fu = pu - floor(pu);
        let fv = pv - floor(pv);
        o = texel_samp(i32(fu * f32(u.w)), i32(fv * f32(u.h)));
    } else {
        o = cubic(pu, pv);
    }
    dst[id.y * u.w + id.x] = o;
}
"#;

    const LEVELS_WGSL: &str = r#"
struct U { w: u32, h: u32, in_lo: f32, d: f32, g: f32, out_lo: f32, out_hi: f32, pad: f32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> dst: array<vec4<f32>>;
fn lv(x: f32) -> f32 {
    let v = clamp((x - u.in_lo) / u.d, 0.0, 1.0);
    var k = v;
    if (abs(u.g - 1.0) >= 1e-6) { k = pow(v, u.g); }
    return u.out_lo + (u.out_hi - u.out_lo) * k;
}
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.w || id.y >= u.h) { return; }
    let p = src[id.y * u.w + id.x];
    dst[id.y * u.w + id.x] = vec4<f32>(lv(p.x), lv(p.y), lv(p.z), p.w);
}
"#;

    const DIRBLUR_WGSL: &str = r#"
struct U { w: u32, h: u32, c: f32, s: f32, length: f32, a: f32, b: f32, e: f32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> dst: array<vec4<f32>>;
fn wrapi(a: i32, m: i32) -> i32 {
    // floor division through floats: signed remainders are not reliable on every driver
    let q = i32(floor(f32(a) / f32(m)));
    return a - q * m;
}
fn texel(x: i32, y: i32) -> vec4<f32> {
    return src[u32(wrapi(y, i32(u.h))) * u.w + u32(wrapi(x, i32(u.w)))];
}
fn samp(uu: f32, vv: f32) -> vec4<f32> {
    let x = uu * f32(u.w) - 0.5;
    let y = vv * f32(u.h) - 0.5;
    let x0 = floor(x);
    let y0 = floor(y);
    let fx = x - x0;
    let fy = y - y0;
    let xa = i32(x0);
    let ya = i32(y0);
    let a = texel(xa, ya);
    let b = texel(xa + 1, ya);
    let c = texel(xa, ya + 1);
    let d = texel(xa + 1, ya + 1);
    let top = a + (b - a) * fx;
    let bot = c + (d - c) * fx;
    return top + (bot - top) * fy;
}
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.w || id.y >= u.h) { return; }
    let uu = (f32(id.x) + 0.5) / f32(u.w);
    let vv = (f32(id.y) + 0.5) / f32(u.h);
    var acc = vec4<f32>(0.0);
    for (var i = 0; i < 24; i = i + 1) {
        let f = (f32(i) / 23.0 - 0.5) * u.length;
        acc = acc + samp(uu + u.c * f, vv + u.s * f) / 24.0;
    }
    dst[id.y * u.w + id.x] = acc;
}
"#;

    const BLEND_WGSL: &str = r#"
struct U { w: u32, h: u32, mode: u32, alpha_mode: u32, opacity: f32, has_mask: u32, a: u32, b: u32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> fg: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> bg: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> msk: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> dst: array<vec4<f32>>;
fn lum(c: vec4<f32>) -> f32 { return 0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z; }
fn bm(mode: u32, f: f32, b: f32) -> f32 {
    switch (mode) {
        case 0u: { return f; }
        case 1u: { return f + b; }
        case 2u: { return b - f; }
        case 3u: { return f * b; }
        case 4u: { return 1.0 - (1.0 - f) * (1.0 - b); }
        case 5u: {
            if (b < 0.5) { return 2.0 * f * b; }
            return 1.0 - 2.0 * (1.0 - f) * (1.0 - b);
        }
        case 6u: { return max(f, b); }
        case 7u: { return min(f, b); }
        case 8u: { return abs(f - b); }
        case 9u: { return (1.0 - 2.0 * f) * b * b + 2.0 * f * b; }
        case 10u: {
            if (f >= 0.999) { return 1.0; }
            return b / (1.0 - f);
        }
        case 11u: {
            if (f <= 0.001) { return 0.0; }
            return 1.0 - (1.0 - b) / f;
        }
        case 12u: { return f + b - 1.0; }
        case 13u: {
            if (f < 0.5) { return 2.0 * f * b; }
            return 1.0 - 2.0 * (1.0 - f) * (1.0 - b);
        }
        case 14u: { return f + b - 2.0 * f * b; }
        case 15u: {
            if (f <= 0.001) { return b; }
            return b / f;
        }
        case 16u: { return b + 2.0 * f - 1.0; }
        case 20u: { return b + (f - 0.5) * 2.0; }
        default: { return f; }
    }
}
fn set_lum(c: vec4<f32>, l: f32) -> vec3<f32> {
    let d = l - lum(c);
    return clamp(vec3<f32>(c.x + d, c.y + d, c.z + d), vec3<f32>(0.0), vec3<f32>(1.0));
}
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.w || id.y >= u.h) { return; }
    let i = id.y * u.w + id.x;
    let f = fg[i];
    let b = bg[i];
    var m = u.opacity;
    if (u.has_mask != 0u) { m = m * lum(msk[i]); }
    var out = b;
    if (u.alpha_mode == 0u) {
        out.w = b.w + (f.w - b.w) * m;
        m = m * f.w;
    } else if (u.alpha_mode >= 2u) {
        out.w = f.w * m + b.w * (1.0 - f.w * m);
        m = m * f.w;
    }
    if (u.mode == 17u || u.mode == 18u) {
        var r: vec3<f32>;
        if (u.mode == 17u) { r = set_lum(f, lum(b)); } else { r = set_lum(b, lum(f)); }
        let o = clamp(b.xyz + (r - b.xyz) * m, vec3<f32>(0.0), vec3<f32>(1.0));
        dst[i] = vec4<f32>(o, out.w);
        return;
    }
    let cr = clamp(b.x + (clamp(bm(u.mode, f.x, b.x), 0.0, 1.0) - b.x) * m, 0.0, 1.0);
    let cg = clamp(b.y + (clamp(bm(u.mode, f.y, b.y), 0.0, 1.0) - b.y) * m, 0.0, 1.0);
    let cb = clamp(b.z + (clamp(bm(u.mode, f.z, b.z), 0.0, 1.0) - b.z) * m, 0.0, 1.0);
    dst[i] = vec4<f32>(cr, cg, cb, out.w);
}
"#;

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
fn gdot(cx: i32, cy: i32, dx: f32, dy: f32, seed: u32, gx: i32, gy: i32) -> f32 {
    let c = grad[hash2(wrapi(cx, gx), wrapi(cy, gy), seed) >> 20u];
    return c.x * dx + c.y * dy;
}
fn perlin(uu: f32, vv: f32, seed: u32) -> f32 { return perlin_g(uu, vv, seed, u.px, u.py); }
fn perlin_g(uu: f32, vv: f32, seed: u32, gx: i32, gy: i32) -> f32 {
    let x = uu * f32(gx);
    let y = vv * f32(gy);
    let fxl = floor(x);
    let fyl = floor(y);
    let ix = i32(fxl);
    let iy = i32(fyl);
    let fx = x - fxl;
    let fy = y - fyl;
    let n00 = gdot(ix, iy, fx, fy, seed, gx, gy);
    let n10 = gdot(ix + 1, iy, fx - 1.0, fy, seed, gx, gy);
    let n01 = gdot(ix, iy + 1, fx, fy - 1.0, seed, gx, gy);
    let n11 = gdot(ix + 1, iy + 1, fx - 1.0, fy - 1.0, seed, gx, gy);
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

    /// Streak noise: tileable Perlin averaged along a direction (octaves = tap count, a/b/c = cos, sin, streak length).
    const DIRNOISE_MAIN_WGSL: &str = r#"
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u.w || id.y >= u.h) { return; }
    let uu = (f32(id.x) + 0.5) / f32(u.w);
    let vv = (f32(id.y) + 0.5) / f32(u.h);
    let k = u.octaves;
    var a1 = 0.0;
    var a2 = 0.0;
    var wsum = 0.0;
    for (var i = 0u; i < k; i = i + 1u) {
        let t = ((f32(i) + 0.5) / f32(k)) * 2.0 - 1.0;
        let w = 1.0 - abs(t);
        let o1 = t * u.c * 0.5;
        let o2 = t * u.c * 0.25;
        a1 = a1 + w * perlin_g(uu + 1.0 + o1 * u.a, vv + 1.0 + o1 * u.b, u.seed, u.px, u.py);
        a2 = a2 + w * perlin_g(uu + 1.0 + o2 * u.a, vv + 1.0 + o2 * u.b, u.seed + 7u, u.px * 2, u.py * 2);
        wsum = wsum + w;
    }
    let m = (a1 * 0.65 + a2 * 0.35) / wsum;
    let g = clamp(0.5 + (m - 0.5) * u.gain, 0.0, 1.0);
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
