//! Блоки и палитро-чанки (§4 ТЗ).
//!
//! Чанк — это кэш, а не хранилище: он целиком вычислим из сида, поэтому
//! хранение обязано быть дешёвым. Палитра: локальный список материалов
//! чанка + N бит на блок; однородный чанк — одна запись палитры и ноль
//! массивов.

use alloc::vec;
use alloc::vec::Vec;

/// Размеры чанка в блоках (§4: 16×16×128). Y — высота.
pub const CHUNK_X: usize = 16;
pub const CHUNK_Y: usize = 128;
pub const CHUNK_Z: usize = 16;
pub const CHUNK_VOLUME: usize = CHUNK_X * CHUNK_Y * CHUNK_Z;

/// Материал блока. Пока — enum; в M2 таблица материалов сделает это
/// просто индексом в данные. Менять номера нельзя: они уходят в сейвы.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum Block {
    #[default]
    Air = 0,
    Stone = 1,
    Dirt = 2,
    Grass = 3,
    Wood = 4,
    Leaves = 5,
    /// Янтарный фонарь — носитель тёплого blocklight (§6).
    Lamp = 6,
}

impl Block {
    pub const COUNT: usize = 7;

    /// Все варианты по номеру — единственное место декодирования id
    /// (сейвы, сеть, инвентарь).
    pub const fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Block::Air),
            1 => Some(Block::Stone),
            2 => Some(Block::Dirt),
            3 => Some(Block::Grass),
            4 => Some(Block::Wood),
            5 => Some(Block::Leaves),
            6 => Some(Block::Lamp),
            _ => None,
        }
    }

    /// Непрозрачен ли блок (закрывает ли соседние грани).
    #[inline]
    pub const fn solid(self) -> bool {
        !matches!(self, Block::Air)
    }

    /// Загораживает ли блок соседнюю грань. Лист — твёрдый (по нему ходят,
    /// он рубится), но НЕ загораживает: сквозь дырки кроны видно соседнюю
    /// листву и ствол, а не пустоту (§5, дырявые блоки).
    #[inline]
    pub const fn occludes(self) -> bool {
        self.solid() && !matches!(self, Block::Leaves)
    }

    /// Сила собственного света 0..=15 (blocklight, §6).
    #[inline]
    pub const fn emission(self) -> u8 {
        match self {
            Block::Lamp => 15,
            _ => 0,
        }
    }
}

/// Палитро-чанк. Инвариант: `data` хранит ровно `CHUNK_VOLUME` индексов
/// в палитру по `bits` бит, плотно упакованных в u64; `bits == 0` —
/// однородный чанк, данных нет вообще.
pub struct Chunk {
    palette: Vec<Block>,
    bits: u32,
    data: Vec<u64>,
}

/// Линейный индекс блока: столбцы по y идут крупными шагами, чтобы
/// горизонтальные срезы (меш, освещение) ходили по памяти подряд.
#[inline]
fn index(x: usize, y: usize, z: usize) -> usize {
    debug_assert!(x < CHUNK_X && y < CHUNK_Y && z < CHUNK_Z);
    x + z * CHUNK_X + y * CHUNK_X * CHUNK_Z
}

