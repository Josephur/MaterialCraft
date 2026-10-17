// SPDX-License-Identifier: MIT OR Apache-2.0
//! GPU PBR viewport (OpenGL 3.3 core through glow / egui_glow paint callbacks).
//!
//! Every shape is a tessellated triangle mesh. The height map displaces the vertices in the vertex shader (true
//! displacement), the fragment shader does GGX PBR with normal mapping, outward parallax relief, height-field contact
//! shadows, an analytic studio environment and ACES tone mapping. Textures are mip-mapped with anisotropic filtering
//! and the window is multisampled, so fine texture detail stays sharp at any zoom level.
use crate::render3d::{build_mesh, View, RELIEF_BUMP, RELIEF_DISPLACE, RELIEF_PARALLAX};
use crate::tex::{lum, par_map, Tex};
use eframe::egui::PaintCallbackInfo;
use eframe::egui_glow::{self, glow};
use glow::HasContext;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Shared handle between the UI and the paint callback.
pub type Shared = Arc<Mutex<Gpu>>;

pub fn new_shared() -> Shared {
    Arc::new(Mutex::new(Gpu::default()))
}

/// Extra viewer options that live next to (not inside) the CPU renderer's `View`.
#[derive(Clone, Debug)]
pub struct GpuOpts {
    /// Contact shadows computed by marching the height field towards the light.
    pub self_shadow: bool,
    /// Anisotropic filtering level (1..16). 0 = use the maximum the GPU offers.
    pub aniso: f32,
}

impl Default for GpuOpts {
    fn default() -> Self {
        GpuOpts { self_shadow: true, aniso: 0.0 }
    }
}

/// Texture data prepared on the CPU (resampled to one size, packed, converted) and ready to upload.
pub struct GpuMaps {
    pub size: usize,
    /// sRGB RGBA8
    pub base: Vec<u8>,
    /// linear RGBA8 tangent-space normal
    pub normal: Vec<u8>,
    /// R = roughness, G = metallic, B = ambient occlusion
    pub rma: Vec<u8>,
    /// sRGB RGBA8, absent when the graph has no emissive output
    pub emissive: Option<Vec<u8>>,
    /// luminance height (w, h, values)
    pub height: Option<(usize, usize, Vec<f32>)>,
}

pub struct MapInputs<'a> {
    pub base: Option<&'a Tex>,
    pub normal: Option<&'a Tex>,
    pub rough: Option<&'a Tex>,
    pub metal: Option<&'a Tex>,
    pub ao: Option<&'a Tex>,
    pub emissive: Option<&'a Tex>,
    pub height: Option<&'a Tex>,
}

fn b8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Convert graph outputs into upload-ready buffers. All colour maps are brought to the largest map size.
/// Bring a map to `size` x `size`: exact box averaging when shrinking by a whole factor (no aliasing, e.g. 8K -> 4K),
/// bilinear otherwise.
fn fit(t: &Tex, size: usize) -> Tex {
    if t.w == size && t.h == size {
        return t.clone();
    }
    if t.w > size && t.h > size && t.w % size == 0 && t.h % size == 0 {
        let (fx, fy) = (t.w / size, t.h / size);
        let inv = 1.0 / (fx * fy) as f32;
        let px = par_map(size * size, |i| {
            let (x0, y0) = ((i % size) * fx, (i / size) * fy);
            let mut a = [0.0f32; 4];
            for y in y0..y0 + fy {
                for x in x0..x0 + fx {
                    let p = t.px[y * t.w + x];
                    for c in 0..4 {
                        a[c] += p[c];
                    }
                }
            }
            [a[0] * inv, a[1] * inv, a[2] * inv, a[3] * inv]
        });
        return Tex { w: size, h: size, px };
    }
    Tex::from_fn(size, size, |u, v| t.sample(u, v))
}

