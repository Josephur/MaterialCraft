//! Software PBR renderer for the 3D viewer (sphere, cube, plane). GGX specular, hemisphere
//! environment lighting, tangent-space normal mapping.
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
}

#[derive(Clone, Debug)]
pub struct View {
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub light_yaw: f32,
    pub light_pitch: f32,
    pub light_intensity: f32,
    pub exposure: f32,
    pub shape: u32, // 0 sphere, 1 cube, 2 plane
    pub tiling: f32,
}

impl Default for View {
    fn default() -> Self {
        View { yaw: 0.5, pitch: 0.3, dist: 3.4, light_yaw: 0.9, light_pitch: 0.7, light_intensity: 3.0, exposure: 1.0, shape: 0, tiling: 1.0 }
    }
}

#[allow(dead_code)]
struct Surf {
    p: V,
    n: V,
    t: V,
    b: V,
    uv: [f32; 2],
}

fn env(d: V) -> V {
    let y = d[1] * 0.5 + 0.5;
    let sky = [0.42, 0.52, 0.68];
    let ground = [0.10, 0.095, 0.09];
    let hor = [0.55, 0.55, 0.55];
    let base = if d[1] > 0.0 { add(mul(hor, 1.0 - d[1]), mul(sky, d[1])) } else { add(mul(hor, 1.0 + d[1] * 1.5), mul(ground, -d[1] * 1.5)) };
    let _ = y;
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
            Some((t, Surf { p, n: p, t: tg, b: cross(p, tg), uv }))
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
            Some((t0, Surf { p, n, t: tg, b: bt, uv }))
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
            Some((t, Surf { p, n, t: tg, b: [0.0, 1.0, 0.0], uv }))
        }
    }
}

fn lin(c: f32) -> f32 {
    c.max(0.0).powf(2.2)
}

pub fn render(m: &Maps, v: &View, w: usize, h: usize) -> Tex {
    let (sy, cy) = v.yaw.sin_cos();
    let (sp, cp) = v.pitch.sin_cos();
    let eye = [v.dist * sy * cp, v.dist * sp, v.dist * cy * cp];
    let fwd = norm(mul(eye, -1.0));
    let right = norm(cross(fwd, [0.0, 1.0, 0.0]));
    let up = cross(right, fwd);
    let tf = (20.0f32).to_radians().tan();
    let aspect = w as f32 / h as f32;
    let (lsy, lcy) = v.light_yaw.sin_cos();
    let (lsp, lcp) = v.light_pitch.sin_cos();
    let ldir = norm([lsy * lcp, lsp, lcy * lcp]);
    Tex::from_fn(w, h, |px, py| {
        let x = (px * 2.0 - 1.0) * aspect * tf;
        let y = (1.0 - py * 2.0) * tf;
        let dir = norm(add(add(fwd, mul(right, x)), mul(up, y)));
        let hit = hit_scene(v.shape, eye, dir, v.tiling.max(0.1));
        let Some((_, s)) = hit else {
            let g = 0.10 + 0.08 * (1.0 - py);
            return [g, g, g * 1.05, 1.0];
        };
        let base = m.base.sample(s.uv[0], s.uv[1]);
        let albedo = [lin(base[0]), lin(base[1]), lin(base[2])];
        let rough = lum(m.rough.sample(s.uv[0], s.uv[1])).clamp(0.04, 1.0);
        let metal = lum(m.metal.sample(s.uv[0], s.uv[1])).clamp(0.0, 1.0);
        let ao = lum(m.ao.sample(s.uv[0], s.uv[1])).clamp(0.0, 1.0);
        let nm = m.normal.sample(s.uv[0], s.uv[1]);
        let tn = norm([nm[0] * 2.0 - 1.0, nm[1] * 2.0 - 1.0, (nm[2] * 2.0 - 1.0).max(0.05)]);
        let n = norm(add(add(mul(s.t, tn[0]), mul(s.b, tn[1])), mul(s.n, tn[2])));
        let vv = norm(mul(dir, -1.0));
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
        let mut out = [0.0; 3];
        for c in 0..3 {
            let f = f0[c] + (1.0 - f0[c]) * fr;
            let spec = d * g * f / (4.0 * ndl.max(1e-3) * ndv) * ndl;
            let diff = albedo[c] * (1.0 - metal) * (1.0 - f) / PI * ndl * v.light_intensity * 3.0;
            let amb_d = albedo[c] * (1.0 - metal) * envd[c];
            let rf = f0[c] + (1.0 - f0[c]) * fr * (1.0 - rough);
            let amb_s = envr[c] * rf * (1.0 - rough * 0.65);
            out[c] = (diff + spec * v.light_intensity * 3.0 + (amb_d + amb_s) * ao) * v.exposure;
        }
        if let Some(e) = m.emissive {
            let ec = e.sample(s.uv[0], s.uv[1]);
            for (c, o) in out.iter_mut().enumerate() {
                *o += lin(ec[c]);
            }
        }
        let tm = |c: f32| (c / (1.0 + c)).max(0.0).powf(1.0 / 2.2) * 1.15;
        [tm(out[0]).min(1.0), tm(out[1]).min(1.0), tm(out[2]).min(1.0), 1.0]
    })
}
