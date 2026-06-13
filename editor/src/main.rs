//! kb-editor — встроенный редактор материалов (§12 ТЗ).
//!
//! Это редактор ДЕСКРИПТОРОВ, а не пиксель-арт-редактор. Превью считается
//! тем же `kb_materials::bake`, что и игра, — расхождение невозможно по
//! построению. Выход — код, не файлы (§12): кнопка кладёт в буфер готовую
//! запись таблицы.
//!
//! Референс из реальной жизни можно перетащить в окно или указать путь —
//! из него снимается СТАТИСТИКА (палитра по квартилям яркости, контраст
//! зерна, доля тёмных вкраплений) и собирается наш процедурный дескриптор.
//! Сам файл-референс никуда не сохраняется и в дистрибутив не попадает
//! (§4, §10): в репозиторий едут только числа.

use eframe::egui;
use egui::{Color32, Pos2, Rect, TextureHandle, TextureOptions, Vec2};
use kb_materials::{bake, variant_seed, Descriptor, Overlay, TEX_SIZE};

const VARIANT_PREVIEWS: usize = 8; // §12: «сетка из 8–16 вариантов»
const MAX_VARIATION: u8 = 38; // инвариант §5: вариация ≤ ±15%

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu, // §12: egui поверх wgpu
        viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native(
        "kb — редактор материалов",
        options,
        Box::new(|_cc| Ok(Box::new(Editor::new()))),
    )
}

/// Авторский блок: имя для таблицы + его дескриптор.
struct Block {
    name: String,
    desc: Descriptor,
}

struct Editor {
    blocks: Vec<Block>,
    sel: usize,
    /// 8 текстур-вариантов выбранного блока; пересобираются при правке.
    tiles: Vec<TextureHandle>,
    dirty: bool,
    /// Превью загруженного референса (только для глаз, в код не идёт).
    reference: Option<TextureHandle>,
    path_input: String,
    status: String,
}

impl Editor {
    fn new() -> Self {
        // Стартовая таблица — текущие материалы игры: можно сразу крутить
        // существующие или добавлять рядом новые.
        let blocks = [
            ("STONE", kb_materials::STONE),
            ("DIRT", kb_materials::DIRT),
            ("GRASS_TOP", kb_materials::GRASS_TOP),
            ("GRASS_SIDE", kb_materials::GRASS_SIDE),
            ("WOOD", kb_materials::WOOD),
            ("LEAVES", kb_materials::LEAVES),
            ("LAMP", kb_materials::LAMP),
        ]
        .into_iter()
        .map(|(name, desc)| Block { name: name.to_string(), desc })
        .collect();

        Self {
            blocks,
            sel: 0,
            tiles: Vec::new(),
            dirty: true,
            reference: None,
            path_input: String::new(),
            status: "перетащи PNG в окно или укажи путь к референсу".into(),
        }
    }

    /// Пересобирает 8 вариантов текущего блока через тот же генератор,
    /// что и игра. Слой фиксирован — варианты различаются только сидом.
    fn rebuild(&mut self, ctx: &egui::Context) {
        let desc = self.blocks[self.sel].desc;
        self.tiles.clear();
        for v in 0..VARIANT_PREVIEWS {
            let px = bake(&desc, variant_seed(0, v));
            let img = egui::ColorImage::from_rgba_unmultiplied([TEX_SIZE, TEX_SIZE], &px);
            self.tiles
                .push(ctx.load_texture(format!("tile{v}"), img, TextureOptions::NEAREST));
        }
    }