/// Convert graph outputs into upload-ready buffers. All colour maps are brought to the largest map size.
pub fn prepare(m: &MapInputs) -> GpuMaps {
    let all = [m.base, m.normal, m.rough, m.metal, m.ao, m.emissive];
    let size = all.iter().flatten().map(|t| t.w.max(t.h)).max().unwrap_or(4).clamp(1, 4096);
    let fitted: Vec<Option<Tex>> = all.iter().map(|t| t.filter(|t| t.w != size || t.h != size).map(|t| fit(t, size))).collect();
    let px = |k: usize, t: Option<&Tex>, dflt: [f32; 4], i: usize| -> [f32; 4] {
        match (&fitted[k], t) {
            (Some(f), _) => f.px[i],
            (None, Some(t)) => t.px[i],
            (None, None) => dflt,
        }
    };
    let n = size * size;
    let rgba = |k: usize, t: Option<&Tex>, dflt: [f32; 4]| -> Vec<u8> {
        let v: Vec<[u8; 4]> = par_map(n, |i| {
            let p = px(k, t, dflt, i);
            [b8(p[0]), b8(p[1]), b8(p[2]), 255]
        });
        v.into_iter().flatten().collect()
    };
    let base = rgba(0, m.base, [0.5, 0.5, 0.5, 1.0]);
    let normal = rgba(1, m.normal, [0.5, 0.5, 1.0, 1.0]);
    let rma: Vec<u8> = {
        let v: Vec<[u8; 4]> = par_map(n, |i| {
            let r = lum(px(2, m.rough, [0.5, 0.5, 0.5, 1.0], i));
            let me = lum(px(3, m.metal, [0.0, 0.0, 0.0, 1.0], i));
            let a = lum(px(4, m.ao, [1.0, 1.0, 1.0, 1.0], i));
            [b8(r), b8(me), b8(a), 255]
        });
        v.into_iter().flatten().collect()
    };
    let emissive = m.emissive.map(|e| rgba(5, Some(e), [0.0, 0.0, 0.0, 1.0]));
    let height = m.height.map(|h| (h.w, h.h, h.px.iter().map(|p| lum(*p)).collect::<Vec<f32>>()));
    GpuMaps { size, base, normal, rma, emissive, height }
}

// ------------------------------------------------------------------------------------------------ GL state
struct Tx {
    id: glow::Texture,
    w: usize,
    h: usize,
}

struct Res {
    prog: glow::Program,
    bg_prog: glow::Program,
    locs: HashMap<&'static str, glow::UniformLocation>,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    index_count: i32,
    mesh_key: (u32, u32, u32),
    base: Tx,
    normal: Tx,
    rma: Tx,
    emis: Option<Tx>,
    height: Option<Tx>,
    dummy_h: Tx,
    max_aniso: f32,
}

#[derive(Default)]
pub struct Gpu {
    pub opts: GpuOpts,
    /// Set when shader compilation or a GL call failed; the UI then falls back to the CPU renderer.
    pub error: Option<String>,
    /// Maps waiting to be uploaded on the next paint.
    pending: Option<GpuMaps>,
    /// Request a pixel read-back of the next frame (screenshots, tests).
    pub want_shot: bool,
    /// Result of a read-back: (width, height, RGBA8 rows from the TOP).
    pub shot: Option<(usize, usize, Vec<u8>)>,
    /// GPU name reported by the driver, for the UI.
    pub renderer: String,
    res: Option<Res>,
    pub frames: u64,
}

impl Gpu {
    pub fn set_maps(&mut self, maps: GpuMaps) {
        self.pending = Some(maps);
    }
    pub fn ready(&self) -> bool {
        self.error.is_none()
    }
}

// ------------------------------------------------------------------------------------------------ shaders
const VS: &str = r#"#version 330 core
layout(location = 0) in vec3 aPos;
layout(location = 1) in vec3 aNrm;
layout(location = 2) in vec3 aTan;
layout(location = 3) in vec2 aUv;
layout(location = 4) in vec3 aMisc; // su, sv, displacement fade
uniform mat4 uVP;
uniform sampler2D uHeight;
uniform float uDisp;
uniform float uLevel;
uniform float uVtxLod;
out vec3 vPos;
out vec3 vN;
out vec3 vT;
out vec2 vUv;
out vec2 vScale;
void main() {
    vec3 p = aPos;
    if (uDisp != 0.0) {
        float h = textureLod(uHeight, aUv, uVtxLod).r;
        p += aNrm * (h - uLevel) * uDisp * aMisc.z;
    }
    vPos = p;
    vN = aNrm;
    vT = aTan;
    vUv = aUv;
    vScale = aMisc.xy;
    gl_Position = uVP * vec4(p, 1.0);
}
"#;

const FS: &str = r#"#version 330 core
in vec3 vPos;
in vec3 vN;
in vec3 vT;
in vec2 vUv;
in vec2 vScale;
uniform sampler2D uBase;
uniform sampler2D uNormal;
uniform sampler2D uRma;
uniform sampler2D uEmis;
uniform sampler2D uHeight;
uniform vec3 uEye;
uniform vec3 uL;
uniform vec3 uFill;
uniform float uLightI;
uniform float uExposure;
uniform float uAmbient;
uniform float uNormalAmt;
uniform float uBump;
uniform float uDepth;
uniform float uLevel;
uniform float uTexel;
uniform int uMode;
uniform int uHasHeight;
uniform int uHasEmis;
uniform int uWire;
uniform int uShadow;
uniform int uCull;
out vec4 oColor;

const float PI = 3.14159265;
vec2 gdx;
vec2 gdy;

// Smooth gradient only: used for diffuse ambient light.
vec3 envSoft(vec3 d) {
    vec3 sky = vec3(0.30, 0.34, 0.42);
    vec3 hor = vec3(0.52, 0.52, 0.53);
    vec3 ground = vec3(0.20, 0.19, 0.18);
    return d.y > 0.0 ? mix(hor, sky, pow(d.y, 0.6)) : mix(hor, ground, pow(-d.y, 0.5));
}

