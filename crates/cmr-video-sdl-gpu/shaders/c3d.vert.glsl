#version 450

layout(location = 0) in ivec3 position_q16;
layout(location = 1) in vec2 texture_uv_input;
layout(location = 2) in ivec3 normal_q16;

layout(set = 1, binding = 0, std140) uniform C3dTransforms {
    mat4 world;
    mat4 view;
    mat4 projection;
    vec4 clip_parameters;
};

layout(location = 0) out vec2 texture_uv;

void main() {
    vec3 position = vec3(position_q16) / 65536.0;
    vec4 clip = projection * view * world * vec4(position, 1.0);
    // D3DVIEWPORT2's Y clip volume is +/- width/height. SDL_GPU's Vulkan
    // viewport supplies the framebuffer-space inversion, so only normalize the
    // recovered clip volume here.
    clip.y = clip.y / clip_parameters.x;
    gl_Position = clip;
    texture_uv = texture_uv_input;
}
