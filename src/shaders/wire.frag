#version 330 core
uniform sampler2D uDepthTex;
uniform vec2 uOrigin;
uniform float uNear;
uniform float uFar;
out vec4 oColor;
void main() {
    float d = texelFetch(uDepthTex, ivec2(gl_FragCoord.xy - uOrigin), 0).r;
    // hidden behind the surface? (compare in linear depth with a small tolerance)
    float zn = gl_FragCoord.z * 2.0 - 1.0;
    float zl = 2.0 * uNear * uFar / (uFar + uNear - zn * (uFar - uNear));
    float dn = d * 2.0 - 1.0;
    float dl = 2.0 * uNear * uFar / (uFar + uNear - dn * (uFar - uNear));
    if (d < 1.0 && zl > dl * 1.0015 + 0.0005) discard;
    oColor = vec4(1.0, 0.62, 0.12, 0.85);
}
