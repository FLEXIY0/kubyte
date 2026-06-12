//! Замер против бюджетов §9: время генерации чанка (< 1 мс) и память
//! палитро-чанков на дистанции 8 (вклад в лимит 10 МБ).
//!
//! Запуск: `cargo run --release -p kb-worldgen --example bench`

use std::time::Instant;

fn main() {
    const R: i32 = 8;
    let seed = 42;

    let t0 = Instant::now();
    let mut bytes = 0usize;
    let mut chunks = 0u32;
    for cz in -R..=R {
        for cx in -R..=R {
            bytes += kb_worldgen::generate(seed, cx, cz).heap_bytes();
            chunks += 1;
        }
    }
    let per_chunk = t0.elapsed() / chunks;

    println!("чанков: {chunks} (дистанция {R})");
    println!("генерация: {per_chunk:?}/чанк  (бюджет < 1 мс)");
    println!(
        "память чанков: {} КиБ всего, {} байт/чанк (бюджет §9: ≤ 10 МиБ всё вместе)",
        bytes / 1024,
        bytes / chunks as usize
    );
    assert!(per_chunk.as_micros() < 1000, "бюджет генерации превышен");
}
