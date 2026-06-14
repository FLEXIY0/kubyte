// Ретро-блит (§6): растягивает offscreen-буфер на экран nearest-сэмплером,
// квантует цвет с дизерингом и рисует HUD. Раскладка HUD повторяет сетку
// беты: хотбар 182×22 GUI-px по центру у нижней кромки, сердца 9×9 с шагом
// 8 над его левым краем; вся пиксельная графика — своя.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
// Стиль HUD из kb_materials::HUD + динамика (hp/слот). Правится в редакторе.
struct Hud {
    hp: vec4<f32>,             // x — здоровье, y — максимум, z — выбранный слот
    rows: array<vec4<u32>, 9>, // .x — биты сердца, .y — биты блика (9×9)
    full: vec4<f32>,
    empty: vec4<f32>,
    outline: vec4<f32>,
    highlight: vec4<f32>,
    bar_bg: vec4<f32>,         // rgb + a (сила наложения подложки)
    bar_border: vec4<f32>,
    bar_sel: vec4<f32>,
    menu: vec4<f32>,           // x — открыто, y — пункт, z — масштаб, w — дизеринг
    slots: array<vec4<u32>, 9>,// .x — id блока (255 пусто), .y — количество
    armor_full: vec4<f32>,
    armor_empty: vec4<f32>,
    misc: vec4<f32>,           // x — броня 0..20
};
@group(0) @binding(2) var<uniform> hud: Hud;
@group(0) @binding(3) var atlas: texture_2d_array<f32>;
@group(0) @binding(4) var ui: texture_2d<f32>;      // ярлыки меню
@group(0) @binding(5) var icons: texture_2d_array<f32>; // изо-иконки блоков

// Масштаб GUI берётся из hud.hp.w (настройка размера интерфейса).