// Studio environment for reflections: softboxes, a rim strip and a horizon line on a dark floor. The boxes soften and
// dim with roughness, so a smooth surface mirrors clear shapes and a rough one only shows a broad sheen.
vec3 env(vec3 d, float r) {
    float soft = r * r;
    vec3 col = mix(envSoft(d), vec3(0.30), soft * 0.5);
    float w = 0.015 + soft * 0.9;
    float keyI = 4.5 / (1.0 + soft * 14.0);
    float key = smoothstep(0.93 - w, 0.93 + 0.015, dot(d, uL));
    col += vec3(1.0, 0.96, 0.90) * key * keyI;
    // tall side strip opposite the key
    vec3 side = normalize(vec3(-uL.z, 0.0, uL.x));
    float sdot = dot(normalize(vec3(d.x, 0.0, d.z) + 1e-5), side);
    float strip = smoothstep(0.93 - w, 0.97, sdot) * smoothstep(0.02 - w, 0.12, d.y) * (1.0 - smoothstep(0.55, 0.75 + w, d.y));
    col += vec3(0.85, 0.92, 1.0) * strip * 1.5 / (1.0 + soft * 10.0);
    // top panel
    float top = smoothstep(0.90 - w, 0.97, d.y);
    col += vec3(1.0) * top * 1.0 / (1.0 + soft * 10.0);
    // bright horizon line
    float hl = exp(-pow(d.y / (0.035 + soft * 0.4), 2.0));
    col += vec3(0.9) * hl * 0.25 / (1.0 + soft * 8.0);
    return col;
}

vec3 aces(vec3 x) {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
}

// Tone map on luminance so saturated colours keep their hue (a per-channel curve turns bright oranges pink and white),
// then let only the very brightest highlights roll off per channel.
float acesL(float x) {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
}
vec3 tonemap(vec3 c) {
    float l = dot(c, vec3(0.2126, 0.7152, 0.0722));
    vec3 hue = c * (acesL(l) / max(l, 1e-4));
    hue = min(hue, vec3(1.0)) ;
    return mix(hue, aces(c), smoothstep(2.0, 8.0, l) * 0.7);
}

