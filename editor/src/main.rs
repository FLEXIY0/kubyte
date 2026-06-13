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
use egui::{Color32, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2};
use kb_materials::{
    bake, variant_seed, Descriptor, Overlay, Pattern, AUTO, AUTO_PATTERN, TEX_SIZE, TRANSPARENT,
};

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
    /// Текущая кисть холста: 0..3 — индекс палитры, AUTO, TRANSPARENT.
    brush: u8,
    /// Референс из KB_REF, ждущий первого кадра (нужен ctx для текстуры).
    pending: Option<String>,
    /// История для отмены (Ctrl+Z): состояние блока ДО серии правок.
    undo_stack: Vec<(usize, Descriptor)>,
    /// Снимок начала текущей серии правок; коммитится в стек, когда
    /// взаимодействие улеглось — так непрерывная правка = один шаг отмены.
    undo_pending: Option<(usize, Descriptor)>,
    /// «Точка возврата» — состояние, которое игрок установил (референс или
    /// выбор блока); кнопка сброса возвращает сюда.
    baseline: Descriptor,
    /// Растущий сид перетасовки — каждый клик даёт новую раскладку.
    shuffle_seed: u64,
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
            brush: AUTO,
            // KB_REF — отладочная автозагрузка референса на старте.
            pending: std::env::var("KB_REF").ok(),
            undo_stack: Vec::new(),
            undo_pending: None,
            baseline: kb_materials::STONE,
            shuffle_seed: 0,
        }
    }

    /// Перетасовка паттерна: обмен соседних ячеек. Множество цветов и
    /// дырок сохраняется (структура остаётся), но раскладка иная — это
    /// «чуть перемешать», а не превратить в шум.
    fn shuffle(&mut self) {
        self.shuffle_seed = self.shuffle_seed.wrapping_add(1);
        let mut s = kb_core::splitmix64(self.shuffle_seed);
        let mut rng = || {
            s = kb_core::splitmix64(s);
            s
        };
        // Сила одноразовой перетасовки берётся из того же ползунка, что и
        // разброс между блоками: одна «ручка интенсивности».
        let swaps = self.blocks[self.sel].desc.pattern_jitter as u32 * 4 + 12;
        let pat = &mut self.blocks[self.sel].desc.pattern;
        for _ in 0..swaps {
            let i = (rng() % 256) as usize;
            let (x, y) = (i % TEX_SIZE, i / TEX_SIZE);
            let (nx, ny) = match rng() % 4 {
                0 => (x + 1, y),
                1 => (x.wrapping_sub(1), y),
                2 => (x, y + 1),
                _ => (x, y.wrapping_sub(1)),
            };
            if nx < TEX_SIZE && ny < TEX_SIZE {
                pat.swap(i, ny * TEX_SIZE + nx);
            }
        }
        self.dirty = true;
    }

    /// Отмена: возвращает блок к состоянию до последней серии правок.
    fn undo(&mut self) {
        if let Some((idx, desc)) = self.undo_stack.pop() {
            self.sel = idx;
            self.blocks[idx].desc = desc;
            self.undo_pending = None;
            self.dirty = true;
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
    /// Из картинки берётся палитра + ПАТТЕРН (квантованная структура) и
    /// прозрачность — так результат «ближе к исходнику», а не чистый шум.
    fn try_load(&mut self, ctx: &egui::Context, path: &str) {
        match image::open(path) {
            Ok(img) => {
                // К нашему разрешению: и палитра, и рисунок берутся в масштабе тайла.
                let small = image::imageops::resize(
                    &img.to_rgba8(),
                    16,
                    16,
                    image::imageops::FilterType::Triangle,
                );
                let desc = analyze(&small);
                self.blocks[self.sel].desc = desc;
                self.baseline = desc; // референс становится точкой возврата
                // Превью референса — апскейл его же 16×16, чтобы видеть источник.
                let mut rgba = Vec::with_capacity(16 * 16 * 4);
                for p in small.pixels() {
                    rgba.extend(p.0);
                }
                let cimg = egui::ColorImage::from_rgba_unmultiplied([16, 16], &rgba);
                self.reference =
                    Some(ctx.load_texture("reference", cimg, TextureOptions::NEAREST));
                self.dirty = true;
                self.status = format!("референс → палитра + рисунок: {path}");
            }
            Err(e) => self.status = format!("не открыть {path}: {e}"),
        }
    }
}

impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Отложенный референс из KB_REF (нужен был ctx).
        if let Some(p) = self.pending.take() {
            self.try_load(ctx, &p);
        }
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

        // Отмена до съёма «before», чтобы её правка не попала в историю.
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            self.undo();
        }

        if self.dirty {
            self.rebuild(ctx);
            self.dirty = false;
        }

        // Снимок состояния на входе в кадр — для коалесинга истории.
        let before = (self.sel, self.blocks[self.sel].desc);

        self.left_panel(ctx);
        self.bottom_panel(ctx);
        self.central_panel(ctx);

        // Коалесинг: непрерывная правка (перетаскивание слайдера/мазок по
        // холсту) — это много кадров с изменениями, но один шаг отмены.
        // Снимок начала серии запоминаем один раз; коммитим, когда правки
        // прекратились и мышь отпущена.
        let changed = before.0 == self.sel && before.1 != self.blocks[self.sel].desc;
        if changed && self.undo_pending.is_none() {
            self.undo_pending = Some(before);
        }
        let interacting = ctx.input(|i| i.pointer.any_down());
        if !changed && !interacting {
            if let Some(p) = self.undo_pending.take() {
                self.undo_stack.push(p);
                // История не бесконечна: держим последние 100 шагов.
                if self.undo_stack.len() > 100 {
                    self.undo_stack.remove(0);
                }
            }
        }
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
                    // Точка возврата — состояние блока на момент выбора.
                    self.baseline = self.blocks[i].desc;
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
                .add(egui::Slider::new(&mut desc.spread, 0..=64).text("разброс яркости вариантов"))
                .changed();
            changed |= ui
                .add(
                    egui::Slider::new(&mut desc.pattern_jitter, 0..=64)
                        .text("разброс структуры между блоками"),
                )
                .changed();

            ui.separator();
            changed |= overlay_ui(ui, &mut desc.overlay);

            if changed {
                self.dirty = true;
            }

            ui.separator();
            ui.columns(2, |cols| {
                cols[0].label("Холст 16×16 — рисуй паттерн (§5):");
                self.canvas(&mut cols[0]);
                cols[1].label("Превью на блоке + 8 вариантов:");
                self.preview(&mut cols[1]);
            });
        });
    }

    /// Холст индексов палитры (§5): кисть красит ячейку фиксированным
    /// цветом, AUTO (шум) или прозрачностью. Это редактор паттерна, а не
    /// пиксель-арт: поверх фиксированных ячеек генератор всё равно кладёт
    /// зерно и тон.
    fn canvas(&mut self, ui: &mut egui::Ui) {
        // Линейка кистей: 4 цвета палитры + AUTO + прозрачность.
        let palette = self.blocks[self.sel].desc.palette;
        ui.horizontal(|ui| {
            for (i, c) in palette.iter().enumerate() {
                let on = self.brush == i as u8;
                if ui
                    .add(egui::Button::new(if on { "●" } else { " " }).fill(rgb(*c)))
                    .clicked()
                {
                    self.brush = i as u8;
                }
            }
            if ui.selectable_label(self.brush == AUTO, "шум").clicked() {
                self.brush = AUTO;
            }
            if ui.selectable_label(self.brush == TRANSPARENT, "✕ дыра").clicked() {
                self.brush = TRANSPARENT;
            }
        });

        // Сетка 16×16; рисуем и обрабатываем мазок одним проходом.
        let cell = 15.0;
        let side = cell * TEX_SIZE as f32;
        let (resp, painter) = ui.allocate_painter(Vec2::splat(side), Sense::drag());
        let origin = resp.rect.min;
        let pat = &mut self.blocks[self.sel].desc.pattern;
        for gy in 0..TEX_SIZE {
            for gx in 0..TEX_SIZE {
                let r = Rect::from_min_size(
                    origin + Vec2::new(gx as f32 * cell, gy as f32 * cell),
                    Vec2::splat(cell),
                );
                match pat[gy * TEX_SIZE + gx] {
                    // Прозрачная — шахматка (общепринятый знак).
                    TRANSPARENT => {
                        painter.rect_filled(r, 0.0, Color32::from_gray(70));
                        let h = Vec2::splat(cell * 0.5);
                        painter.rect_filled(Rect::from_min_size(r.min, h), 0.0, Color32::from_gray(120));
                        painter.rect_filled(Rect::from_min_size(r.center(), h), 0.0, Color32::from_gray(120));
                    }
                    // AUTO — тёмно-серая «процедурная» ячейка.
                    AUTO => {
                        painter.rect_filled(r, 0.0, Color32::from_gray(48));
                    }
                    // Фиксированный индекс — соответствующий цвет палитры.
                    i => {
                        painter.rect_filled(r, 0.0, rgb(palette[i as usize]));
                    }
                }
            }
        }
        // Рисование: позиция мыши → ячейка → кисть.
        if resp.dragged() || resp.is_pointer_button_down_on() {
            if let Some(p) = resp.interact_pointer_pos() {
                let gx = ((p.x - origin.x) / cell) as i32;
                let gy = ((p.y - origin.y) / cell) as i32;
                if (0..16).contains(&gx) && (0..16).contains(&gy) {
                    pat[gy as usize * TEX_SIZE + gx as usize] = self.brush;
                    self.dirty = true;
                }
            }
        }
        ui.horizontal(|ui| {
            if ui.button("⟲ перетасовать").clicked() {
                self.shuffle();
            }
            if ui.button("↺ сброс к референсу").clicked() {
                self.blocks[self.sel].desc = self.baseline;
                self.dirty = true;
            }
        });
        ui.horizontal(|ui| {
            if ui.button("весь холст → шум").clicked() {
                self.blocks[self.sel].desc.pattern = AUTO_PATTERN;
                self.dirty = true;
            }
            if ui
                .add_enabled(!self.undo_stack.is_empty(), egui::Button::new("⮌ отмена (Ctrl+Z)"))
                .clicked()
            {
                self.undo();
            }
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

fn rgb([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Снимает с картинки СТАТИСТИКУ стиля и строит наш дескриптор:
/// палитру по квартилям яркости + ПАТТЕРН (каждый пиксель → ближайший
/// индекс палитры, прозрачные → дырки). Никаких пикселей источника в
/// результате — только индексы и числа (§5, §10).
fn analyze(img: &image::RgbaImage) -> Descriptor {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let grid: Vec<[u8; 4]> = img.pixels().map(|p| p.0).collect();
    let luma = |c: [u8; 3]| 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;

    // Палитра — средний цвет каждого яркостного квартиля непрозрачных
    // пикселей (тёмный → светлый).
    let mut opaque: Vec<[u8; 3]> =
        grid.iter().filter(|p| p[3] >= 128).map(|p| [p[0], p[1], p[2]]).collect();
    if opaque.is_empty() {
        opaque.push([128, 128, 128]);
    }
    opaque.sort_by(|&a, &b| luma(a).total_cmp(&luma(b)));
    let mut palette = [[0u8; 3]; 4];
    let m = opaque.len();
    for (q, slot) in palette.iter_mut().enumerate() {
        let seg = &opaque[q * m / 4..((q + 1) * m / 4).max(q * m / 4 + 1)];
        for ch in 0..3 {
            let sum: u32 = seg.iter().map(|c| c[ch] as u32).sum();
            slot[ch] = (sum / seg.len() as u32) as u8;
        }
    }

    // Паттерн: каждый пиксель → ближайший индекс палитры; прозрачный →
    // дырка. Это и есть «структура исходника», а не чистый шум (§5).
    let nearest = |c: [u8; 3]| -> u8 {
        (0..4)
            .min_by_key(|&i| {
                let p = palette[i];
                (0..3).map(|k| (c[k] as i32 - p[k] as i32).pow(2)).sum::<i32>()
            })
            .unwrap() as u8
    };
    let mut pattern = AUTO_PATTERN;
    for (i, p) in grid.iter().enumerate().take(pattern.len()) {
        pattern[i] = if p[3] < 128 { TRANSPARENT } else { nearest([p[0], p[1], p[2]]) };
    }

    // Зерно — средний контраст соседей по горизонтали → амплитуда вариации.
    let mut grain = 0.0;
    let mut cnt = 0u32;
    for y in 0..h {
        for x in 0..w - 1 {
            grain += (luma([grid[y * w + x][0], grid[y * w + x][1], grid[y * w + x][2]])
                - luma([grid[y * w + x + 1][0], grid[y * w + x + 1][1], grid[y * w + x + 1][2]]))
            .abs();
            cnt += 1;
        }
    }
    let variation = (grain / cnt.max(1) as f32).round().clamp(0.0, MAX_VARIATION as f32) as u8;

    // Оверлей не нужен: всю структуру несёт паттерн (оверлей действует
    // только на AUTO-ячейки, которых после анализа нет). pattern_jitter
    // по умолчанию ненулевой — чтобы блоки в мире сразу различались.
    Descriptor {
        palette,
        cell_log2: 0,
        variation,
        spread: 12,
        overlay: Overlay::None,
        pattern,
        pattern_jitter: 16,
    }
}

/// Генерирует запись таблицы как валидный Rust (§12: выход — код).
fn gen_code(name: &str, d: &Descriptor) -> String {
    format!(
        "pub const {}: Descriptor = Descriptor {{\n    palette: {:?},\n    cell_log2: {},\n    variation: {},\n    spread: {},\n    overlay: {},\n    pattern: {},\n    pattern_jitter: {},\n}};\n\n",
        name.to_uppercase(),
        d.palette,
        d.cell_log2,
        d.variation,
        d.spread,
        overlay_code(&d.overlay),
        pattern_code(&d.pattern),
        d.pattern_jitter,
    )
}

/// Паттерн в код: всё-AUTO → ссылка на готовую константу, иначе массив
/// по 16 чисел в ряд (§5: «битмап-паттерн прямо в коде»).
fn pattern_code(p: &Pattern) -> String {
    if p.iter().all(|&c| c == AUTO) {
        return "AUTO_PATTERN".into();
    }
    let mut s = String::from("[\n");
    for row in p.chunks(TEX_SIZE) {
        s.push_str("        ");
        for &c in row {
            s.push_str(&format!("{c}, "));
        }
        s.push('\n');
    }
    s.push_str("    ]");
    s
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
