//! kb-worldgen: детерминированный генератор мира (§4, §13 M1).
//!
//! Ландшафт = чистая функция от (сид, координаты чанка). Никакого float,
//! никакого состояния: integer value-noise на fixed-point — бит-в-бит
//! одинаков на native и wasm32. `no_std` гарантирует это конструктивно.

#![no_std]
#![forbid(unsafe_code)]

use kb_core::{hash2, splitmix64, Block, Chunk, CHUNK_X, CHUNK_Y, CHUNK_Z};

/// Версия генератора (§4): хранится в сейве; менять формулы можно только
/// добавив новую версию, старые остаются в коде навсегда.
pub const VERSION: u32 = 1;

/// Уровень «моря» — базовая высота рельефа.
const BASE_HEIGHT: i32 = 56;
/// Толщина дёрна под травой.
const DIRT_DEPTH: i32 = 3;

/// 2D value-noise в мировых координатах: 0..=255.
/// Дробная часть — fixed-point 8 бит, сглаживание — целочисленный
/// smoothstep, билинейная интерполяция на i32 (сдвиг арифметический,
/// поведение определено языком).
fn noise2(seed: u64, wx: i32, wz: i32, cell_log2: u32) -> i32 {
    let (cx, cz) = (wx >> cell_log2, wz >> cell_log2);
    let mask = (1i32 << cell_log2) - 1;
    // t ∈ 0..256, затем smoothstep: s = 3t² − 2t³ в fixed-point.
    let smooth = |t: i32| (t * t * (768 - 2 * t)) >> 16;
    let fx = smooth(((wx & mask) << 8) >> cell_log2);
    let fz = smooth(((wz & mask) << 8) >> cell_log2);
    let corner = |dx, dz| (hash2(seed, cx + dx, cz + dz) & 0xFF) as i32;
    let lerp = |a: i32, b: i32, t: i32| a + (((b - a) * t) >> 8);
    lerp(
        lerp(corner(0, 0), corner(1, 0), fx),
        lerp(corner(0, 1), corner(1, 1), fx),
        fz,
    )
}

/// Высота рельефа в столбце (wx, wz): fBm из трёх октав.
/// Веса 4:2:1 — крупная форма доминирует, мелочь даёт фактуру.
/// Публична: клиенту нужна точка спавна, тестам — проверка бесшовности.
pub fn height(seed: u64, wx: i32, wz: i32) -> i32 {
    let octave = |o: u64, cell| noise2(splitmix64(seed ^ o), wx, wz, cell) - 128;
    let fbm = 4 * octave(1, 6) + 2 * octave(2, 4) + octave(3, 3); // ±~890
    (BASE_HEIGHT + ((fbm * 40) >> 10)).clamp(1, CHUNK_Y as i32 - 1)
}

/// Радиус кроны: дерево «дотягивается» в чанк из соседних столбцов.
const CROWN: i32 = 2;
/// Шанс дерева на столбец, из 256.
const TREE_CHANCE: u32 = 3;

/// Дерево в столбце (wx, wz)? Чистая функция мировых координат —
/// деревья бесшовны через границы чанков так же, как рельеф.
fn tree_at(seed: u64, wx: i32, wz: i32) -> Option<(i32, i32)> {
    let roll = hash2(splitmix64(seed ^ 0xDEED), wx, wz);
    (roll & 0xFF < TREE_CHANCE).then(|| {
        let ground = height(seed, wx, wz);
        (ground, 4 + (roll >> 8 & 1) as i32) // высота ствола 4–5
    })
}

/// Генерация чанка (cx, cz). Бюджет §9: < 1 мс.
pub fn generate(seed: u64, cx: i32, cz: i32) -> Chunk {
    let (ox, oz) = (cx * CHUNK_X as i32, cz * CHUNK_Z as i32);

    // Карта высот считается один раз на столбец, не на блок.
    let mut heights = [0i32; CHUNK_X * CHUNK_Z];
    for z in 0..CHUNK_Z {
        for x in 0..CHUNK_X {
            heights[x + z * CHUNK_X] = height(seed, ox + x as i32, oz + z as i32);
        }
    }

    // Деревья, чьи кроны достают до чанка: ствол + крона рисуются в
    // буфер поверх рельефа — чанк остаётся чистой функцией координат.
    let mut flora = [Block::Air; CHUNK_X * CHUNK_Y * CHUNK_Z];
    let mut put = |x: i32, y: i32, z: i32, b: Block| {
        if (0..CHUNK_X as i32).contains(&x)
            && (0..CHUNK_Y as i32).contains(&y)
            && (0..CHUNK_Z as i32).contains(&z)
        {
            flora[x as usize + z as usize * CHUNK_X + y as usize * CHUNK_X * CHUNK_Z] = b;
        }
    };
    for tz in -CROWN..CHUNK_Z as i32 + CROWN {
        for tx in -CROWN..CHUNK_X as i32 + CROWN {
            let Some((ground, trunk)) = tree_at(seed, ox + tx, oz + tz) else { continue };
            let top = ground + trunk;
            // Крона: два слоя 5×5 под макушкой, слой 3×3 и шапка сверху.
            for (dy, r) in [(0, 2), (1, 2), (2, 1), (3, 1)] {
                for dz in -r..=r {
                    for dx in -r..=r {
                        // Срезанные углы — крона круглее.
                        if dx * dx + dz * dz <= r * r + 1 {
                            put(tx + dx, top - 1 + dy, tz + dz, Block::Leaves);
                        }
                    }
                }
            }
            for y in ground + 1..=top {
                put(tx, y, tz, Block::Wood);
            }
        }
    }

    Chunk::from_fn(|x, y, z| {
        let f = flora[x + z * CHUNK_X + y * CHUNK_X * CHUNK_Z];
        if f != Block::Air {
            return f;
        }
        let h = heights[x + z * CHUNK_X];
        match y as i32 {
            y if y > h => Block::Air,
            y if y == h => Block::Grass,
            y if y > h - DIRT_DEPTH => Block::Dirt,
            _ => Block::Stone,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §16: одинаковый сид → одинаковый хеш чанка. Эталон зафиксирован
    /// для VERSION = 1; его изменение = слом существующих миров.
    #[test]
    fn chunk_is_deterministic() {
        assert_eq!(generate(42, 0, 0).content_hash(), GOLDEN_CHUNK_42_0_0);
        assert_eq!(
            generate(42, -3, 7).content_hash(),
            generate(42, -3, 7).content_hash()
        );
        assert_ne!(
            generate(42, 0, 0).content_hash(),
            generate(43, 0, 0).content_hash()
        );
    }

    // Обновлён вместе с деревьями (M4). После альфы такое изменение
    // потребует VERSION+1 с сохранением старой формулы (§4).
    const GOLDEN_CHUNK_42_0_0: u64 = 8179010388496247302;

    /// Рельеф бесшовен: блок на границе чанка совпадает с предсказанием
    /// `height` в мировых координатах — генератор не знает о чанках.
    #[test]
    fn seamless_across_chunks() {
        let b = generate(7, 1, 0);
        for z in 0..CHUNK_Z {
            let h = height(7, CHUNK_X as i32, z as i32); // мировой столбец x=16
            assert_eq!(b.get(0, h as usize, z), Block::Grass);
            assert_eq!(b.get(0, h as usize + 1, z), Block::Air);
        }
    }
}
