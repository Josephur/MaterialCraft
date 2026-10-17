// SPDX-License-Identifier: MIT OR Apache-2.0
//! Path nodes: stroke, fill, distance, border, shapes along a path, railway track, cable and seam stitches. They all
//! read a spline (an embedded `.spl` resource named by `file`, or a built-in `preset`), measure every pixel against
//! it with a wrap-around distance grid, and shade from the distance, the position along the path and the side.
//! Every node tiles seamlessly (the path wraps with the texture) and works on non-square graphs. Pixel sizes are
//! given for a 512 px texture and scaled by `size / 512`. `compute` returns Ok(None) for kinds it does not know.
use crate::graph::{Error, Node};
use crate::noise;
use crate::ops;
use crate::spline::{Polyline, Spline};
use crate::tex::{lum, par_rows, Tex};
use std::collections::BTreeMap;
use std::f32::consts::TAU;
use std::sync::Arc as Rc;

fn grey(v: f32) -> [f32; 4] {
    [v, v, v, 1.0]
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let d = b - a;
    let t = if d.abs() < 1e-9 { if x >= b { 1.0 } else { 0.0 } } else { ((x - a) / d).clamp(0.0, 1.0) };
    t * t * (3.0 - 2.0 * t)
}

/// Make `period` divide `len` evenly, so a pattern along a path that wraps around the texture meets itself.
fn fit_period(len: f32, period: f32, on: bool) -> f32 {
    let period = period.max(0.5);
    if !on || len < period {
        return period;
    }
    len / (len / period).round().max(1.0)
}

// ====================================================================== distance field of a set of paths
struct Seg {
    a: [f32; 2],
    d: [f32; 2],
    len: f32,
    s0: f32,
    wa: f32,
    wb: f32,
    path: u32,
}

struct PInfo {
    len: f32,
    closed: bool,
}

struct Field {
    cs: f32,
    gw: usize,
    gh: usize,
    w: f32,
    h: f32,
    bins: Vec<Vec<(u32, i8, i8)>>,
    segs: Vec<Seg>,
    paths: Vec<PInfo>,
    reach: f32,
}

#[derive(Clone, Copy)]
struct Hit {
    /// distance to the path in pixels
    d: f32,
    /// distance along its path in pixels
    s: f32,
    /// +1 on the left of the direction of travel, -1 on the right
    side: f32,
    /// width factor at the nearest point
    wf: f32,
    plen: f32,
    closed: bool,
}

impl Field {
    fn build(polys: &[Polyline], w: usize, h: usize, reach: f32) -> Field {
        let (wf, hf) = (w as f32, h as f32);
        let reach = reach.min(wf.min(hf) * 0.5).max(1.0);
        let mut segs: Vec<Seg> = Vec::new();
        let mut paths: Vec<PInfo> = Vec::new();
        for (pi, poly) in polys.iter().enumerate() {
            let n = poly.pts.len();
            let closed = poly.closed && n > 2;
            let count = if closed { n } else { n.saturating_sub(1) };
            let mut s0 = 0.0;
            for i in 0..count {
                let (a, b) = (poly.pts[i], poly.pts[(i + 1) % n]);
                let d = [b[0] - a[0], b[1] - a[1]];
                let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
                if len < 1e-6 || !len.is_finite() {
                    continue;
                }
                segs.push(Seg { a, d, len, s0, wa: poly.w[i], wb: poly.w[(i + 1) % n], path: pi as u32 });
                s0 += len;
            }
            paths.push(PInfo { len: s0.max(1e-3), closed });
        }
        let cs = reach.clamp(8.0, 64.0);
        let (gw, gh) = (((wf / cs).ceil() as usize).max(1), ((hf / cs).ceil() as usize).max(1));
        let mut bins: Vec<Vec<(u32, i8, i8)>> = vec![Vec::new(); gw * gh];
        for (si, sg) in segs.iter().enumerate() {
            for sy in -1i8..=1 {
                for sx in -1i8..=1 {
                    let (ox, oy) = (sx as f32 * wf, sy as f32 * hf);
                    let (ax, ay) = (sg.a[0] + ox, sg.a[1] + oy);
                    let (bx, by) = (ax + sg.d[0], ay + sg.d[1]);
                    let (x0, x1) = (ax.min(bx) - reach, ax.max(bx) + reach);
                    let (y0, y1) = (ay.min(by) - reach, ay.max(by) + reach);
                    if x1 < 0.0 || y1 < 0.0 || x0 >= wf || y0 >= hf {
                        continue;
                    }
                    let (cx0, cx1) = (((x0.max(0.0)) / cs) as usize, (((x1.min(wf - 1e-3)) / cs) as usize).min(gw - 1));
                    let (cy0, cy1) = (((y0.max(0.0)) / cs) as usize, (((y1.min(hf - 1e-3)) / cs) as usize).min(gh - 1));
                    for cy in cy0..=cy1 {
                        for cx in cx0..=cx1 {
                            bins[cy * gw + cx].push((si as u32, sx, sy));
                        }
                    }
                }
            }
        }
        Field { cs, gw, gh, w: wf, h: hf, bins, segs, paths, reach }
    }

