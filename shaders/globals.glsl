#ifndef MELON_GLOBALS_GLSL
#define MELON_GLOBALS_GLSL

// Frame-wide world constants (set 3). Vanilla keeps the same values in the
// `Fog` and `LightmapInfo` uniform blocks (`include/globals.glsl` is only
// camera/time there). Explicit set+binding: Vulkan GLSL, not shaderc auto-bind.
layout(std140, set = 3, binding = 0) uniform Globals {
    // rgb: the fog's colour, which the frame is cleared to; a: 0 without fog.
    vec4 fog_color;
    // The environmental fog's start and end, then the render distance fog's.
    vec4 fog_env;
    // x: where the sky has faded into the fog. y: the clouds'. z: aspect.
    vec4 fog_sky;
    // `AMBIENT_LIGHT_COLOR`: what light level 0 shows.
    vec4 ambient_color;
    // `BLOCK_LIGHT_TINT`.
    vec4 block_tint;
    // `CardinalLighting` relative to the default: down, up, north, south,
    // then west and east. 1 in every dimension but the nether.
    vec4 cardinal_a;
    vec4 cardinal_b;
    // xyz: the world's up direction in view space.
    vec4 view_up;
    // x: `Util.getMillis() * glintSpeed * 8`, y: `GlintAlpha`.
    vec4 glint;
} g;

#endif
