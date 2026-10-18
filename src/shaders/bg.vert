#version 330 core
out vec2 vP;
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    vP = p * 2.0 - 1.0;
    gl_Position = vec4(p * 2.0 - 1.0, 1.0, 1.0);
}
