#version 450
// 共有メモリの画素（4 バイト / 画素、`stride` 画素 / 行）を、並びを揃えて書き写す。
layout(set = 0, binding = 0) uniform samplerBuffer src;
layout(push_constant) uniform Params {
    uint opaque;
    uint bgr;
    uint stride;
    uint offset;
} p;
layout(location = 0) out vec4 color;
void main() {
    ivec2 xy = ivec2(gl_FragCoord.xy);
    vec4 c = texelFetch(src, int(p.offset) + xy.y * int(p.stride) + xy.x);
    color = vec4(p.bgr != 0 ? c.bgr : c.rgb, p.opaque != 0 ? 1.0 : c.a);
}