vec3 envBrdf(vec3 f0, float rough, float ndv) {
    const vec4 c0 = vec4(-1.0, -0.0275, -0.572, 0.022);
    const vec4 c1 = vec4(1.0, 0.0425, 1.04, -0.04);
    vec4 r = rough * c0 + c1;
    float a004 = min(r.x * r.x, exp2(-9.28 * ndv)) * r.x + r.y;
    vec2 ab = vec2(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
}

float hAt(vec2 uv) {
    return textureGrad(uHeight, uv, gdx, gdy).r;
}

// Outward relief mapping: the height field is a slab of thickness uDepth sitting on the surface; uLevel is flush with
// the polygon and brighter texels rise out of it. Returns the shifted uv and how deep (0..1) the hit lies below the top.
vec2 pom(vec2 uv, vec3 vt, out float cav) {
    float depth = uDepth;
    float vz = max(vt.z, 0.2);
    vec2 d = vec2(-vt.x / vz / vScale.x, vt.y / vz / vScale.y);
    float top = (1.0 - clamp(uLevel, 0.0, 1.0)) * depth;
    cav = 0.0;
    float f0 = (1.0 - hAt(uv + d * (0.0 - top))) * depth;
    if (f0 <= 0.0) return uv + d * (0.0 - top);
    const int STEPS = 64;
    float prevD = 0.0;
    for (int i = 1; i <= STEPS; i++) {
        float dd = depth * float(i) / float(STEPS);
        float f = (1.0 - hAt(uv + d * (dd - top))) * depth - dd;
        if (f <= 0.0) {
            float lo = prevD;
            float hi = dd;
            for (int k = 0; k < 6; k++) {
                float mid = 0.5 * (lo + hi);
                float fm = (1.0 - hAt(uv + d * (mid - top))) * depth - mid;
                if (fm > 0.0) lo = mid; else hi = mid;
            }
            float dh = 0.5 * (lo + hi);
            cav = clamp(dh / depth, 0.0, 1.0);
            return uv + d * (dh - top);
        }
        prevD = dd;
    }
    cav = 1.0;
    return uv + d * (depth - top);
}

// Contact shadow: march from the shaded point towards the light through the height field.
float hshadow(vec2 uv, vec3 lt) {
    if (lt.z <= 0.02) return 1.0;
    float depth = uDepth;
    float h0 = (hAt(uv) - uLevel) * depth + 0.01 * depth;
    float slope = lt.z / max(length(lt.xy), 1e-3);
    vec2 hd = lt.xy / max(length(lt.xy), 1e-4);
    float maxS = ((1.0 - uLevel) * depth - h0) / slope;
    if (maxS <= 0.0) return 1.0;
    float res = 1.0;
    const int N = 32;
    // interleaved gradient noise: turns the stepping bands of the march into fine noise
    float jit = fract(52.9829189 * fract(dot(gl_FragCoord.xy, vec2(0.06711056, 0.00583715))));
    for (int i = 1; i <= N; i++) {
        float s = maxS * (float(i) - 1.0 + jit) / float(N);
        vec2 u2 = uv + vec2(hd.x / vScale.x, -hd.y / vScale.y) * s;
        float hs = (hAt(u2) - uLevel) * depth;
        float ray = h0 + s * slope;
        res = min(res, 1.0 - smoothstep(0.0, 0.10 * depth + 1e-5, hs - ray));
    }
    return mix(0.55, 1.0, res);
}

void main() {
    if (uWire == 1) {
        oColor = vec4(1.0, 0.62, 0.12, 1.0);
        return;
    }
    vec3 N = normalize(vN);
    // shapes seen from the inside: walls that face away from the camera are not drawn
    if (uCull == 1 && dot(N, uEye - vPos) < 0.0) discard;
    vec3 T = normalize(vT - N * dot(vT, N));
    vec3 B = cross(N, T);
    vec3 V = normalize(uEye - vPos);
    bool back = !gl_FrontFacing;
    gdx = dFdx(vUv);
    gdy = dFdy(vUv);
    vec2 uv = vUv;
    float cav = 1.0;
    bool hasH = uHasHeight == 1 && uDepth > 0.0;
    if (uMode == 2 && hasH) {
        vec3 vt = vec3(dot(V, T), dot(V, B), dot(V, N) * (back ? -1.0 : 1.0));
        float dep;
        uv = pom(uv, vt, dep);
        cav = 1.0 - 0.45 * dep;
    }
    vec3 albedo = textureGrad(uBase, uv, gdx, gdy).rgb; // sRGB texture: already linear here
    vec3 rma = textureGrad(uRma, uv, gdx, gdy).rgb;
    float rough = clamp(rma.r, 0.04, 1.0);
    float metal = clamp(rma.g, 0.0, 1.0);
    float ao = mix(1.0, clamp(rma.b, 0.0, 1.0), 0.85) * cav;
    vec3 nm = textureGrad(uNormal, uv, gdx, gdy).xyz;
    vec3 tn = vec3((nm.xy * 2.0 - 1.0) * uNormalAmt, max(nm.z * 2.0 - 1.0, 0.05));
    if (uMode == 1 && uHasHeight == 1 && uBump > 0.0) {
        float e = uTexel;
        float dhu = (hAt(uv + vec2(e, 0.0)) - hAt(uv - vec2(e, 0.0))) / (2.0 * e);
        float dhv = (hAt(uv + vec2(0.0, e)) - hAt(uv - vec2(0.0, e))) / (2.0 * e);
        float k = uBump * 0.12;
        tn.x -= k * dhu / vScale.x;
        tn.y += k * dhv / vScale.y;
    }
    tn = normalize(tn);
    vec3 n = normalize(T * tn.x + B * tn.y + N * tn.z);
    if (back) n = -n;

    float ndv = max(dot(n, V), 1e-3);
    vec3 f0 = mix(vec3(0.04), albedo, metal);
    float a = rough * rough;
    vec3 hv = normalize(uL + V);
    float ndl = max(dot(n, uL), 0.0);
    float ndh = max(dot(n, hv), 0.0);
    float vdh = max(dot(V, hv), 0.0);
    float dd = a * a / (PI * pow(ndh * ndh * (a * a - 1.0) + 1.0, 2.0) + 1e-6);
    float kk = (rough + 1.0) * (rough + 1.0) / 8.0;
    float g = (ndl / (ndl * (1.0 - kk) + kk)) * (ndv / (ndv * (1.0 - kk) + kk));
    float fr = pow(1.0 - vdh, 5.0);
    float sh = 1.0;
    if (uShadow == 1 && hasH && (uMode == 2 || uMode == 3) && !back) {
        vec3 lt = vec3(dot(uL, T), dot(uL, B), dot(uL, N));
        sh = hshadow(uv, lt);
    }
    vec3 f = f0 + (1.0 - f0) * fr;
    vec3 spec = dd * g * f / (4.0 * max(ndl, 1e-3) * ndv) * ndl;
    vec3 diff = albedo * (1.0 - metal) * (1.0 - f) / PI * ndl * uLightI * 1.15;
    float ndl2 = max(dot(n, uFill), 0.0);
    vec3 diffFill = albedo * (1.0 - metal) / PI * ndl2 * uLightI * 0.5;
    vec3 refl = reflect(-V, n);
    vec3 envR = env(refl, rough);
    vec3 envD = envSoft(n);
    vec3 ambD = albedo * (1.0 - metal) * envD * uAmbient;
    float specOcc = clamp(pow(ndv + ao, exp2(-16.0 * rough - 1.0)) - 1.0 + ao, 0.0, 1.0);
    float horizon = clamp(1.0 + 1.2 * dot(refl, back ? -N : N), 0.0, 1.0);
    vec3 ambS = envR * envBrdf(f0, rough, ndv) * uAmbient * specOcc * horizon * horizon;
    vec3 col = ((diff + spec * uLightI * 1.2) * sh + diffFill + (ambD * ao + ambS)) * uExposure;
    if (uHasEmis == 1) col += textureGrad(uEmis, uv, gdx, gdy).rgb;
    col = tonemap(col);
    col = pow(col, vec3(1.0 / 2.2));
    oColor = vec4(col, 1.0);
}
"#;

const BG_VS: &str = r#"#version 330 core
out float vY;
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    vY = p.y;
    gl_Position = vec4(p * 2.0 - 1.0, 0.9999, 1.0);
}
"#;

