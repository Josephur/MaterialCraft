#version 330 core
// Post-processing, a line by line port of src/post.rs (the CPU renderer) so both views agree.
uniform sampler2D uSrc;
uniform sampler2D uDepthTex;
uniform sampler2D uBright;
uniform int uStage;      // 0 ambient occlusion, 1 depth of field, 2 bloom threshold, 3 final
uniform int uBypass;     // channel views are shown as they are
uniform vec2 uSize;
uniform vec2 uOrigin;   // lower left corner of the viewport in window pixels (0 for offscreen passes)
uniform float uNear;
uniform float uFar;
uniform float uTanHalf;
uniform float uAspect;
uniform float uCentreDist;
// settings
uniform float uSsao;
uniform float uSsaoRadius;
uniform float uDof;
uniform float uFocus;
uniform float uBloom;
uniform float uBloomThreshold;
uniform int uBloomLevels;
uniform float uBloomW[8];
uniform float uVignette;
uniform float uContrast;
uniform float uSaturation;
uniform vec3 uWb;
uniform int uTone;
out vec4 oColor;

const float PI = 3.14159265;
const float INF = 1e30;

int W;
int H;

float linDepth(float d) {
    if (d >= 1.0) return INF;
    float zn = d * 2.0 - 1.0;
    return 2.0 * uNear * uFar / (uFar + uNear - zn * (uFar - uNear));
}
float zAtCpu(int x, int y) {
    x = clamp(x, 0, W - 1);
    y = clamp(y, 0, H - 1);
    return linDepth(texelFetch(uDepthTex, ivec2(x, H - 1 - y), 0).r);
}
vec3 srcAtCpu(int x, int y) {
    return texelFetch(uSrc, ivec2(x, H - 1 - y), 0).rgb;
}
float ign(float x, float y) {
    return fract(52.982919 * fract(x * 0.06711056 + y * 0.00583715));
}
vec3 viewPos(float x, float y, float z) {
    float nx = (x + 0.5) / float(W) * 2.0 - 1.0;
    float ny = 1.0 - (y + 0.5) / float(H) * 2.0;
    return vec3(nx * uAspect * uTanHalf * z, ny * uTanHalf * z, z);
}

float ssaoFactor(int x, int y) {
    float z = zAtCpu(x, y);
    if (z > 1e29) return 1.0;
    vec3 p = viewPos(float(x), float(y), z);
    float zxp = zAtCpu(x + 1, y);
    float zxm = zAtCpu(x - 1, y);
    float zyp = zAtCpu(x, y + 1);
    float zym = zAtCpu(x, y - 1);
    float zx, sx, zy, sy;
    if (abs(zxp - z) < abs(zxm - z)) { zx = zxp; sx = 1.0; } else { zx = zxm; sx = -1.0; }
    if (abs(zyp - z) < abs(zym - z)) { zy = zyp; sy = 1.0; } else { zy = zym; sy = -1.0; }
    vec3 px = viewPos(float(x) + sx, float(y), zx < 1e29 ? zx : z);
    vec3 py = viewPos(float(x), float(y) + sy, zy < 1e29 ? zy : z);
    vec3 n = normalize(cross(px - p, py - p));
    if (dot(n, p) > 0.0) n = -n;
    vec3 up = abs(n.y) < 0.999 ? vec3(0.0, 1.0, 0.0) : vec3(1.0, 0.0, 0.0);
    vec3 t = normalize(cross(up, n));
    vec3 b = cross(n, t);
    float rot = ign(float(x), float(y)) * 2.0 * PI;
    const int N = 14;
    float occ = 0.0;
    for (int i = 0; i < N; i++) {
        float f = (float(i) + 0.5) / float(N);
        float a = f * 7.0 * PI + rot;
        float hz = max(1.0 - f * 0.95, 0.05);
        float hr = sqrt(max(1.0 - hz * hz, 0.0));
        vec3 v = vec3(cos(a) * hr, sin(a) * hr, hz);
        float scale = uSsaoRadius * (0.2 + 0.8 * f * f);
        vec3 sp = p + (t * v.x + b * v.y + n * v.z) * scale;
        if (sp.z <= 0.05) continue;
        float sxs = ((sp.x / (sp.z * uAspect * uTanHalf)) * 0.5 + 0.5) * float(W) - 0.5;
        float sys = (0.5 - sp.y / (sp.z * uTanHalf) * 0.5) * float(H) - 0.5;
        float sz = zAtCpu(int(floor(sxs + 0.5)), int(floor(sys + 0.5)));
        if (sz < 1e29 && sz < sp.z - 0.01 * uSsaoRadius) {
            float r = clamp(uSsaoRadius / max(abs(p.z - sz), 1e-4), 0.0, 1.0);
            occ += r * r * (3.0 - 2.0 * r);
        }
    }
    return 1.0 - occ / float(N);
}

