#version 450
// 取り込んだ DMA-BUF の画像を、そのままの値で書き写す（形式の並びはイメージの形式で解釈される）。
layout(set = 0, binding = 0) uniform sampler2D src;
layout(push_constant) uniform Params {
    uint opaque;
    uint bgr;
    uint stride;
    uint offset;
} p;
layout(location = 0) out vec4 color;
void main() {
    vec4 c = texelFetch(src, ivec2(gl_FragCoord.xy), 0);
    color = vec4(p.bgr != 0 ? c.bgr : c.rgb, p.opaque != 0 ? 1.0 : c.a);
}