const BG_FS: &str = r#"#version 330 core
in float vY;
out vec4 oColor;
void main() {
    float t = clamp(vY, 0.0, 1.0);
    float g = 0.12 + 0.08 * (1.0 - t);
    oColor = vec4(vec3(g, g, g * 1.05), 1.0);
}
"#;

// ------------------------------------------------------------------------------------------------ matrices
type M4 = [f32; 16]; // column-major

fn m_mul(a: &M4, b: &M4) -> M4 {
    let mut o = [0.0f32; 16];
    for c in 0..4 {
        for r in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[k * 4 + r] * b[c * 4 + k];
            }
            o[c * 4 + r] = s;
        }
    }
    o
}

struct CamOut {
    vp: M4,
    eye: [f32; 3],
}

fn camera(v: &View, aspect: f32) -> CamOut {
    use crate::v3::*;
    let (sy, cy) = v.yaw.sin_cos();
    let (sp, cp) = v.pitch.sin_cos();
    let eye0 = [v.dist * sy * cp, v.dist * sp, v.dist * cy * cp];
    let fwd = norm(mul(eye0, -1.0));
    let right = norm(cross(fwd, [0.0, 1.0, 0.0]));
    let up = cross(right, fwd);
    let eye = add(eye0, add(mul(right, v.pan_x), mul(up, v.pan_y)));
    // view: rows right, up, -fwd
    let view: M4 = [
        right[0], up[0], -fwd[0], 0.0, //
        right[1], up[1], -fwd[1], 0.0, //
        right[2], up[2], -fwd[2], 0.0, //
        -dot(right, eye), -dot(up, eye), dot(fwd, eye), 1.0,
    ];
    let tf = (20.0f32).to_radians().tan();
    let (n, f) = (0.03f32, 60.0f32);
    let proj: M4 = [
        1.0 / (aspect * tf), 0.0, 0.0, 0.0, //
        0.0, 1.0 / tf, 0.0, 0.0, //
        0.0, 0.0, -(f + n) / (f - n), -1.0, //
        0.0, 0.0, -2.0 * f * n / (f - n), 0.0,
    ];
    CamOut { vp: m_mul(&proj, &view), eye }
}

// ------------------------------------------------------------------------------------------------ GL helpers
const ANISO_PARAM: u32 = 0x84FE;
const ANISO_MAX: u32 = 0x84FF;

unsafe fn compile(gl: &glow::Context, vs: &str, fs: &str) -> Result<glow::Program, String> {
    let prog = gl.create_program()?;
    let mut shaders = Vec::new();
    for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
        let sh = gl.create_shader(kind)?;
        gl.shader_source(sh, src);
        gl.compile_shader(sh);
        if !gl.get_shader_compile_status(sh) {
            let log = gl.get_shader_info_log(sh);
            return Err(format!("shader compile failed: {log}"));
        }
        gl.attach_shader(prog, sh);
        shaders.push(sh);
    }
    gl.link_program(prog);
    if !gl.get_program_link_status(prog) {
        return Err(format!("shader link failed: {}", gl.get_program_info_log(prog)));
    }
    for sh in shaders {
        gl.detach_shader(prog, sh);
        gl.delete_shader(sh);
    }
    Ok(prog)
}

unsafe fn make_tex(gl: &glow::Context, internal: u32, fmt: u32, ty: u32, w: usize, h: usize, data: &[u8], aniso: f32) -> Result<Tx, String> {
    let id = gl.create_texture()?;
    gl.bind_texture(glow::TEXTURE_2D, Some(id));
    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
    gl.tex_image_2d(glow::TEXTURE_2D, 0, internal as i32, w as i32, h as i32, 0, fmt, ty, Some(data));
    gl.generate_mipmap(glow::TEXTURE_2D);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR_MIPMAP_LINEAR as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
    if aniso > 1.0 {
        gl.tex_parameter_f32(glow::TEXTURE_2D, ANISO_PARAM, aniso);
    }
    Ok(Tx { id, w, h })
}

