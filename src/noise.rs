//! Seeded, tileable procedural noise.
use std::f32::consts::TAU;

pub fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d)
        ^ (y as u32).wrapping_mul(0x1656_67b1)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    h
}

pub fn rnd(x: i32, y: i32, seed: u32) -> f32 {
    (hash2(x, y, seed) >> 8) as f32 / 16_777_216.0
}

fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Unit gradient vectors for 4096 evenly spaced angles (replaces a cos/sin per lattice corner).
fn grad_table() -> &'static [[f32; 2]; 4096] {
    static T: std::sync::OnceLock<Box<[[f32; 2]; 4096]>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = Box::new([[0.0f32; 2]; 4096]);
        for (i, e) in t.iter_mut().enumerate() {
            let a = i as f32 / 4096.0 * TAU;
            *e = [a.cos(), a.sin()];
        }
        t
    })
}

/// Gradient at a lattice point: same pseudo-random angle as `rnd(x, y, seed) * TAU`, quantised to 4096 steps.
#[inline]
fn grad_at(x: i32, y: i32, seed: u32) -> [f32; 2] {
    grad_table()[(hash2(x, y, seed) >> 20) as usize]
}

/// Tileable gradient (Perlin) noise in 0..1. `px`,`py` are integer lattice periods.
pub fn perlin(u: f32, v: f32, px: i32, py: i32, seed: u32) -> f32 {
    let px = px.max(1);
    let py = py.max(1);
    let x = u * px as f32;
    let y = v * py as f32;
    let fxl = x.floor();
    let fyl = y.floor();
    let ix = fxl as i32;
    let iy = fyl as i32;
    let fx = x - fxl;
    let fy = y - fyl;
    let tab = grad_table();
    let g = |cx: i32, cy: i32, dx: f32, dy: f32| {
        let c = tab[(hash2(cx.rem_euclid(px), cy.rem_euclid(py), seed) >> 20) as usize];
        c[0] * dx + c[1] * dy
    };
    let n00 = g(ix, iy, fx, fy);
    let n10 = g(ix + 1, iy, fx - 1.0, fy);
    let n01 = g(ix, iy + 1, fx, fy - 1.0);
    let n11 = g(ix + 1, iy + 1, fx - 1.0, fy - 1.0);
    let sx = fade(fx);
    let sy = fade(fy);
    let n = lerp(lerp(n00, n10, sx), lerp(n01, n11, sx), sy);
    (n * std::f32::consts::FRAC_1_SQRT_2 + 0.5).clamp(0.0, 1.0)
}

/// Fractal noise. mode: 0 = fBm, 1 = ridged, 2 = turbulence.
pub fn fbm(u: f32, v: f32, px: i32, py: i32, octaves: u32, gain: f32, seed: u32, mode: u32) -> f32 {
    let mut amp = 1.0;
    let mut sum = 0.0;
    let mut norm = 0.0;
    let cx = px.max(1);
    let cy = py.max(1);
    // Every octave is rotated by about 26.6 degrees and scaled by sqrt(5) with the INTEGER matrix [[2, -1], [1, 2]], so
    // the lattice directions of different octaves never line up (no visible grid) and the noise still tiles exactly.
    let (mut a, mut b) = (u, v);
    for o in 0..octaves.clamp(1, 12) {
        let n = perlin(a, b, cx, cy, seed.wrapping_add(o.wrapping_mul(131)));
        let n = match mode {
            1 => 1.0 - (2.0 * n - 1.0).abs(),
            2 => (2.0 * n - 1.0).abs(),
            _ => n,
        };
        sum += n * amp;
        norm += amp;
        amp *= gain;
        let (na, nb) = (2.0 * a - b, a + 2.0 * b);
        // keep the coordinates small: whole periods do not change a periodic function
        a = na.rem_euclid(1.0);
        b = nb.rem_euclid(1.0);
    }
    if norm > 0.0 {
        // averaged octaves cluster around mid grey; stretch them so the result uses most of the 0..1 range
        let m = sum / norm;
        let k = match mode {
            0 => 1.7,
            _ => 1.35,
        };
        (0.5 + (m - 0.5) * k).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Tileable Voronoi. Returns (f1, f2, cell_id) with distances normalised to ~0..1.
pub fn voronoi(u: f32, v: f32, px: i32, py: i32, seed: u32, jitter: f32) -> (f32, f32, f32) {
    let px = px.max(1);
    let py = py.max(1);
    let x = u * px as f32;
    let y = v * py as f32;
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let mut f1 = f32::MAX;
    let mut f2 = f32::MAX;
    let mut id = 0.0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let cx = ix + dx;
            let cy = iy + dy;
            let wx = cx.rem_euclid(px);
            let wy = cy.rem_euclid(py);
            let fx = cx as f32 + 0.5 + (rnd(wx, wy, seed) - 0.5) * jitter;
            let fy = cy as f32 + 0.5 + (rnd(wx, wy, seed.wrapping_add(77)) - 0.5) * jitter;
            let d = ((fx - x).powi(2) + (fy - y).powi(2)).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
                id = rnd(wx, wy, seed.wrapping_add(999));
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    let k = std::f32::consts::FRAC_1_SQRT_2;
    ((f1 * k).clamp(0.0, 1.0), (f2 * k).clamp(0.0, 1.0), id)
}

/// Soft round spots, one disc per cell, tileable. `shape(id)` gives the (inner, outer) radius of a cell's disc in the
/// same normalised distance units as `voronoi`; discs may overlap the neighbouring cells, so the union of the 3x3
/// surrounding discs is taken (no straight cuts where cells meet). `grain` is added to the distance.
pub fn spots<F: Fn(f32) -> (f32, f32)>(u: f32, v: f32, px: i32, py: i32, seed: u32, shape: F, grain: f32) -> f32 {
    let (px, py) = (px.max(1), py.max(1));
    let (x, y) = (u * px as f32, v * py as f32);
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let k = std::f32::consts::FRAC_1_SQRT_2;
    let mut best = 0.0f32;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (ix + dx, iy + dy);
            let (wx, wy) = (cx.rem_euclid(px), cy.rem_euclid(py));
            let fx = cx as f32 + 0.5 + (rnd(wx, wy, seed) - 0.5);
            let fy = cy as f32 + 0.5 + (rnd(wx, wy, seed.wrapping_add(77)) - 0.5);
            let d = (((fx - x).powi(2) + (fy - y).powi(2)).sqrt() * k + grain).max(0.0);
            let (lo, hi) = shape(rnd(wx, wy, seed.wrapping_add(999)));
            let t = ((d - lo) / (hi - lo).max(1e-6)).clamp(0.0, 1.0);
            best = best.max(1.0 - t * t * (3.0 - 2.0 * t));
        }
    }
    best
}

