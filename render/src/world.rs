//! Стриминг чанков вокруг камеры + дифф-лог правок (§4).
//!
//! Состояние мира = generate(seed) + diff_log, и ничего больше. Чанк —
//! кэш: вышел из зоны видимости — выброшен без записи; правка блока —
//! запись в дифф-лог и перевычисление чанка из функции. Сейв — это сид,
//! версия генератора и сжатый список правок.

use std::collections::HashMap;

use kb_core::{Block, Chunk, CHUNK_X, CHUNK_Y, CHUNK_Z};
use wgpu::util::DeviceExt;

use crate::{math, mesh};

/// Дистанция видимости в чанках (бюджет §9 меряется на 8).
pub const VIEW_RADIUS: i32 = 8;
/// Сколько чанков мешится за кадр: размазывает холодный старт по кадрам,
/// убирая фризы; генерация дешёвая и в лимит не упирается.
const MESH_BUDGET_PER_FRAME: usize = 6;

/// Магия + версия формата сейва.
const SAVE_MAGIC: &[u8; 4] = b"kbs\x01";

struct Entry {
    chunk: Chunk,
    /// None — меш ещё не строился; пустой меш хранится как (None, true).
    mesh: Option<wgpu::Buffer>,
    quads: u32,
    meshed: bool,
}

/// Правки чанка: локальный индекс блока → материал. Локальный u16 вместо
/// мировых координат — 8–10 байт на правку в сейве (§4).
type ChunkDiff = HashMap<u16, Block>;

pub struct World {
    pub seed: u64,
    chunks: HashMap<(i32, i32), Entry>,
    diffs: HashMap<(i32, i32), ChunkDiff>,
}

/// Видимый чанк: смещение в мире + его вершинный буфер.
pub struct Draw<'a> {
    pub origin: [f32; 3],
    pub vertices: &'a wgpu::Buffer,
    pub quads: u32,
}

#[inline]
fn local_index(x: usize, y: usize, z: usize) -> u16 {
    (x + z * CHUNK_X + y * CHUNK_X * CHUNK_Z) as u16
}

/// Мировая координата → (чанк, локальная).
#[inline]
fn split(p: [i32; 3]) -> ((i32, i32), (usize, usize, usize)) {
    (
        (p[0].div_euclid(CHUNK_X as i32), p[2].div_euclid(CHUNK_Z as i32)),
        (
            p[0].rem_euclid(CHUNK_X as i32) as usize,
            p[1] as usize,
            p[2].rem_euclid(CHUNK_Z as i32) as usize,
        ),
    )
}

impl World {
    pub fn new(seed: u64) -> Self {
        Self { seed, chunks: HashMap::new(), diffs: HashMap::new() }
    }

    /// Чанк как функция: генерация + наложение дифф-лога (§4).
    fn compute(&self, pos: (i32, i32)) -> Chunk {
        let base = kb_worldgen::generate(self.seed, pos.0, pos.1);
        match self.diffs.get(&pos) {
            None => base,
            Some(d) => Chunk::from_fn(|x, y, z| {
                d.get(&local_index(x, y, z)).copied().unwrap_or_else(|| base.get(x, y, z))
            }),
        }
    }

    fn ensure(&mut self, pos: (i32, i32)) {
        if !self.chunks.contains_key(&pos) {
            let chunk = self.compute(pos);
            self.chunks
                .insert(pos, Entry { chunk, mesh: None, quads: 0, meshed: false });
        }
    }

    /// Блок в мировых координатах; за пределами высоты — воздух.
    /// Генерирует чанк при промахе кэша — физика не проваливается в
    /// несгенерированный мир.
    pub fn block(&mut self, p: [i32; 3]) -> Block {
        if !(0..CHUNK_Y as i32).contains(&p[1]) {
            return Block::Air;
        }
        let (pos, (x, y, z)) = split(p);
        self.ensure(pos);
        self.chunks[&pos].chunk.get(x, y, z)
    }

