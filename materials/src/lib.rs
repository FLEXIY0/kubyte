//! kb-materials: текстура = функция `(дескриптор, сид) → 16×16 RGBA` (§5).
//!
//! Никаких ассетов и никаких float: вся математика — целочисленная,
//! результат бит-в-бит одинаков на всех платформах. Крейт `no_std`:
//! он умеет только считать пиксели.

#![no_std]
#![forbid(unsafe_code)]

use kb_core::hash2;

/// Сторона текстуры в пикселях. Константа, а не параметр: единый размер —
/// это то, что позволяет складывать все материалы в один texture array (§5).
pub const TEX_SIZE: usize = 16;
/// Размер одной текстуры в байтах (RGBA8).
pub const TEX_BYTES: usize = TEX_SIZE * TEX_SIZE * 4;

/// Вариантов на материал (§5, базовый режим): каждый блок в мире — та же
/// суть, но своё расположение пикселей. Вариант нигде не хранится: рендер
/// выводит его из мировой позиции блока (мир-как-функция, §1).
pub const VARIANTS: usize = 16;

/// Сид варианта текстуры: материал × вариант → свой шумовой мир.
/// Фиксированная формула — texture array одинаков у всех игроков (§5).
pub const fn variant_seed(layer: usize, variant: usize) -> u64 {
    kb_core::splitmix64(((layer as u64) << 8) | variant as u64)
}

/// Значения ячейки паттерна-рисунка (§5):
/// 0..=3 — фиксированный индекс палитры (структура из референса/рисовки),
/// AUTO — процедурный шум (поведение по умолчанию),
/// TRANSPARENT — прозрачный пиксель (дырки листвы, стекло).
pub const AUTO: u8 = 4;
pub const TRANSPARENT: u8 = 5;

/// 16×16 индексов палитры (§5: «битмап-паттерн как массив индексов»).
pub type Pattern = [u8; TEX_SIZE * TEX_SIZE];

/// Паттерн «всё процедурно» — дефолт для блоков без рисунка.
/// Блок с ним печётся бит-в-бит как до появления паттернов.
pub const AUTO_PATTERN: Pattern = [AUTO; TEX_SIZE * TEX_SIZE];

/// Дескриптор материала. Это *данные*, а не код (§1): новый материал —
/// новая константа в таблице, генератор один на всех.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptor {
    /// Палитра от тёмного к светлому, RGB. Шум выбирает индекс.
    pub palette: [[u8; 3]; 4],
    /// log2 размера ячейки шума в пикселях (1 → ячейка 2 px, 2 → 4 px).
    pub cell_log2: u8,
    /// Амплитуда вариации яркости, в 1/256 долях. Инвариант грамматики
    /// «блок читается глазом» (§5): не более 38 (≈ ±15%).
    pub variation: u8,
    /// Разброс тона между вариантами блока (§5): насколько сильно
    /// 16 вариантов отличаются общей яркостью. 0 — все одинаковы,
    /// 16 — «контролируемый хаос» по умолчанию, выше — разнобой заметнее.
    pub spread: u8,
    /// Оверлей поверх базового шума (§5).
    pub overlay: Overlay,
    /// Паттерн-рисунок 16×16 (§5): где AUTO — генератор кладёт шум, где
    /// 0..3 — фиксированный цвет (структура), где TRANSPARENT — дырка.
    /// Так блок становится «ближе к исходнику», а не чистым шумом.
    pub pattern: Pattern,
    /// Разброс структуры МЕЖДУ вариантами (§5): сколько обменов соседних
    /// ячеек делает каждый вариант своим сидом. 0 — все 16 вариантов
    /// со структурой один-в-один (фиксированный рисунок), выше — блоки
    /// в мире всё реже похожи друг на друга. Цвета и дырки сохраняются.
    pub pattern_jitter: u8,
}

/// Оверлеи — те самые «полосы и крапинки» из §5: маленький закрытый
/// словарь приёмов, из которого собираются все материалы.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    None,
    /// Рваная полоса сверху (бок травы): глубина кромки пляшет по столбцам.
    TopBand { palette: [[u8; 3]; 2], min_depth: u8, max_depth: u8 },
    /// Кластеры-крапинки 2×2 (камешки в земле; этим же приёмом — руды).
    Speckle { color: [u8; 3], chance: u8 },
    /// Вертикальные полосы с дрожанием яркости — кора, доски.
    Stripes { color: [u8; 3], period: u8 },
}

