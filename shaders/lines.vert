#version 450

// Lines and points a fixed number of pixels wide, however far away
// (vanilla `core/rendertype_lines.vsh` and `core/debug_point.vsh`): the
// block outline and the debug gizmos. Every line is one quad of four
// `WorldVertex`es, two at each end; the vertex shader moves each one
// sideways on screen by half the width. See `WorldVertex::line`.

#include "fog.glsl"
#include "world_push.glsl"
#include "world_fog.glsl"

// The end of the line (or the point) this vertex belongs to.
layout(location = 0) in vec3 in_pos;
// The line's direction, octahedron-encoded.
layout(location = 1) in vec2 in_dir;
layout(location = 2) in vec4 in_color;
// The width (a point's size) in sixteenths of a pixel, as a little-endian
// `u16`.
layout(location = 3) in uvec2 in_width;
// 0 and 1: which side of the line; 2 to 5: which corner of a point.
layout(location = 4) in uint in_corner;

layout(location = 0) out vec2 frag_uv;
layout(location = 1) out vec4 frag_color;
layout(location = 2) out vec2 frag_light;
layout(location = 3) out vec2 frag_fog;

// Vanilla's `VIEW_SHRINK`: everything a 256th closer to the eye, so a line
// on a face it outlines lies in front of it.
const float VIEW_SHRINK = 1.0 - (1.0 / 256.0);

vec3 octahedron_decode(vec2 e) {
    vec3 v = vec3(e, 1.0 - abs(e.x) - abs(e.y));
    if (v.z < 0.0) {
        vec2 sign_not_zero = vec2(v.x >= 0.0 ? 1.0 : -1.0, v.y >= 0.0 ? 1.0 : -1.0);
        v.xy = (1.0 - abs(v.yx)) * sign_not_zero;
    }
    return normalize(v);
}

void main() {
    // `pc.offset.xyz` is the eye in the mesh's space for these draws: the
    // point vanilla shrinks the view space toward.
    vec3 eye = pc.offset.xyz;
    vec4 clip = pc.view_proj * vec4(eye + (in_pos - eye) * VIEW_SHRINK, 1.0);
    float width = float(in_width.x | (in_width.y << 8)) / 16.0;
    // Half the width in NDC: the NDC's 2 units span the screen's pixels.
    vec2 half_width = vec2(width) / g.screen.xy;
    vec2 offset;
    if (in_corner >= 2u) {
        const vec2 corners[4] = vec2[](
            vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
        offset = corners[in_corner - 2u] * half_width;
    } else {
        // The line's direction on screen where this end is: how its NDC
        // position changes along `dir` (the derivative of xy / w), in
        // pixels. The same at both ends, so their sides match.
        vec4 along = pc.view_proj * vec4(octahedron_decode(in_dir), 0.0);
        vec2 screen = (along.xy * clip.w - clip.xy * along.w) * g.screen.xy;
        vec2 dir = dot(screen, screen) > 1.0e-20 ? normalize(screen) : vec2(1.0, 0.0);
        float side = in_corner == 0u ? 1.0 : -1.0;
        offset = vec2(-dir.y, dir.x) * half_width * side;
    }
    // TODO(parity): an end behind the eye (w < 0) turns the offset around,
    // so a line crossing the near plane is drawn twisted. Vanilla's shader
    // does the same for the block outline (its gizmos are cut on the CPU,
    // as `GizmoMesh` does); cutting the outline's lines at the near plane
    // as well would be the fix, if standing in a block shows it.
    gl_Position = vec4(clip.xy + offset * clip.w, clip.zw);
    frag_uv = vec2(0.5);
    frag_light = vec2(-1.0);
    frag_color = vec4(in_color.rgb, in_color.a * pc.ambient.a);
    frag_fog = pc.view.z == 2.0
        ? model_fog_distances(pc.view_proj * vec4(in_pos, 1.0))
        : vec2(-1.0);
}
