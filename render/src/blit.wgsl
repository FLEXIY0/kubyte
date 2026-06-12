// Ретро-блит (§6): растягивает offscreen-буфер на экран nearest-сэмплером.
// Дизеринг и квантование цвета доедут сюда же в M4 — это их законное место.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Один треугольник, накрывающий весь экран: три вершины из vertex_index,
// без буферов вовсе.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let uv = vec2(f32((i << 1u) & 2u), f32(i & 2u));
    return VsOut(vec4(uv * 2.0 - 1.0, 0.0, 1.0), vec2(uv.x, 1.0 - uv.y));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(src, samp, in.uv);
}