// Внутри ли (x,y) силуэта сердца / блика (битмаски из юниформа).
fn heart_at(x: i32, y: i32) -> bool {
    if x < 0 || x > 8 || y < 0 || y > 8 {
        return false;
    }
    return ((hud.rows[y].x >> u32(8 - x)) & 1u) == 1u;
}
fn glint_at(x: i32, y: i32) -> bool {
    if x < 0 || x > 8 || y < 0 || y > 8 {
        return false;
    }
    return ((hud.rows[y].y >> u32(8 - x)) & 1u) == 1u;
}
fn armor_at(x: i32, y: i32) -> bool {
    if x < 0 || x > 8 || y < 0 || y > 8 {
        return false;
    }
    return ((hud.rows[y].z >> u32(8 - x)) & 1u) == 1u;
}

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
    if hud.menu.w > 0.5 { // дизеринг — настройка меню
        let src_px = vec2<u32>(in.uv * vec2<f32>(textureDimensions(src)));
        let threshold = (bayer[(src_px.y % 4u) * 4u + src_px.x % 4u] + 0.5) / 16.0;
        color = floor(color * 31.0 + threshold) / 31.0;
    }

    // Экран в GUI-пикселях; размер восстановлен из производных uv.
    let res = 1.0 / vec2(dpdx(in.uv.x), dpdy(in.uv.y));
    let gui = hud.hp.w; // размер интерфейса (настройка)
    let g = in.uv * res / gui;
    let size = res / gui;
    let x0 = size.x / 2.0 - 91.0; // левый край хотбара (как в оригинале)

    // Прицел: крестик в центре, инверсия читается на любом фоне.
    let cx = abs(g - size / 2.0);
    if (cx.x < 0.6 && cx.y < 5.0) || (cx.y < 0.6 && cx.x < 5.0) {
        color = 1.0 - color;
    }

    // --- Хотбар: фон 182×22 у нижней кромки -----------------------------
    let hb = vec2(g.x - x0, g.y - (size.y - 22.0));
    if all(hb >= vec2(0.0)) && all(hb < vec2(182.0, 22.0)) {
        // Рамка и полупрозрачная подложка (цвета из стиля HUD).
        color = mix(color, hud.bar_bg.rgb, hud.bar_bg.a);
        if hb.x < 1.0 || hb.x >= 181.0 || hb.y < 1.0 || hb.y >= 21.0 {
            color = hud.bar_border.rgb;
        }
        // Изо-иконка блока из слота: слот 20 GUI-px, икона 16×16 внутри.
        let slot = i32(floor((hb.x - 1.0) / 20.0));
        let local = vec2(hb.x - 1.0 - f32(slot) * 20.0, hb.y) - vec2(2.0, 3.0);
        if slot >= 0 && slot < 9 && all(local >= vec2(0.0)) && all(local < vec2(16.0)) {
            let id = hud.slots[slot].x; // id блока (255 = пусто)
            if id != 255u && id != 0u {
                let texel = textureSampleLevel(icons, samp, (local + 0.5) / 16.0, id, 0.0);
                color = mix(color, texel.rgb, texel.a);
            }
        }
    }
    // Подсветка выбранного слота: рамка 24×24 вокруг ячейки (как в бете).
    let sel = hud.hp.z;
    let sb = vec2(g.x - (x0 - 1.0 + sel * 20.0), g.y - (size.y - 23.0));
    if all(sb >= vec2(0.0)) && all(sb < vec2(24.0, 23.0)) {
        let edge = sb.x < 1.0 || sb.x >= 23.0 || sb.y < 1.0 || sb.y >= 22.0;
        if edge {
            color = hud.bar_sel.rgb;
        }
    }

    // --- Сердца: над левым краем хотбара ---------------------------------
    // Шаг 10; силуэт сдвинут на +1 внутрь ячейки (body = heart_at(hx-1)),
    // поэтому обводка слева (hx=0) и справа (hx=8) целиком влезает в ячейку.
    let hy = g.y - (size.y - 33.0);
    let hi = floor((g.x - x0) / 10.0);
    let hx = i32(floor(g.x - x0 - hi * 10.0));
    let hyi = i32(floor(hy));
    if hi >= 0.0 && hi < 10.0 && hyi >= -1 && hyi <= 9 && hx >= 0 && hx <= 9 {
        let idx = i32(hi);
        if heart_at(hx - 1, hyi) {
            let full = f32(idx) * 2.0 + 2.0 <= hud.hp.x;
            let half = !full && f32(idx) * 2.0 + 1.0 <= hud.hp.x && hx <= 4;
            if full || half {
                color = hud.full.rgb;
                // Блик из битмаски стиля (поверх заливки).
                if glint_at(hx - 1, hyi) {
                    color = hud.highlight.rgb;
                }
            } else {
                color = hud.empty.rgb; // пустая ячейка
            }
        } else if heart_at(hx - 2, hyi) || heart_at(hx, hyi)
            || heart_at(hx - 1, hyi - 1) || heart_at(hx - 1, hyi + 1) {
            color = hud.outline.rgb; // обводка по всему контуру
        }
    }

    // --- Броня: справа над хотбаром (зеркально сердцам, как в бете) -------
    let ax0 = x0 + 182.0; // правый край хотбара
    let ai = floor((ax0 - g.x) / 10.0); // отсчёт справа налево
    let axl = (ax0 - g.x) - ai * 10.0;
    let ax = 8 - i32(floor(axl)); // зеркалим X
    if ai >= 0.0 && ai < 10.0 && hyi >= -1 && hyi <= 9 && ax >= 0 && ax <= 9 {
        let idx = i32(ai);
        if armor_at(ax - 1, hyi) {
            // Полный щиток, если этот ранг покрыт бронёй (2 ед. = 1 щиток).
            let full = f32(idx) * 2.0 + 2.0 <= hud.misc.x;
            color = select(hud.armor_empty.rgb, hud.armor_full.rgb, full);
        } else if armor_at(ax - 2, hyi) || armor_at(ax, hyi)
            || armor_at(ax - 1, hyi - 1) || armor_at(ax - 1, hyi + 1) {
            color = hud.outline.rgb;
        }
    }

    // --- Меню паузы (§6: настройки) --------------------------------------
    if hud.menu.x > 0.5 {
        color *= 0.35; // затемняем мир
        let pw = 160.0;
        let ph = 106.0;
        let m = vec2(g.x - (size.x / 2.0 - pw / 2.0), g.y - (size.y / 2.0 - ph / 2.0));
        if all(m >= vec2(0.0)) && all(m < vec2(pw, ph)) {
            color = mix(color, vec3(0.07, 0.08, 0.10), 0.88);
            if m.x < 1.0 || m.x >= pw - 1.0 || m.y < 1.0 || m.y >= ph - 1.0 {
                color = vec3(0.55);
            }
            // Пять строк по 18 px: GUI / PIXELS / DITHER / EDITOR / RESUME.
            let row = floor((m.y - 8.0) / 18.0);
            let inrow = m.y - 8.0 - row * 18.0;
            if row >= 0.0 && row < 5.0 && inrow >= 0.0 && inrow < 16.0 {
                if row == hud.menu.y {
                    color = mix(color, vec3(0.20, 0.28, 0.40), 0.6); // подсветка
                }
                let lx = m.x - 96.0; // колонка управления справа
                let pip = floor(lx / 12.0);
                let inpip = lx - pip * 12.0;
                if row == 0.0 {
                    // Размер интерфейса: 4 пипса (2..5), залит = gui-1.
                    if pip >= 0.0 && pip < 4.0 && inrow >= 4.0 && inrow < 12.0
                        && inpip >= 0.0 && inpip < 8.0 {
                        color = select(vec3(0.25), vec3(0.85), pip < gui - 1.0);
                    }
                } else if row == 1.0 {
                    // Масштаб пикселей: 4 пипса, залитые = текущий.
                    if pip >= 0.0 && pip < 4.0 && inrow >= 4.0 && inrow < 12.0
                        && inpip >= 0.0 && inpip < 8.0 {
                        color = select(vec3(0.25), vec3(0.85), pip < hud.menu.z);
                    }
                } else if row == 2.0 {
                    // Дизеринг: квадрат-индикатор (зелёный вкл / серый выкл).
                    if lx >= 0.0 && lx < 12.0 && inrow >= 3.0 && inrow < 13.0 {
                        color = select(vec3(0.30), vec3(0.35, 0.80, 0.40), hud.menu.w > 0.5);
                    }
                }
            }
            // Поверх — ярлыки из текстуры (белый текст слева).
            let lab = textureSampleLevel(ui, samp, m / vec2(pw, ph), 0.0);
            color = mix(color, lab.rgb, lab.a);
        }
    }

    return vec4(color, 1.0);
}