    fn query(&self, x: f32, y: f32) -> Option<Hit> {
        let cx = ((x / self.cs) as usize).min(self.gw - 1);
        let cy = ((y / self.cs) as usize).min(self.gh - 1);
        let mut best = self.reach * self.reach;
        let mut found: Option<(usize, f32, f32, f32)> = None;
        for &(si, sx, sy) in &self.bins[cy * self.gw + cx] {
            let sg = &self.segs[si as usize];
            let (ax, ay) = (sg.a[0] + sx as f32 * self.w, sg.a[1] + sy as f32 * self.h);
            let (px, py) = (x - ax, y - ay);
            let t = ((px * sg.d[0] + py * sg.d[1]) / (sg.len * sg.len)).clamp(0.0, 1.0);
            let (qx, qy) = (px - sg.d[0] * t, py - sg.d[1] * t);
            let d2 = qx * qx + qy * qy;
            if d2 < best {
                best = d2;
                let cross = sg.d[0] * py - sg.d[1] * px;
                found = Some((si as usize, t, cross, d2));
            }
        }
        found.map(|(si, t, cross, d2)| {
            let sg = &self.segs[si];
            let p = &self.paths[sg.path as usize];
            Hit { d: d2.sqrt(), s: sg.s0 + t * sg.len, side: if cross >= 0.0 { 1.0 } else { -1.0 }, wf: sg.wa + (sg.wb - sg.wa) * t, plen: p.len, closed: p.closed }
        })
    }
}

// ====================================================================== shared parameter handling
struct Ctx6<'a> {
    kind: &'a str,
    n: &'a Node,
    s: usize,
    hs: usize,
}

