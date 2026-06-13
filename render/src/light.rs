//! Освещение в духе беты (§6): skylight + blocklight, flood-fill по
//! воксельной сетке. Свет — производная блоков, как и меш: считается при
//! ремеше чанка и нигде не хранится между кадрами (§1: вычислимо — не
//! храним). Смена дня и ночи свет не трогает — она лишь меняет вклад
//! skylight в шейдере.

use std::collections::VecDeque;

use kb_core::{Block, CHUNK_X, CHUNK_Y, CHUNK_Z};

/// Поле считается с запасом в одну клетку вокруг чанка: свет затекает
/// через границы, и граням на краю чанка есть у кого спросить яркость.
const PAD: i32 = 1;
const SX: usize = CHUNK_X + 2 * PAD as usize;
const SZ: usize = CHUNK_Z + 2 * PAD as usize;

/// Один байт на клетку: skylight в младших 4 битах, blocklight в старших.
pub struct Light(Vec<u8>);

#[inline]
fn idx(x: i32, y: i32, z: i32) -> usize {
    (x + PAD) as usize + (z + PAD) as usize * SX + y as usize * SX * SZ
}

impl Light {
    /// (skylight, blocklight) клетки в локальных координатах чанка,
    /// x/z ∈ -1..=16.
    #[inline]
    pub fn at(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
        if !(0..CHUNK_Y as i32).contains(&y) {
            return (15, 0); // над миром — небо, под миром не спрашивают
        }
        let v = self.0[idx(x, y, z)];
        (v & 15, v >> 4)
    }

    /// Flood-fill света для чанка и кольца соседей.
    pub fn compute(get: impl Fn([i32; 3]) -> Block) -> Self {
        let mut sky = vec![0u8; SX * SZ * CHUNK_Y];
        let mut blk = vec![0u8; SX * SZ * CHUNK_Y];
        let mut queue: VecDeque<([i32; 3], u8)> = VecDeque::new();

        // Skylight: столбы 15 от неба до первой тверди (вниз без затухания),
        // затем BFS разносит свет вбок — под кроны и в укрытия.
        for z in -PAD..(CHUNK_Z as i32 + PAD) {
            for x in -PAD..(CHUNK_X as i32 + PAD) {
                for y in (0..CHUNK_Y as i32).rev() {
                    // Свет проходит сквозь прозрачные блоки (листву), иначе
                    // грани за листвой и сама листва были бы чёрными.
                    if get([x, y, z]).occludes() {
                        break;
                    }
                    sky[idx(x, y, z)] = 15;
                    queue.push_back(([x, y, z], 15));
                }
            }
        }
        flood(&mut sky, queue, &get);

        // Blocklight: источники — излучающие блоки (фонари).
        let mut queue: VecDeque<([i32; 3], u8)> = VecDeque::new();
        for z in -PAD..(CHUNK_Z as i32 + PAD) {
            for x in -PAD..(CHUNK_X as i32 + PAD) {
                for y in 0..CHUNK_Y as i32 {
                    let e = get([x, y, z]).emission();
                    if e > 0 {
                        blk[idx(x, y, z)] = e;
                        queue.push_back(([x, y, z], e));
                    }
                }
            }
        }
        flood(&mut blk, queue, &get);

        Self(sky.iter().zip(&blk).map(|(s, b)| s | b << 4).collect())
    }
}

/// BFS-распространение: каждый шаг в сторону теряет один уровень,
/// твёрдые блоки свет не пропускают.
fn flood(field: &mut [u8], mut queue: VecDeque<([i32; 3], u8)>, get: &impl Fn([i32; 3]) -> Block) {
    while let Some(([x, y, z], level)) = queue.pop_front() {
        let next = level - 1;
        if next == 0 {
            continue;
        }
        for [nx, ny, nz] in [
            [x + 1, y, z],
            [x - 1, y, z],
            [x, y + 1, z],
            [x, y - 1, z],
            [x, y, z + 1],
            [x, y, z - 1],
        ] {
            let inside = (-PAD..CHUNK_X as i32 + PAD).contains(&nx)
                && (0..CHUNK_Y as i32).contains(&ny)
                && (-PAD..CHUNK_Z as i32 + PAD).contains(&nz);
            if inside && field[idx(nx, ny, nz)] < next && !get([nx, ny, nz]).occludes() {
                field[idx(nx, ny, nz)] = next;
                queue.push_back(([nx, ny, nz], next));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sunlit_above_shadow_below() {
        // Плита на y=64 во весь чанк.
        let get = |p: [i32; 3]| if p[1] == 64 { Block::Stone } else { Block::Air };
        let l = Light::compute(get);
        assert_eq!(l.at(8, 100, 8).0, 15, "над плитой — небо");
        // Под бесконечной плитой темно (бок не подсвечивает середину).
        assert_eq!(l.at(8, 40, 8).0, 0, "под плитой — тьма");
    }

    #[test]
    fn lamp_glows_and_decays() {
        let get = |p: [i32; 3]| match p {
            [8, 64, 8] => Block::Lamp,
            [_, y, _] if y < 64 => Block::Stone,
            _ => Block::Air,
        };
        let l = Light::compute(get);
        assert_eq!(l.at(8, 64, 8).1, 15, "сам фонарь");
        assert_eq!(l.at(8, 65, 8).1, 14, "сосед на 1 тусклее");
        assert_eq!(l.at(8, 70, 8).1, 9, "затухание по дистанции");
    }
}
