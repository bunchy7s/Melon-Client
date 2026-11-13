#version 450

// Vanilla `shaders/core/panorama.fsh`.

layout(location = 0) in vec3 frag_dir;

layout(location = 0) out vec4 out_color;

layout(set = 0, binding = 0) uniform samplerCube cube_sampler;

void main() {
    out_color = texture(cube_sampler, frag_dir);
}
