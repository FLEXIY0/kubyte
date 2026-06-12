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
// Инстанс 0 — солнце, 1 — луна (напротив, меньше и бледнее).
const SIZE = array<f32, 2>(18.0, 12.0);

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) inst: u32,
    @location(1) @interpolate(flat) height: f32, // высота светила над горизонтом
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
    let c = CORNERS[v] * SIZE[inst];
    // За туманом, но внутри дальней плоскости (far = 2×fog_end).
    let world = camera.pos_fog.xyz + dir * camera.pos_fog.w * 1.55 + right * c.x + up * c.y;
    return VsOut(camera.vp * vec4(world, 1.0), inst, dir.y);
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
    return vec4(0.72, 0.76, 0.86, 0.9 * vis); // бледная луна
}