fn bytes_of<T>(v: &[T]) -> &[u8] {
    // plain-old-data (f32, u32, [u32; 3]) only
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

const UNIFORMS: &[&str] = &[
    "uVP", "uHeight", "uDisp", "uLevel", "uVtxLod", "uBase", "uNormal", "uRma", "uEmis", "uEye", "uL", "uFill", "uLightI", "uExposure", "uAmbient", "uNormalAmt", "uBump",
    "uDepth", "uTexel", "uMode", "uHasHeight", "uHasEmis", "uWire", "uShadow", "uCull",
];

unsafe fn init(gl: &glow::Context, maps: &GpuMaps, aniso_req: f32) -> Result<Res, String> {
    let prog = compile(gl, VS, FS)?;
    let bg_prog = compile(gl, BG_VS, BG_FS)?;
    let mut locs = HashMap::new();
    for name in UNIFORMS {
        if let Some(l) = gl.get_uniform_location(prog, name) {
            locs.insert(*name, l);
        }
    }
    let ext = gl.supported_extensions();
    let has_aniso = ext.contains("GL_EXT_texture_filter_anisotropic") || ext.contains("GL_ARB_texture_filter_anisotropic");
    let max_aniso = if has_aniso { gl.get_parameter_f32(ANISO_MAX).max(1.0) } else { 1.0 };
    let aniso = if aniso_req <= 0.0 { max_aniso } else { aniso_req.min(max_aniso) };
    let vao = gl.create_vertex_array()?;
    let vbo = gl.create_buffer()?;
    let ebo = gl.create_buffer()?;
    let s = maps.size;
    let base = make_tex(gl, glow::SRGB8_ALPHA8, glow::RGBA, glow::UNSIGNED_BYTE, s, s, &maps.base, aniso)?;
    let normal = make_tex(gl, glow::RGBA8, glow::RGBA, glow::UNSIGNED_BYTE, s, s, &maps.normal, aniso)?;
    let rma = make_tex(gl, glow::RGBA8, glow::RGBA, glow::UNSIGNED_BYTE, s, s, &maps.rma, aniso)?;
    let emis = match &maps.emissive {
        Some(e) => Some(make_tex(gl, glow::SRGB8_ALPHA8, glow::RGBA, glow::UNSIGNED_BYTE, s, s, e, aniso)?),
        None => None,
    };
    let height = match &maps.height {
        Some((w, h, v)) => Some(make_tex(gl, glow::R16F, glow::RED, glow::FLOAT, *w, *h, bytes_of(v), aniso)?),
        None => None,
    };
    let dummy_h = make_tex(gl, glow::R16F, glow::RED, glow::FLOAT, 1, 1, bytes_of(&[0.0f32]), 1.0)?;
    Ok(Res { prog, bg_prog, locs, vao, vbo, ebo, index_count: 0, mesh_key: (u32::MAX, 0, 0), base, normal, rma, emis, height, dummy_h, max_aniso })
}

unsafe fn free_tex(gl: &glow::Context, t: &Tx) {
    gl.delete_texture(t.id);
}

unsafe fn upload_maps(gl: &glow::Context, r: &mut Res, maps: &GpuMaps, aniso_req: f32) -> Result<(), String> {
    let aniso = if aniso_req <= 0.0 { r.max_aniso } else { aniso_req.min(r.max_aniso) };
    let s = maps.size;
    let base = make_tex(gl, glow::SRGB8_ALPHA8, glow::RGBA, glow::UNSIGNED_BYTE, s, s, &maps.base, aniso)?;
    let normal = make_tex(gl, glow::RGBA8, glow::RGBA, glow::UNSIGNED_BYTE, s, s, &maps.normal, aniso)?;
    let rma = make_tex(gl, glow::RGBA8, glow::RGBA, glow::UNSIGNED_BYTE, s, s, &maps.rma, aniso)?;
    let emis = match &maps.emissive {
        Some(e) => Some(make_tex(gl, glow::SRGB8_ALPHA8, glow::RGBA, glow::UNSIGNED_BYTE, s, s, e, aniso)?),
        None => None,
    };
    let height = match &maps.height {
        Some((w, h, v)) => Some(make_tex(gl, glow::R16F, glow::RED, glow::FLOAT, *w, *h, bytes_of(v), aniso)?),
        None => None,
    };
    free_tex(gl, &r.base);
    free_tex(gl, &r.normal);
    free_tex(gl, &r.rma);
    if let Some(t) = &r.emis {
        free_tex(gl, t);
    }
    if let Some(t) = &r.height {
        free_tex(gl, t);
    }
    r.base = base;
    r.normal = normal;
    r.rma = rma;
    r.emis = emis;
    r.height = height;
    Ok(())
}

unsafe fn upload_mesh(gl: &glow::Context, r: &mut Res, v: &View) {
    let key = (v.shape, v.tess.clamp(8, 768), (v.tiling.max(0.1) * 1000.0) as u32);
    if r.mesh_key == key {
        return;
    }
    let m = build_mesh(v.shape, key.1, v.tiling, None, 0.0, 0.0);
    let mut data: Vec<f32> = Vec::with_capacity(m.pos.len() * 14);
    for i in 0..m.pos.len() {
        data.extend_from_slice(&m.pos[i]);
        data.extend_from_slice(&m.nrm[i]);
        data.extend_from_slice(&m.tan[i]);
        data.extend_from_slice(&m.uv[i]);
        data.extend_from_slice(&[m.su[i], m.sv[i], m.fade[i]]);
    }
    gl.bind_vertex_array(Some(r.vao));
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(r.vbo));
    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes_of(&data), glow::STATIC_DRAW);
    let stride = 14 * 4;
    for (loc, size, off) in [(0u32, 3, 0), (1, 3, 3), (2, 3, 6), (3, 2, 9), (4, 3, 11)] {
        gl.enable_vertex_attrib_array(loc);
        gl.vertex_attrib_pointer_f32(loc, size, glow::FLOAT, false, stride, off * 4);
    }
    gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(r.ebo));
    gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytes_of(&m.tris), glow::STATIC_DRAW);
    r.index_count = (m.tris.len() * 3) as i32;
    r.mesh_key = key;
}