// Палитры — «настроение альфы»: трава ярче и сочнее, земля теплее и
// рыжее, камень светлый и ровный. Сами пиксели — наш генератор.

// Палитры и уровни зерна выведены из агрегатной статистики стиля эпохи
// (квартильные средние цвета, контраст соседей, доля тёмных вкраплений) —
// это числа о стиле, а не изображения; рисунок пикселей рождает наш шум.

pub const STONE: Descriptor = Descriptor {
    // Узкая серая гамма, очень ровное зерно, вкраплений нет.
    palette: [[113, 113, 113], [119, 119, 119], [127, 127, 127], [142, 142, 142]],
    cell_log2: 1,
    variation: 16,
    spread: 14,
    overlay: Overlay::None,
    pattern: AUTO_PATTERN,
    pattern_jitter: 0,
};

pub const DIRT: Descriptor = Descriptor {
    // Тёплая охра с широким разбегом яркости и заметными тёмными гнёздами.
    palette: [[104, 72, 49], [121, 85, 58], [142, 103, 72], [169, 124, 88]],
    cell_log2: 0,
    variation: 38,
    spread: 18,
    overlay: Overlay::Speckle { color: [88, 58, 40], chance: 35 },
    pattern: AUTO_PATTERN,
    pattern_jitter: 0,
};

pub const GRASS_TOP: Descriptor = Descriptor {
    // Жёлто-зелёная, светлый верхний квартиль почти пастельный;
    // тёмных вкраплений практически нет.
    palette: [[94, 158, 52], [106, 169, 64], [120, 180, 76], [149, 198, 102]],
    cell_log2: 0,
    variation: 32,
    spread: 16,
    overlay: Overlay::Speckle { color: [86, 146, 48], chance: 12 },
    pattern: AUTO_PATTERN,
    pattern_jitter: 0,
};

pub const GRASS_SIDE: Descriptor = Descriptor {
    // Земля с рваной зелёной кромкой сверху: травяной слой «свисает»
    // на бок блока на 2–4 пикселя.
    palette: DIRT.palette,
    cell_log2: 0,
    variation: 36,
    spread: 16,
    overlay: Overlay::TopBand {
        palette: [[100, 162, 58], [122, 181, 78]],
        min_depth: 2,
        max_depth: 4,
    },
    pattern: AUTO_PATTERN,
    pattern_jitter: 0,
};

pub const WOOD: Descriptor = Descriptor {
    // Кора: сильный вертикальный контраст, треть площади — тёмные борозды.
    palette: [[61, 48, 29], [92, 74, 45], [110, 88, 54], [145, 115, 70]],
    cell_log2: 2,
    variation: 28,
    spread: 12,
    overlay: Overlay::Stripes { color: [52, 40, 25], period: 3 },
    pattern: AUTO_PATTERN,
    pattern_jitter: 0,
};

// Листва: авторский паттерн из редактора материалов (§12). Пятёрки —
// прозрачные пиксели (дырки кроны), pattern_jitter раздаёт 16 непохожих
// раскладок по блокам мира.
pub const LEAVES: Descriptor = Descriptor {
    palette: [[26, 122, 26], [52, 177, 36], [73, 218, 45], [85, 245, 53]],
    cell_log2: 0,
    variation: 38,
    spread: 36,
    overlay: Overlay::None,
    pattern: [
        2, 5, 3, 2, 0, 5, 5, 5, 1, 5, 5, 5, 5, 0, 5, 3, //
        1, 5, 3, 2, 5, 0, 5, 5, 0, 0, 5, 3, 3, 0, 5, 0, //
        5, 5, 2, 5, 0, 3, 2, 5, 2, 3, 0, 3, 2, 5, 3, 3, //
        5, 5, 5, 5, 5, 3, 1, 5, 3, 2, 0, 3, 1, 5, 3, 2, //
        5, 2, 2, 5, 5, 2, 0, 5, 2, 2, 5, 5, 2, 5, 2, 1, //
        5, 3, 1, 5, 0, 2, 0, 5, 0, 2, 5, 0, 5, 0, 5, 1, //
        0, 2, 5, 0, 0, 5, 2, 3, 5, 5, 0, 5, 5, 0, 0, 0, //
        2, 3, 5, 3, 3, 0, 3, 2, 5, 3, 3, 0, 5, 0, 5, 0, //
        3, 2, 5, 2, 2, 0, 2, 2, 5, 2, 2, 5, 5, 3, 2, 0, //
        2, 0, 5, 0, 2, 5, 2, 5, 0, 5, 2, 5, 0, 3, 2, 5, //
        0, 5, 5, 5, 5, 0, 0, 5, 3, 3, 5, 5, 0, 2, 5, 5, //
        3, 5, 3, 3, 5, 5, 5, 5, 3, 2, 5, 3, 1, 5, 0, 3, //
        1, 5, 3, 2, 5, 3, 3, 5, 5, 1, 5, 2, 2, 5, 0, 2, //
        2, 5, 0, 2, 5, 2, 1, 5, 5, 0, 5, 2, 0, 3, 3, 5, //
        0, 5, 5, 0, 0, 1, 5, 5, 5, 0, 0, 2, 5, 3, 2, 0, //
        2, 5, 5, 5, 5, 2, 5, 0, 3, 3, 0, 5, 5, 2, 0, 3, //
    ],
    pattern_jitter: 26,
};

