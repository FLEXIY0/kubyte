// Сцена M0: один куб с процедурной текстурой и ламбертовым светом.
// Рисуется в offscreen-буфер пониженного разрешения (§6, ретро-пайплайн).

struct Camera { mvp: mat4x4<f32> };
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>,
           @location(1) normal: vec3<f32>,
           @location(2) uv: vec2<f32>) -> VsOut {
    return VsOut(camera.mvp * vec4(pos, 1.0), uv, normal);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let albedo = textureSample(tex, samp, in.uv).rgb;
    // Нормаль — в осях модели, так что яркость грани постоянна, как
    // фейс-шейдинг классического Minecraft. Подвал 0.35 — чтобы теневая
    // сторона читалась.
    let light = max(dot(normalize(in.normal), normalize(vec3(0.5, 1.0, 0.6))), 0.0);
    return vec4(albedo * (0.35 + 0.65 * light), 1.0);
}
