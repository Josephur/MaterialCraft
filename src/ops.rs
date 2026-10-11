//! Image-processing operators used by graph nodes. All are wrap-around (tileable).
use crate::tex::{lum, Tex};
use std::f32::consts::{PI, TAU};

fn sm(a: f32, b: f32, x: f32) -> f32 {
    let d = b - a;
    let t = if d.abs() < 1e-9 { if x >= b { 1.0 } else { 0.0 } } else { ((x - a) / d).clamp(0.0, 1.0) };
    t * t * (3.0 - 2.0 * t)
}

// ---------- blur / sharpen ----------
pub fn blur(t: &Tex, sigma: f32) -> Tex {
    if sigma < 0.1 {
        return t.clone();
    }
    let r = ((sigma * 3.0).ceil() as i64).clamp(1, 64);
    let mut k: Vec<f32> = (-r..=r).map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp()).collect();
    let s: f32 = k.iter().sum();
    k.iter_mut().for_each(|x| *x /= s);
    let (w, h) = (t.w, t.h);
    let mut tmp = t.clone();
    for y in 0..h {
        for x in 0..w {
            let mut a = [0.0f32; 4];
            for (i, kv) in k.iter().enumerate() {
                let p = t.at(x as i64 + i as i64 - r, y as i64);
                for c in 0..4 {
                    a[c] += p[c] * kv;
                }
            }
            tmp.px[y * w + x] = a;
        }
    }
    let mut out = tmp.clone();
    for y in 0..h {
        for x in 0..w {
            let mut a = [0.0f32; 4];
            for (i, kv) in k.iter().enumerate() {
                let p = tmp.at(x as i64, y as i64 + i as i64 - r);
                for c in 0..4 {
                    a[c] += p[c] * kv;
                }
            }
            out.px[y * w + x] = a;
        }
    }
    out
}

pub fn directional_blur(t: &Tex, angle: f32, length: f32) -> Tex {
    let (s, c) = (angle * TAU).sin_cos();
    let n = 24;
    Tex::from_fn(t.w, t.h, |u, v| {
        let mut a = [0.0f32; 4];
        for i in 0..n {
            let f = (i as f32 / (n - 1) as f32 - 0.5) * length;
            let p = t.sample(u + c * f, v + s * f);
            for k in 0..4 {
                a[k] += p[k] / n as f32;
            }
        }
        a
    })
}

pub fn sharpen(t: &Tex, radius: f32, amount: f32) -> Tex {
    let b = blur(t, radius);
    let mut o = t.clone();
    for (i, p) in o.px.iter_mut().enumerate() {
        for c in 0..3 {
            p[c] = (t.px[i][c] + (t.px[i][c] - b.px[i][c]) * amount).clamp(0.0, 1.0);
        }
    }
    o
}

// ---------- tonal ----------
pub fn levels(t: &Tex, in_lo: f32, in_hi: f32, gamma: f32, out_lo: f32, out_hi: f32) -> Tex {
    let d = if in_hi >= in_lo { (in_hi - in_lo).max(1e-6) } else { (in_hi - in_lo).min(-1e-6) };
    let g = 1.0 / gamma.max(1e-3);
    t.map(|p| {
        let mut o = p;
        for c in 0..3 {
            let v = ((p[c] - in_lo) / d).clamp(0.0, 1.0).powf(g);
            o[c] = out_lo + (out_hi - out_lo) * v;
        }
        o
    })
}

pub fn adjust(t: &Tex, brightness: f32, contrast: f32) -> Tex {
    t.map(|p| {
        let mut o = p;
        for c in 0..3 {
            o[c] = ((p[c] - 0.5) * contrast + 0.5 + brightness).clamp(0.0, 1.0);
        }
        o
    })
}

pub fn invert(t: &Tex) -> Tex {
    t.map(|p| [1.0 - p[0], 1.0 - p[1], 1.0 - p[2], p[3]])
}