pub const LAMP: Descriptor = Descriptor {
    // Янтарь — тёплый полюс палитры мира (§6: контраст температур).
    palette: [[196, 134, 56], [216, 156, 66], [232, 176, 80], [244, 196, 100]],
    cell_log2: 1,
    variation: 20,
    spread: 10,
    overlay: Overlay::Speckle { color: [255, 222, 150], chance: 70 },
    pattern: AUTO_PATTERN,
    pattern_jitter: 0,
};

/// «Кожа» мобов — те же дескрипторы, что у блоков (§1: один генератор
/// на всё). Дизайн — свой, «в духе эпохи» (§7), без копирования Mojang.
pub const PIG: Descriptor = Descriptor {
    palette: [[196, 124, 124], [212, 140, 138], [226, 154, 150], [238, 170, 164]],
    cell_log2: 2,
    variation: 16,
    spread: 12,
    overlay: Overlay::Speckle { color: [178, 108, 110], chance: 24 },
    pattern: AUTO_PATTERN,
    pattern_jitter: 0,
};

pub const ZOMBIE: Descriptor = Descriptor {
    // Болотная гниль: холодная и тусклая — силуэт мрачнее любого блока.
    palette: [[58, 84, 58], [66, 96, 64], [76, 108, 72], [86, 118, 80]],
    cell_log2: 1,
    variation: 30,
    spread: 14,
    overlay: Overlay::Speckle { color: [44, 62, 46], chance: 60 },
    pattern: AUTO_PATTERN,
    pattern_jitter: 0,
};

/// Таблица текстур (§1: данные вместо кода). Индекс = слой texture array.
/// Порядок зафиксирован: на него ссылается FACE_LAYERS.
pub const TEXTURES: &[Descriptor] =
    &[STONE, DIRT, GRASS_TOP, GRASS_SIDE, WOOD, LEAVES, LAMP, PIG, ZOMBIE];

/// Слои «кожи» мобов в texture array.
pub const PIG_LAYER: u32 = 7;
pub const ZOMBIE_LAYER: u32 = 8;

/// Слой текстуры для каждой грани блока: [материал][грань],
/// грани в порядке нормалей рендера: +X −X +Y −Y +Z −Z.
/// Air (0) текстуры не имеет — строка-заглушка, граней у него не бывает.
pub const FACE_LAYERS: [[u8; 6]; 7] = [
    [0; 6],             // Air
    [0; 6],             // Stone
    [1; 6],             // Dirt
    [3, 3, 2, 1, 3, 3], // Grass: бока, верх, дно-земля
    [4; 6],             // Wood
    [5; 6],             // Leaves
    [6; 6],             // Lamp
];

// Инварианты грамматики §5 проверяются компилятором (с приходом таблиц
// в M2 переедут в валидацию загрузчика):
//  — вариация яркости любого материала ≤ ±15%;
//  — верх травы светлее её боков.
const _: () = {
    let mut i = 0;
    while i < TEXTURES.len() {
        assert!(TEXTURES[i].variation <= 38, "±15% — предел по §5");
        i += 1;
    }
    assert!(
        luma(GRASS_TOP.palette[0]) > luma(GRASS_SIDE.palette[0]),
        "верх травы светлее боков (§5)"
    );
};

