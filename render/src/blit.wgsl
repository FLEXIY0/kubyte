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
    var color = textureSample(src, samp, in.uv).rgb;
    // Прицел-крестик. Размер экрана восстанавливается из производных uv —
    // ни юниформа, ни знания разрешения не нужно.
    let px = abs(in.uv - 0.5) / vec2(dpdx(in.uv.x), dpdy(in.uv.y));
    if (px.x < 1.0 && px.y < 8.0) || (px.y < 1.0 && px.x < 8.0) {
        color = 1.0 - color; // инверсия читается на любом фоне
    }
    return vec4(color, 1.0);
}