pub fn threshold(t: &Tex, th: f32, soft: f32) -> Tex {
    t.map(|p| {
        let g = sm(th - soft * 0.5, th + soft * 0.5, lum(p));
        [g, g, g, 1.0]
    })
}

pub fn histogram_scan(t: &Tex, pos: f32, width: f32) -> Tex {
    let lo = pos - width * 0.5;
    let w = width.max(1e-4);
    t.map(|p| {
        let g = ((lum(p) - lo) / w).clamp(0.0, 1.0);
        [g, g, g, 1.0]
    })
}

pub fn quantize(t: &Tex, steps: f32) -> Tex {
    let s = steps.max(2.0) - 1.0;
    t.map(|p| {
        let q = |v: f32| (v * s).round() / s;
        [q(p[0]), q(p[1]), q(p[2]), p[3]]
    })
}

pub fn to_gray(t: &Tex) -> Tex {
    t.map(|p| {
        let g = lum(p);
        [g, g, g, 1.0]
    })
}

pub fn channel(t: &Tex, ch: usize) -> Tex {
    let ch = ch.min(3);
    t.map(|p| [p[ch], p[ch], p[ch], 1.0])
}

pub fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let h = if d < 1e-9 {
        0.0
    } else if mx == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, if mx <= 0.0 { 0.0 } else { d / mx }, mx)
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h6 = h.rem_euclid(1.0) * 6.0;
    let i = h6.floor();
    let f = h6 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

pub fn hsl_adjust(t: &Tex, hue: f32, sat: f32, val: f32) -> Tex {
    t.map(|p| {
        let (h, s, v) = rgb_to_hsv(p[0], p[1], p[2]);
        let (r, g, b) = hsv_to_rgb(h + hue, (s * sat).clamp(0.0, 1.0), (v * val).clamp(0.0, 1.0));
        [r, g, b, p[3]]
    })
}

/// Gradient map. stops: (position, rgb).
pub fn colorize(t: &Tex, stops: &[(f32, [f32; 3])]) -> Tex {
    let mut st: Vec<(f32, [f32; 3])> = stops.to_vec();
    st.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    t.map(|p| {
        let g = lum(p).clamp(0.0, 1.0);
        if st.is_empty() {
            return [g, g, g, p[3]];
        }
        if g <= st[0].0 {
            let c = st[0].1;
            return [c[0], c[1], c[2], p[3]];
        }
        for i in 1..st.len() {
            if g <= st[i].0 {
                let (a, b) = (st[i - 1], st[i]);
                let d = (b.0 - a.0).max(1e-9);
                let f = (g - a.0) / d;
                return [
                    a.1[0] + (b.1[0] - a.1[0]) * f,
                    a.1[1] + (b.1[1] - a.1[1]) * f,
                    a.1[2] + (b.1[2] - a.1[2]) * f,
                    p[3],
                ];
            }
        }
        let c = st[st.len() - 1].1;
        [c[0], c[1], c[2], p[3]]
    })
}

// ---------- blending ----------
fn bm(mode: u32, f: f32, b: f32) -> f32 {
    match mode {
        0 => f,
        1 => f + b,
        2 => b - f,
        3 => f * b,
        4 => 1.0 - (1.0 - f) * (1.0 - b),
        5 => {
            if b < 0.5 {
                2.0 * f * b
            } else {
                1.0 - 2.0 * (1.0 - f) * (1.0 - b)
            }
        }
        6 => f.max(b),
        7 => f.min(b),
        8 => (f - b).abs(),
        9 => (1.0 - 2.0 * f) * b * b + 2.0 * f * b,
        _ => f,
    }
}

/// Modes: 0 copy,1 add,2 sub,3 multiply,4 screen,5 overlay,6 max,7 min,8 difference,9 soft light.
pub fn blend(fg: &Tex, bg: &Tex, mask: Option<&Tex>, mode: u32, opacity: f32) -> Tex {
    let mut out = bg.clone();
    for i in 0..out.px.len() {
        let f = fg.px.get(i).copied().unwrap_or([0.0; 4]);
        let b = bg.px[i];
        let m = mask.and_then(|m| m.px.get(i)).map(|p| lum(*p)).unwrap_or(1.0) * opacity;
        for c in 0..3 {
            out.px[i][c] = (b[c] + (bm(mode, f[c], b[c]).clamp(0.0, 1.0) - b[c]) * m).clamp(0.0, 1.0);
        }
    }
    out
}

