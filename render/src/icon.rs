//! Изометрические иконки блоков для хотбара (как в классике): кубик из
//! трёх граней рисуется на CPU из тех же процедурных текстур, что и мир,
//! и складывается в texture array (слой = id блока). Ноль ассетов (§4).

use kb_core::Block;
use kb_materials::{bake, variant_seed, FACE_LAYERS, TEXTURES, TEX_SIZE};

/// Сторона иконки в пикселях.
pub const ICON: usize = 16;

/// (size, layers, rgba) — texture array иконок, слой = id блока.
pub fn render() -> (u32, u32, Vec<u8>) {
    let mut out = vec![0u8; ICON * ICON * 4 * Block::COUNT];
    for id in 1..Block::COUNT {
        let Some(block) = Block::from_id(id as u8) else { continue };
        let face = |n: usize| {
            let layer = FACE_LAYERS[id][n] as usize;
            bake(&TEXTURES[layer], variant_seed(layer, 0))
        };
        let top = face(2); // +Y
        let side = face(0); // +X
        let dst = &mut out[id * ICON * ICON * 4..][..ICON * ICON * 4];
        draw_cube(dst, &top, &side, block);
    }
    (ICON as u32, ICON as u32, out)
}

/// Грань-параллелограмм: начало O и рёбра A (u), B (v) в пикселях иконки.
struct Face {
    o: [f32; 2],
    a: [f32; 2],
    b: [f32; 2],
    shade: f32,
}

fn draw_cube(dst: &mut [u8], top: &[u8], side: &[u8], _block: Block) {
    // Геометрия изо-кубика на всю иконку 16×16: верхний ромб 16×8, боковые
    // грани по 8 px высотой — полноценный куб, а не «полублок».
    let (n, e, s, w) = ([8.0, 0.0], [16.0, 4.0], [8.0, 8.0], [0.0, 4.0]);
    let (wb, sb) = ([0.0, 12.0], [8.0, 16.0]);
    let sub = |p: [f32; 2], q: [f32; 2]| [p[0] - q[0], p[1] - q[1]];
    let faces = [
        Face { o: w, a: sub(n, w), b: sub(s, w), shade: 1.0 }, // верх
        Face { o: w, a: sub(s, w), b: sub(wb, w), shade: 0.72 }, // лево
        Face { o: s, a: sub(e, s), b: sub(sb, s), shade: 0.55 }, // право
    ];
    for y in 0..ICON {
        for x in 0..ICON {
            for (fi, f) in faces.iter().enumerate() {
                let p = [x as f32 + 0.5 - f.o[0], y as f32 + 0.5 - f.o[1]];
                let det = f.a[0] * f.b[1] - f.a[1] * f.b[0];
                let u = (p[0] * f.b[1] - p[1] * f.b[0]) / det;
                let v = (f.a[0] * p[1] - f.a[1] * p[0]) / det;
                if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                    continue;
                }
                let tex = if fi == 0 { top } else { side };
                let tx = (u * (TEX_SIZE - 1) as f32) as usize;
                let ty = (v * (TEX_SIZE - 1) as f32) as usize;
                let src = &tex[(ty * TEX_SIZE + tx) * 4..][..4];
                if src[3] < 128 {
                    continue; // прозрачный пиксель текстуры (листва)
                }
                let o = (y * ICON + x) * 4;
                for c in 0..3 {
                    dst[o + c] = (src[c] as f32 * f.shade) as u8;
                }
                dst[o + 3] = 255;
                break; // первая накрывшая грань выигрывает
            }
        }
    }
}
