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
    let g = |cx: i32, cy: i32, dx: f32, dy: f32| {
        let a = rnd(cx.rem_euclid(px), cy.rem_euclid(py), seed) * TAU;
        a.cos() * dx + a.sin() * dy
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
    let mut cx = px.max(1);
    let mut cy = py.max(1);
    for o in 0..octaves.clamp(1, 10) {
        let n = perlin(u, v, cx, cy, seed.wrapping_add(o.wrapping_mul(131)));
        let n = match mode {
            1 => 1.0 - (2.0 * n - 1.0).abs(),
            2 => (2.0 * n - 1.0).abs(),
            _ => n,
        };
        sum += n * amp;
        norm += amp;
        amp *= gain;
        cx = cx.saturating_mul(2).min(1 << 14);
        cy = cy.saturating_mul(2).min(1 << 14);
    }
    if norm > 0.0 {
        sum / norm
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
                let a = rnd(x, y, seed) * TAU;
                g.push([a.cos(), a.sin()]);
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
