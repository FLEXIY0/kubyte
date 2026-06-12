//! Greedy meshing (§6): соседние одинаковые грани сливаются в один квад.
//!
//! Алгоритм один для всех трёх осей: между каждой парой соседних срезов
//! строится 2D-маска видимых граней, по маске жадно вырезаются максимальные
//! прямоугольники. Текстура тайлится по кваду в шейдере (uv = мировые
//! координаты), поэтому слияние не искажает рисунок.

use kb_core::{Block, Chunk, CHUNK_X, CHUNK_Y, CHUNK_Z};

/// Упакованная вершина чанка — один u32 (§6):
/// биты 0..5 — x (0..=16), 5..10 — z (0..=16), 10..18 — y (0..=128),
/// 18..21 — индекс нормали (+X −X +Y −Y +Z −Z), 21..29 — слой текстуры.
#[inline]
fn pack(p: [i32; 3], normal: u32, layer: u32) -> u32 {
    (p[0] as u32) | (p[2] as u32) << 5 | (p[1] as u32) << 10 | normal << 18 | layer << 21
}

/// Грань в маске среза: материал + направление нормали. Одинаковые грани
/// (производный слой текстуры совпадает) — кандидаты на слияние.
type Cell = Option<(Block, bool)>;

const DIMS: [i32; 3] = [CHUNK_X as i32, CHUNK_Y as i32, CHUNK_Z as i32];

/// Соседи чанка по горизонтали в порядке +X −X +Z −Z; по вертикали мир
/// кончается воздухом.
pub type Neighbors<'a> = [&'a Chunk; 4];

/// Строит вершины меша чанка (4 вершины на квад; индексы — общий буфер
/// с фиксированным паттерном, см. world.rs).
pub fn build(chunk: &Chunk, neighbors: &Neighbors) -> Vec<u32> {
    let get = |p: [i32; 3]| -> Block {
        let [x, y, z] = p;
        if !(0..CHUNK_Y as i32).contains(&y) {
            return Block::Air;
        }
        let (y, xu, zu) = (y as usize, x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
        match (x, z) {
            (0..=15, 0..=15) => chunk.get(x as usize, y, z as usize),
            (16.., _) => neighbors[0].get(0, y, zu),
            (..=-1, _) => neighbors[1].get(15, y, zu),
            (_, 16..) => neighbors[2].get(xu, y, 0),
            _ => neighbors[3].get(xu, y, 15),
        }
    };

    let mut verts = Vec::new();
    for d in 0..3usize {
        let (u, v) = ((d + 1) % 3, (d + 2) % 3);
        let (du, dv) = (DIMS[u] as usize, DIMS[v] as usize);
        let mut mask: Vec<Cell> = vec![None; du * dv];

        // Срез s — плоскость между слоями s-1 и s вдоль оси d.
        for s in 0..=DIMS[d] {
            for (n, cell) in mask.iter_mut().enumerate() {
                let mut a = [0i32; 3];
                a[u] = (n % du) as i32;
                a[v] = (n / du) as i32;
                let mut b = a;
                (a[d], b[d]) = (s - 1, s);
                let (ba, bb) = (get(a), get(b));
                *cell = match (ba.solid(), bb.solid()) {
                    (true, false) => Some((ba, true)),  // нормаль +d
                    (false, true) => Some((bb, false)), // нормаль −d
                    _ => None,
                };
            }

            // Жадная нарезка маски на максимальные прямоугольники.
            for n in 0..mask.len() {
                let Some(cell) = mask[n] else { continue };
                let (i, j) = (n % du, n / du);
                let mut w = 1;
                while i + w < du && mask[n + w] == Some(cell) {
                    w += 1;
                }
                let mut h = 1;
                while j + h < dv
                    && mask[n + h * du..n + h * du + w].iter().all(|c| *c == Some(cell))
                {
                    h += 1;
                }
                emit(&mut verts, (d, u, v), s, (i, j), (w, h), cell);
                for row in 0..h {
                    mask[n + row * du..n + row * du + w].fill(None);
                }
            }
        }
    }
    verts
}

/// Квад w×h в срезе s. Порядок вершин подобран под общий индексный
/// паттерн [0,1,2, 2,1,3]; у задних граней обход развёрнут переменой
/// ролей u/v — нормаль остаётся наружу.
fn emit(
    verts: &mut Vec<u32>,
    (d, u, v): (usize, usize, usize),
    s: i32,
    (i, j): (usize, usize),
    (w, h): (usize, usize),
    (block, positive): (Block, bool),
) {
    let normal = (2 * d + usize::from(!positive)) as u32;
    let layer = kb_materials::FACE_LAYERS[block as usize][normal as usize] as u32;
    let corners: [(usize, usize); 4] = if positive {
        [(0, 0), (w, 0), (0, h), (w, h)]
    } else {
        [(0, 0), (0, h), (w, 0), (w, h)]
    };
    for (cu, cv) in corners {
        let mut p = [0i32; 3];
        p[d] = s;
        p[u] = (i + cu) as i32;
        p[v] = (j + cv) as i32;
        verts.push(pack(p, normal, layer));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solo(block: Block) -> Chunk {
        Chunk::from_fn(move |x, y, z| if (x, y, z) == (8, 8, 8) { block } else { Block::Air })
    }

    #[test]
    fn lone_block_is_six_quads() {
        let air = Chunk::from_fn(|_, _, _| Block::Air);
        let c = solo(Block::Stone);
        let verts = build(&c, &[&air, &air, &air, &air]);
        assert_eq!(verts.len(), 6 * 4);
    }

    #[test]
    fn flat_slab_merges_to_single_top_quad() {
        let slab = Chunk::from_fn(|_, y, _| if y == 0 { Block::Stone } else { Block::Air });
        let flat = build(&slab, &[&slab, &slab, &slab, &slab]);
        // Соседи такие же → боковых граней нет: один квад верха 16×16
        // и один — дна. Greedy обязан схлопнуть 256 граней в 1.
        assert_eq!(flat.len(), 2 * 4);
    }

    #[test]
    fn buried_block_emits_nothing() {
        let full = Chunk::from_fn(|_, _, _| Block::Stone);
        let v = build(&full, &[&full, &full, &full, &full]);
        // Видимы только верх и низ столба (мир сверху/снизу — воздух).
        assert_eq!(v.len(), 2 * 4);
    }
}
