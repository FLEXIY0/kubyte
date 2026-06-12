// Ретро-блит (§6): растягивает offscreen-буфер на экран nearest-сэмплером,
// квантует цвет с дизерингом и рисует HUD. Раскладка HUD повторяет сетку
// беты: хотбар 182×22 GUI-px по центру у нижней кромки, сердца 9×9 с шагом
// 8 над его левым краем; вся пиксельная графика — своя.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
// HUD: x — здоровье, y — максимум, z — выбранный слот хотбара.
@group(0) @binding(2) var<uniform> hud: vec4<f32>;
@group(0) @binding(3) var atlas: texture_2d_array<f32>;

// Масштаб GUI: 1 GUI-пиксель = 2 экранных (дефолт эпохи).
const GUI: f32 = 2.0;

// Слои texture array для слотов хотбара; −1 — пустой слот.
// ОБЯЗАН совпадать с kb_render::HOTBAR (см. lib.rs).
const SLOT_LAYERS = array<i32, 9>(0, 1, 3, 4, 5, 6, -1, -1, -1);

// Сердце 9×9: битовая маска строк (старший бит — левый столбец).
const HEART = array<u32, 9>(
    0x0D8u, 0x1FCu, 0x1FCu, 0x1FCu, 0x0F8u, 0x070u, 0x020u, 0x000u, 0x000u,
);

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

    // Дизеринг (§6): квантование до 5 бит/канал с матрицей Байера 4×4.
    // Градиенты неба и ночи рассыпаются в ретро-зерно — носитель
    // атмосферы §14. Зерно в offscreen-пикселях — совпадает по масштабу
    // с пикселизацией мира.
    let bayer = array<f32, 16>(
         0.0,  8.0,  2.0, 10.0,
        12.0,  4.0, 14.0,  6.0,
         3.0, 11.0,  1.0,  9.0,
        15.0,  7.0, 13.0,  5.0,
    );
    let src_px = vec2<u32>(in.uv * vec2<f32>(textureDimensions(src)));
    let threshold = (bayer[(src_px.y % 4u) * 4u + src_px.x % 4u] + 0.5) / 16.0;
    color = floor(color * 31.0 + threshold) / 31.0;

    // Экран в GUI-пикселях; размер восстановлен из производных uv.
    let res = 1.0 / vec2(dpdx(in.uv.x), dpdy(in.uv.y));
    let g = in.uv * res / GUI;
    let size = res / GUI;
    let x0 = size.x / 2.0 - 91.0; // левый край хотбара (как в оригинале)

    // Прицел: крестик в центре, инверсия читается на любом фоне.
    let cx = abs(g - size / 2.0);
    if (cx.x < 0.6 && cx.y < 5.0) || (cx.y < 0.6 && cx.x < 5.0) {
        color = 1.0 - color;
    }

    // --- Хотбар: фон 182×22 у нижней кромки -----------------------------
    let hb = vec2(g.x - x0, g.y - (size.y - 22.0));
    if all(hb >= vec2(0.0)) && all(hb < vec2(182.0, 22.0)) {
        // Рамка и полупрозрачная подложка.
        color = mix(color, vec3(0.05), 0.78);
        if hb.x < 1.0 || hb.x >= 181.0 || hb.y < 1.0 || hb.y >= 21.0 {
            color = vec3(0.22);
        }
        // Иконка блока: слот 20 GUI-px, икона 16×16 внутри.
        let slot = i32(floor((hb.x - 1.0) / 20.0));
        let local = vec2(hb.x - 1.0 - f32(slot) * 20.0, hb.y) - vec2(2.0, 3.0);
        if slot >= 0 && slot < 9 && all(local >= vec2(0.0)) && all(local < vec2(16.0)) {
            let layer = SLOT_LAYERS[slot];
            if layer >= 0 {
                // Иконка — вариант 0 (атлас хранит по 16 вариантов на материал).
                let texel =
                    textureSampleLevel(atlas, samp, (local + 0.5) / 16.0, u32(layer) * 16u, 0.0);
                color = mix(color, texel.rgb, 1.0);
            }
        }
    }
    // Подсветка выбранного слота: рамка 24×24 вокруг ячейки (как в бете).
    let sel = hud.z;
    let sb = vec2(g.x - (x0 - 1.0 + sel * 20.0), g.y - (size.y - 23.0));
    if all(sb >= vec2(0.0)) && all(sb < vec2(24.0, 23.0)) {
        let edge = sb.x < 1.0 || sb.x >= 23.0 || sb.y < 1.0 || sb.y >= 22.0;
        if edge {
            color = vec3(0.92);
        }
    }

    // --- Сердца: над левым краем хотбара, y = низ − 32 -------------------
    let hy = g.y - (size.y - 32.0);
    if hy >= 0.0 && hy < 9.0 {
        let hi = floor((g.x - x0) / 8.0); // шаг 8 — сердца внахлёст
        let hx = g.x - x0 - hi * 8.0;
        if hi >= 0.0 && hi < 10.0 && hx >= 0.0 && hx < 9.0 {
            let bit = (HEART[u32(hy)] >> (8u - u32(hx))) & 1u;
            if bit == 1u {
                let full = hi * 2.0 + 2.0 <= hud.x;
                let half = !full && hi * 2.0 + 1.0 <= hud.x && hx < 4.0;
                if full || half {
                    color = vec3(0.80, 0.11, 0.13);
                } else {
                    color = vec3(0.15, 0.04, 0.05); // пустая ячейка
                }
            }
        }
    }

    return vec4(color, 1.0);
}
