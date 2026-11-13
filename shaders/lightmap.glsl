#ifndef MELON_LIGHTMAP_GLSL
#define MELON_LIGHTMAP_GLSL

// Vanilla `core/lightmap.fsh`, evaluated per fragment in `world.frag`.
#include "globals.glsl"

// `BLOCK_LIGHT_TINT` and `AMBIENT_LIGHT_COLOR` of the dimension (the
// overworld's 0xFFD88C and 0x0A0A0A: light 0 is dark, not black) come from
// the frame's `Globals`.
// `LightmapRenderState.blockFactor` without the flicker.
const float BLOCK_FACTOR = 1.4;

// Levels are floats so smooth lighting's blended corners work.
float get_brightness(float level) {
    float v = level / 15.0;
    return v / (4.0 - 3.0 * v);
}

vec3 not_gamma(vec3 c) {
    float maxc = max(c.r, max(c.g, c.b));
    if (maxc <= 0.0) {
        return vec3(0.0);
    }
    float inv = 1.0 - maxc;
    float scaled = 1.0 - inv * inv * inv * inv;
    return c * (scaled / maxc);
}

vec3 lightmap(float block_level, float sky_level, vec3 sky_light, float gamma) {
    float block_b = get_brightness(block_level) * BLOCK_FACTOR;
    float sky_b = get_brightness(sky_level);
    float t = block_level / 15.0;
    float parabolic = (2.0 * t - 1.0);
    parabolic *= parabolic;
    vec3 block_color = mix(g.block_tint.rgb, vec3(1.0), 0.9 * parabolic);
    // Clamped before `notGamma` like vanilla: torch plus daylight sums past
    // 1, where the curve would turn back down.
    vec3 color = clamp(g.ambient_color.rgb + sky_light * sky_b + block_color * block_b, 0.0, 1.0);
    return mix(color, not_gamma(color), gamma);
}

#endif
