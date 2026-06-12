//! Население мира: спавн/деспавн и тик мобов вокруг игрока.
//!
//! До мультиплеера (M6) мобы — клиентская симуляция; сервер заберёт этот
//! же код из /core, здесь останется только рисование.

use kb_core::{splitmix64, Mob, MobKind};

use crate::{math, world::World};

/// Потолок населения и радиусы жизни.
const MAX_MOBS: usize = 12;
const SPAWN_NEAR: f32 = 24.0;
const SPAWN_FAR: f32 = 40.0;
const DESPAWN: f32 = 64.0;
/// Пауза между попытками спавна, секунд.
const SPAWN_EVERY: f32 = 2.0;

pub struct Mobs {
    list: Vec<Mob>,
    spawn_timer: f32,
    /// Счётчик спавнов — источник сидов для новых мобов.
    counter: u64,
}

/// Часть тела для отрисовки: модельная матрица + слой кожи.
pub type Part = (math::Mat4, u32);

impl Mobs {
    pub fn new() -> Self {
        Self { list: Vec::new(), spawn_timer: 0.0, counter: 0 }
    }

    /// Тик населения. Возвращает суммарный урон игроку.
    pub fn tick(&mut self, dt: f32, player: [f32; 3], night: bool, world: &mut World) -> i8 {
        let mut damage = 0i8;
        for mob in &mut self.list {
            damage =
                damage.saturating_add(mob.step(dt, player, |x, y, z| {
                    world.block([x, y, z]).solid()
                }));
        }
        self.list.retain(|m| {
            let (dx, dz) = (m.body.pos[0] - player[0], m.body.pos[2] - player[2]);
            m.hp > 0 && dx * dx + dz * dz < DESPAWN * DESPAWN
        });

        // Спавн по освещённости (§7): днём — свиньи, ночью — зомби.
        // Кольцо за туманом не нужно: 24..40 блоков, на поверхности.
        self.spawn_timer += dt;
        if self.spawn_timer >= SPAWN_EVERY && self.list.len() < MAX_MOBS {
            self.spawn_timer = 0.0;
            self.counter += 1;
            let roll = splitmix64(world.seed ^ self.counter.wrapping_mul(0x9E37));
            let angle = (roll & 0xFFFF) as f32 / 65536.0 * core::f32::consts::TAU;
            let radius = SPAWN_NEAR + (roll >> 16 & 0xFF) as f32 / 255.0 * (SPAWN_FAR - SPAWN_NEAR);
            let (wx, wz) = (
                player[0] + angle.sin() * radius,
                player[2] + angle.cos() * radius,
            );
            let ground = kb_worldgen::height(world.seed, wx as i32, wz as i32) as f32;
            let kind = if night { MobKind::Zombie } else { MobKind::Pig };
            self.list.push(Mob::new(kind, [wx, ground + 1.0, wz], roll));
        }
        damage
    }

    /// Каждый моб — два куба: тело и голова, повёрнутые по курсу.
    pub fn parts(&self) -> Vec<Part> {
        self.list
            .iter()
            .flat_map(|m| {
                let p = m.body.pos;
                let (body, head, layer) = match m.kind {
                    MobKind::Pig => (
                        math::mob_part(p, m.yaw, [0.6, 0.55, 0.9], 0.45, 0.0),
                        math::mob_part(p, m.yaw, [0.45, 0.45, 0.45], 0.75, 0.55),
                        kb_materials::PIG_LAYER,
                    ),
                    MobKind::Zombie => (
                        math::mob_part(p, m.yaw, [0.55, 1.1, 0.35], 0.75, 0.0),
                        math::mob_part(p, m.yaw, [0.5, 0.5, 0.5], 1.55, 0.0),
                        kb_materials::ZOMBIE_LAYER,
                    ),
                };
                [(body, layer), (head, layer)]
            })
            .collect()
    }
}