/// Яркость по rec.601 в целых весах (нормировка не нужна для сравнения).
const fn luma([r, g, b]: [u8; 3]) -> u32 {
    299 * r as u32 + 587 * g as u32 + 114 * b as u32
}

/// Целочисленный value-noise в точке (x, y) текстурной решётки.
/// Возвращает 0..=255. Билинейная интерполяция в fixed-point: дробная
/// часть координаты — это просто младшие `cell_log2` бит пикселя.
fn value_noise(seed: u64, x: u32, y: u32, cell_log2: u8) -> i32 {
    let (cx, cy) = ((x >> cell_log2) as i32, (y >> cell_log2) as i32);
    // Дробная часть координаты, отмасштабированная к 0..256 (fixed-point 8 бит).
    let mask = (1u32 << cell_log2) - 1;
    let fx = ((x & mask) << (8 - cell_log2)) as i32;
    let fy = ((y & mask) << (8 - cell_log2)) as i32;
    let corner = |dx, dy| (hash2(seed, cx + dx, cy + dy) & 0xFF) as i32;
    let lerp = |a: i32, b: i32, t: i32| a + (((b - a) * t) >> 8);
    let top = lerp(corner(0, 0), corner(1, 0), fx);
    let bot = lerp(corner(0, 1), corner(1, 1), fx);
    lerp(top, bot, fy)
}

/// Перетасовка паттерна для одного варианта: `strength` обменов соседних
/// ячеек, детерминированно от `seed`. Возвращает копию; оригинал в
/// дескрипторе не трогаем. strength=0 или всё-AUTO → копия без изменений
/// (обмен AUTO↔AUTO ничего не меняет → байт-в-байт как раньше).
fn jitter_pattern(base: &Pattern, seed: u64, strength: u8) -> Pattern {
    let mut p = *base;
    let mut s = seed ^ 0x5EED_1357;
    for _ in 0..strength as u32 {
        s = kb_core::splitmix64(s);
        let idx = (s % (TEX_SIZE * TEX_SIZE) as u64) as usize;
        let (x, y) = (idx % TEX_SIZE, idx / TEX_SIZE);
        // Обмен с соседом по направлению из старших бит того же слова.
        let (nx, ny) = match (s >> 32) & 3 {
            0 if x + 1 < TEX_SIZE => (x + 1, y),
            1 if x > 0 => (x - 1, y),
            2 if y + 1 < TEX_SIZE => (x, y + 1),
            3 if y > 0 => (x, y - 1),
            _ => continue,
        };
        p.swap(idx, ny * TEX_SIZE + nx);
    }
    p
}