pub struct Vor {
    pub f1: f32,
    pub f2: f32,
    pub id1: f32,
    pub id2: f32,
    pub dx: f32,
    pub dy: f32,
}

/// Tileable Voronoi with the two nearest cells and the vector to the nearest site.
pub fn voronoi_ext(u: f32, v: f32, px: i32, py: i32, seed: u32, jitter: f32) -> Vor {
    let px = px.max(1);
    let py = py.max(1);
    let x = u * px as f32;
    let y = v * py as f32;
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let mut r = Vor { f1: f32::MAX, f2: f32::MAX, id1: 0.0, id2: 0.0, dx: 0.0, dy: 0.0 };
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (ix + dx, iy + dy);
            let (wx, wy) = (cx.rem_euclid(px), cy.rem_euclid(py));
            let fx = cx as f32 + 0.5 + (rnd(wx, wy, seed) - 0.5) * jitter;
            let fy = cy as f32 + 0.5 + (rnd(wx, wy, seed.wrapping_add(77)) - 0.5) * jitter;
            let d = ((fx - x).powi(2) + (fy - y).powi(2)).sqrt();
            let id = rnd(wx, wy, seed.wrapping_add(999));
            if d < r.f1 {
                r.f2 = r.f1;
                r.id2 = r.id1;
                r.f1 = d;
                r.id1 = id;
                r.dx = fx - x;
                r.dy = fy - y;
            } else if d < r.f2 {
                r.f2 = d;
                r.id2 = id;
            }
        }
    }
    let k = std::f32::consts::FRAC_1_SQRT_2;
    r.f1 = (r.f1 * k).clamp(0.0, 1.0);
    r.f2 = (r.f2 * k).clamp(0.0, 1.0);
    r
}

/// Precomputed gradient lattice for fast, repeated tileable Perlin lookups (no hashing or trig per sample).
/// `sample` returns exactly the same values as `perlin(u, v, px, py, seed)`.
pub struct GradGrid {
    px: i32,
    py: i32,
    g: Vec<[f32; 2]>,
}

impl GradGrid {
    pub fn new(px: i32, py: i32, seed: u32) -> GradGrid {
        let px = px.clamp(1, 4096);
        let py = py.clamp(1, 4096);
        let mut g = Vec::with_capacity((px as usize) * (py as usize));
        for y in 0..py {
            for x in 0..px {
                g.push(grad_at(x, y, seed));
            }
        }
        GradGrid { px, py, g }
    }

    /// Periodic in u and v with period 1, for any (also negative or > 1) coordinates.
    pub fn sample(&self, u: f32, v: f32) -> f32 {
        let x = u * self.px as f32;
        let y = v * self.py as f32;
        let (fxl, fyl) = (x.floor(), y.floor());
        let (ix, iy) = (fxl as i64, fyl as i64);
        let (fx, fy) = (x - fxl, y - fyl);
        let (px, py) = (self.px as i64, self.py as i64);
        let g = |cx: i64, cy: i64, dx: f32, dy: f32| {
            let c = self.g[(cy.rem_euclid(py) * px + cx.rem_euclid(px)) as usize];
            c[0] * dx + c[1] * dy
        };
        let n00 = g(ix, iy, fx, fy);
        let n10 = g(ix + 1, iy, fx - 1.0, fy);
        let n01 = g(ix, iy + 1, fx, fy - 1.0);
        let n11 = g(ix + 1, iy + 1, fx - 1.0, fy - 1.0);
        let (sx, sy) = (fade(fx), fade(fy));
        let n = lerp(lerp(n00, n10, sx), lerp(n01, n11, sx), sy);
        (n * std::f32::consts::FRAC_1_SQRT_2 + 0.5).clamp(0.0, 1.0)
    }
}
