// Блочные облака «в духе эпохи»: плоский слой на высоте неба, рисунок —
// процедурные прямоугольные клетки из integer-хеша. Геометрия — один квад
// вокруг камеры; вся форма считается в пикселе. Медленно плывут по ветру.

struct Camera {
    vp: mat4x4<f32>,
    pos_fog: vec4<f32>,
    sun: vec4<f32>,
    sky: vec4<f32>, // rgb — небо, w — игровое время (ветер)
};
@group(0) @binding(0) var<uniform> camera: Camera;

const CLOUD_Y: f32 = 108.0;
/// Клетка облачного растра, блоков. Крупная — облака читаются большими
/// блоками, а не рваной мелочью.
const CELL: f32 = 12.0;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
};

// Квад ±R вокруг камеры из 6 вершин, без буферов (cull выключен —
// порядок обхода не важен).
const CORNERS = array<vec2<f32>, 6>(
    vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(-1.0, 1.0),
    vec2(-1.0, 1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
);

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let r = camera.pos_fog.w * 1.6;
    let corner = CORNERS[i];
    let world = vec3(
        camera.pos_fog.x + corner.x * r,
        CLOUD_Y,
        camera.pos_fog.z + corner.y * r,
    );
    return VsOut(camera.vp * vec4(world, 1.0), world);
}

fn hash2(p: vec2<i32>) -> f32 {
    var h = u32(p.x) * 0x8DA6B343u ^ u32(p.y) * 0xD8163841u;
    h = (h ^ (h >> 13u)) * 0x9E3779B1u;
    return f32((h >> 16u) & 0xFFFFu) / 65535.0;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Ветер: слой плывёт целиком, клетки не мерцают (сдвиг до floor).
    let p = in.world.xz + vec2(camera.sky.w * 1.5, 0.0);
    // Крупные поля задают большие массивы облаков; внутри поля лишь
    // редкие разрывы — спокойный блочный силуэт, не рваная мелочь.
    let field = hash2(vec2<i32>(floor(p / (CELL * 4.0))));
    let cell = hash2(vec2<i32>(floor(p / CELL)));
    if field > 0.5 || cell > 0.82 {
        discard;
    }
    // Цвет — белый в тоне солнца; вдали растворяются в небе.
    let dist = distance(in.world.xz, camera.pos_fog.xz);
    let fade = smoothstep(camera.pos_fog.w * 1.5, camera.pos_fog.w * 0.9, dist);
    let body = camera.sun.rgb * (0.55 + 0.45 * camera.sun.a);
    return vec4(mix(camera.sky.rgb, body, fade), 0.82 * fade);
}