// ---------- geometry ----------
pub fn transform(t: &Tex, ox: f32, oy: f32, sx: f32, sy: f32, rot: f32) -> Tex {
    let (s, c) = (rot * TAU).sin_cos();
    let sx = sx.abs().max(1e-4);
    let sy = sy.abs().max(1e-4);
    Tex::from_fn(t.w, t.h, |u, v| {
        let x = u - 0.5;
        let y = v - 0.5;
        let rx = c * x + s * y;
        let ry = -s * x + c * y;
        t.sample(rx / sx + 0.5 - ox, ry / sy + 0.5 - oy)
    })
}

pub fn mirror(t: &Tex, axis: u32) -> Tex {
    let (w, h) = (t.w as i64, t.h as i64);
    let mut o = t.clone();
    for y in 0..h {
        for x in 0..w {
            let sx = if axis == 0 || axis == 2 { x.min(w - 1 - x) } else { x };
            let sy = if axis == 1 || axis == 2 { y.min(h - 1 - y) } else { y };
            o.px[(y * w + x) as usize] = t.at(sx, sy);
        }
    }
    o
}

/// Displace by the gradient of `w` (slope warp).
pub fn warp(t: &Tex, w: &Tex, intensity: f32) -> Tex {
    Tex::from_fn(t.w, t.h, |u, v| {
        let x = (u * w.w as f32) as i64;
        let y = (v * w.h as f32) as i64;
        let dx = w.gray_at(x + 1, y) - w.gray_at(x - 1, y);
        let dy = w.gray_at(x, y + 1) - w.gray_at(x, y - 1);
        t.sample(u + dx * intensity, v + dy * intensity)
    })
}

pub fn directional_warp(t: &Tex, w: &Tex, angle: f32, intensity: f32) -> Tex {
    let (s, c) = (angle * TAU).sin_cos();
    Tex::from_fn(t.w, t.h, |u, v| {
        let d = (lum(w.sample(u, v)) - 0.5) * intensity;
        t.sample(u + c * d, v + s * d)
    })
}

// ---------- surface analysis ----------
pub fn normal_from_height(h: &Tex, strength: f32, directx: bool) -> Tex {
    let k = strength * h.w as f32 * 0.05;
    Tex::from_fn(h.w, h.h, |u, v| {
        let x = (u * h.w as f32) as i64;
        let y = (v * h.h as f32) as i64;
        let dx = (h.gray_at(x + 1, y) - h.gray_at(x - 1, y)) * 0.5 * k;
        let mut dy = (h.gray_at(x, y + 1) - h.gray_at(x, y - 1)) * 0.5 * k;
        if directx {
            dy = -dy;
        }
        let (nx, ny, nz) = (-dx, dy, 1.0f32);
        let l = (nx * nx + ny * ny + nz * nz).sqrt();
        [nx / l * 0.5 + 0.5, ny / l * 0.5 + 0.5, nz / l * 0.5 + 0.5, 1.0]
    })
}

pub fn normal_blend(a: &Tex, b: &Tex) -> Tex {
    let mut o = a.clone();
    for i in 0..o.px.len() {
        let pa = a.px[i];
        let pb = b.px.get(i).copied().unwrap_or([0.5, 0.5, 1.0, 1.0]);
        let x = (pa[0] - 0.5) * 2.0 + (pb[0] - 0.5) * 2.0;
        let y = (pa[1] - 0.5) * 2.0 + (pb[1] - 0.5) * 2.0;
        let z = (pa[2] - 0.5) * 2.0 * (pb[2] - 0.5) * 2.0;
        let l = (x * x + y * y + z * z).sqrt().max(1e-6);
        o.px[i] = [x / l * 0.5 + 0.5, y / l * 0.5 + 0.5, z / l * 0.5 + 0.5, 1.0];
    }
    o
}

