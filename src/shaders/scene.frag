#version 330 core
in vec3 vPos;
in vec3 vN;
in vec3 vT;
in vec2 vUv;
in vec2 vScale;
flat in int vObj;
uniform sampler2D uBase;
uniform sampler2D uNormal;
uniform sampler2D uRma;
uniform sampler2D uEmis;
uniform sampler2D uHeight;
uniform sampler2D uEnv;
uniform sampler2DArray uShadowMap;
uniform vec3 uSH[9];
uniform vec2 uEnvRot;      // cos, sin of the environment rotation
uniform float uEnvGain;    // lighting gain (exposure stops x ambient)
uniform float uEnvLevels;  // number of prefiltered levels
uniform int uNLights;
uniform vec4 uLAxis[6];    // xyz: unit direction towards the light from the middle, w: distance
uniform vec4 uLInfo[6];    // x: kind (0 directional, 1 point, 2 spot), y: cos outer, z: cos inner, w: softness
uniform vec3 uLCol[6];     // colour x intensity
uniform int uLShadow[6];   // shadow map layer or -1
uniform mat4 uLVP[3];
uniform vec4 uObjA[8];     // x: 0 graph / 1 fixed, y rough, z metal, w cull
uniform vec3 uObjC[8];     // fixed base colour (linear)
uniform vec3 uEye;
uniform float uExposure;
uniform float uNormalAmt;
uniform float uBump;
uniform float uDepth;
uniform float uLevel;
uniform float uTexel;
uniform int uMode;
uniform int uHasHeight;
uniform int uHasEmis;
uniform int uShadow;       // height-field contact shadows (parallax mode)
uniform int uChannel;
out vec4 oColor;

const float PI = 3.14159265;
vec2 gdx;
vec2 gdy;

// ---------------------------------------------------------------- environment
vec3 envIn(vec3 d) {
    return vec3(d.x * uEnvRot.x - d.z * uEnvRot.y, d.y, d.x * uEnvRot.y + d.z * uEnvRot.x);
}
vec3 envLod(vec3 d, float lod) {
    vec3 e = envIn(d);
    vec2 uv = vec2(0.5 + atan(e.x, -e.z) / (2.0 * PI), acos(clamp(e.y, -1.0, 1.0)) / PI);
    return textureLod(uEnv, uv, lod).rgb;
}
vec3 envSpec(vec3 d, float rough) {
    return envLod(d, clamp(rough, 0.0, 1.0) * (uEnvLevels - 1.0)) * uEnvGain;
}
vec3 envIrr(vec3 n) {
    vec3 d = envIn(n);
    float x = d.x, y = d.y, z = d.z;
    float b[9] = float[9](0.282095, 0.488603 * y, 0.488603 * z, 0.488603 * x, 1.092548 * x * y, 1.092548 * y * z, 0.315392 * (3.0 * z * z - 1.0), 1.092548 * x * z, 0.546274 * (x * x - y * y));
    vec3 o = vec3(0.0);
    for (int i = 0; i < 9; i++) o += uSH[i] * b[i];
    return max(o, vec3(0.0)) * uEnvGain;
}
vec2 envBrdfAB(float rough, float ndv) {
    const vec4 c0 = vec4(-1.0, -0.0275, -0.572, 0.022);
    const vec4 c1 = vec4(1.0, 0.0425, 1.04, -0.04);
    vec4 r = rough * c0 + c1;
    float a004 = min(r.x * r.x, exp2(-9.28 * ndv)) * r.x + r.y;
    return vec2(-1.04, 1.04) * a004 + r.zw;
}

