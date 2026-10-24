#version 450

layout(set = 2, binding = 0) uniform usampler2D material_indices;
layout(set = 2, binding = 1) uniform usampler2D mapped_palette;

layout(location = 0) in vec2 texture_uv;
layout(location = 0) out vec4 output_color;

void main() {
    uint palette_index = texture(material_indices, texture_uv).r;
    uvec3 rgb = texelFetch(mapped_palette, ivec2(int(palette_index), 0), 0).rgb;
    output_color = vec4(vec3(rgb) / 255.0, 1.0);
}
