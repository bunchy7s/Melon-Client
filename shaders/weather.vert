#version 450

// Rain and snow (vanilla `WeatherEffectRenderer`) as only the drops: the
// texture is ~97% clear, and a column quad shaded every one of those
// texels only to discard it. A static template holds the texture's drop
// streaks (`world::weather::template`); each instance is one repeat of the
// texture along one column (`world::weather::pieces`). A streak is cut to
// its column and placed on the column's quad where that quad would have
// sampled it: the same plane, UVs and texels (nearest), so the same image,
// with fragments only where drops are. Same outputs as `world.vert`.

#include "fog.glsl"
#include "world_push.glsl"
#include "world_fog.glsl"

// Template: texel column, top and bottom row as fractions of the height;
// one texel's width; the corner (0-3) and which copy (1: wrapped round).
layout(location = 0) in vec3 in_streak;
layout(location = 1) in vec2 in_texel;
layout(location = 3) in uvec2 in_corner;
// Instance: centre x, z, half width along x; half width along z, bottom;
// light and fade; top, u offset, v at the top; the texture repeat.
layout(location = 4) in vec3 in_place;
layout(location = 5) in vec2 in_place2;
layout(location = 6) in vec4 in_color;
layout(location = 7) in vec3 in_span;
layout(location = 8) in vec2 in_repeat;

layout(location = 0) out vec2 frag_uv;
layout(location = 1) out vec4 frag_color;
layout(location = 2) out vec2 frag_light;
layout(location = 3) out vec2 frag_fog;

void main() {
    frag_light = vec2(-1.0);
    // Across the quad `s` runs 0..1 and `u` = u offset + s, so the texel
    // column starts at s = fract(column / width - u offset). Its other copy
    // takes the part past s = 1, back at the quad's start.
    float width = in_texel.x;
    float s_lo = fract(in_streak.x * width - in_span.y);
    float s_hi = s_lo + width;
    vec2 s = in_corner.y == 0u ? vec2(s_lo, min(s_hi, 1.0)) : vec2(0.0, s_hi - 1.0);
    // Down the column `v` runs from the top's to the bottom's; this repeat
    // holds the streak at k + its rows.
    float bottom = in_place2.y;
    float top = in_span.x;
    float v_top = in_span.z;
    float v_bottom = v_top + (top - bottom) * 0.25;
    vec2 v = vec2(
        max(in_repeat.x + in_streak.y, v_top),
        min(in_repeat.x + in_streak.z, v_bottom));
    if (s.y <= s.x || v.y <= v.x) {
        // Nothing of it here: off screen, no fragments.
        gl_Position = vec4(2.0, 2.0, 2.0, 1.0);
        frag_uv = vec2(0.0);
        frag_color = vec4(0.0);
        frag_fog = vec2(-1.0);
        return;
    }
    uint corner = in_corner.x;
    float at_s = (corner == 1u || corner == 2u) ? s.y : s.x;
    float at_v = corner >= 2u ? v.y : v.x;
    vec2 half_size = vec2(in_place.z, in_place2.x);
    vec2 xz = in_place.xy - half_size + at_s * 2.0 * half_size;
    vec3 pos = vec3(xz.x, top - (at_v - v_top) * 4.0, xz.y);

    gl_Position = pc.view_proj * vec4(pos + pc.offset.xyz, 1.0);
    frag_uv = vec2(in_span.y + at_s, at_v);
    frag_color = vec4(in_color.rgb, in_color.a * pc.ambient.a);
    if (pc.view.z == 1.0) {
        frag_fog = fog_distances(pos + pc.offset.xyz);
    } else if (pc.view.z == 2.0) {
        frag_fog = model_fog_distances(gl_Position);
    } else {
        frag_fog = vec2(-1.0);
    }
}