float coc(float z) {
    float maxR = 0.014 * float(H);
    if (z > 1e29) return maxR;
    return min(uDof * abs(z - (uFocus > 0.0 ? uFocus : uCentreDist)) / max(z, 0.05) * 0.045 * float(H), maxR);
}

float aces1(float x) {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
}
vec3 aces(vec3 c) { return vec3(aces1(c.r), aces1(c.g), aces1(c.b)); }
float hable1(float x) {
    const float A = 0.15, B = 0.50, C = 0.10, D = 0.20, E = 0.02, F = 0.30;
    return ((x * (A * x + C * B) + D * E) / (x * (A * x + B) + D * F)) - E / F;
}
vec3 tonemap(vec3 c) {
    c = max(c, vec3(0.0));
    if (uTone == 4) return min(c, vec3(1.0));
    if (uTone == 3) return c / (1.0 + c);
    if (uTone == 1) return aces(c);
    if (uTone == 2) {
        float w = 1.0 / hable1(11.2);
        return clamp(vec3(hable1(2.0 * c.r), hable1(2.0 * c.g), hable1(2.0 * c.b)) * w, 0.0, 1.0);
    }
    float l = dot(c, vec3(0.2126, 0.7152, 0.0722));
    float k = aces1(l) / max(l, 1e-4);
    vec3 hue = min(c * k, vec3(1.0));
    vec3 a = aces(c);
    return mix(hue, a, smoothstep(2.0, 8.0, l) * 0.7);
}

void main() {
    W = int(uSize.x);
    H = int(uSize.y);
    vec2 fc = gl_FragCoord.xy - uOrigin;
    ivec2 P = ivec2(fc);
    int x = P.x;
    int y = H - 1 - P.y; // row counted from the top, as in the CPU code
    vec2 uv = fc / uSize;
    if (uStage == 0) {
        vec3 c = texelFetch(uSrc, P, 0).rgb;
        float k = 1.0 - uSsao * (1.0 - ssaoFactor(x, y));
        oColor = vec4(c * k, 1.0);
        return;
    }
    if (uStage == 1) {
        vec3 c0 = texelFetch(uSrc, P, 0).rgb;
        float r = coc(zAtCpu(x, y));
        if (r < 0.5) { oColor = vec4(c0, 1.0); return; }
        float rot = ign(float(x), float(y)) * 2.0 * PI;
        vec3 acc = vec3(0.0);
        float ws = 0.0;
        const int N = 28;
        for (int k = 0; k < N; k++) {
            float f = (float(k) + 0.5) / float(N);
            float a = float(k) * 2.399963 + rot;
            float rr = sqrt(f) * r;
            int sx = int(clamp(floor(float(x) + cos(a) * rr + 0.5), 0.0, float(W - 1)));
            int sy = int(clamp(floor(float(y) + sin(a) * rr + 0.5), 0.0, float(H - 1)));
            float wgt = (coc(zAtCpu(sx, sy)) + 0.5 >= rr) ? 1.0 : 0.0;
            acc += srcAtCpu(sx, sy) * wgt;
            ws += wgt;
        }
        if (ws < 1.0) { oColor = vec4(c0, 1.0); return; }
        oColor = vec4(acc / ws, 1.0);
        return;
    }
    if (uStage == 2) {
        vec3 c = texelFetch(uSrc, P, 0).rgb;
        float l = dot(c, vec3(0.2126, 0.7152, 0.0722));
        float thr = uBloomThreshold;
        float k = smoothstep(thr, thr + 0.5 + thr * 0.5, l);
        oColor = vec4(c * k, 1.0);
        return;
    }
    vec3 c = texelFetch(uSrc, P, 0).rgb;
    if (uBypass == 1) { oColor = vec4(c, 1.0); return; }
    if (uBloom > 0.0) {
        vec3 b = vec3(0.0);
        for (int k = 0; k < 8; k++) {
            if (k >= uBloomLevels) break;
            b += textureLod(uBright, uv, float(k)).rgb * uBloomW[k];
        }
        c += b * uBloom;
    }
    c *= uWb;
    if (abs(uContrast - 1.0) > 1e-4) c = 0.18 * pow(max(c, vec3(0.0)) / 0.18, vec3(uContrast));
    if (abs(uSaturation - 1.0) > 1e-4) {
        float l = dot(c, vec3(0.2126, 0.7152, 0.0722));
        c = vec3(l) + (c - vec3(l)) * uSaturation;
    }
    if (uVignette > 0.0) {
        float nx = (float(x) + 0.5) / float(W) * 2.0 - 1.0;
        float ny = (float(y) + 0.5) / float(H) * 2.0 - 1.0;
        float r = sqrt(nx * nx * clamp(uAspect, 0.5, 1.5) + ny * ny) / 1.5;
        c *= 1.0 - uVignette * smoothstep(0.35, 1.0, r);
    }
    oColor = vec4(pow(clamp(tonemap(c), 0.0, 1.0), vec3(1.0 / 2.2)), 1.0);
}
