#version 450

// `core/sky.fsh`: the colour mixed into the fog by where the sky ends.
// Vanilla calls `apply_fog(ColorModulator, spherical, cylindrical, 0,
// FogSkyEnd, FogSkyEnd, FogSkyEnd, FogColor)`.

#include "fog.glsl"
#include "world_push.glsl"

layout(location = 0) in vec2 frag_fog;

layout(location = 0) out vec4 out_color;

void main() {
    // The environmental fog from the camera to the sky's end, the render
    // distance's (a step there) beyond it.
    float sky_end = g.fog_sky.x;
    float value = max(
        linear_fog_value(frag_fog.x, 0.0, sky_end),
        linear_fog_value(frag_fog.y, sky_end, sky_end));
    out_color = vec4(mix(pc.ambient.rgb, g.fog_color.rgb, value * g.fog_color.a), 1.0);
}