    /// Загрузка референса: drag-and-drop отдаёт путь, текстовое поле — тоже.
    fn try_load(&mut self, ctx: &egui::Context, path: &str) {
        match image::open(path) {
            Ok(img) => {
                // К нашему разрешению: статистика берётся в масштабе тайла.
                let small = img.to_rgb8();
                let small =
                    image::imageops::resize(&small, 16, 16, image::imageops::FilterType::Triangle);
                self.blocks[self.sel].desc = analyze(&small);
                // Превью референса — апскейл его же 16×16, чтобы видеть источник.
                let mut rgba = Vec::with_capacity(16 * 16 * 4);
                for p in small.pixels() {
                    rgba.extend([p[0], p[1], p[2], 255]);
                }
                let cimg = egui::ColorImage::from_rgba_unmultiplied([16, 16], &rgba);
                self.reference =
                    Some(ctx.load_texture("reference", cimg, TextureOptions::NEAREST));
                self.dirty = true;
                self.status = format!("референс проанализирован: {path}");
            }
            Err(e) => self.status = format!("не открыть {path}: {e}"),
        }
    }
}

impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Перетащенные файлы: берём первый с путём.
        let dropped: Option<String> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .find_map(|f| f.path.as_ref().map(|p| p.display().to_string()))
        });
        if let Some(p) = dropped {
            self.try_load(ctx, &p);
        }

        if self.dirty {
            self.rebuild(ctx);
            self.dirty = false;
        }

        self.left_panel(ctx);
        self.bottom_panel(ctx);
        self.central_panel(ctx);
    }
}

impl Editor {
    fn left_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("blocks").default_width(180.0).show(ctx, |ui| {
            ui.heading("Блоки");
            for i in 0..self.blocks.len() {
                if ui
                    .selectable_label(self.sel == i, &self.blocks[i].name)
                    .clicked()
                {
                    self.sel = i;
                    self.dirty = true;
                }
            }
            ui.separator();
            if ui.button("+ новый блок").clicked() {
                self.blocks.push(Block {
                    name: format!("BLOCK_{}", self.blocks.len()),
                    desc: kb_materials::STONE,
                });
                self.sel = self.blocks.len() - 1;
                self.dirty = true;
            }
            ui.separator();
            ui.label("Референс (drag PNG или путь):");
            ui.text_edit_singleline(&mut self.path_input);
            if ui.button("Загрузить и проанализировать").clicked() {
                let p = self.path_input.clone();
                self.try_load(ctx, &p);
            }
            if let Some(r) = &self.reference {
                ui.label("источник:");
                ui.add(egui::Image::from_texture(egui::load::SizedTexture::new(
                    r.id(),
                    Vec2::splat(96.0),
                )));
            }
            ui.separator();
            ui.label(&self.status);
        });
    }

    fn central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("имя:");
                ui.text_edit_singleline(&mut self.blocks[self.sel].name);
            });
            ui.separator();

            let desc = &mut self.blocks[self.sel].desc;

            ui.label("Палитра (тёмный → светлый):");
            ui.horizontal(|ui| {
                for c in desc.palette.iter_mut() {
                    changed |= ui.color_edit_button_srgb(c).changed();
                }
            });

            changed |= ui
                .add(egui::Slider::new(&mut desc.cell_log2, 0..=3).text("масштаб шума (cell_log2)"))
                .changed();
            changed |= ui
                .add(
                    egui::Slider::new(&mut desc.variation, 0..=MAX_VARIATION)
                        .text("зерно (вариация ≤38, §5)"),
                )
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut desc.spread, 0..=64).text("разнообразность вариантов"))
                .changed();

            ui.separator();
            changed |= overlay_ui(ui, &mut desc.overlay);

            if changed {
                self.dirty = true;
            }

            ui.separator();
            ui.label("Превью на блоке + 8 вариантов (тот же генератор, что в игре):");
            self.preview(ui);
        });
    }

    fn preview(&self, ui: &mut egui::Ui) {
        if self.tiles.is_empty() {
            return;
        }
        // Изо-куб из варианта 0: три грани с фейс-шейдингом как в игре.
        let (rect, _) = ui.allocate_exact_size(Vec2::new(160.0, 160.0), egui::Sense::hover());
        iso_cube(ui, rect, self.tiles[0].id());

        // Лента из 8 вариантов — наглядно показывает «контролируемый хаос».
        ui.horizontal_wrapped(|ui| {
            for t in &self.tiles {
                ui.add(egui::Image::from_texture(egui::load::SizedTexture::new(
                    t.id(),
                    Vec2::splat(64.0),
                )));
            }
        });
    }

    fn bottom_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("code").default_height(160.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Код таблицы (§12: источник истины — репозиторий):");
                if ui.button("копировать этот блок").clicked() {
                    let b = &self.blocks[self.sel];
                    ctx.copy_text(gen_code(&b.name, &b.desc));
                }
                if ui.button("копировать всю таблицу").clicked() {
                    let all: String =
                        self.blocks.iter().map(|b| gen_code(&b.name, &b.desc)).collect();
                    ctx.copy_text(all);
                }
            });
            let b = &self.blocks[self.sel];
            let mut code = gen_code(&b.name, &b.desc);
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut code)
                        .code_editor()
                        .desired_width(f32::INFINITY),
                );
            });
        });
    }
}