// ---------------------------------------------------------------- lights
void lightAt(int i, vec3 p, out vec3 L, out vec3 E, out float dist) {
    vec4 ax = uLAxis[i];
    vec4 info = uLInfo[i];
    vec3 c = uLCol[i];
    if (info.x < 0.5) {
        L = ax.xyz;
        E = c;
        dist = 1e30;
        return;
    }
    vec3 v = ax.xyz * ax.w - p;
    float d2 = max(dot(v, v), 1e-4);
    float d = sqrt(d2);
    L = v / d;
    float k = 1.0 / d2;
    if (info.x > 1.5) {
        float cd = dot(L, ax.xyz);
        float t = clamp((cd - info.y) / max(info.z - info.y, 1e-4), 0.0, 1.0);
        k *= t * t * (3.0 - 2.0 * t);
    }
    E = c * k;
    dist = d;
}

float ign(vec2 p) {
    return fract(52.9829189 * fract(dot(p, vec2(0.06711056, 0.00583715))));
}

float shadowAt(int li, vec3 p, vec3 ng, float soft) {
    int layer = uLShadow[li];
    if (layer < 0) return 1.0;
    vec4 q = uLVP[layer] * vec4(p + ng * 0.012, 1.0);
    vec3 s = q.xyz / q.w * 0.5 + 0.5;
    if (s.x < 0.0 || s.x > 1.0 || s.y < 0.0 || s.y > 1.0 || s.z > 1.0) return 1.0;
    vec2 texel = 1.0 / vec2(textureSize(uShadowMap, 0).xy);
    float rad = (1.5 + soft * 10.0) * texel.x;
    float rot = ign(gl_FragCoord.xy) * 2.0 * PI;
    float lit = 0.0;
    const int N = 16;
    for (int i = 0; i < N; i++) {
        float f = (float(i) + 0.5) / float(N);
        float a = float(i) * 2.399963 + rot;
        vec2 o = vec2(cos(a), sin(a)) * sqrt(f) * rad;
        float d = texture(uShadowMap, vec3(s.xy + o, float(layer))).r;
        lit += (s.z - 0.0015 > d) ? 0.0 : 1.0;
    }
    return lit / float(N);
}