// ------------------------------------------------------------------------------------------------ paint
/// Draw the viewport into the rectangle of the paint callback. Call from an `egui_glow::CallbackFn`.
pub fn paint(shared: &Shared, info: &PaintCallbackInfo, painter: &egui_glow::Painter, view: &View) {
    let mut g = match shared.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if g.error.is_some() {
        return;
    }
    let gl: &glow::Context = painter.gl();
    let aniso_req = g.opts.aniso;
    // first use: create everything from the pending maps (or neutral defaults)
    if g.res.is_none() {
        let maps = g.pending.take().unwrap_or_else(|| prepare(&MapInputs { base: None, normal: None, rough: None, metal: None, ao: None, emissive: None, height: None }));
        match unsafe { init(gl, &maps, aniso_req) } {
            Ok(r) => {
                g.renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };
                g.res = Some(r);
            }
            Err(e) => {
                g.error = Some(e);
                return;
            }
        }
    } else if let Some(maps) = g.pending.take() {
        let mut r = g.res.take().unwrap();
        let res = unsafe { upload_maps(gl, &mut r, &maps, aniso_req) };
        g.res = Some(r);
        if let Err(e) = res {
            g.error = Some(e);
            return;
        }
    }
    let self_shadow = g.opts.self_shadow;
    let want_shot = std::mem::take(&mut g.want_shot);
    let r = g.res.as_mut().unwrap();
    let vp_px = info.viewport_in_pixels();
    if vp_px.width_px <= 0 || vp_px.height_px <= 0 {
        return;
    }
    let aspect = vp_px.width_px as f32 / vp_px.height_px as f32;
    let cam = camera(view, aspect);
    let (ldir, fill) = crate::render3d::light_dirs(view);
    let has_h = r.height.is_some();
    let disp = if has_h && view.relief == RELIEF_DISPLACE { view.height_scale } else { 0.0 };
    let mesh_n = {
        let n = view.tess.clamp(8, 768);
        match view.shape {
            1 | 4 | 5 | 9 => (n * 5 / 8).max(8),
            6 => (n * 2).min(1024),
            _ => n,
        }
    } as f32;
    let (hw, hh) = r.height.as_ref().map(|t| (t.w, t.h)).unwrap_or((1, 1));
    let vtx_lod = ((hw as f32 * view.tiling.max(0.1) / mesh_n).max(1.0)).log2();
    let mode = if view.relief == RELIEF_BUMP {
        1
    } else if view.relief == RELIEF_PARALLAX {
        2
    } else if view.relief == RELIEF_DISPLACE {
        3
    } else {
        0
    };
    let mut result: Result<(), String> = Ok(());
    unsafe {
        upload_mesh(gl, r, view);
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LESS);
        gl.depth_mask(true);
        gl.clear_depth_f32(1.0);
        gl.clear(glow::DEPTH_BUFFER_BIT);
        // background gradient (no depth write)
        gl.depth_mask(false);
        gl.use_program(Some(r.bg_prog));
        gl.bind_vertex_array(Some(r.vao));
        gl.draw_arrays(glow::TRIANGLES, 0, 3);
        gl.depth_mask(true);
        // material pass
        gl.use_program(Some(r.prog));
        let loc = |n: &str| r.locs.get(n);
        gl.uniform_matrix_4_f32_slice(loc("uVP"), false, &cam.vp);
        gl.uniform_3_f32(loc("uEye"), cam.eye[0], cam.eye[1], cam.eye[2]);
        gl.uniform_3_f32(loc("uL"), ldir[0], ldir[1], ldir[2]);
        gl.uniform_3_f32(loc("uFill"), fill[0], fill[1], fill[2]);
        gl.uniform_1_f32(loc("uLightI"), view.light_intensity);
        gl.uniform_1_f32(loc("uExposure"), view.exposure);
        gl.uniform_1_f32(loc("uAmbient"), view.ambient);
        gl.uniform_1_f32(loc("uNormalAmt"), view.normal_amt.clamp(0.0, 1.0));
        gl.uniform_1_f32(loc("uBump"), view.bump);
        gl.uniform_1_f32(loc("uDepth"), view.height_scale);
        gl.uniform_1_f32(loc("uLevel"), view.height_level);
        gl.uniform_1_f32(loc("uDisp"), disp);
        gl.uniform_1_f32(loc("uVtxLod"), vtx_lod);
        gl.uniform_1_f32(loc("uTexel"), 1.0 / hw.max(hh).max(1) as f32);
        gl.uniform_1_i32(loc("uMode"), mode);
        gl.uniform_1_i32(loc("uHasHeight"), has_h as i32);
        gl.uniform_1_i32(loc("uHasEmis"), r.emis.is_some() as i32);
        gl.uniform_1_i32(loc("uShadow"), self_shadow as i32);
        gl.uniform_1_i32(loc("uCull"), crate::render3d::shape_culls(view.shape) as i32);
        gl.uniform_1_i32(loc("uWire"), 0);
        let units: [(&str, glow::Texture); 5] = [
            ("uBase", r.base.id),
            ("uNormal", r.normal.id),
            ("uRma", r.rma.id),
            ("uEmis", r.emis.as_ref().map(|t| t.id).unwrap_or(r.base.id)),
            ("uHeight", r.height.as_ref().map(|t| t.id).unwrap_or(r.dummy_h.id)),
        ];
        for (i, (name, tex)) in units.iter().enumerate() {
            gl.active_texture(glow::TEXTURE0 + i as u32);
            gl.bind_texture(glow::TEXTURE_2D, Some(*tex));
            gl.uniform_1_i32(loc(name), i as i32);
        }
        gl.bind_vertex_array(Some(r.vao));
        gl.draw_elements(glow::TRIANGLES, r.index_count, glow::UNSIGNED_INT, 0);
        if view.wireframe {
            gl.uniform_1_i32(loc("uWire"), 1);
            gl.polygon_mode(glow::FRONT_AND_BACK, glow::LINE);
            gl.enable(glow::POLYGON_OFFSET_LINE);
            gl.polygon_offset(-1.0, -1.0);
            gl.depth_func(glow::LEQUAL);
            gl.draw_elements(glow::TRIANGLES, r.index_count, glow::UNSIGNED_INT, 0);
            gl.disable(glow::POLYGON_OFFSET_LINE);
            gl.polygon_mode(glow::FRONT_AND_BACK, glow::FILL);
            gl.depth_func(glow::LESS);
        }
        // leave the state the way egui expects it
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.bind_vertex_array(None);
        gl.disable(glow::DEPTH_TEST);
        let e = gl.get_error();
        if e != glow::NO_ERROR {
            result = Err(format!("OpenGL error 0x{e:x} while drawing the 3D view"));
        }
    }
    if want_shot && result.is_ok() {
        let (w, h) = (vp_px.width_px as usize, vp_px.height_px as usize);
        let mut buf = vec![0u8; w * h * 4];
        unsafe {
            gl.read_pixels(vp_px.left_px, vp_px.from_bottom_px, w as i32, h as i32, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(&mut buf));
        }
        // flip: GL rows start at the bottom
        let mut top = vec![0u8; w * h * 4];
        for y in 0..h {
            top[y * w * 4..(y + 1) * w * 4].copy_from_slice(&buf[(h - 1 - y) * w * 4..(h - y) * w * 4]);
        }
        g.shot = Some((w, h, top));
    }
    g.frames += 1;
    if let Err(e) = result {
        g.error = Some(e);
    }
}

/// Free all GL objects (call when the viewer is closed or the GL context goes away).
pub fn release(shared: &Shared, gl: &glow::Context) {
    if let Ok(mut g) = shared.lock() {
        if let Some(r) = g.res.take() {
            unsafe {
                gl.delete_program(r.prog);
                gl.delete_program(r.bg_prog);
                gl.delete_vertex_array(r.vao);
                gl.delete_buffer(r.vbo);
                gl.delete_buffer(r.ebo);
                free_tex(gl, &r.base);
                free_tex(gl, &r.normal);
                free_tex(gl, &r.rma);
                free_tex(gl, &r.dummy_h);
                if let Some(t) = &r.emis {
                    free_tex(gl, t);
                }
                if let Some(t) = &r.height {
                    free_tex(gl, t);
                }
            }
        }
    }
}
