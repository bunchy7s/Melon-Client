#version 450

// Vanilla `post/box_blur.fsh`: a box blur of `radius` pixels along one axis,
// taken as taps two pixels apart so the linear filter averages two texels
// each. Run once per axis. The extra half-weight sample at `radius` and the
// divide by `radius + 0.5` are vanilla's.

layout(location = 0) in vec2 frag_uv;

layout(location = 0) out vec4 out_color;

layout(set = 0, binding = 0) uniform sampler2D tex_sampler;

layout(push_constant) uniform PushConstants {
    layout(offset = 0) vec2 texel_step;
    layout(offset = 8) float radius;
} pc;

void main() {
    vec4 blurred = vec4(0.0);
    float radius = pc.radius;
    for (float a = -radius + 0.5; a <= radius; a += 2.0) {
        blurred += texture(tex_sampler, frag_uv + pc.texel_step * a);
    }
    blurred += texture(tex_sampler, frag_uv + pc.texel_step * radius) * 0.5;
    out_color = blurred / (radius + 0.5);
}
