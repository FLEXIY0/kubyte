//! Превью текстур: пишет PPM-файлы в /tmp для визуальной проверки.
//! `cargo run -p kb-materials --example preview`

use kb_materials::{bake, TEXTURES, TEX_SIZE};

fn main() {
    let names = ["stone", "dirt", "grass_top", "grass_side"];
    for (name, desc) in names.iter().zip(TEXTURES) {
        let px = bake(desc, 0);
        let mut ppm = format!("P3\n{TEX_SIZE} {TEX_SIZE}\n255\n");
        for p in px.chunks(4) {
            ppm += &format!("{} {} {}\n", p[0], p[1], p[2]);
        }
        std::fs::write(format!("/tmp/tex_{name}.ppm"), ppm).unwrap();
    }
    println!("ok");
}
