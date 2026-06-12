// Мобы: кубы с модельной матрицей. Камера и атлас — те же, что у чанков;
// освещение — солнце по нормали (вокс-свет на сущностях — упрощение M4).

struct Camera {
    vp: mat4x4<f32>,
    pos_fog: vec4<f32>,
    sun: vec4<f32>,
    sky: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var tex: texture_2d_array<f32>;
@group(0) @binding(2) var samp: sampler;
// Модель части моба; динамический оффсет — один буфер на всех.
struct Part { model: mat4x4<f32>, misc: vec4<f32> }; // misc.x — слой кожи
@group(1) @binding(0) var<uniform> part: Part;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) dist: f32,
};

@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
) -> VsOut {
    let world = part.model * vec4(pos, 1.0);
    var out: VsOut;
    out.clip = camera.vp * world;
    out.uv = uv;
    // Модель — поворот+перенос: нормаль вращается тем же базисом.
    out.normal = (part.model * vec4(normal, 0.0)).xyz;
    out.dist = distance(world.xyz, camera.pos_fog.xyz);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let albedo = textureSample(tex, samp, fract(in.uv), u32(part.misc.x)).rgb;
    let lambert = 0.45 + 0.55 * max(dot(normalize(in.normal), normalize(vec3(0.4, 1.0, 0.5))), 0.0);
    let lit = albedo * lambert * max(camera.sun.a, 0.25) * camera.sun.rgb;
    let fog = smoothstep(camera.pos_fog.w * 0.7, camera.pos_fog.w, in.dist);
    return vec4(mix(lit, camera.sky.rgb, fog), 1.0);
}
