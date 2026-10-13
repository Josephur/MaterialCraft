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
pub fn colorize(t: &Tex, stops: &[(f32, [f32; 3])], interp: u32) -> Tex {
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
                let mut f = (g - a.0) / d;
                if interp == 1 {
                    f = f * f * (3.0 - 2.0 * f);
                } else if interp == 2 {
                    f = 0.0;
                }
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
pub fn transform(t: &Tex, ox: f32, oy: f32, sx: f32, sy: f32, rot: f32, tile: bool) -> Tex {
    let (s, c) = (rot * TAU).sin_cos();
    let sx = sx.abs().max(1e-4);
    let sy = sy.abs().max(1e-4);
    Tex::from_fn(t.w, t.h, |u, v| {
        let x = u - 0.5;
        let y = v - 0.5;
        let rx = c * x + s * y;
        let ry = -s * x + c * y;
        let (su, sv) = (rx / sx + 0.5 - ox, ry / sy + 0.5 - oy);
        if !tile && !(0.0..1.0).contains(&su) || !tile && !(0.0..1.0).contains(&sv) {
            return [0.0, 0.0, 0.0, 1.0];
        }
        t.sample(su, sv)
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
pub fn splatter(
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

// =====================================================================================
// Additional nodes
// =====================================================================================
use crate::noise::{rnd, voronoi_ext};

pub fn contrast_invert(t: &Tex, contrast: f32, invert: bool) -> Tex {
    t.map(|p| {
        let mut o = p;
        for c in 0..3 {
            let v = ((p[c] - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
            o[c] = if invert { 1.0 - v } else { v };
        }
        o
    })
}

/// Procedural shape. kind: 0 square, 1 disc, 2 paraboloid, 3 gaussian, 4 diamond, 5 pyramid, 6 half sphere, 7 cone.
#[allow(clippy::too_many_arguments)]
pub fn shape_value(kind: u32, u: f32, v: f32, px: f32, py: f32, sx: f32, sy: f32, rot: f32, tiling: f32, soft: f32) -> f32 {
    let t = tiling.round().clamp(1.0, 32.0);
    let x = ((u - 0.5 - px) * t + 0.5).rem_euclid(1.0) - 0.5;
    let y = ((v - 0.5 - py) * t + 0.5).rem_euclid(1.0) - 0.5;
    let (s, c) = (rot * TAU).sin_cos();
    let (rx, ry) = (c * x + s * y, -s * x + c * y);
    let nx = rx / (sx.max(1e-3) * 0.5);
    let ny = ry / (sy.max(1e-3) * 0.5);
    let r = (nx * nx + ny * ny).sqrt();
    let so = soft.clamp(0.0, 1.0).max(1e-3);
    match kind {
        0 => 1.0 - sm(1.0 - so, 1.0, nx.abs().max(ny.abs())),
        1 => 1.0 - sm(1.0 - so, 1.0, r),
        2 => (1.0 - r * r).clamp(0.0, 1.0),
        3 => (-(r * 1.8).powi(2)).exp(),
        4 => 1.0 - sm(1.0 - so, 1.0, nx.abs() + ny.abs()),
        5 => (1.0 - nx.abs().max(ny.abs())).clamp(0.0, 1.0),
        6 => (1.0 - r * r).max(0.0).sqrt(),
        _ => (1.0 - r).clamp(0.0, 1.0),
    }
}

pub struct TileP {
    pub nx: f32,
    pub ny: f32,
    pub size: f32,
    pub size_rand: f32,
    pub rot: f32,
    pub rot_rand: f32,
    pub pos_rand: f32,
    pub color: f32,
    pub color_rand: f32,
    pub offset: f32,
    pub seed: u32,
    pub thresh: f32,
    pub default_disc: bool,
    pub brick: bool,
}

/// Grid of pattern instances (Tile Sampler / Tile Generator).
pub fn tile_grid(w: usize, h: usize, pattern: Option<&Tex>, mask: Option<&Tex>, p: &TileP) -> Tex {
    let nx = p.nx.round().clamp(1.0, 64.0) as i32;
    let ny = p.ny.round().clamp(1.0, 64.0) as i32;
    let (fnx, fny) = (nx as f32, ny as f32);
    Tex::from_fn(w, h, |u, v| {
        let (ci, cj) = ((u * fnx).floor() as i32, (v * fny).floor() as i32);
        let mut best = 0.0f32;
        for dj in -1..=1 {
            for di in -1..=1 {
                let (i, j) = (ci + di, cj + dj);
                let (wi, wj) = (i.rem_euclid(nx), j.rem_euclid(ny));
                let shift = if p.brick && (wj & 1) == 1 { p.offset } else { 0.0 };
                let (r1, r2, r3, r4) = (rnd(wi, wj, p.seed), rnd(wi, wj, p.seed.wrapping_add(1)), rnd(wi, wj, p.seed.wrapping_add(2)), rnd(wi, wj, p.seed.wrapping_add(3)));
                let cx = (i as f32 + 0.5 + shift) / fnx + (r1 - 0.5) * p.pos_rand / fnx;
                let cy = (j as f32 + 0.5) / fny + (r2 - 0.5) * p.pos_rand / fny;
                if let Some(m) = mask {
                    if lum(m.sample(cx, cy)) < p.thresh {
                        continue;
                    }
                }
                let s = (p.size * (1.0 + (r3 - 0.5) * 2.0 * p.size_rand)).max(0.01);
                let ang = (p.rot + (r4 - 0.5) * 2.0 * p.rot_rand) * TAU;
                let col = p.color * (1.0 - p.color_rand * rnd(wi, wj, p.seed.wrapping_add(9)));
                let dx = (u - cx + 0.5).rem_euclid(1.0) - 0.5;
                let dy = (v - cy + 0.5).rem_euclid(1.0) - 0.5;
                let (sn, cs) = ang.sin_cos();
                let lx = (cs * dx + sn * dy) * fnx / s + 0.5;
                let ly = (-sn * dx + cs * dy) * fny / s + 0.5;
                if !(0.0..=1.0).contains(&lx) || !(0.0..=1.0).contains(&ly) {
                    continue;
                }
                let val = match pattern {
                    Some(pt) => lum(pt.sample(lx, ly)),
                    None if p.default_disc => 1.0 - sm(0.85, 1.0, (((lx - 0.5) * 2.0).powi(2) + ((ly - 0.5) * 2.0).powi(2)).sqrt()),
                    None => 1.0,
                };
                best = best.max(val * col);
            }
        }
        [best, best, best, 1.0]
    })
}

/// Instances placed on a circle.
#[allow(clippy::too_many_arguments)]
pub fn splatter_circular(w: usize, h: usize, pattern: Option<&Tex>, n: u32, radius: f32, size: f32, size_rand: f32, rot: f32, rot_rand: f32, pos_rand: f32, seed: u32) -> Tex {
    let n = n.clamp(1, 256);
    let inst: Vec<(f32, f32, f32, f32)> = (0..n)
        .map(|k| {
            let i = k as i32;
            let a = k as f32 / n as f32 * TAU;
            let cx = 0.5 + radius * a.cos() + (rnd(i, 1, seed) - 0.5) * pos_rand * 0.2;
            let cy = 0.5 + radius * a.sin() + (rnd(i, 2, seed) - 0.5) * pos_rand * 0.2;
            let s = (size * (1.0 + (rnd(i, 3, seed) - 0.5) * 2.0 * size_rand)).max(0.005);
            let r = (rot + (rnd(i, 4, seed) - 0.5) * 2.0 * rot_rand) * TAU;
            (cx, cy, s, r)
        })
        .collect();
    Tex::from_fn(w, h, |u, v| {
        let mut best = 0.0f32;
        for &(cx, cy, s, r) in &inst {
            let (dx, dy) = (u - cx, v - cy);
            let (sn, cs) = r.sin_cos();
            let (lx, ly) = ((cs * dx + sn * dy) / s + 0.5, (-sn * dx + cs * dy) / s + 0.5);
            if !(0.0..=1.0).contains(&lx) || !(0.0..=1.0).contains(&ly) {
                continue;
            }
            let val = match pattern {
                Some(p) => lum(p.sample(lx, ly)),
                None => 1.0 - sm(0.85, 1.0, (((lx - 0.5) * 2.0).powi(2) + ((ly - 0.5) * 2.0).powi(2)).sqrt()),
            };
            best = best.max(val);
        }
        [best, best, best, 1.0]
    })
}

// ---------------- flood fill family ----------------
pub fn flood_fill(t: &Tex, thresh: f32) -> Tex {
    let (w, h) = (t.w, t.h);
    let n = w * h;
    let mask: Vec<bool> = t.px.iter().map(|p| lum(*p) > thresh).collect();
    let mut label = vec![0u32; n];
    let mut bbox: Vec<[u32; 4]> = vec![[0; 4]];
    let mut stack: Vec<usize> = Vec::new();
    let mut count = 0u32;
    for s in 0..n {
        if !mask[s] || label[s] != 0 {
            continue;
        }
        count += 1;
        label[s] = count;
        stack.push(s);
        let mut bb = [w as u32, h as u32, 0u32, 0u32];
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            bb = [bb[0].min(x as u32), bb[1].min(y as u32), bb[2].max(x as u32), bb[3].max(y as u32)];
            for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let nx = (x as i64 + dx).rem_euclid(w as i64) as usize;
                let ny = (y as i64 + dy).rem_euclid(h as i64) as usize;
                let j = ny * w + nx;
                if mask[j] && label[j] == 0 {
                    label[j] = count;
                    stack.push(j);
                }
            }
        }
        bbox.push(bb);
    }
    let px = (0..n)
        .map(|i| {
            if label[i] == 0 {
                return [0.0, 0.0, 0.0, 1.0];
            }
            let b = bbox[label[i] as usize];
            let (x, y) = (i % w, i / w);
            let gx = ((x as f32 - b[0] as f32) + 0.5) / ((b[2] - b[0] + 1) as f32);
            let gy = ((y as f32 - b[1] as f32) + 0.5) / ((b[3] - b[1] + 1) as f32);
            [0.1 + 0.9 * rnd(label[i] as i32, 3, 17), gx, gy, 1.0]
        })
        .collect();
    Tex { w, h, px }
}

fn region_id(p: [f32; 4]) -> Option<i32> {
    if p[0] == 0.0 && p[1] == 0.0 && p[2] == 0.0 {
        None
    } else {
        Some((p[0] * 100_000.0) as i32)
    }
}

pub fn flood_fill_random(t: &Tex, seed: u32, lo: f32, hi: f32) -> Tex {
    t.map(|p| match region_id(p) {
        None => [0.0, 0.0, 0.0, 1.0],
        Some(id) => {
            let g = lo + (hi - lo) * rnd(id, 5, seed);
            [g, g, g, 1.0]
        }
    })
}

pub fn flood_fill_color(t: &Tex, seed: u32) -> Tex {
    t.map(|p| match region_id(p) {
        None => [0.0, 0.0, 0.0, 1.0],
        Some(id) => [0.2 + 0.8 * rnd(id, 6, seed), 0.2 + 0.8 * rnd(id, 7, seed), 0.2 + 0.8 * rnd(id, 8, seed), 1.0],
    })
}

pub fn flood_fill_gradient(t: &Tex, angle: f32, variation: f32, seed: u32) -> Tex {
    t.map(|p| match region_id(p) {
        None => [0.0, 0.0, 0.0, 1.0],
        Some(id) => {
            let a = angle * TAU + (rnd(id, 9, seed) - 0.5) * variation * TAU;
            let (s, c) = a.sin_cos();
            let g = (0.5 + ((p[1] - 0.5) * c + (p[2] - 0.5) * s) / (c.abs() + s.abs()).max(1e-3)).clamp(0.0, 1.0);
            [g, g, g, 1.0]
        }
    })
}

// ---------------- distance / bevel ----------------
/// Approximate Euclidean distance (pixels) from each `true` pixel to the nearest `false` pixel. Wraps (tileable).
pub fn distance_field(mask: &[bool], w: usize, h: usize) -> Vec<f32> {
    let mut d: Vec<f32> = mask.iter().map(|&m| if m { 1e9 } else { 0.0 }).collect();
    let idx = |x: i64, y: i64| (y.rem_euclid(h as i64) as usize) * w + x.rem_euclid(w as i64) as usize;
    for _ in 0..2 {
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let i = idx(x, y);
                for (dx, dy, c) in [(-1i64, 0i64, 1.0f32), (0, -1, 1.0), (-1, -1, 1.414), (1, -1, 1.414)] {
                    let j = idx(x + dx, y + dy);
                    d[i] = d[i].min(d[j] + c);
                }
            }
        }
        for y in (0..h as i64).rev() {
            for x in (0..w as i64).rev() {
                let i = idx(x, y);
                for (dx, dy, c) in [(1i64, 0i64, 1.0f32), (0, 1, 1.0), (1, 1, 1.414), (-1, 1, 1.414)] {
                    let j = idx(x + dx, y + dy);
                    d[i] = d[i].min(d[j] + c);
                }
            }
        }
    }
    d
}

pub fn bevel(t: &Tex, dist_px: f32, smoothing: f32, thr: f32) -> Tex {
    let mask: Vec<bool> = t.px.iter().map(|p| lum(*p) > thr).collect();
    let d = distance_field(&mask, t.w, t.h);
    let px = d
        .iter()
        .zip(&mask)
        .map(|(d, m)| {
            if !*m {
                return [0.0, 0.0, 0.0, 1.0];
            }
            let tt = (d / dist_px.max(1.0)).clamp(0.0, 1.0);
            let g = tt + (tt * tt * (3.0 - 2.0 * tt) - tt) * smoothing.clamp(0.0, 1.0);
            [g, g, g, 1.0]
        })
        .collect();
    Tex { w: t.w, h: t.h, px }
}

/// mode: 0 inside, 1 outside, 2 both (0.5 = edge).
pub fn distance(t: &Tex, range_px: f32, thr: f32, mode: u32) -> Tex {
    let mask: Vec<bool> = t.px.iter().map(|p| lum(*p) > thr).collect();
    let inside = distance_field(&mask, t.w, t.h);
    let inv: Vec<bool> = mask.iter().map(|m| !m).collect();
    let outside = distance_field(&inv, t.w, t.h);
    let r = range_px.max(1.0);
    let px = (0..mask.len())
        .map(|i| {
            let (a, b) = ((inside[i] / r).clamp(0.0, 1.0), (outside[i] / r).clamp(0.0, 1.0));
            let g = match mode {
                0 => a,
                1 => b,
                _ => 0.5 + (a - b) * 0.5,
            };
            [g, g, g, 1.0]
        })
        .collect();
    Tex { w: t.w, h: t.h, px }
}

// ---------------- warps ----------------
pub fn vector_warp(t: &Tex, vecmap: &Tex, intensity: f32) -> Tex {
    Tex::from_fn(t.w, t.h, |u, v| {
        let m = vecmap.sample(u, v);
        t.sample(u + (m[0] - 0.5) * 2.0 * intensity, v + (m[1] - 0.5) * 2.0 * intensity)
    })
}

/// mode: 0 blur (average), 1 min, 2 max.
pub fn slope_blur(t: &Tex, slope: &Tex, samples: u32, intensity: f32, mode: u32) -> Tex {
    let g = Tex::from_fn(slope.w, slope.h, |u, v| {
        let x = (u * slope.w as f32) as i64;
        let y = (v * slope.h as f32) as i64;
        [slope.gray_at(x + 1, y) - slope.gray_at(x - 1, y), slope.gray_at(x, y + 1) - slope.gray_at(x, y - 1), 0.0, 1.0]
    });
    let n = samples.clamp(1, 64);
    let step = intensity / n as f32;
    let gain = slope.w as f32 * 0.5;
    Tex::from_fn(t.w, t.h, |u, v| {
        let (mut pu, mut pv) = (u, v);
        let mut acc = match mode {
            1 => [1.0f32; 4],
            2 => [0.0f32; 4],
            _ => [0.0f32; 4],
        };
        for _ in 0..n {
            let p = t.sample(pu, pv);
            for c in 0..4 {
                acc[c] = match mode {
                    1 => acc[c].min(p[c]),
                    2 => acc[c].max(p[c]),
                    _ => acc[c] + p[c] / n as f32,
                };
            }
            let gv = g.sample(pu, pv);
            let l = (gv[0] * gv[0] + gv[1] * gv[1]).sqrt().max(1e-6);
            let mag = (l * gain).min(1.0);
            pu += gv[0] / l * mag * step;
            pv += gv[1] / l * mag * step;
        }
        acc
    })
}

pub fn swirl(t: &Tex, amount: f32, radius: f32) -> Tex {
    let rad = radius.max(1e-3);
    Tex::from_fn(t.w, t.h, |u, v| {
        let (x, y) = (u - 0.5, v - 0.5);
        let r = (x * x + y * y).sqrt();
        let k = (1.0 - r / rad).clamp(0.0, 1.0);
        let a = amount * k * k * TAU;
        let (s, c) = a.sin_cos();
        t.sample(0.5 + c * x - s * y, 0.5 + s * x + c * y)
    })
}

pub fn highpass(t: &Tex, radius: f32) -> Tex {
    let b = blur(t, radius);
    let mut o = t.clone();
    for (i, p) in o.px.iter_mut().enumerate() {
        for c in 0..3 {
            p[c] = (t.px[i][c] - b.px[i][c] + 0.5).clamp(0.0, 1.0);
        }
    }
    o
}

// ---------------- colour / mask helpers ----------------
pub fn color_match(t: &Tex, reference: &Tex, strength: f32) -> Tex {
    let stats = |x: &Tex| -> ([f32; 3], [f32; 3]) {
        let n = x.px.len().max(1) as f32;
        let mut m = [0.0f32; 3];
        for p in &x.px {
            for c in 0..3 {
                m[c] += p[c] / n;
            }
        }
        let mut sd = [0.0f32; 3];
        for p in &x.px {
            for c in 0..3 {
                sd[c] += (p[c] - m[c]).powi(2) / n;
            }
        }
        (m, [sd[0].sqrt(), sd[1].sqrt(), sd[2].sqrt()])
    };
    let ((m1, s1), (m2, s2)) = (stats(t), stats(reference));
    t.map(|p| {
        let mut o = p;
        for c in 0..3 {
            let matched = (p[c] - m1[c]) * (s2[c] / s1[c].max(1e-4)) + m2[c];
            o[c] = (p[c] + (matched - p[c]) * strength.clamp(0.0, 1.0)).clamp(0.0, 1.0);
        }
        o
    })
}

/// Curvature from a tangent-space normal map (divergence of the normal), smoothed by `smooth_px`.
pub fn curvature_from_normal(n: &Tex, smooth_px: f32, intensity: f32) -> Tex {
    let c = Tex::from_fn(n.w, n.h, |u, v| {
        let (x, y) = ((u * n.w as f32) as i64, (v * n.h as f32) as i64);
        let div = (n.at(x + 1, y)[0] - n.at(x - 1, y)[0]) - (n.at(x, y + 1)[1] - n.at(x, y - 1)[1]);
        let g = (0.5 + div * intensity).clamp(0.0, 1.0);
        [g, g, g, 1.0]
    });
    blur(&c, smooth_px)
}

/// Returns (height, blend factor).
pub fn height_blend(bg: &Tex, fg: &Tex, mask: Option<&Tex>, opacity: f32, contrast: f32) -> (Tex, Tex) {
    let mut h = bg.clone();
    let mut f = bg.clone();
    for i in 0..h.px.len() {
        let m = mask.and_then(|m| m.px.get(i)).map(|p| lum(*p)).unwrap_or(1.0) * opacity;
        let (b, g) = (lum(bg.px[i]), lum(fg.px.get(i).copied().unwrap_or([0.0; 4])));
        let score = (g + (m - 0.5) * 2.0) - b;
        let k = (0.5 + score * contrast.max(0.05) * 2.0).clamp(0.0, 1.0) * sm(0.0, 0.08, m);
        let out = b + (g - b) * k;
        h.px[i] = [out, out, out, 1.0];
        f.px[i] = [k, k, k, 1.0];
    }
    (h, f)
}

pub fn make_tile_photo(t: &Tex, width: f32) -> Tex {
    let w = width.clamp(0.1, 1.0);
    let a = |x: f32| sm(0.0, w, 1.0 - (2.0 * x - 1.0).abs());
    Tex::from_fn(t.w, t.h, |u, v| {
        let (au, av) = (a(u), a(v));
        let s = |du: f32, dv: f32| t.sample((u + du).rem_euclid(1.0), (v + dv).rem_euclid(1.0));
        let (c00, c10, c01, c11) = (s(0.0, 0.0), s(0.5, 0.0), s(0.0, 0.5), s(0.5, 0.5));
        let (w00, w10, w01, w11) = (au * av, (1.0 - au) * av, au * (1.0 - av), (1.0 - au) * (1.0 - av));
        let mut o = [0.0; 4];
        for c in 0..4 {
            o[c] = c00[c] * w00 + c10[c] * w10 + c01[c] * w01 + c11[c] * w11;
        }
        o
    })
}

pub fn make_tile_patch(t: &Tex, patches: f32, softness: f32, seed: u32) -> Tex {
    let p = patches.round().clamp(2.0, 32.0) as i32;
    let soft = softness.clamp(0.01, 1.0) * 0.3;
    Tex::from_fn(t.w, t.h, |u, v| {
        let vr = voronoi_ext(u, v, p, p, seed, 1.0);
        let off = |id: f32| ((id * 10_000.0) as i32, rnd((id * 10_000.0) as i32, 1, seed), rnd((id * 10_000.0) as i32, 2, seed));
        let (a, b) = (off(vr.id1), off(vr.id2));
        let (s1, s2) = (t.sample(u + a.1, v + a.2), t.sample(u + b.1, v + b.2));
        let k = 0.5 + 0.5 * sm(0.0, soft, vr.f2 - vr.f1);
        let mut o = [0.0; 4];
        for c in 0..4 {
            o[c] = s2[c] + (s1[c] - s2[c]) * k;
        }
        o
    })
}