pub fn ambient_occlusion(h: &Tex, radius: f32, intensity: f32) -> Tex {
    let b = blur(&to_gray(h), radius);
    let g = to_gray(h);
    let mut o = g.clone();
    for i in 0..o.px.len() {
        let v = (1.0 - (b.px[i][0] - g.px[i][0]) * intensity).clamp(0.0, 1.0);
        o.px[i] = [v, v, v, 1.0];
    }
    o
}

pub fn curvature(h: &Tex, radius: f32, intensity: f32) -> Tex {
    let g = to_gray(h);
    let b = blur(&g, radius);
    let mut o = g.clone();
    for i in 0..o.px.len() {
        let v = (0.5 + (g.px[i][0] - b.px[i][0]) * intensity).clamp(0.0, 1.0);
        o.px[i] = [v, v, v, 1.0];
    }
    o
}

pub fn edge_detect(t: &Tex, intensity: f32) -> Tex {
    Tex::from_fn(t.w, t.h, |u, v| {
        let x = (u * t.w as f32) as i64;
        let y = (v * t.h as f32) as i64;
        let gx = -t.gray_at(x - 1, y - 1) - 2.0 * t.gray_at(x - 1, y) - t.gray_at(x - 1, y + 1)
            + t.gray_at(x + 1, y - 1)
            + 2.0 * t.gray_at(x + 1, y)
            + t.gray_at(x + 1, y + 1);
        let gy = -t.gray_at(x - 1, y - 1) - 2.0 * t.gray_at(x, y - 1) - t.gray_at(x + 1, y - 1)
            + t.gray_at(x - 1, y + 1)
            + 2.0 * t.gray_at(x, y + 1)
            + t.gray_at(x + 1, y + 1);
        let m = ((gx * gx + gy * gy).sqrt() * intensity).clamp(0.0, 1.0);
        [m, m, m, 1.0]
    })
}

pub fn emboss(t: &Tex, angle: f32, intensity: f32) -> Tex {
    let (s, c) = (angle * TAU).sin_cos();
    Tex::from_fn(t.w, t.h, |u, v| {
        let x = (u * t.w as f32) as i64;
        let y = (v * t.h as f32) as i64;
        let dx = t.gray_at(x + 1, y) - t.gray_at(x - 1, y);
        let dy = t.gray_at(x, y + 1) - t.gray_at(x, y - 1);
        let g = (0.5 + (dx * c + dy * s) * intensity).clamp(0.0, 1.0);
        [g, g, g, 1.0]
    })
}

// ---------- shapes ----------
pub fn polygon_sdf(x: f32, y: f32, sides: f32, radius: f32, rot: f32) -> f32 {
    let n = sides.round().max(3.0);
    let a = y.atan2(x) - rot * TAU;
    let sector = TAU / n;
    let r = (x * x + y * y).sqrt();
    let local = a.rem_euclid(sector) - sector * 0.5;
    r * local.cos() - radius * (PI / n).cos()
}

pub fn bricks(u: f32, v: f32, rows: f32, cols: f32, mortar: f32, bevel: f32, offset: f32, var: f32, seed: u32) -> f32 {
    let rows = rows.round().max(1.0);
    let cols = cols.round().max(1.0);
    let row = (v * rows).floor();
    let shift = if (row as i64) % 2 != 0 { offset } else { 0.0 };
    let cx = u * cols + shift;
    let col = cx.floor();
    let fu = cx - col;
    let fv = v * rows - row;
    let e = fu.min(1.0 - fu).min(fv.min(1.0 - fv));
    let hgt = ((e - mortar) / bevel.max(1e-4)).clamp(0.0, 1.0);
    let id = crate::noise::rnd(col.rem_euclid(cols) as i32, row as i32, seed);
    hgt * (1.0 - var + var * id)
}