/// Печёт текстуру материала в готовые RGBA-байты.
///
/// Два независимых шумовых слоя: крупный выбирает цвет из палитры,
/// попиксельный слегка качает яркость — так блок получает и структуру,
/// и «зерно», оставаясь в рамках инварианта ±15%.
pub fn bake(desc: &Descriptor, seed: u64) -> [u8; TEX_BYTES] {
    let mut out = [0u8; TEX_BYTES];
    // Тон варианта: лёгкий общий сдвиг яркости ±5% — варианты блока
    // различимы и в упор, и силуэтом издалека, оставаясь «той же сутью».
    let tone = ((hash2(seed, -1, -1) & 0xFF) as i32 - 128) * desc.spread as i32 / 128;
    // Структура этого варианта: паттерн, перетасованный его же сидом.
    // У каждого варианта своя раскладка → блоки в мире различаются, при
    // этом набор цветов и дырок сохранён (§5: «контролируемый хаос»).
    let pattern = jitter_pattern(&desc.pattern, seed, desc.pattern_jitter);
    for y in 0..TEX_SIZE as u32 {
        for x in 0..TEX_SIZE as u32 {
            let i = y as usize * TEX_SIZE + x as usize;
            // Прозрачная ячейка паттерна: out уже нулевой → alpha 0.
            if pattern[i] == TRANSPARENT {
                continue;
            }
            // База: фиксированный цвет рисунка (структура §5) либо, для
            // AUTO, процедурный шум + оверлей — как было до паттернов.
            let cell = pattern[i];
            let color = if (cell as usize) < 4 {
                desc.palette[cell as usize]
            } else {
                let structure = value_noise(seed, x, y, desc.cell_log2);
                let mut c = desc.palette[(structure as usize).clamp(0, 255) * 4 / 256];
                match desc.overlay {
                    Overlay::None => {}
                    // Кластеры 2×2: один бросок на ячейку — крапинка читается
                    // как объект, а не как шум-«песок».
                    Overlay::Speckle { color: oc, chance } => {
                        if hash2(seed ^ 0xC3, (x / 2) as i32, (y / 2) as i32) & 0xFF
                            < chance as u32
                        {
                            c = oc;
                        }
                    }
                    // Полоса каждые `period` столбцов, с дрожанием положения.
                    Overlay::Stripes { color: oc, period } => {
                        let jitter = hash2(seed ^ 0x51, (x / period as u32) as i32, 0) & 1;
                        if x % period as u32 == jitter {
                            c = oc;
                        }
                    }
                    // Глубина кромки своя в каждом столбце → рваный край.
                    Overlay::TopBand { palette, min_depth, max_depth } => {
                        let depth = min_depth as u32
                            + hash2(seed ^ 0x77, x as i32, 0) % (max_depth - min_depth + 1) as u32;
                        if y < depth {
                            c = palette[(hash2(seed ^ 0x77, x as i32, y as i32) & 1) as usize];
                        }
                    }
                }
                c
            };

            // Зерно: независимый сид, знаковое отклонение яркости −v..=+v.
            let grain = (hash2(seed ^ 0xA5A5, x as i32, y as i32) & 0xFF) as i32;
            let bright = 256 + tone + ((grain - 128) * desc.variation as i32) / 128;
            let px = &mut out[(i * 4)..][..4];
            for (dst, &c) in px[..3].iter_mut().zip(&color) {
                *dst = ((c as i32 * bright) >> 8).clamp(0, 255) as u8;
            }
            px[3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §16: одинаковый сид → одинаковый хеш текстуры. Эталон зафиксирован;
    /// этот же тест обязан проходить на wasm32 (`cargo test --target ...`).
    #[test]
    fn bake_is_deterministic() {
        let a = bake(&STONE, 42);
        assert_eq!(a, bake(&STONE, 42));
        assert_ne!(a, bake(&STONE, 43));
        assert_eq!(kb_core::fnv1a(&a), GOLDEN_STONE_42);
    }

    /// Зафиксированный хеш эталонной текстуры. Меняется только вместе
    /// с версией генератора (§4).
    // Обновлён вместе с полем spread (разброс тона вариантов §5).
    // Паттерн STONE — весь AUTO, поэтому хеш НЕ изменился: путь AUTO
    // байт-в-байт совпадает с прежним. До альфы менять эталон законно.
    const GOLDEN_STONE_42: u64 = 12642292647876812355;

    /// Паттерн-рисунок честно правит пиксели: фиксированная ячейка даёт
    /// точный цвет палитры, прозрачная — нулевую альфу (§5).
    #[test]
    fn pattern_draws_and_punches_holes() {
        let mut d = STONE;
        d.variation = 0; // убираем зерно, чтобы сверять чистый цвет
        d.spread = 0;
        d.pattern[0] = 2; // ячейка (0,0) — фиксированный индекс палитры 2
        d.pattern[1] = TRANSPARENT; // ячейка (1,0) — дырка
        let tex = bake(&d, 7);
        assert_eq!(&tex[0..3], &STONE.palette[2]); // точный цвет рисунка
        assert_eq!(tex[3], 255); // непрозрачна
        assert_eq!(tex[7], 0); // соседняя — прозрачна (alpha 0)
    }

    /// При jitter>0 разные варианты дают разную раскладку (блоки в мире
    /// не одинаковы), но при jitter=0 — идентичны (фиксированный рисунок).
    #[test]
    fn jitter_makes_variants_differ() {
        let mut d = STONE;
        d.variation = 0;
        d.spread = 0;
        // Рисунок из двух цветов в шахматку, чтобы перестановки были видны.
        for (i, c) in d.pattern.iter_mut().enumerate() {
            *c = (i % 2) as u8;
        }
        let a0 = bake(&d, variant_seed(0, 0));
        let b0 = bake(&d, variant_seed(0, 1));
        assert_eq!(a0, b0, "jitter=0 → варианты одинаковы");

        d.pattern_jitter = 24;
        let a = bake(&d, variant_seed(0, 0));
        let b = bake(&d, variant_seed(0, 1));
        assert_ne!(a, b, "jitter>0 → варианты различаются");
    }
}
