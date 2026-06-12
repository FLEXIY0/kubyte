// Чанковый пайплайн: вершина — один u32 (§6), текстуры — texture array,
// фейс-шейдинг по нормали + туман дистанции. Рисует в offscreen ретро-буфер.

struct Camera {
    vp: mat4x4<f32>,
    // xyz — позиция камеры (для тумана), w — дальность тумана.
    pos_fog: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var tex: texture_2d_array<f32>;
@group(0) @binding(2) var samp: sampler;
// Смещение чанка в мире; отдельная группа с динамическим оффсетом —
// один буфер на все чанки кадра.
@group(1) @binding(0) var<uniform> chunk_origin: vec4<f32>;

const NORMALS = array<vec3<f32>, 6>(
    vec3( 1.0, 0.0, 0.0), vec3(-1.0, 0.0, 0.0),
    vec3( 0.0, 1.0, 0.0), vec3( 0.0,-1.0, 0.0),
    vec3( 0.0, 0.0, 1.0), vec3( 0.0, 0.0,-1.0),
);
// Классический фейс-шейдинг: верх ярче, дно темнее, бока различимы.
const SHADE = array<f32, 6>(0.80, 0.80, 1.00, 0.50, 0.65, 0.65);

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) @interpolate(flat) shade: f32,
    @location(3) dist: f32,
};

@vertex
fn vs_main(@location(0) packed: u32) -> VsOut {
    let local = vec3<f32>(
        f32(packed & 31u),
        f32((packed >> 10u) & 255u),
        f32((packed >> 5u) & 31u),
    );
    let n = (packed >> 18u) & 7u;
    let world = chunk_origin.xyz + local;

    // UV — мировые координаты в плоскости грани: текстура тайлится по
    // слитому greedy-кваду сама собой; −y, чтобы рисунок не был вверх ногами.
    var uv: vec2<f32>;
    switch n >> 1u {
        case 0u: { uv = vec2(world.z, -world.y); }
        case 1u: { uv = vec2(world.x, world.z); }
        default: { uv = vec2(world.x, -world.y); }
    }

    var out: VsOut;
    out.clip = camera.vp * vec4(world, 1.0);
    out.uv = uv;
    out.layer = (packed >> 21u) & 255u;
    out.shade = SHADE[n];
    out.dist = distance(world, camera.pos_fog.xyz);
    return out;
}

// Цвет неба = цвет тумана: дальние чанки растворяются, а не обрезаются.
const SKY = vec3<f32>(0.030, 0.040, 0.070);

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let albedo = textureSample(tex, samp, fract(in.uv), in.layer).rgb;
    let fog_end = camera.pos_fog.w;
    let fog = smoothstep(fog_end * 0.7, fog_end, in.dist);
    return vec4(mix(albedo * in.shade, SKY, fog), 1.0);
}
