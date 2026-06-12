//! Воксельный рейкаст (DDA, Amanatides & Woo): каким блоком игрок целится
//! и к какой грани прислонить новый.

use crate::physics::ifloor;

/// Луч из `origin` вдоль `dir` (не обязан быть нормирован — дистанция
/// меряется в его длинах). Возвращает (клетка-попадание, соседняя клетка
/// со стороны луча — куда ставить блок).
pub fn raycast(
    origin: [f32; 3],
    dir: [f32; 3],
    max_dist: f32,
    mut hit: impl FnMut(i32, i32, i32) -> bool,
) -> Option<([i32; 3], [i32; 3])> {
    let mut cell = [ifloor(origin[0]), ifloor(origin[1]), ifloor(origin[2])];
    let step = dir.map(|d| if d > 0.0 { 1 } else { -1 });
    // Сколько t луча укладывается в одну клетку по каждой оси.
    let t_delta = dir.map(|d| 1.0 / d.abs());
    // t до первой границы клетки по каждой оси.
    let mut t_max = [0usize, 1, 2].map(|i| {
        let frac = origin[i] - cell[i] as f32;
        let to_edge = if dir[i] > 0.0 { 1.0 - frac } else { frac };
        to_edge * t_delta[i] // NaN/inf при dir=0 — такая ось никогда не выиграет min
    });

    let mut prev;
    loop {
        // Шагаем через ближайшую границу.
        let axis = (0..3).min_by(|&a, &b| t_max[a].total_cmp(&t_max[b])).unwrap();
        if t_max[axis] > max_dist {
            return None;
        }
        prev = cell;
        cell[axis] += step[axis];
        t_max[axis] += t_delta[axis];
        if hit(cell[0], cell[1], cell[2]) {
            return Some((cell, prev));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_block_and_reports_face_neighbor() {
        // Единственный твёрдый блок — (5, 0, 0); смотрим вдоль +X.
        let world = |x: i32, y: i32, z: i32| (x, y, z) == (5, 0, 0);
        let (hit, prev) = raycast([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 10.0, world).unwrap();
        assert_eq!(hit, [5, 0, 0]);
        assert_eq!(prev, [4, 0, 0]); // ставить будем к ближней грани
    }

    #[test]
    fn respects_max_distance() {
        let world = |x: i32, _: i32, _: i32| x == 50;
        assert!(raycast([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 10.0, world).is_none());
    }

    #[test]
    fn diagonal_never_corner_cuts() {
        // DDA шагает по граням, а не по диагонали: каждый следующий prev
        // отличается от hit ровно одной координатой.
        let world = |x: i32, y: i32, z: i32| x + y + z > 9;
        let (hit, prev) = raycast([0.1, 0.2, 0.3], [1.0, 0.9, 0.8], 20.0, world).unwrap();
        let diff = (0..3).filter(|&i| hit[i] != prev[i]).count();
        assert_eq!(diff, 1);
    }
}
