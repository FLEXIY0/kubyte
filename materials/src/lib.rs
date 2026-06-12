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
    /// Оверлей поверх базового шума (§5).
    pub overlay: Overlay,
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

pub const STONE: Descriptor = Descriptor {
    // Серый с холодным подтоном, пятна читаются кластерами «в духе эпохи».
    palette: [[100, 100, 100], [118, 118, 118], [128, 128, 128], [143, 143, 143]],
    cell_log2: 1,
    variation: 14,
    overlay: Overlay::Speckle { color: [88, 88, 88], chance: 36 },
};

pub const DIRT: Descriptor = Descriptor {
    palette: [[110, 78, 54], [122, 87, 60], [134, 96, 67], [146, 106, 74]],
    cell_log2: 1,
    variation: 22,
    overlay: Overlay::Speckle { color: [87, 60, 42], chance: 56 },
};

pub const GRASS_TOP: Descriptor = Descriptor {
    palette: [[90, 134, 58], [100, 146, 64], [110, 158, 70], [122, 170, 78]],
    cell_log2: 1,
    variation: 24,
    overlay: Overlay::Speckle { color: [80, 120, 50], chance: 48 },
};

pub const GRASS_SIDE: Descriptor = Descriptor {
    // Земля с рваной зелёной кромкой сверху — как в бете: травяной слой
    // «свисает» на бок блока на 2–4 пикселя.
    palette: DIRT.palette,
    cell_log2: 1,
    variation: 22,
    overlay: Overlay::TopBand {
        palette: [[96, 140, 62], [114, 162, 72]],
        min_depth: 2,
        max_depth: 4,
    },
};

pub const WOOD: Descriptor = Descriptor {
    palette: [[74, 56, 34], [86, 66, 40], [97, 75, 46], [106, 83, 52]],
    cell_log2: 2,
    variation: 18,
    overlay: Overlay::Stripes { color: [62, 46, 28], period: 4 },
};

pub const LEAVES: Descriptor = Descriptor {
    // Темнее травы: крона читается силуэтом, не сливаясь с лугом.
    palette: [[38, 72, 34], [46, 84, 40], [54, 96, 46], [64, 108, 52]],
    cell_log2: 1,
    variation: 34,
    overlay: Overlay::Speckle { color: [30, 58, 28], chance: 64 },
};

pub const LAMP: Descriptor = Descriptor {
    // Янтарь — тёплый полюс палитры мира (§6: контраст температур).
    palette: [[196, 134, 56], [216, 156, 66], [232, 176, 80], [244, 196, 100]],
    cell_log2: 1,
    variation: 20,
    overlay: Overlay::Speckle { color: [255, 222, 150], chance: 70 },
};

/// «Кожа» мобов — те же дескрипторы, что у блоков (§1: один генератор
/// на всё). Дизайн — свой, «в духе эпохи» (§7), без копирования Mojang.
pub const PIG: Descriptor = Descriptor {
    palette: [[196, 124, 124], [212, 140, 138], [226, 154, 150], [238, 170, 164]],
    cell_log2: 2,
    variation: 16,
    overlay: Overlay::Speckle { color: [178, 108, 110], chance: 24 },
};

pub const ZOMBIE: Descriptor = Descriptor {
    // Болотная гниль: холодная и тусклая — силуэт мрачнее любого блока.
    palette: [[58, 84, 58], [66, 96, 64], [76, 108, 72], [86, 118, 80]],
    cell_log2: 1,
    variation: 30,
    overlay: Overlay::Speckle { color: [44, 62, 46], chance: 60 },
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

/// Печёт текстуру материала в готовые RGBA-байты.
///
/// Два независимых шумовых слоя: крупный выбирает цвет из палитры,
/// попиксельный слегка качает яркость — так блок получает и структуру,
/// и «зерно», оставаясь в рамках инварианта ±15%.
pub fn bake(desc: &Descriptor, seed: u64) -> [u8; TEX_BYTES] {
    let mut out = [0u8; TEX_BYTES];
    for y in 0..TEX_SIZE as u32 {
        for x in 0..TEX_SIZE as u32 {
            let structure = value_noise(seed, x, y, desc.cell_log2);
            let mut color = desc.palette[(structure as usize).clamp(0, 255) * 4 / 256];

            match desc.overlay {
                Overlay::None => {}
                // Кластеры 2×2: один бросок на ячейку — крапинка читается
                // как объект, а не как шум-«песок».
                Overlay::Speckle { color: c, chance } => {
                    if hash2(seed ^ 0xC3, (x / 2) as i32, (y / 2) as i32) & 0xFF < chance as u32 {
                        color = c;
                    }
                }
                // Полоса каждые `period` столбцов, с дрожанием положения.
                Overlay::Stripes { color: c, period } => {
                    let jitter = hash2(seed ^ 0x51, (x / period as u32) as i32, 0) & 1;
                    if x % period as u32 == jitter {
                        color = c;
                    }
                }
                // Глубина кромки своя в каждом столбце → рваный край.
                Overlay::TopBand { palette, min_depth, max_depth } => {
                    let depth = min_depth as u32
                        + hash2(seed ^ 0x77, x as i32, 0) % (max_depth - min_depth + 1) as u32;
                    if y < depth {
                        color = palette[(hash2(seed ^ 0x77, x as i32, y as i32) & 1) as usize];
                    }
                }
            }

            // Зерно: независимый сид, знаковое отклонение яркости −v..=+v.
            let grain = (hash2(seed ^ 0xA5A5, x as i32, y as i32) & 0xFF) as i32;
            let bright = 256 + ((grain - 128) * desc.variation as i32) / 128;
            let px = &mut out[((y as usize * TEX_SIZE + x as usize) * 4)..][..4];
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
    // Обновлён вместе с редизайном текстур «под бету» (мир ещё не имеет
    // публичных сейвов — менять эталон до альфы законно).
    const GOLDEN_STONE_42: u64 = 867192019363213887;
}