// ---------------------------------------------------------------- relief
float hAt(vec2 uv) {
    return textureGrad(uHeight, uv, gdx, gdy).r;
}

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
    float jit = ign(gl_FragCoord.xy);
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
    vec4 oa = uObjA[vObj];
    bool fixedMat = oa.x > 0.5;
    vec3 N = normalize(vN);
    // shapes seen from the inside: walls that face away from the camera are not drawn
    if (oa.w > 0.5 && dot(N, uEye - vPos) < 0.0) discard;
    vec3 T = normalize(vT - N * dot(vT, N));
    vec3 B = cross(N, T);
    vec3 V = normalize(uEye - vPos);
    bool back = !gl_FrontFacing;
    float flip = back ? -1.0 : 1.0;
    gdx = dFdx(vUv);
    gdy = dFdy(vUv);
    vec2 uv = vUv;
    float cav = 1.0;
    bool hasH = uHasHeight == 1 && uDepth > 0.0 && !fixedMat;

    vec3 albedo;
    float rough, metal, ao;
    vec3 nm = vec3(0.5, 0.5, 1.0);
    vec3 n;
    vec3 emis = vec3(0.0);
    vec3 baseRaw;
    if (fixedMat) {
        albedo = uObjC[vObj];
        rough = clamp(oa.y, 0.04, 1.0);
        metal = oa.z;
        ao = 1.0;
        n = N * flip;
        baseRaw = pow(albedo, vec3(1.0 / 2.2));
    } else {
        if (uMode == 2 && hasH) {
            vec3 vt = vec3(dot(V, T), dot(V, B), dot(V, N) * flip);
            float dep;
            uv = pom(uv, vt, dep);
            cav = 1.0 - 0.45 * dep;
        }
        baseRaw = textureGrad(uBase, uv, gdx, gdy).rgb;   // sRGB texture: linear here
        albedo = baseRaw;
        baseRaw = pow(baseRaw, vec3(1.0 / 2.2));
        vec3 rma = textureGrad(uRma, uv, gdx, gdy).rgb;
        rough = clamp(rma.r, 0.04, 1.0);
        metal = clamp(rma.g, 0.0, 1.0);
        ao = clamp(rma.b, 0.0, 1.0) * cav;
        nm = textureGrad(uNormal, uv, gdx, gdy).xyz;
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
        n = normalize(T * tn.x + B * tn.y + N * tn.z) * flip;
        if (uHasEmis == 1) emis = textureGrad(uEmis, uv, gdx, gdy).rgb;
    }

    // ------------------------------------------------ channel views (display referred)
    if (uChannel != 0) {
        vec3 c = vec3(0.0);
        if (uChannel == 1) c = baseRaw;
        else if (uChannel == 2) c = vec3(rough);
        else if (uChannel == 3) c = vec3(metal);
        else if (uChannel == 4) c = nm;
        else if (uChannel == 5) c = vec3(ao);
        else if (uChannel == 6) c = fixedMat ? vec3(0.5) : vec3(uHasHeight == 1 ? textureGrad(uHeight, uv, gdx, gdy).r : 0.5);
        else if (uChannel == 7) c = fixedMat ? vec3(0.0) : pow(emis, vec3(1.0 / 2.2));
        else if (uChannel == 8) c = n * 0.5 + 0.5;
        else {
            vec2 f = fract(uv);
            float chk = (int(floor(uv.x * 8.0)) + int(floor(uv.y * 8.0))) % 2 == 0 ? 1.0 : 0.8;
            c = vec3(f.x * chk, f.y * chk, 0.2 * chk);
        }
        oColor = vec4(c, 1.0);
        return;
    }

    vec3 ng = N * flip;
    float ndv = max(dot(n, V), 1e-3);
    vec3 f0 = mix(vec3(0.04), albedo, metal);
    float a = rough * rough;
    float a2 = a * a;
    float kk = (rough + 1.0) * (rough + 1.0) / 8.0;

    // ------------------------------------------------ image based lighting
    vec3 refl = reflect(-V, n);
    vec2 ab = envBrdfAB(rough, ndv);
    float specOcc = clamp(pow(ndv + ao, exp2(-16.0 * rough - 1.0)) - 1.0 + ao, 0.0, 1.0);
    vec3 col = albedo * (1.0 - metal) * envIrr(n) * ao + envSpec(refl, rough) * (f0 * ab.x + ab.y) * specOcc;

    // ------------------------------------------------ lights
    for (int i = 0; i < uNLights; i++) {
        vec3 L;
        vec3 E;
        float dist;
        lightAt(i, vPos, L, E, dist);
        float ndl = max(dot(n, L), 0.0);
        if (ndl <= 0.0) continue;
        float vis = shadowAt(i, vPos, ng, uLInfo[i].w);
        if (uShadow == 1 && i == 0 && hasH && uMode == 2 && !back) {
            vec3 lt = vec3(dot(L, T), dot(L, B), dot(L, N));
            vis *= hshadow(uv, lt);
        }
        if (vis <= 0.0) continue;
        vec3 hv = normalize(L + V);
        float ndh = max(dot(n, hv), 0.0);
        float vdh = max(dot(V, hv), 0.0);
        float d = a2 / max(PI * pow(ndh * ndh * (a2 - 1.0) + 1.0, 2.0), 1e-20);
        float g = (ndl / (ndl * (1.0 - kk) + kk)) * (ndv / (ndv * (1.0 - kk) + kk));
        float fr = pow(1.0 - vdh, 5.0);
        vec3 f = f0 + (1.0 - f0) * fr;
        vec3 spec = d * g * f / (4.0 * max(ndl, 1e-3) * ndv);
        vec3 diff = albedo * (1.0 - metal) * (1.0 - f) / PI;
        col += (diff + spec) * E * ndl * vis;
    }
    col += emis;
    col *= uExposure;
    oColor = vec4(min(col, vec3(60000.0)), 1.0);
}
