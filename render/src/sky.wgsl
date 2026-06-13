// Квадратное солнце и луна — биллборды по направлению светила.
// Рисуются первыми, без записи глубины: мир и облака закрашивают их сами.

struct Camera {
    vp: mat4x4<f32>,
    pos_fog: vec4<f32>,
    sun: vec4<f32>,
    sky: vec4<f32>, // w — игровое время
};
@group(0) @binding(0) var<uniform> camera: Camera;

// Длительность суток в секундах — синхронизировано с DAY_SECONDS (lib.rs).
const DAY_SECONDS: f32 = 600.0;
const TAU: f32 = 6.2831853;

const CORNERS = array<vec2<f32>, 6>(
    vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(-1.0, 1.0),
    vec2(-1.0, 1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
);
// Инстанс 0 — солнце, 1 — луна. Оба крупные; луна чуть меньше солнца.
const SIZE = array<f32, 2>(34.0, 30.0);

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>, // угол квада в −1..1 (для кратеров луны)
    @location(1) @interpolate(flat) inst: u32,
    @location(2) @interpolate(flat) height: f32, // высота светила над горизонтом
};

@vertex
fn vs_main(@builtin(vertex_index) v: u32, @builtin(instance_index) inst: u32) -> VsOut {
    let phase = camera.sky.w / DAY_SECONDS * TAU;
    var dir = normalize(vec3(cos(phase), sin(phase), 0.18));
    if inst == 1u {
        dir = -dir;
    }
    let right = normalize(cross(vec3(0.0, 1.0, 0.0), dir));
    let up = cross(dir, right);
    let corner = CORNERS[v];
    let c = corner * SIZE[inst];
    // За туманом, но внутри дальней плоскости (far = 2×fog_end).
    let world = camera.pos_fog.xyz + dir * camera.pos_fog.w * 1.55 + right * c.x + up * c.y;
    return VsOut(camera.vp * vec4(world, 1.0), corner, inst, dir.y);
}

// Кратер: квадратное тёмное пятно (в духе блочной эстетики) — чтобы луна
// не путалась с солнцем. Сторона 2r вокруг центра c.
fn crater(uv: vec2<f32>, c: vec2<f32>, r: f32) -> f32 {
    let d = max(abs(uv.x - c.x), abs(uv.y - c.y));
    return select(1.0, 0.74, d < r);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Тонем у горизонта, чтобы светило не светило из-под земли.
    let vis = smoothstep(-0.06, 0.08, in.height);
    if in.inst == 0u {
        // Солнце греется цветом зари через camera.sun.
        let body = mix(vec3(1.0, 0.96, 0.82), camera.sun.rgb, 0.45);
        return vec4(body, vis);
    }
    // Луна: бледный диск с кратерами — чтобы не путать с солнцем.
    var shade = 1.0;
    shade *= crater(in.uv, vec2(-0.35, 0.30), 0.30);
    shade *= crater(in.uv, vec2(0.40, -0.10), 0.26);
    shade *= crater(in.uv, vec2(0.05, 0.55), 0.20);
    shade *= crater(in.uv, vec2(-0.10, -0.45), 0.22);
    return vec4(vec3(0.72, 0.76, 0.86) * shade, 0.9 * vis);
}
