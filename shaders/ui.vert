#version 450

// GUI quads in pixel coords. Y is already down, matching Vulkan NDC, so
// there is no clip-Y flip. Vanilla `core/position_tex_color.vsh` uses a
// projection UBO instead.

layout(location = 0) in vec2 in_pos;
layout(location = 1) in vec2 in_uv;
layout(location = 2) in vec4 in_color;

layout(location = 0) out vec2 frag_uv;
layout(location = 1) out vec4 frag_color;

layout(push_constant) uniform PushConstants {
    layout(offset = 0) vec2 viewport_size;
} pc;

void main() {
    vec2 ndc = (in_pos / pc.viewport_size) * 2.0 - 1.0;
    gl_Position = vec4(ndc, 0.0, 1.0);
    frag_uv = in_uv;
    frag_color = in_color;
}
