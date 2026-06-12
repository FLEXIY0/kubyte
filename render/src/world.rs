//! Стриминг чанков вокруг камеры (§4: чанки = кэш, не хранилище).
//!
//! Вышел из зоны видимости — чанк выбрасывается без записи; понадобился —
//! перегенерируется из сида. Данные чанков и их меши живут раздельно:
//! для меша нужны соседи, поэтому данные держатся на кольцо шире.

use std::collections::HashMap;

use kb_core::{Chunk, CHUNK_X, CHUNK_Y, CHUNK_Z};
use wgpu::util::DeviceExt;

use crate::{math, mesh};

/// Дистанция видимости в чанках (бюджет §9 меряется на 8).
pub const VIEW_RADIUS: i32 = 8;
/// Сколько чанков мешится за кадр: размазывает холодный старт по кадрам,
/// убирая фризы; генерация дешёвая и в лимит не упирается.
const MESH_BUDGET_PER_FRAME: usize = 6;

struct Entry {
    chunk: Chunk,
    /// None — меш ещё не строился; пустой меш хранится как (None, true).
    mesh: Option<wgpu::Buffer>,
    quads: u32,
    meshed: bool,
}

pub struct World {
    pub seed: u64,
    chunks: HashMap<(i32, i32), Entry>,
}

/// Видимый чанк: смещение в мире + его вершинный буфер.
pub struct Draw<'a> {
    pub origin: [f32; 3],
    pub vertices: &'a wgpu::Buffer,
    pub quads: u32,
}

impl World {
    pub fn new(seed: u64) -> Self {
        Self { seed, chunks: HashMap::new() }
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
            self.chunks.entry(pos).or_insert_with(|| Entry {
                chunk: kb_worldgen::generate(self.seed, pos.0, pos.1),
                mesh: None,
                quads: 0,
                meshed: false,
            });
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
            (e.quads, e.mesh, e.meshed) = (verts.len() as u32 / 4, buffer, true);
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
