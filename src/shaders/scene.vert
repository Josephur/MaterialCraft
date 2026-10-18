#version 330 core
layout(location = 0) in vec3 aPos;
layout(location = 1) in vec3 aNrm;
layout(location = 2) in vec3 aTan;
layout(location = 3) in vec2 aUv;
layout(location = 4) in vec3 aMisc; // su, sv, displacement fade
layout(location = 5) in vec3 aDisp; // direction the vertex moves in when displaced
layout(location = 6) in float aObj; // object index (selects the material)
uniform mat4 uVP;
uniform sampler2D uHeight;
uniform float uDisp;
uniform float uLevel;
uniform float uVtxLod;
uniform vec4 uObjA[8]; // x: 0 = graph material, 1 = fixed; y rough; z metal; w cull inside walls
out vec3 vPos;
out vec3 vN;
out vec3 vT;
out vec2 vUv;
out vec2 vScale;
flat out int vObj;
void main() {
    int oi = int(aObj + 0.5);
    vec3 p = aPos;
    if (uDisp != 0.0 && uObjA[oi].x < 0.5) {
        float h = textureLod(uHeight, aUv, uVtxLod).r;
        p += aDisp * (h - uLevel) * uDisp * aMisc.z;
    }
    vPos = p;
    vN = aNrm;
    vT = aTan;
    vUv = aUv;
    vScale = aMisc.xy;
    vObj = oi;
    gl_Position = uVP * vec4(p, 1.0);
}
