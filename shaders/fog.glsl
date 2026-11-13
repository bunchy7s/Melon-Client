#ifndef MELON_FOG_GLSL
#define MELON_FOG_GLSL

// Vanilla `include/fog.glsl`, over the frame's `Globals`.
#include "globals.glsl"

float linear_fog_value(float distance, float fog_start, float fog_end) {
    if (distance <= fog_start) {
        return 0.0;
    } else if (distance >= fog_end) {
        return 1.0;
    }
    return (distance - fog_start) / (fog_end - fog_start);
}

// `fog_spherical_distance` and `fog_cylindrical_distance` of a position
// relative to the camera: x the first, y the second.
vec2 fog_distances(vec3 pos) {
    return vec2(length(pos), max(length(pos.xz), abs(pos.y)));
}

// `apply_fog`: the thicker of the environmental fog (a sphere) and the
// render distance's (a cylinder). A negative distance is "no fog here".
vec4 apply_fog(vec4 color, vec2 distances) {
    if (distances.x < 0.0) {
        return color;
    }
    float value = max(
        linear_fog_value(distances.x, g.fog_env.x, g.fog_env.y),
        linear_fog_value(distances.y, g.fog_env.z, g.fog_env.w));
    return vec4(mix(color.rgb, g.fog_color.rgb, value * g.fog_color.a), color.a);
}

#endif