impl Ctx6<'_> {
    fn f(&self, k: &str, d: f32) -> f32 {
        self.n.nums.get(k).copied().filter(|v| v.is_finite()).unwrap_or(d)
    }

    /// Closed shapes by default for the nodes that need them, a wave for the others.
    fn default_preset(&self) -> f32 {
        if self.kind.starts_with("path_fill") || self.kind == "path_border" {
            3.0
        } else {
            2.0
        }
    }

    fn spline(&self, id: u32, load: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> Result<Spline, Error> {
        match self.n.strs.get("file").filter(|f| !f.is_empty()) {
            Some(file) => {
                let bytes = load(file).map_err(|m| Error::BadParam(id, m))?;
                Spline::from_bytes(&bytes).map_err(|m| Error::BadParam(id, format!("'{file}': {m}")))
            }
            None => Ok(Spline::preset(self.f("preset", self.default_preset()).round().clamp(0.0, 6.0) as u32 + 1, self.f("amplitude", 0.12), self.f("frequency", 2.0))),
        }
    }

    /// Flattened paths for this texture size; `path` picks one (-1 = all).
    fn polys(&self, id: u32, load: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> Result<Vec<Polyline>, Error> {
        let sp = self.spline(id, load)?;
        let all = sp.flatten(self.s as f32, self.hs as f32);
        let pick = self.f("path", -1.0).round();
        Ok(if pick >= 0.0 { all.into_iter().skip(pick as usize).take(1).collect() } else { all })
    }
}

fn max_wf(polys: &[Polyline]) -> f32 {
    polys.iter().flat_map(|p| p.w.iter().copied()).fold(1.0f32, f32::max).clamp(0.0, 16.0)
}

fn shade(c: &Ctx6, field: &Field, f: impl Fn(Option<Hit>, usize, usize) -> f32 + Sync) -> Tex {
    let (w, h) = (c.s, c.hs);
    let mut px = vec![[0.0f32; 4]; w * h];
    par_rows(&mut px, w, |start, piece| {
        for (i, o) in piece.iter_mut().enumerate() {
            let (x, y) = ((start + i) % w, (start + i) / w);
            *o = grey(f(field.query(x as f32 + 0.5, y as f32 + 0.5), x, y));
        }
    });
    Tex { w, h, px }
}

/// Inside mask (coverage) of the closed paths, drawn with wrapped copies so it tiles.
fn inside_mask(polys: &[Polyline], w: usize, h: usize, even_odd: bool) -> Vec<f32> {
    let mut vp: Vec<crate::vector::Poly> = Vec::new();
    for sy in -1i32..=1 {
        for sx in -1i32..=1 {
            for p in polys {
                if p.pts.len() < 3 {
                    continue;
                }
                let pts = p.pts.iter().map(|q| [q[0] + sx as f32 * w as f32, q[1] + sy as f32 * h as f32]).collect();
                vp.push(crate::vector::Poly { pts, closed: true });
            }
        }
    }
    crate::vector::fill(&vp, w, h, even_odd).a
}

fn aa(x: f32) -> f32 {
    (x + 0.5).clamp(0.0, 1.0)
}

// ====================================================================== nodes
pub fn compute(
    kind: &str,
    id: u32,
    n: &Node,
    ins: &BTreeMap<String, Rc<Tex>>,
    s: usize,
    hs: usize,
    seed: u32,
    load: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<Option<Tex>, Error> {
    let rs = s as f32 / 512.0;
    let c = Ctx6 { kind, n, s, hs };
    let f = |k: &str, d: f32| c.f(k, d);
    let out_idx = f("_out", 0.0).round() as usize;
    let (con, inv) = (f("contrast", 1.0).clamp(0.0, 16.0), f("invert", 0.0) > 0.5);
    let fin = |t: Tex| -> Tex {
        if (con - 1.0).abs() > 1e-4 || inv {
            ops::contrast_invert(&t, con, inv)
        } else {
            t
        }
    };
    let fit = f("fit", 1.0) > 0.5;
    let out = match kind {
        "path_stroke" => {
            let polys = c.polys(id, load)?;
            let width = f("width", 12.0).clamp(0.1, 1024.0) * rs;
            let soft = f("softness", 0.0).clamp(0.0, 1.0);
            let profile = f("profile", 0.0).round() as u32;
            let (tin, tout) = (f("taper_start", 0.0).clamp(0.0, 1.0), f("taper_end", 0.0).clamp(0.0, 1.0));
            let (dash, gap) = (f("dash_length", 0.0).clamp(0.0, 4096.0) * rs, f("dash_gap", 8.0).clamp(0.0, 4096.0) * rs);
            let reach = width * 0.5 * max_wf(&polys) * (1.0 + soft) + 3.0;
            let field = Field::build(&polys, s, hs, reach);
            fin(shade(&c, &field, |hit, _, _| {
                let Some(h) = hit else { return 0.0 };
                let mut half = width * 0.5 * h.wf;
                if !h.closed {
                    let (a, b) = (if tin > 0.0 { (h.s / (tin * h.plen)).min(1.0) } else { 1.0 }, if tout > 0.0 { ((h.plen - h.s) / (tout * h.plen)).min(1.0) } else { 1.0 });
                    half *= smooth(0.0, 1.0, a.min(b));
                }
                if half < 0.05 {
                    return 0.0;
                }
                let edge = (soft * half).max(0.0);
                let mut cov = if edge > 1.0 { 1.0 - smooth(half - edge, half, h.d) } else { aa(half - h.d) };
                if dash > 0.5 {
                    let period = fit_period(h.plen, dash + gap, fit);
                    let dl = (period - gap).max(1.0);
                    let ds = h.s % period;
                    cov *= aa(ds.min(dl - ds) + 0.0);
                    if ds > dl {
                        cov = 0.0;
                    }
                }
                let q = (h.d / half).clamp(0.0, 1.0);
                match profile {
                    1 => cov * (1.0 - q * q).sqrt(),
                    2 => cov * ((1.0 - q) / 0.4).clamp(0.0, 1.0),
                    _ => cov,
                }
            }))
        }
        "path_fill" => {
            let polys = c.polys(id, load)?;
            let m = inside_mask(&polys, s, hs, f("even_odd", 0.0) > 0.5);
            let mut t = Tex { w: s, h: hs, px: m.into_iter().map(grey).collect() };
            let soft = f("softness", 0.0).clamp(0.0, 1.0);
            if soft > 0.0 {
                t = ops::blur(&t, soft * 24.0 * rs);
            }
            fin(t)
        }
        "path_distance" => {
            let polys = c.polys(id, load)?;
            let range = f("range", 64.0).clamp(1.0, 2048.0) * rs;
            let mode = f("mode", 0.0).round() as u32;
            let field = Field::build(&polys, s, hs, range);
            let inside = if polys.iter().any(|p| p.closed) && mode != 0 { Some(inside_mask(&polys, s, hs, false)) } else { None };
            fin(shade(&c, &field, |hit, x, y| {
                let ins_v = inside.as_ref().map(|m| m[y * s + x] > 0.5);
                match mode {
                    0 => hit.map_or(0.0, |h| (1.0 - h.d / range).clamp(0.0, 1.0)),
                    1 => match (hit, ins_v) {
                        (Some(h), Some(i)) => 0.5 + 0.5 * if i { 1.0 } else { -1.0 } * (h.d / range).min(1.0),
                        (Some(h), None) => 0.5 + 0.5 * h.side * (h.d / range).min(1.0),
                        (None, Some(i)) => {
                            if i {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        (None, None) => 0.5,
                    },
                    2 => match (hit, ins_v) {
                        (_, Some(false)) | (_, None) => 0.0,
                        (Some(h), Some(true)) => (h.d / range).clamp(0.0, 1.0),
                        (None, Some(true)) => 1.0,
                    },
                    _ => match (hit, ins_v) {
                        (_, Some(true)) | (_, None) => 0.0,
                        (Some(h), Some(false)) => (h.d / range).clamp(0.0, 1.0),
                        (None, Some(false)) => 1.0,
                    },
                }
            }))
        }
        "path_border" => {
            let polys: Vec<Polyline> = c.polys(id, load)?.into_iter().filter(|p| p.closed).collect();
            let width = f("width", 32.0).clamp(0.5, 1024.0) * rs;
            let side = f("side", 0.0).round() as u32;
            let profile = f("profile", 0.0).round() as u32;
            let flat = f("flat", 0.0).clamp(0.0, 0.95);
            let field = Field::build(&polys, s, hs, width);
            let inside = inside_mask(&polys, s, hs, false);
            let mask_out = out_idx == 1;
            fin(shade(&c, &field, |hit, x, y| {
                let Some(h) = hit else { return 0.0 };
                let ins_v = inside[y * s + x] > 0.5;
                let ok = match side {
                    0 => ins_v,
                    1 => !ins_v,
                    _ => true,
                };
                if !ok {
                    return 0.0;
                }
                let q = (h.d / width).clamp(0.0, 1.0);
                if mask_out {
                    return aa(width - h.d);
                }
                let r = ((q - flat) / (1.0 - flat)).clamp(0.0, 1.0);
                // the path line is the top of the border and the far edge is level with the surface
                let hgt = match profile {
                    1 => (1.0 - r * r).sqrt(),
                    2 => 1.0 - smooth(0.0, 1.0, r),
                    _ => 1.0 - r,
                };
                hgt * aa(width - h.d)
            }))
        }
        "path_scatter" => {
            let polys = c.polys(id, load)?;
            let pat = ins.get("in").map(|r| r.as_ref());
            let spacing = f("spacing", 24.0).clamp(1.0, 2048.0) * rs;
            let size = f("scale", 24.0).clamp(1.0, 2048.0) * rs;
            let (var, jal, jac) = (f("scale_variation", 0.2).clamp(0.0, 1.0), f("jitter_along", 0.0).clamp(0.0, 1.0), f("jitter_across", 0.0).clamp(0.0, 512.0) * rs);
            let rot = f("rotation", 0.0).round() as u32; // 0 follow the path, 1 fixed, 2 random
            let (ang0, ang_var) = (f("angle", 0.0) * TAU, f("angle_variation", 0.0).clamp(0.0, 1.0) * TAU);
            let (density, op_var) = (f("density", 1.0).clamp(0.0, 1.0), f("opacity_variation", 0.0).clamp(0.0, 1.0));
            let blend = f("blend", 0.0).round() as u32; // 0 max, 1 add
            let mut dest = vec![0.0f32; s * hs];
            let mut budget: i64 = 400_000_000;
            'all: for (pi, poly) in polys.iter().enumerate() {
                let step = fit_period(poly.length(), spacing, fit);
                for (i, sm) in poly.resample(step, f("offset", 0.0) * rs).into_iter().enumerate() {
                    let key = (pi as i32) * 100_003 + i as i32;
                    let r = |k: i32| noise::rnd(key, k, seed);
                    if r(1) > density {
                        continue;
                    }
                    let sz = (size * sm.w * (1.0 + var * (r(2) - 0.5) * 2.0)).max(0.5);
                    let nrm = [-sm.t[1], sm.t[0]];
                    let (ja, jc) = (jal * step * (r(3) - 0.5), jac * (r(4) - 0.5) * 2.0);
                    let (cx, cy) = (sm.p[0] + sm.t[0] * ja + nrm[0] * jc, sm.p[1] + sm.t[1] * ja + nrm[1] * jc);
                    let ang = match rot {
                        0 => sm.t[1].atan2(sm.t[0]) + ang0 + ang_var * (r(5) - 0.5),
                        1 => ang0 + ang_var * (r(5) - 0.5),
                        _ => r(5) * TAU,
                    };
                    let amp = 1.0 - op_var * r(6);
                    let (sa, ca) = ang.sin_cos();
                    let rad = sz * 0.7072 + 1.0;
                    let (x0, x1, y0, y1) = ((cx - rad).floor() as i64, (cx + rad).ceil() as i64, (cy - rad).floor() as i64, (cy + rad).ceil() as i64);
                    budget -= (x1 - x0 + 1) * (y1 - y0 + 1);
                    if budget < 0 {
                        break 'all;
                    }
                    for py in y0..=y1 {
                        for px in x0..=x1 {
                            let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
                            let (lx, ly) = (dx * ca + dy * sa, -dx * sa + dy * ca);
                            let (u, v) = (lx / sz + 0.5, ly / sz + 0.5);
                            if u < 0.0 || u >= 1.0 || v < 0.0 || v >= 1.0 {
                                continue;
                            }
                            let val = match pat {
                                Some(t) => lum(t.sample(u, v)) * t.sample(u, v)[3],
                                None => 1.0 - smooth(0.44, 0.5, (lx * lx + ly * ly).sqrt() / sz),
                            } * amp;
                            if val <= 0.0 {
                                continue;
                            }
                            let idx = (py.rem_euclid(hs as i64) as usize) * s + px.rem_euclid(s as i64) as usize;
                            dest[idx] = if blend == 1 { (dest[idx] + val).min(1.0) } else { dest[idx].max(val) };
                        }
                    }
                }
            }
            fin(Tex { w: s, h: hs, px: dest.into_iter().map(grey).collect() })
        }
        "path_track" => {
            let polys = c.polys(id, load)?;
            let gauge = f("gauge", 60.0).clamp(2.0, 1024.0) * rs;
            let rw = f("rail_width", 8.0).clamp(0.5, 256.0) * rs;
            let (spacing, sl, sw) = (f("sleeper_spacing", 28.0).clamp(2.0, 1024.0) * rs, f("sleeper_length", 110.0).clamp(2.0, 2048.0) * rs, f("sleeper_width", 14.0).clamp(1.0, 512.0) * rs);
            let (rh, sh) = (f("rail_height", 0.9).clamp(0.0, 1.0), f("sleeper_height", 0.55).clamp(0.0, 1.0));
            let field = Field::build(&polys, s, hs, (sl * 0.5).max(gauge * 0.5 + rw) + 3.0);
            fin(shade(&c, &field, |hit, _, _| {
                let Some(h) = hit else { return 0.0 };
                let (rail, sleeper) = track_parts(&h, gauge, rw, spacing, sl, sw, fit);
                match out_idx {
                    1 => rail,
                    2 => sleeper,
                    _ => (rh * rail.max(0.0) * track_dome(&h, gauge, rw)).max(sh * sleeper),
                }
            }))
        }
        "path_cable" => {
            let polys = c.polys(id, load)?;
            let width = f("width", 40.0).clamp(1.0, 1024.0) * rs;
            let strands = f("strands", 3.0).round().clamp(2.0, 8.0) as usize;
            let size = f("strand_size", 0.5).clamp(0.15, 0.95);
            let twist = f("twist", 3.0).clamp(0.0, 64.0);
            let field = Field::build(&polys, s, hs, width * 0.5 * max_wf(&polys) + 3.0);
            fin(shade(&c, &field, |hit, _, _| {
                let Some(h) = hit else { return 0.0 };
                let half = width * 0.5 * h.wf;
                if half < 0.05 {
                    return 0.0;
                }
                let u = h.side * h.d / half;
                let turns = if h.closed || twist > 0.0 { (twist * h.plen / (512.0 * rs)).round().max(1.0) } else { 0.0 };
                let theta = TAU * turns * h.s / h.plen;
                let rr = 1.0 - size;
                let mut best = (0.0f32, 0usize);
                for k in 0..strands {
                    let ph = theta + TAU * k as f32 / strands as f32;
                    let q = (u - rr * ph.sin()) / size;
                    if q.abs() < 1.0 {
                        let v = 0.3 * (0.5 + 0.5 * ph.cos()) + 0.7 * (1.0 - q * q).sqrt();
                        if v > best.0 {
                            best = (v, k);
                        }
                    }
                }
                let edge = aa((1.0 - u.abs()) * half);
                match out_idx {
                    1 => if best.0 > 0.0 { edge } else { 0.0 },
                    2 => if best.0 > 0.0 { (best.1 + 1) as f32 / strands as f32 * edge } else { 0.0 },
                    _ => best.0 * edge,
                }
            }))
        }
        "path_seam" => {
            let polys = c.polys(id, load)?;
            let (len, gap) = (f("stitch_length", 18.0).clamp(1.0, 1024.0) * rs, f("stitch_gap", 10.0).clamp(0.0, 1024.0) * rs);
            let (sw, gw) = (f("stitch_width", 5.0).clamp(0.5, 256.0) * rs, f("groove_width", 12.0).clamp(0.0, 512.0) * rs);
            let (depth, sh) = (f("groove_depth", 0.5).clamp(0.0, 1.0), f("stitch_height", 0.7).clamp(0.0, 1.0));
            let field = Field::build(&polys, s, hs, (gw * 0.5).max(len * 0.5 + sw) + 3.0);
            fin(shade(&c, &field, |hit, _, _| {
                let Some(h) = hit else { return if out_idx == 1 { 0.0 } else { 0.5 } };
                let period = fit_period(h.plen, len + gap, fit);
                let l = (period - gap).min(period).max(0.5);
                let a = ((h.s % period) - period * 0.5).abs();
                let excess = (a - l * 0.5).max(0.0);
                let dd = (excess * excess + h.d * h.d).sqrt();
                let cov = aa(sw * 0.5 - dd);
                if out_idx == 1 {
                    return cov;
                }
                let dome = (1.0 - (dd / (sw * 0.5)).min(1.0).powi(2)).sqrt() * cov;
                let groove = if gw > 0.0 { (1.0 - (h.d / (gw * 0.5)).min(1.0).powi(2)).max(0.0) } else { 0.0 };
                (0.5 + 0.5 * sh * dome - 0.5 * depth * groove).clamp(0.0, 1.0)
            }))
        }
        _ => return Ok(None),
    };
    Ok(Some(out))
}

/// (rail coverage, sleeper coverage) of a track at a hit.
fn track_parts(h: &Hit, gauge: f32, rw: f32, spacing: f32, sl: f32, sw: f32, fit: bool) -> (f32, f32) {
    let rail = aa(rw * 0.5 - (h.d - gauge * 0.5).abs());
    let period = fit_period(h.plen, spacing, fit);
    let a = ((h.s % period) - period * 0.5).abs();
    let sleeper = aa(sw * 0.5 - a).min(aa(sl * 0.5 - h.d));
    (rail, sleeper)
}

fn track_dome(h: &Hit, gauge: f32, rw: f32) -> f32 {
    let x = ((h.d - gauge * 0.5).abs() / (rw * 0.5)).min(1.0);
    (1.0 - x * x).sqrt().max(0.35)
}
