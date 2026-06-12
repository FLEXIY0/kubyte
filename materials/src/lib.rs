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
///
/// M0 несёт минимум полей; оверлеи, акценты и анимация (§5) доедут в M2,
/// расширяя эту же структуру.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptor {
    /// Палитра от тёмного к светлому, RGB. Шум выбирает индекс.
    pub palette: [[u8; 3]; 4],
    /// log2 размера ячейки шума в пикселях (1 → ячейка 2 px, 2 → 4 px).
    pub cell_log2: u8,
    /// Амплитуда вариации яркости, в 1/256 долях. Инвариант грамматики
    /// «блок читается глазом» (§5): не более 38 (≈ ±15%).
    pub variation: u8,
}

pub const STONE: Descriptor = Descriptor {
    palette: [[88, 88, 92], [108, 108, 112], [125, 125, 129], [144, 144, 148]],
    cell_log2: 2,
    variation: 30,
};

pub const DIRT: Descriptor = Descriptor {
    palette: [[86, 60, 38], [104, 74, 48], [121, 87, 58], [134, 99, 69]],
    cell_log2: 1,
    variation: 32,
};

pub const GRASS_TOP: Descriptor = Descriptor {
    palette: [[62, 104, 46], [74, 122, 52], [88, 138, 60], [104, 152, 70]],
    cell_log2: 1,
    variation: 26,
};

pub const GRASS_SIDE: Descriptor = Descriptor {
    // Земля с зелёным «налётом» сверху по шуму; светлее не бывает —
    // верх травы обязан читаться светлее боков (§5).
    palette: [[86, 60, 38], [104, 74, 48], [82, 104, 50], [96, 120, 58]],
    cell_log2: 1,
    variation: 30,
};

/// Таблица текстур (§1: данные вместо кода). Индекс = слой texture array.
/// Порядок зафиксирован: на него ссылается FACE_LAYERS.
pub const TEXTURES: &[Descriptor] = &[STONE, DIRT, GRASS_TOP, GRASS_SIDE];

/// Слой текстуры для каждой грани блока: [материал][грань],
/// грани в порядке нормалей рендера: +X −X +Y −Y +Z −Z.
/// Air (0) текстуры не имеет — строка-заглушка, граней у него не бывает.
pub const FACE_LAYERS: [[u8; 6]; 4] = [
    [0; 6],             // Air
    [0; 6],             // Stone
    [1; 6],             // Dirt
    [3, 3, 2, 1, 3, 3], // Grass: бока, верх, дно-земля
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
            let color = desc.palette[(structure as usize * 4) >> 8];
            // Второй слой — независимый сид (²), знаковое отклонение −v..=+v.
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
    const GOLDEN_STONE_42: u64 = 5781321939385730377;
}
