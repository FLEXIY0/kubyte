//! Мобы: минимальный набор и простейший ИИ (§7 — агро по дистанции,
//! блуждание). Симуляция — в ядре; рендер только рисует кубы по позициям.
//!
//! Свинья — мирная, бродит. Зомби — ночной, идёт к игроку по прямой,
//! прыгает на препятствиях, бьёт вплотную. Никаких скриптов и
//! джампскейров (§14): угроза — медленная и читаемая.

use crate::physics::Player;
use crate::splitmix64;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MobKind {
    Pig,
    Zombie,
}

impl MobKind {
    pub const fn max_hp(self) -> i8 {
        match self {
            MobKind::Pig => 10,
            MobKind::Zombie => 20,
        }
    }

    const fn speed(self) -> f32 {
        match self {
            MobKind::Pig => 1.4,
            MobKind::Zombie => 2.6,
        }
    }
}

/// Моб = та же физика, что у игрока (один код коллизий на всех),
/// плюс крошечный мозг.
pub struct Mob {
    pub kind: MobKind,
    pub body: Player,
    pub yaw: f32,
    pub hp: i8,
    /// Личный сид — источник всей «случайности» поведения: блуждание
    /// детерминировано от (сид, время), состояния-таймеры не нужны.
    seed: u64,
    /// Время жизни, секунд; часы для смены направления и кулдауна атаки.
    age: f32,
    last_attack: f32,
    /// Упёрся в стену на прошлом шаге — пора прыгать.
    blocked: bool,
}

/// Дистанция, с которой зомби видит игрока.
const AGGRO: f32 = 16.0;
/// Дистанция удара и его цена.
const ATTACK_RANGE: f32 = 1.7;
const ATTACK_DAMAGE: i8 = 2;
const ATTACK_COOLDOWN: f32 = 1.0;

impl Mob {
    pub fn new(kind: MobKind, pos: [f32; 3], seed: u64) -> Self {
        Self {
            kind,
            body: Player::new(pos),
            yaw: 0.0,
            hp: kind.max_hp(),
            seed,
            age: 0.0,
            last_attack: -ATTACK_COOLDOWN,
            blocked: false,
        }
    }

    /// Шаг ИИ + физики. Возвращает урон игроку за этот тик.
    pub fn step(
        &mut self,
        dt: f32,
        player: [f32; 3],
        mut solid: impl FnMut(i32, i32, i32) -> bool,
    ) -> i8 {
        self.age += dt;
        let dx = player[0] - self.body.pos[0];
        let dz = player[2] - self.body.pos[2];
        let dist = libm::sqrtf(dx * dx + dz * dz);

        let chasing = self.kind == MobKind::Zombie && dist < AGGRO;
        let (dir, pace) = if chasing {
            // Прямо на игрока, без поиска пути: читаемо и в духе эпохи.
            (libm::atan2f(dx, dz), 1.0)
        } else {
            // Блуждание: направление — функция (сид, номер 3-секундного
            // окна); каждое четвёртое окно мирно стоим.
            let window = splitmix64(self.seed ^ (self.age / 3.0) as u64);
            let stand = window & 3 == 0;
            (
                (window >> 8 & 0xFFFF) as f32 / 65536.0 * core::f32::consts::TAU,
                if stand { 0.0 } else { 0.5 },
            )
        };
        self.yaw = dir;

        let v = self.kind.speed() * pace;
        let wish = [libm::sinf(dir) * v, libm::cosf(dir) * v];
        let before = [self.body.pos[0], self.body.pos[2]];
        self.body.step(dt, wish, self.blocked, &mut solid);
        // Хотели идти, но не сдвинулись → на следующем шаге прыжок.
        self.blocked = pace > 0.0
            && (self.body.pos[0] - before[0]).abs() + (self.body.pos[2] - before[1]).abs()
                < v * dt * 0.1;

        if chasing && dist < ATTACK_RANGE && self.age - self.last_attack >= ATTACK_COOLDOWN {
            self.last_attack = self.age;
            return ATTACK_DAMAGE;
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor10(_: i32, y: i32, _: i32) -> bool {
        y < 10
    }

    #[test]
    fn zombie_walks_to_player_and_bites() {
        let mut z = Mob::new(MobKind::Zombie, [0.5, 10.0, 0.5], 7);
        let player = [8.5, 10.0, 0.5];
        let mut hurt = 0i32;
        for _ in 0..400 {
            hurt += z.step(0.05, player, floor10) as i32;
        }
        assert!((z.body.pos[0] - player[0]).abs() < ATTACK_RANGE, "дошёл");
        assert!(hurt > 0, "укусил");
    }

    #[test]
    fn pig_ignores_player() {
        let mut p = Mob::new(MobKind::Pig, [0.5, 10.0, 0.5], 7);
        let mut hurt = 0i32;
        for _ in 0..400 {
            hurt += p.step(0.05, [1.5, 10.0, 0.5], floor10) as i32;
        }
        assert_eq!(hurt, 0);
    }
}