// ---------- scattering ----------
#[allow(clippy::too_many_arguments)]
pub fn tile_sampler(
    w: usize,
    h: usize,
    pattern: Option<&Tex>,
    count: u32,
    scale: f32,
    scale_var: f32,
    rot_var: f32,
    lum_var: f32,
    seed: u32,
) -> Tex {
    use crate::noise::rnd;
    let inst: Vec<(f32, f32, f32, f32, f32)> = (0..count.min(2000))
        .map(|i| {
            let i = i as i32;
            (
                rnd(i, 1, seed),
                rnd(i, 2, seed),
                (scale * (1.0 + (rnd(i, 3, seed) - 0.5) * 2.0 * scale_var)).max(1e-3),
                (rnd(i, 4, seed) - 0.5) * rot_var * TAU,
                1.0 - lum_var * rnd(i, 5, seed),
            )
        })
        .collect();
    Tex::from_fn(w, h, |u, v| {
        let mut best = 0.0f32;
        for &(cx, cy, s, r, l) in &inst {
            let dx = (u - cx + 0.5).rem_euclid(1.0) - 0.5;
            let dy = (v - cy + 0.5).rem_euclid(1.0) - 0.5;
            let (sn, cs) = r.sin_cos();
            let lx = (cs * dx + sn * dy) / s + 0.5;
            let ly = (-sn * dx + cs * dy) / s + 0.5;
            if !(0.0..=1.0).contains(&lx) || !(0.0..=1.0).contains(&ly) {
                continue;
            }
            let val = match pattern {
                Some(p) => lum(p.sample(lx, ly)),
                None => {
                    let d = ((lx - 0.5).powi(2) + (ly - 0.5).powi(2)).sqrt() * 2.0;
                    1.0 - sm(0.8, 1.0, d)
                }
            };
            best = best.max(val * l);
        }
        [best, best, best, 1.0]
    })
}

// ---------- 3D-ish preview ----------
pub fn lit_preview(base: &Tex, normal: &Tex, rough: &Tex, metal: &Tex, ao: &Tex, light: [f32; 3]) -> Tex {
    let l = {
        let n = (light[0] * light[0] + light[1] * light[1] + light[2] * light[2]).sqrt().max(1e-6);
        [light[0] / n, light[1] / n, light[2] / n]
    };
    let hv = {
        let x = l[0];
        let y = l[1];
        let z = l[2] + 1.0;
        let n = (x * x + y * y + z * z).sqrt().max(1e-6);
        [x / n, y / n, z / n]
    };
    Tex::from_fn(base.w, base.h, |u, v| {
        let b = base.sample(u, v);
        let n = normal.sample(u, v);
        let (nx, ny, nz) = (n[0] * 2.0 - 1.0, n[1] * 2.0 - 1.0, n[2] * 2.0 - 1.0);
        let r = lum(rough.sample(u, v)).clamp(0.05, 1.0);
        let m = lum(metal.sample(u, v)).clamp(0.0, 1.0);
        let o = lum(ao.sample(u, v));
        let ndl = (nx * l[0] + ny * l[1] + nz * l[2]).max(0.0);
        let ndh = (nx * hv[0] + ny * hv[1] + nz * hv[2]).max(0.0);
        let shin = 2.0 / (r * r * r * r) - 2.0;
        let spec = ndh.powf(shin.clamp(1.0, 2000.0)) * (1.0 - r * 0.5);
        let mut out = [0.0; 4];
        for c in 0..3 {
            let diff = b[c] * (1.0 - m) * ndl;
            let sp = spec * (0.04 * (1.0 - m) + b[c] * m);
            out[c] = ((0.1 * b[c] + diff + sp) * o).clamp(0.0, 1.0);
        }
        out[3] = 1.0;
        out
    })
}

// ---------- SDF (2D and procedural 3D) ----------
fn sd_box(x: f32, y: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = x.abs() - hx + r;
    let qy = y.abs() - hy + r;
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r
}

