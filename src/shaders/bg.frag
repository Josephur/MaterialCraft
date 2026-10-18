#version 330 core
in vec2 vP;
uniform sampler2D uEnv;
uniform int uBgMode;   // 0 gradient, 1 colour, 2 environment
uniform int uChannel;  // != 0: display referred output
uniform vec3 uBgColor; // linear
uniform vec2 uEnvRot;
uniform float uBgGain;
uniform float uBgLod;
uniform float uTanHalf;
uniform float uAspect;
uniform vec3 uCamR;
uniform vec3 uCamU;
uniform vec3 uCamF;
out vec4 oColor;
const float PI = 3.14159265;
void main() {
    vec3 c;
    float t = vP.y * 0.5 + 0.5; // 1 at the top
    vec3 grad = vec3(0.044 + 0.026 * t);
    grad.b *= 1.05;
    if (uChannel != 0) {
        oColor = vec4(pow(grad, vec3(1.0 / 2.2)), 1.0);
        return;
    }
    if (uBgMode == 0) {
        c = grad;
    } else if (uBgMode == 1) {
        c = uBgColor;
    } else {
        vec3 d = normalize(uCamF + uCamR * (vP.x * uAspect * uTanHalf) + uCamU * (vP.y * uTanHalf));
        vec3 e = vec3(d.x * uEnvRot.x - d.z * uEnvRot.y, d.y, d.x * uEnvRot.y + d.z * uEnvRot.x);
        vec2 uv = vec2(0.5 + atan(e.x, -e.z) / (2.0 * PI), acos(clamp(e.y, -1.0, 1.0)) / PI);
        c = textureLod(uEnv, uv, uBgLod).rgb * uBgGain;
    }
    oColor = vec4(min(c, vec3(60000.0)), 1.0);
}
