//! Software PBR renderer for the 3D viewer (sphere, cube, plane). GGX specular, hemisphere
//! environment lighting, tangent-space normal mapping, height-map parallax relief and bump.
use crate::tex::{lum, Tex};
use crate::v3::*;
use std::f32::consts::{PI, TAU};

pub struct Maps<'a> {
    pub base: &'a Tex,
    pub normal: &'a Tex,
    pub rough: &'a Tex,
    pub metal: &'a Tex,
    pub ao: &'a Tex,
    pub emissive: Option<&'a Tex>,
    pub height: Option<&'a Tex>,
}

#[derive(Clone, Debug)]
pub struct View {
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub light_yaw: f32,
    pub light_pitch: f32,
    pub light_intensity: f32,
    pub exposure: f32,
    pub ambient: f32,
    pub shape: u32, // 0 sphere, 1 cube, 2 plane
    pub tiling: f32,
    /// Depth of the height map in object units (parallax relief). 0 = off.
    pub height_scale: f32,
    /// Strength of normals derived from the height map. 0 = off.
    pub bump: f32,
}

impl Default for View {
    fn default() -> Self {
        View { yaw: 0.5, pitch: 0.3, dist: 3.4, pan_x: 0.0, pan_y: 0.0, light_yaw: 0.9, light_pitch: 0.7, light_intensity: 3.0, exposure: 1.0, ambient: 1.0, shape: 0, tiling: 1.0, height_scale: 0.08, bump: 1.0 }
    }
}

#[allow(dead_code)]
struct Surf {
    p: V,
    n: V,
    t: V,
    b: V,
    uv: [f32; 2],
    /// object-space length of one uv unit along u and v (for parallax)
    su: f32,
    sv: f32,
}

fn env(d: V) -> V {
    let sky = [0.42, 0.52, 0.68];
    let ground = [0.30, 0.28, 0.26];
    let hor = [0.58, 0.58, 0.58];
    let base = if d[1] > 0.0 { add(mul(hor, 1.0 - d[1]), mul(sky, d[1])) } else { add(mul(hor, 1.0 + d[1] * 0.8), mul(ground, -d[1] * 0.8)) };
    [base[0].max(0.0), base[1].max(0.0), base[2].max(0.0)]
}

fn hit_scene(shape: u32, eye: V, dir: V, tiling: f32) -> Option<(f32, Surf)> {
    match shape {
        0 => {
            let b = dot(eye, dir);
            let c = dot(eye, eye) - 1.0;
            let disc = b * b - c;
            if disc < 0.0 {
                return None;
            }
            let t = -b - disc.sqrt();
            if t < 0.0 {
                return None;
            }
            let p = add(eye, mul(dir, t));
            let lon = p[0].atan2(p[2]);
            let tg = [lon.cos(), 0.0, -lon.sin()];
            let uv = [(lon / TAU + 0.5) * tiling * 2.0, (p[1].clamp(-1.0, 1.0).acos() / PI) * tiling];
            let sin_t = (1.0 - p[1] * p[1]).max(0.0).sqrt().max(0.05);
            Some((t, Surf { p, n: p, t: tg, b: cross(p, tg), uv, su: TAU * sin_t / (tiling * 2.0), sv: PI / tiling }))
        }
        1 => {
            let h = 0.8;
            let (mut t0, mut t1) = (f32::MIN, f32::MAX);
            let mut axis = 0;
            for k in 0..3 {
                let inv = 1.0 / if dir[k].abs() < 1e-9 { 1e-9 } else { dir[k] };
                let (a, b) = ((-h - eye[k]) * inv, (h - eye[k]) * inv);
                let (lo, hi) = (a.min(b), a.max(b));
                if lo > t0 {
                    t0 = lo;
                    axis = k;
                }
                t1 = t1.min(hi);
            }
            if t0 > t1 || t1 < 0.0 || t0 < 0.0 {
                return None;
            }
            let p = add(eye, mul(dir, t0));
            let sgn = if dir[axis] > 0.0 { -1.0 } else { 1.0 };
            let mut n = [0.0; 3];
            n[axis] = sgn;
            let (tg, bt) = match (axis, sgn > 0.0) {
                (0, true) => ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
                (0, false) => ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
                (1, true) => ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
                (1, false) => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                (_, true) => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                (_, false) => ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            };
            let uv = [(dot(p, tg) / (2.0 * h) + 0.5) * tiling, (0.5 - dot(p, bt) / (2.0 * h)) * tiling];
            Some((t0, Surf { p, n, t: tg, b: bt, uv, su: 2.0 * h / tiling, sv: 2.0 * h / tiling }))
        }
        _ => {
            if dir[2].abs() < 1e-9 {
                return None;
            }
            let t = -eye[2] / dir[2];
            if t < 0.0 {
                return None;
            }
            let p = add(eye, mul(dir, t));
            if p[0].abs() > 1.3 || p[1].abs() > 1.3 {
                return None;
            }
            let n = if eye[2] > 0.0 { [0.0, 0.0, 1.0] } else { [0.0, 0.0, -1.0] };
            let tg = if eye[2] > 0.0 { [1.0, 0.0, 0.0] } else { [-1.0, 0.0, 0.0] };
            let uv = [(p[0] / 2.6 + 0.5) * tiling, (0.5 - p[1] / 2.6) * tiling];
            Some((t, Surf { p, n, t: tg, b: [0.0, 1.0, 0.0], uv, su: 2.6 / tiling, sv: 2.6 / tiling }))
        }
    }
}

