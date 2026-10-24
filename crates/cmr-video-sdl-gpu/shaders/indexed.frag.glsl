#version 450

layout(set = 2, binding = 0) uniform usampler2D frame_indices;
layout(set = 2, binding = 1) uniform usampler2D frame_palette;
layout(location = 0) out vec4 output_color;

void main() {
    ivec2 pixel = ivec2(gl_FragCoord.xy);
    uint palette_index = texelFetch(frame_indices, pixel, 0).r;
    uvec3 rgb = texelFetch(frame_palette, ivec2(int(palette_index), 0), 0).rgb;
    output_color = vec4(vec3(rgb) / 255.0, 1.0);
}
