//! Физика игрока: AABB против воксельной сетки, разрешение по осям.
//!
//! float здесь допустим (§1 запрещает его только в генерации мира и
//! текстур); с приходом сервера (M6) физика станет валидируемой, и при
//! необходимости переедет на fixed-point — интерфейс не изменится.

/// Габариты игрока — классические для жанра.
pub const PLAYER_WIDTH: f32 = 0.6;
pub const PLAYER_HEIGHT: f32 = 1.8;
/// Высота глаз от ступней.
pub const EYE_HEIGHT: f32 = 1.62;

const GRAVITY: f32 = 28.0;
const TERMINAL: f32 = 60.0;
/// Скорость прыжка: высота ≈ 1.3 блока — на блок запрыгиваем, на два нет.
const JUMP: f32 = 8.6;
/// Зазор при разрешении коллизии: не даём AABB «прилипнуть» к грани.
const SKIN: f32 = 1e-4;

/// `floor` для no_std: усечение `as i32` округляет к нулю, поправляем
/// отрицательные.
#[inline]
pub(crate) fn ifloor(v: f32) -> i32 {
    let t = v as i32;
    t - (v < t as f32) as i32
}

/// Игрок: позиция — центр ступней. Вся симуляция — в этом структе,
/// рендер только читает позицию.
pub struct Player {
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub on_ground: bool,
}

impl Player {
    pub fn new(pos: [f32; 3]) -> Self {
        Self { pos, vel: [0.0; 3], on_ground: false }
    }

    /// Шаг симуляции: `wish` — желаемая горизонтальная скорость (x, z),
    /// `solid` — мир как функция «клетка → твёрдость» (§1).
    pub fn step(
        &mut self,
        dt: f32,
        wish: [f32; 2],
        jump: bool,
        mut solid: impl FnMut(i32, i32, i32) -> bool,
    ) {
        if jump && self.on_ground {
            self.vel[1] = JUMP;
        }
        self.vel[0] = wish[0];
        self.vel[2] = wish[1];
        self.vel[1] = (self.vel[1] - GRAVITY * dt).max(-TERMINAL);

        // Оси независимы: сначала горизонталь (скольжение вдоль стен),
        // затем вертикаль (приземление).
        self.on_ground = false;
        for axis in [0, 2, 1] {
            self.slide(axis, self.vel[axis] * dt, &mut solid);
        }
    }

    /// Сдвиг по одной оси с выталкиванием из твёрдых блоков. Все блоки,
    /// в которые мы въехали, разделяют одну плоскость грани, поэтому
    /// первого столкновения достаточно.
    fn slide(&mut self, axis: usize, delta: f32, solid: &mut impl FnMut(i32, i32, i32) -> bool) {
        self.pos[axis] += delta;
        let half = PLAYER_WIDTH / 2.0;
        let min = [self.pos[0] - half, self.pos[1], self.pos[2] - half];
        let max = [self.pos[0] + half, self.pos[1] + PLAYER_HEIGHT, self.pos[2] + half];
        for x in ifloor(min[0])..=ifloor(max[0] - SKIN) {
            for y in ifloor(min[1])..=ifloor(max[1] - SKIN) {
                for z in ifloor(min[2])..=ifloor(max[2] - SKIN) {
                    if !solid(x, y, z) {
                        continue;
                    }
                    let cell = [x, y, z][axis] as f32;
                    if delta > 0.0 {
                        self.pos[axis] -= max[axis] - cell + SKIN;
                    } else {
                        self.pos[axis] += cell + 1.0 - min[axis] + SKIN;
                        self.on_ground |= axis == 1;
                    }
                    self.vel[axis] = 0.0;
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Пол на y=10: твёрдо всё, что ниже.
    fn floor10(_: i32, y: i32, _: i32) -> bool {
        y < 10
    }

    #[test]
    fn falls_and_lands() {
        let mut p = Player::new([0.5, 20.0, 0.5]);
        for _ in 0..200 {
            p.step(0.05, [0.0, 0.0], false, floor10);
        }
        assert!(p.on_ground);
        assert!((p.pos[1] - 10.0).abs() < 0.01, "стоит на полу, y={}", p.pos[1]);
    }

    #[test]
    fn jump_clears_one_block_not_two() {
        let mut p = Player::new([0.5, 10.0, 0.5]);
        p.on_ground = true;
        let mut peak = 0.0f32;
        for _ in 0..100 {
            p.step(0.01, [0.0, 0.0], true, floor10);
            peak = peak.max(p.pos[1]);
        }
        assert!(peak - 10.0 > 1.0 && peak - 10.0 < 2.0, "пик прыжка {peak}");
    }

    #[test]
    fn wall_blocks_and_slides() {
        // Стена x ≥ 12 во весь рост; пол как раньше.
        let world = |x: i32, y: i32, _: i32| y < 10 || x >= 12;
        let mut p = Player::new([10.5, 10.0, 0.5]);
        for _ in 0..100 {
            p.step(0.02, [4.0, 4.0], false, world);
        }
        assert!(p.pos[0] < 12.0 - PLAYER_WIDTH / 2.0 + 0.01, "упёрся в стену");
        assert!(p.pos[2] > 4.0, "вдоль стены скользит");
    }
}