/// UI выбора и параметров оверлея. Возвращает true при любой правке.
fn overlay_ui(ui: &mut egui::Ui, ov: &mut Overlay) -> bool {
    let mut changed = false;
    let tag = match ov {
        Overlay::None => 0,
        Overlay::Speckle { .. } => 1,
        Overlay::Stripes { .. } => 2,
        Overlay::TopBand { .. } => 3,
    };
    let mut new_tag = tag;
    egui::ComboBox::from_label("оверлей")
        .selected_text(["нет", "крапинки", "полосы", "кромка сверху"][tag])
        .show_ui(ui, |ui| {
            for (i, name) in ["нет", "крапинки", "полосы", "кромка сверху"].iter().enumerate() {
                ui.selectable_value(&mut new_tag, i, *name);
            }
        });
    if new_tag != tag {
        // Смена типа — конструируем дефолт выбранного оверлея.
        *ov = match new_tag {
            1 => Overlay::Speckle { color: [80, 80, 80], chance: 40 },
            2 => Overlay::Stripes { color: [60, 45, 28], period: 3 },
            3 => Overlay::TopBand { palette: [[100, 160, 60], [120, 180, 75]], min_depth: 2, max_depth: 4 },
            _ => Overlay::None,
        };
        changed = true;
    }
    match ov {
        Overlay::None => {}
        Overlay::Speckle { color, chance } => {
            ui.horizontal(|ui| {
                changed |= ui.color_edit_button_srgb(color).changed();
                changed |= ui.add(egui::Slider::new(chance, 0..=255).text("частота")).changed();
            });
        }
        Overlay::Stripes { color, period } => {
            ui.horizontal(|ui| {
                changed |= ui.color_edit_button_srgb(color).changed();
                changed |= ui.add(egui::Slider::new(period, 1..=8).text("период")).changed();
            });
        }
        Overlay::TopBand { palette, min_depth, max_depth } => {
            ui.horizontal(|ui| {
                for c in palette.iter_mut() {
                    changed |= ui.color_edit_button_srgb(c).changed();
                }
                changed |= ui.add(egui::Slider::new(min_depth, 1..=8).text("мин")).changed();
                changed |= ui.add(egui::Slider::new(max_depth, 1..=8).text("макс")).changed();
            });
            if max_depth < min_depth {
                *max_depth = *min_depth;
            }
        }
    }
    changed
}