    /// Правка мира — единственный способ его изменить (§1): запись в
    /// дифф-лог и перевычисление чанка. Соседи перемешиваются тоже —
    /// правка на границе меняет их грани.
    pub fn set_block(&mut self, p: [i32; 3], b: Block) {
        if !(0..CHUNK_Y as i32).contains(&p[1]) {
            return;
        }
        let (pos, (x, y, z)) = split(p);
        self.diffs.entry(pos).or_default().insert(local_index(x, y, z), b);
        let chunk = self.compute(pos);
        self.ensure(pos);
        let e = self.chunks.get_mut(&pos).unwrap();
        (e.chunk, e.meshed) = (chunk, false);
        for d in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if let Some(n) = self.chunks.get_mut(&(pos.0 + d.0, pos.1 + d.1)) {
                n.meshed = false;
            }
        }
    }

    /// Сейв = сид + версия генератора + дифф-лог (§4): 11 байт на правку.
    /// Уплотнение лога (RLE областей, схлопывание) — фоновая задача M4+.
    pub fn save(&self) -> Vec<u8> {
        let mut out = Vec::from(*SAVE_MAGIC);
        out.extend(self.seed.to_le_bytes());
        out.extend(kb_worldgen::VERSION.to_le_bytes());
        for (&(cx, cz), d) in &self.diffs {
            for (&idx, &b) in d {
                out.extend(cx.to_le_bytes());
                out.extend(cz.to_le_bytes());
                out.extend(idx.to_le_bytes());
                out.push(b as u8);
            }
        }
        out
    }

    /// Мир из сейва. None — чужой формат или версия генератора из будущего.
    pub fn restore(bytes: &[u8]) -> Option<Self> {
        let body = bytes.strip_prefix(SAVE_MAGIC)?;
        let (head, mut rest) = body.split_at_checked(12)?;
        let seed = u64::from_le_bytes(head[..8].try_into().ok()?);
        if u32::from_le_bytes(head[8..].try_into().ok()?) > kb_worldgen::VERSION {
            return None;
        }
        let mut world = Self::new(seed);
        while let Some((entry, tail)) = rest.split_at_checked(11) {
            let cx = i32::from_le_bytes(entry[..4].try_into().ok()?);
            let cz = i32::from_le_bytes(entry[4..8].try_into().ok()?);
            let idx = u16::from_le_bytes(entry[8..10].try_into().ok()?);
            let block = Block::from_id(entry[10])?;
            world.diffs.entry((cx, cz)).or_default().insert(idx, block);
            rest = tail;
        }
        Some(world)
    }

    /// Один шаг стриминга + сбор видимых чанков для отрисовки.
    pub fn update<'a>(
        &'a mut self,
        device: &wgpu::Device,
        camera_pos: [f32; 3],
        planes: &[[f32; 4]; 6],
    ) -> Vec<Draw<'a>> {
        let center = (
            (camera_pos[0] as i32).div_euclid(CHUNK_X as i32),
            (camera_pos[2] as i32).div_euclid(CHUNK_Z as i32),
        );
        let dist = |(x, z): (i32, i32)| (x - center.0).abs().max((z - center.1).abs());

        // Данные нужны на кольцо шире видимости — ради соседей при мешинге.
        self.chunks.retain(|&pos, _| dist(pos) <= VIEW_RADIUS + 1);
        let mut needed: Vec<(i32, i32)> = (-VIEW_RADIUS - 1..=VIEW_RADIUS + 1)
            .flat_map(|dz| {
                (-VIEW_RADIUS - 1..=VIEW_RADIUS + 1).map(move |dx| (center.0 + dx, center.1 + dz))
            })
            .collect();
        needed.sort_by_key(|&p| dist(p)); // ближние первыми: мир растёт от игрока
        for &pos in &needed {
            self.ensure(pos);
        }

        // Мешим понемногу, ближние первыми. Соседи гарантированно есть:
        // данные сгенерированы на кольцо шире, чем мешится.
        let mut budget = MESH_BUDGET_PER_FRAME;
        for &pos in needed.iter().filter(|&&p| dist(p) <= VIEW_RADIUS) {
            if budget == 0 {
                break;
            }
            if self.chunks[&pos].meshed {
                continue;
            }
            let n = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .map(|(dx, dz)| &self.chunks[&(pos.0 + dx, pos.1 + dz)].chunk);
            let verts = mesh::build(&self.chunks[&pos].chunk, &n);
            let buffer = (!verts.is_empty()).then(|| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("chunk.vb"),
                    contents: bytemuck::cast_slice(&verts),
                    usage: wgpu::BufferUsages::VERTEX,
                })
            });
            let e = self.chunks.get_mut(&pos).unwrap();
            (e.quads, e.mesh, e.meshed) = (verts.len() as u32 / 8, buffer, true);
            budget -= 1;
        }

        // Frustum culling по AABB чанков (§6).
        self.chunks
            .iter()
            .filter(|(&pos, e)| e.mesh.is_some() && dist(pos) <= VIEW_RADIUS)
            .filter_map(|(&(cx, cz), e)| {
                let origin = [(cx * CHUNK_X as i32) as f32, 0.0, (cz * CHUNK_Z as i32) as f32];
                let max = [
                    origin[0] + CHUNK_X as f32,
                    CHUNK_Y as f32,
                    origin[2] + CHUNK_Z as f32,
                ];
                math::aabb_visible(planes, origin, max).then(|| Draw {
                    origin,
                    vertices: e.mesh.as_ref().unwrap(),
                    quads: e.quads,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_then_save_then_restore() {
        let mut w = World::new(42);
        let ground = kb_worldgen::height(42, 3, 3);
        w.set_block([3, ground + 1, 3], Block::Stone);
        w.set_block([3, ground, 3], Block::Air);

        let mut r = World::restore(&w.save()).expect("формат сейва");
        assert_eq!(r.seed, 42);
        assert_eq!(r.block([3, ground + 1, 3]), Block::Stone);
        assert_eq!(r.block([3, ground, 3]), Block::Air);
        // Нетронутый мир — из генератора, не из сейва.
        assert_eq!(r.block([100, kb_worldgen::height(42, 100, 7), 7]), Block::Grass);
    }

    #[test]
    fn save_is_tiny() {
        let mut w = World::new(1);
        for i in 0..100 {
            w.set_block([i, 60, 0], Block::Stone);
        }
        // 16 байт заголовка + 11 байт на правку (§4: цель < 100 КБ на мир).
        assert_eq!(w.save().len(), 16 + 100 * 11);
    }
}
