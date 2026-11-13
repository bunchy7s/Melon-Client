#version 450

// The sky's flat colour (vanilla `core/sky.vsh`): a disc above the camera
// whose rim has faded into the fog. The vertex keeps its place relative to
// the camera for the fog's distances, whatever the model moved it by.

#include "fog.glsl"
#include "world_push.glsl"

layout(location = 0) in vec3 in_pos;

layout(location = 0) out vec2 frag_fog;

void main() {
    gl_Position = pc.view_proj * vec4(in_pos, 1.0);
    frag_fog = fog_distances(in_pos);
}