impl Chunk {
    /// Строит чанк из функции «координата → блок» — единственный способ
    /// его создать, что буквально воплощает «мир = функция» (§1).
    /// Палитризация происходит здесь же: временный плоский массив живёт
    /// только внутри вызова.
    pub fn from_fn(mut f: impl FnMut(usize, usize, usize) -> Block) -> Self {
        let mut raw = vec![Block::Air; CHUNK_VOLUME];
        let mut palette: Vec<Block> = Vec::new();
        for y in 0..CHUNK_Y {
            for z in 0..CHUNK_Z {
                for x in 0..CHUNK_X {
                    let b = f(x, y, z);
                    raw[index(x, y, z)] = b;
                    if !palette.contains(&b) {
                        palette.push(b);
                    }
                }
            }
        }
        // Сортировка делает палитру канонической: одинаковое содержимое →
        // одинаковые байты → одинаковый content_hash (§16).
        palette.sort_unstable();

        let bits = bits_for(palette.len());
        if bits == 0 {
            return Self { palette, bits, data: Vec::new() };
        }
        let mut data = vec![0u64; (CHUNK_VOLUME * bits as usize).div_ceil(64)];
        for (i, b) in raw.iter().enumerate() {
            let pi = palette.iter().position(|p| p == b).unwrap() as u64;
            let bit = i * bits as usize;
            data[bit / 64] |= pi << (bit % 64);
            // Индекс, переезжающий границу слова, докладывает старшие биты
            // в следующее слово (индексы никогда не выровнены искусственно —
            // плотность важнее простоты распаковки).
            if bit % 64 + bits as usize > 64 {
                data[bit / 64 + 1] |= pi >> (64 - bit % 64);
            }
        }
        Self { palette, bits, data }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> Block {
        if self.bits == 0 {
            return self.palette[0];
        }
        let bit = index(x, y, z) * self.bits as usize;
        let mut v = self.data[bit / 64] >> (bit % 64);
        if bit % 64 + self.bits as usize > 64 {
            v |= self.data[bit / 64 + 1] << (64 - bit % 64);
        }
        self.palette[(v & ((1 << self.bits) - 1)) as usize]
    }

    /// Однородный ли чанк (весь воздух / весь камень).
    pub fn is_uniform(&self) -> bool {
        self.bits == 0
    }

    /// Байты в куче — для замеров против бюджета §9.
    pub fn heap_bytes(&self) -> usize {
        self.palette.capacity() + self.data.capacity() * 8
    }

    /// Детерминированный хеш содержимого: материал за материалом, блок за
    /// блоком. Тест §16 сравнивает его между native и wasm32.
    pub fn content_hash(&self) -> u64 {
        let mut h = crate::splitmix64(self.palette.len() as u64);
        for b in &self.palette {
            h = crate::splitmix64(h ^ *b as u64);
        }
        for w in &self.data {
            h = crate::splitmix64(h ^ w);
        }
        h
    }
}

/// Бит на индекс для палитры данного размера (0 для однородного чанка).
const fn bits_for(palette_len: usize) -> u32 {
    match palette_len {
        0 | 1 => 0,
        n => (n - 1).ilog2() + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_chunk_is_free() {
        let c = Chunk::from_fn(|_, _, _| Block::Stone);
        assert!(c.is_uniform());
        assert_eq!(c.heap_bytes(), c.palette.capacity()); // ноль массивов
        assert_eq!(c.get(7, 100, 3), Block::Stone);
    }

    #[test]
    fn roundtrip_and_packing() {
        let f = |x: usize, y: usize, z: usize| match (x + y + z) % 3 {
            0 => Block::Air,
            1 => Block::Stone,
            _ => Block::Dirt,
        };
        let c = Chunk::from_fn(f);
        assert_eq!(c.bits, 2); // 3 материала → 2 бита на блок
        for (x, y, z) in [(0, 0, 0), (15, 127, 15), (3, 64, 9), (15, 0, 1)] {
            assert_eq!(c.get(x, y, z), f(x, y, z), "at {x},{y},{z}");
        }
        // 2 бита × 32768 блоков = 8 КиБ — бюджетная цена смешанного чанка
        assert_eq!(c.data.len(), CHUNK_VOLUME * 2 / 64);
    }

    #[test]
    fn hash_ignores_construction_order() {
        // Один контент, разный порядок появления материалов в from_fn —
        // канонизация палитры обязана дать одинаковый хеш.
        let a = Chunk::from_fn(|_, y, _| if y < 64 { Block::Stone } else { Block::Air });
        let b = Chunk::from_fn(|_, y, _| if y >= 64 { Block::Air } else { Block::Stone });
        assert_eq!(a.content_hash(), b.content_hash());
    }
}