fn lin(c: f32) -> f32 {
    c.max(0.0).powf(2.2)
}

/// Relief mapping: march the view ray through the height field. Returns (uv, depth fraction 0..1).
fn parallax(hm: &Tex, s: &Surf, vt: V, depth: f32) -> ([f32; 2], f32) {
    let vz = vt[2].max(0.2);
    let du = -vt[0] / vz / s.su;
    let dv = vt[1] / vz / s.sv; // uv v runs opposite to the bitangent
    let at = |d: f32| -> f32 {
        let (u, v) = (s.uv[0] + du * d, s.uv[1] + dv * d);
        (1.0 - lum(hm.sample(u, v))) * depth - d
    };
    let steps = 40;
    let mut prev_d = 0.0;
    let mut prev_f = at(0.0);
    if prev_f <= 0.0 {
        return (s.uv, 0.0);
    }
    for i in 1..=steps {
        let d = depth * i as f32 / steps as f32;
        let f = at(d);
        if f <= 0.0 {
            let t = prev_f / (prev_f - f).max(1e-6);
            let dh = prev_d + (d - prev_d) * t;
            return ([s.uv[0] + du * dh, s.uv[1] + dv * dh], dh / depth);
        }
        prev_d = d;
        prev_f = f;
    }
    ([s.uv[0] + du * depth, s.uv[1] + dv * depth], 1.0)
}

