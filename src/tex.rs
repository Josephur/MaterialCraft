//! Float RGBA texture with wrapped (tileable) sampling.

use std::sync::atomic::{AtomicU64, Ordering};

/// Id of the newest editor evaluation. A running evaluation whose own id differs is stale: the parallel loops below
/// stop at the next row, so a superseded job gives the CPU back almost at once instead of finishing a heavy node.
pub static CURRENT_JOB: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static MY_JOB: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Mark the calling thread as running evaluation `id` (0 = not an editor job, never stale).
pub fn set_job(id: u64) {
    MY_JOB.with(|c| c.set(id));
}

pub fn my_job() -> u64 {
    MY_JOB.with(|c| c.get())
}

pub fn stale(my: u64) -> bool {
    my != 0 && CURRENT_JOB.load(Ordering::Relaxed) != my
}

pub fn job_stale() -> bool {
    stale(my_job())
}

#[derive(Clone, Debug)]
pub struct Tex {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[f32; 4]>,
}

pub fn lum(p: [f32; 4]) -> f32 {
    0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
}

fn san(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

impl Tex {
    pub fn solid(w: usize, h: usize, c: [f32; 4]) -> Tex {
        Tex { w, h, px: vec![c; w * h] }
    }

    /// Build a texture from a per-pixel function of (u, v) in [0,1). Runs multi-threaded.
    pub fn from_fn<F: Fn(f32, f32) -> [f32; 4] + Sync>(w: usize, h: usize, f: F) -> Tex {
        let w = w.max(1);
        let h = h.max(1);
        let mut px = vec![[0.0f32; 4]; w * h];
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(1);
        let rows_per = (h + n - 1) / n;
        let fref = &f;
        let my = my_job();
        std::thread::scope(|s| {
            for (ci, chunk) in px.chunks_mut(rows_per * w).enumerate() {
                s.spawn(move || {
                    for (i, p) in chunk.iter_mut().enumerate() {
                        if i % w == 0 && stale(my) {
                            return;
                        }
                        let idx = ci * rows_per * w + i;
                        let x = idx % w;
                        let y = idx / w;
                        let o = fref((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                        *p = [san(o[0]), san(o[1]), san(o[2]), san(o[3])];
                    }
                });
            }
        });
        Tex { w, h, px }
    }

    pub fn gray_fn<F: Fn(f32, f32) -> f32 + Sync>(w: usize, h: usize, f: F) -> Tex {
        Tex::from_fn(w, h, |u, v| {
            let g = f(u, v);
            [g, g, g, 1.0]
        })
    }

    pub fn at(&self, x: i64, y: i64) -> [f32; 4] {
        let xi = x.rem_euclid(self.w as i64) as usize;
        let yi = y.rem_euclid(self.h as i64) as usize;
        self.px[yi * self.w + xi]
    }

    /// Bilinear, wrapped sampling in uv space.
    pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let u = if u.is_finite() { u } else { 0.0 };
        let v = if v.is_finite() { v } else { 0.0 };
        let x = u * self.w as f32 - 0.5;
        let y = v * self.h as f32 - 0.5;
        let x0 = x.floor();
        let y0 = y.floor();
        let fx = x - x0;
        let fy = y - y0;
        let xi = x0 as i64;
        let yi = y0 as i64;
        let a = self.at(xi, yi);
        let b = self.at(xi + 1, yi);
        let c = self.at(xi, yi + 1);
        let d = self.at(xi + 1, yi + 1);
        let mut o = [0.0; 4];
        for i in 0..4 {
            let top = a[i] + (b[i] - a[i]) * fx;
            let bot = c[i] + (d[i] - c[i]) * fx;
            o[i] = top + (bot - top) * fy;
        }
        o
    }

    /// Catmull-Rom (bicubic) wrapped sampling in uv space: sharper than bilinear for rotations, scaling and warps. The
    /// result stays inside the range of the four nearest texels, so it never overshoots.
    pub fn sample_cubic(&self, u: f32, v: f32) -> [f32; 4] {
        let u = if u.is_finite() { u } else { 0.0 };
        let v = if v.is_finite() { v } else { 0.0 };
        let x = u * self.w as f32 - 0.5;
        let y = v * self.h as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (xi, yi) = (x0 as i64, y0 as i64);
        let wts = |t: f32| {
            [
                0.5 * (-t * t * t + 2.0 * t * t - t),
                0.5 * (3.0 * t * t * t - 5.0 * t * t + 2.0),
                0.5 * (-3.0 * t * t * t + 4.0 * t * t + t),
                0.5 * (t * t * t - t * t),
            ]
        };
        let (wx, wy) = (wts(fx), wts(fy));
        let mut o = [0.0f32; 4];
        for j in 0..4 {
            for i in 0..4 {
                let p = self.at(xi + i as i64 - 1, yi + j as i64 - 1);
                let w = wx[i] * wy[j];
                for c in 0..4 {
                    o[c] += p[c] * w;
                }
            }
        }
        let n = [self.at(xi, yi), self.at(xi + 1, yi), self.at(xi, yi + 1), self.at(xi + 1, yi + 1)];
        for c in 0..4 {
            let lo = n.iter().map(|p| p[c]).fold(f32::MAX, f32::min);
            let hi = n.iter().map(|p| p[c]).fold(f32::MIN, f32::max);
            o[c] = o[c].clamp(lo, hi);
        }
        o
    }

    pub fn map<F: Fn([f32; 4]) -> [f32; 4] + Sync>(&self, f: F) -> Tex {
        let mut out = self.clone();
        for p in out.px.iter_mut() {
            *p = f(*p);
        }
        out
    }

    pub fn gray_at(&self, x: i64, y: i64) -> f32 {
        lum(self.at(x, y))
    }
}

/// Parallel map over 0..n, preserving order.
pub fn par_map<T: Send + Default + Clone, F: Fn(usize) -> T + Sync>(n: usize, f: F) -> Vec<T> {
    let mut out = vec![T::default(); n];
    if n == 0 {
        return out;
    }
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(1);
    let per = (n + threads - 1) / threads;
    let fref = &f;
    let my = my_job();
    std::thread::scope(|s| {
        for (ci, chunk) in out.chunks_mut(per).enumerate() {
            s.spawn(move || {
                for (i, slot) in chunk.iter_mut().enumerate() {
                    if i % 4096 == 0 && stale(my) {
                        return;
                    }
                    *slot = fref(ci * per + i);
                }
            });
        }
    });
    out
}
