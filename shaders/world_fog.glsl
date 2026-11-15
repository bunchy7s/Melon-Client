#ifndef MELON_WORLD_FOG_GLSL
#define MELON_WORLD_FOG_GLSL

// The fog distances of a world draw's vertex, shared by `world.vert` and
// `lines.vert`.
#include "fog.glsl"
#include "world_push.glsl"

// Camera-relative position, as the shader sees it in view space, for
// meshes placed by `view_proj` alone: the projection is undone per axis.
// The world's up direction tells height from distance. Clip Y is Vulkan
// (down); `view.xy` already holds 1/m00 and 1/m11 of that projection.
vec2 model_fog_distances(vec4 clip) {
    vec3 v = vec3(clip.x * pc.view.x, clip.y * pc.view.y, clip.w);
    float height = dot(v * vec3(1.0, 1.0, -1.0), g.view_up.xyz);
    float horizontal = sqrt(max(dot(v, v) - height * height, 0.0));
    return vec2(length(v), max(horizontal, abs(height)));
}

#endif