/// Изометрический куб: три грани одной текстурой с фейс-шейдингом игры
/// (верх ярче, бока темнее). Тинт — серый множитель egui по вершинам.
fn iso_cube(ui: &egui::Ui, rect: Rect, tex: egui::TextureId) {
    let c = rect.center();
    let (w, hh) = (52.0, 30.0); // полуширина ромба, полувысота
    let v = 60.0; // высота вертикальной грани
    let top = c.y - v / 2.0;

    let mut mesh = egui::Mesh::with_texture(tex);
    let uv = [Pos2::new(0.0, 0.0), Pos2::new(1.0, 0.0), Pos2::new(1.0, 1.0), Pos2::new(0.0, 1.0)];
    let mut quad = |p: [Pos2; 4], shade: u8| {
        let tint = Color32::from_gray(shade);
        let base = mesh.vertices.len() as u32;
        for (pos, uv) in p.iter().zip(uv) {
            mesh.vertices.push(egui::epaint::Vertex { pos: *pos, uv, color: tint });
        }
        mesh.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    // Верхняя грань (ромб), затем левая и правая.
    quad(
        [
            Pos2::new(c.x, top - hh),
            Pos2::new(c.x + w, top),
            Pos2::new(c.x, top + hh),
            Pos2::new(c.x - w, top),
        ],
        255,
    );
    quad(
        [
            Pos2::new(c.x - w, top),
            Pos2::new(c.x, top + hh),
            Pos2::new(c.x, top + hh + v),
            Pos2::new(c.x - w, top + v),
        ],
        165,
    );
    quad(
        [
            Pos2::new(c.x, top + hh),
            Pos2::new(c.x + w, top),
            Pos2::new(c.x + w, top + v),
            Pos2::new(c.x, top + hh + v),
        ],
        205,
    );
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// Снимает с картинки СТАТИСТИКУ стиля и строит наш дескриптор.
/// Никаких пикселей источника в результате — только числа (§10).
fn analyze(img: &image::RgbImage) -> Descriptor {
    let px: Vec<[u8; 3]> = img.pixels().map(|p| [p[0], p[1], p[2]]).collect();
    let n = px.len();
    let luma = |c: [u8; 3]| 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;

    // Палитра — средний цвет каждого яркостного квартиля (тёмный → светлый).
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| luma(px[a]).total_cmp(&luma(px[b])));
    let mut palette = [[0u8; 3]; 4];
    for (q, slot) in palette.iter_mut().enumerate() {
        let seg = &order[q * n / 4..(q + 1) * n / 4];
        for ch in 0..3 {
            let sum: u32 = seg.iter().map(|&i| px[i][ch] as u32).sum();
            slot[ch] = (sum / seg.len().max(1) as u32) as u8;
        }
    }

    // Зерно — средний контраст соседей по горизонтали → амплитуда вариации.
    let w = img.width() as usize;
    let h = img.height() as usize;
    let mut grain = 0.0;
    let mut cnt = 0u32;
    for y in 0..h {
        for x in 0..w - 1 {
            grain += (luma(px[y * w + x]) - luma(px[y * w + x + 1])).abs();
            cnt += 1;
        }
    }
    let variation = (grain / cnt.max(1) as f32).round().clamp(0.0, MAX_VARIATION as f32) as u8;

    // Доля тёмных пикселей → вкрапления (тем же приёмом делаются руды).
    let mean = px.iter().map(|&c| luma(c)).sum::<f32>() / n as f32;
    let dark = px.iter().filter(|&&c| luma(c) < mean * 0.8).count() as f32 / n as f32;
    let overlay = if dark > 0.04 {
        Overlay::Speckle { color: palette[0], chance: ((dark * 256.0) as u32).min(255) as u8 }
    } else {
        Overlay::None
    };

    Descriptor { palette, cell_log2: 0, variation, spread: 16, overlay }
}

/// Генерирует запись таблицы как валидный Rust (§12: выход — код).
fn gen_code(name: &str, d: &Descriptor) -> String {
    format!(
        "pub const {}: Descriptor = Descriptor {{\n    palette: {:?},\n    cell_log2: {},\n    variation: {},\n    spread: {},\n    overlay: {},\n}};\n\n",
        name.to_uppercase(),
        d.palette,
        d.cell_log2,
        d.variation,
        d.spread,
        overlay_code(&d.overlay),
    )
}

fn overlay_code(ov: &Overlay) -> String {
    match ov {
        Overlay::None => "Overlay::None".into(),
        Overlay::Speckle { color, chance } => {
            format!("Overlay::Speckle {{ color: {color:?}, chance: {chance} }}")
        }
        Overlay::Stripes { color, period } => {
            format!("Overlay::Stripes {{ color: {color:?}, period: {period} }}")
        }
        Overlay::TopBand { palette, min_depth, max_depth } => format!(
            "Overlay::TopBand {{ palette: {palette:?}, min_depth: {min_depth}, max_depth: {max_depth} }}"
        ),
    }
}
