#version 450

// Vanilla `shaders/core/panorama.vsh`: cube position is the cubemap sample
// direction. Push-constant is already `ProjMat * ModelViewMat`.

layout(location = 0) in vec3 in_pos;

layout(location = 0) out vec3 frag_dir;

layout(push_constant) uniform PushConstants {
    layout(offset = 0) mat4 view_proj;
} pc;

void main() {
    gl_Position = pc.view_proj * vec4(in_pos, 1.0);
    frag_dir = in_pos;
}