/// 2D signed distance (negative inside). shape: 0 circle, 1 box, 2 polygon, 3 ring.
#[allow(clippy::too_many_arguments)]
pub fn sdf2d(shape: u32, x: f32, y: f32, size: f32, aspect: f32, sides: f32, rot: f32, round: f32) -> f32 {
    let (s, c) = (rot * TAU).sin_cos();
    let (rx, ry) = (c * x + s * y, -s * x + c * y);
    match shape {
        0 => (x * x + y * y).sqrt() - size,
        1 => sd_box(rx, ry, size, size * aspect.max(0.01), round.min(size * 0.99)),
        2 => polygon_sdf(x, y, sides, size, rot),
        _ => ((x * x + y * y).sqrt() - size).abs() - (size * 0.2).max(0.005),
    }
}

pub fn sdf_to_value(d: f32, range: f32) -> f32 {
    (0.5 - d / (2.0 * range.max(1e-4))).clamp(0.0, 1.0)
}

pub fn value_to_sdf(v: f32, range: f32) -> f32 {
    (0.5 - v) * 2.0 * range.max(1e-4)
}

fn smin(a: f32, b: f32, k: f32) -> f32 {
    if k < 1e-5 {
        return a.min(b);
    }
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b * (1.0 - h) + a * h - k * h * (1.0 - h)
}

/// op: 0 union, 1 subtract (a - b), 2 intersect. k = smoothing radius.
pub fn sdf_combine(op: u32, da: f32, db: f32, k: f32) -> f32 {
    match op {
        0 => smin(da, db, k),
        1 => -smin(-da, db, k),
        _ => -smin(-da, -db, k),
    }
}

fn map3(shape: u32, x: f32, y: f32, z: f32, s: f32, r: f32) -> f32 {
    match shape {
        0 => (x * x + y * y + z * z).sqrt() - s,
        1 => {
            let q = [x.abs() - s + r, y.abs() - s + r, z.abs() - s * 0.8 + r];
            let o = (q[0].max(0.0).powi(2) + q[1].max(0.0).powi(2) + q[2].max(0.0).powi(2)).sqrt();
            o + q[0].max(q[1]).max(q[2]).min(0.0) - r
        }
        2 => {
            let a = ((x * x + y * y).sqrt() - s * 0.7, z);
            (a.0 * a.0 + a.1 * a.1).sqrt() - s * 0.3
        }
        _ => {
            let d = (((x * x + y * y).sqrt() - s), z.abs() - s);
            d.0.max(d.1).min(0.0) + (d.0.max(0.0).powi(2) + d.1.max(0.0).powi(2)).sqrt()
        }
    }
}

/// Height of a procedural 3D SDF form, ray-marched from the top. Domain-repeated and tileable.
#[allow(clippy::too_many_arguments)]
pub fn sdf_form(u: f32, v: f32, shape: u32, size: f32, repeat: f32, round: f32, scatter: f32, seed: u32, hscale: f32) -> f32 {
    let rep = repeat.round().clamp(1.0, 32.0);
    let (cu, cv) = (u * rep, v * rep);
    let (ci, cj) = (cu.floor() as i32, cv.floor() as i32);
    let (x, y) = (cu - ci as f32 - 0.5, cv - cj as f32 - 0.5);
    let (wi, wj) = (ci.rem_euclid(rep as i32), cj.rem_euclid(rep as i32));
    let r1 = crate::noise::rnd(wi, wj, seed);
    let s = (size * (1.0 - scatter * 0.6 * r1)).clamp(0.01, 0.5);
    let zc = match shape {
        0 | 3 => s,
        1 => s * 0.8,
        _ => s * 0.3,
    };
    let top = 2.0f32;
    let mut t = 0.0f32;
    for _ in 0..72 {
        let z = top - t;
        let d = map3(shape, x, y, z - zc, s, round.min(s * 0.9));
        if d < 0.0007 {
            return (z * hscale).clamp(0.0, 1.0);
        }
        t += d.max(0.003);
        if t > top {
            break;
        }
    }
    0.0
}