pub fn render(m: &Maps, v: &View, w: usize, h: usize) -> Tex {
    let (sy, cy) = v.yaw.sin_cos();
    let (sp, cp) = v.pitch.sin_cos();
    let eye0 = [v.dist * sy * cp, v.dist * sp, v.dist * cy * cp];
    let fwd = norm(mul(eye0, -1.0));
    let right = norm(cross(fwd, [0.0, 1.0, 0.0]));
    let up = cross(right, fwd);
    let eye = add(eye0, add(mul(right, v.pan_x), mul(up, v.pan_y)));
    let tf = (20.0f32).to_radians().tan();
    let aspect = w as f32 / h as f32;
    let (lsy, lcy) = v.light_yaw.sin_cos();
    let (lsp, lcp) = v.light_pitch.sin_cos();
    let ldir = norm([lsy * lcp, lsp, lcy * lcp]);
    let fill = norm([-ldir[0] * 0.7, -0.45, -ldir[2] * 0.5]);
    Tex::from_fn(w, h, |px, py| {
        let x = (px * 2.0 - 1.0) * aspect * tf;
        let y = (1.0 - py * 2.0) * tf;
        let dir = norm(add(add(fwd, mul(right, x)), mul(up, y)));
        let hit = hit_scene(v.shape, eye, dir, v.tiling.max(0.1));
        let Some((_, s)) = hit else {
            let g = 0.12 + 0.08 * (1.0 - py);
            return [g, g, g * 1.05, 1.0];
        };
        let vv = norm(mul(dir, -1.0));
        let mut uv = s.uv;
        let mut cavity = 1.0;
        if let (Some(hm), true) = (m.height, v.height_scale > 0.0) {
            let vt = [dot(vv, s.t), dot(vv, s.b), dot(vv, s.n)];
            let (nuv, d) = parallax(hm, &s, vt, v.height_scale);
            uv = nuv;
            cavity = 1.0 - 0.45 * d;
        }
        let base = m.base.sample(uv[0], uv[1]);
        let albedo = [lin(base[0]), lin(base[1]), lin(base[2])];
        let rough = lum(m.rough.sample(uv[0], uv[1])).clamp(0.04, 1.0);
        let metal = lum(m.metal.sample(uv[0], uv[1])).clamp(0.0, 1.0);
        let ao = lum(m.ao.sample(uv[0], uv[1])).clamp(0.0, 1.0) * cavity;
        let nm = m.normal.sample(uv[0], uv[1]);
        let mut tn = [nm[0] * 2.0 - 1.0, nm[1] * 2.0 - 1.0, (nm[2] * 2.0 - 1.0).max(0.05)];
        if let (Some(hm), true) = (m.height, v.bump > 0.0) {
            let e = 1.0 / hm.w.max(1) as f32;
            let hs = |du: f32, dv: f32| lum(hm.sample(uv[0] + du, uv[1] + dv));
            let (dhu, dhv) = ((hs(e, 0.0) - hs(-e, 0.0)) / (2.0 * e), (hs(0.0, e) - hs(0.0, -e)) / (2.0 * e));
            let k = v.bump * 0.12;
            tn[0] -= k * dhu / s.su;
            tn[1] += k * dhv / s.sv;
        }
        let tn = norm(tn);
        let n = norm(add(add(mul(s.t, tn[0]), mul(s.b, tn[1])), mul(s.n, tn[2])));
        let ndv = dot(n, vv).max(1e-3);
        let f0 = [0.04 * (1.0 - metal) + albedo[0] * metal, 0.04 * (1.0 - metal) + albedo[1] * metal, 0.04 * (1.0 - metal) + albedo[2] * metal];
        let a = rough * rough;
        let hv = norm(add(ldir, vv));
        let (ndl, ndh, vdh) = (dot(n, ldir).max(0.0), dot(n, hv).max(0.0), dot(vv, hv).max(0.0));
        let d = a * a / (PI * (ndh * ndh * (a * a - 1.0) + 1.0).powi(2)).max(1e-6);
        let k = (rough + 1.0).powi(2) / 8.0;
        let g = (ndl / (ndl * (1.0 - k) + k)) * (ndv / (ndv * (1.0 - k) + k));
        let fr = (1.0 - vdh).powi(5);
        let refl = norm(sub(mul(n, 2.0 * dot(n, vv)), vv));
        let envr = env(refl);
        let envd = env(n);
        let ndl2 = dot(n, fill).max(0.0);
        let mut out = [0.0; 3];
        for c in 0..3 {
            let f = f0[c] + (1.0 - f0[c]) * fr;
            let spec = d * g * f / (4.0 * ndl.max(1e-3) * ndv) * ndl;
            let diff = albedo[c] * (1.0 - metal) * (1.0 - f) / PI * ndl * v.light_intensity * 3.0;
            let diff_fill = albedo[c] * (1.0 - metal) / PI * ndl2 * v.light_intensity * 0.9;
            let amb_d = albedo[c] * (1.0 - metal) * envd[c] * v.ambient;
            let rf = f0[c] + (1.0 - f0[c]) * fr * (1.0 - rough);
            let amb_s = envr[c] * rf * (1.0 - rough * 0.65) * v.ambient;
            out[c] = (diff + diff_fill + spec * v.light_intensity * 3.0 + (amb_d + amb_s) * ao) * v.exposure;
        }
        if let Some(e) = m.emissive {
            let ec = e.sample(uv[0], uv[1]);
            for (c, o) in out.iter_mut().enumerate() {
                *o += lin(ec[c]);
            }
        }
        let tm = |c: f32| (c / (1.0 + c)).max(0.0).powf(1.0 / 2.2) * 1.15;
        [tm(out[0]).min(1.0), tm(out[1]).min(1.0), tm(out[2]).min(1.0), 1.0]
    })
}
