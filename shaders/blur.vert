#version 450

// A full-screen triangle from the vertex index alone: no vertex buffer.
// Same construction as vanilla `core/screenquad.vsh`. `gl_VertexIndex` is
// the Vulkan builtin. UV (0,0) is the top-left of Vulkan NDC.

layout(location = 0) out vec2 frag_uv;

void main() {
    frag_uv = vec2((gl_VertexIndex << 1) & 2, gl_VertexIndex & 2);
    gl_Position = vec4(frag_uv * 2.0 - 1.0, 0.0, 1.0);
}
