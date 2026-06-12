//! kb-render: wgpu-рендер с ретро-пайплайном (§6).
//!
//! Архитектурный инвариант с M0 и до конца проекта: сцена ВСЕГДА рисуется
//! в offscreen-буфер пониженного разрешения и растягивается на экран
//! nearest-блитом. Мир, туман и небо идут через один общий буфер —
//! раздельная пикселизация слоёв запрещена ТЗ.

mod light;
mod math;
mod mesh;
mod world;

use wgpu::util::DeviceExt;

/// Платформе нужен словарь блоков (выбор в хотбаре) без зависимости от ядра.
pub use kb_core::Block;

/// Во сколько раз offscreen-буфер меньше экрана. Целое — пиксели обязаны
/// быть одинаковой ширины (integer scaling, §6). Станет настройкой в M4.
const PIXEL_SCALE: u32 = 3;

const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Сид мира M1. Станет выбором игрока вместе с сейвами (M3).
pub const SEED: u64 = 42;

/// Дальность тумана = край прогруженного мира: дальние чанки растворяются,
/// а не обрезаются. Цвет тумана = цвет неба (см. chunk.wgsl).
const FOG_END: f32 = (world::VIEW_RADIUS * 16) as f32;

/// Максимум чанков в кадре: полный квадрат видимости с запасом.
const MAX_DRAWS: usize = 512;
/// Максимум квадов в одном чанк-меше, на который рассчитан общий
/// индексный буфер (1.5 МиБ GPU). Реальный рельеф даёт порядки меньше.
const MAX_QUADS: usize = 65536;
/// Выравнивание динамических оффсетов uniform-буфера (минимум WebGL2).
const UB_ALIGN: usize = 256;

/// Скорость ходьбы — классическая для жанра.
const WALK_SPEED: f32 = 4.3;
/// Дистанция взаимодействия (ломание/установка), в блоках.
const REACH: f32 = 5.0;

/// Полный цикл суток, секунд (10 минут — как в эпоху беты).
const DAY_SECONDS: f32 = 600.0;

/// Цвета времени суток (§6, §14): день — нейтрально-тёплый свет и
/// приглушённое небо; ночь — холодный лунный свет, мир выцветает,
/// ночи темнее ванильных. Всё остальное — интерполяция.
const SUN_DAY: [f32; 4] = [1.0, 0.97, 0.90, 1.0];
const SUN_NIGHT: [f32; 4] = [0.45, 0.55, 0.85, 0.16];
const SKY_DAY: [f32; 3] = [0.35, 0.52, 0.74];
const SKY_NIGHT: [f32; 3] = [0.010, 0.014, 0.032];

/// Положение солнца → факторы дня. Возвращает (sun rgba, sky rgb):
/// плавные сумерки через smoothstep по синусу суточной фазы.
fn daylight(time: f32) -> ([f32; 4], [f32; 3]) {
    let phase = (time / DAY_SECONDS) * core::f32::consts::TAU;
    // 1 в полдень, 0 в полночь, рассвет/закат — узкая полоса у горизонта.
    let s = phase.sin();
    let t = ((s + 0.15) / 0.4).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    (
        [
            lerp(SUN_NIGHT[0], SUN_DAY[0]),
            lerp(SUN_NIGHT[1], SUN_DAY[1]),
            lerp(SUN_NIGHT[2], SUN_DAY[2]),
            lerp(SUN_NIGHT[3], SUN_DAY[3]),
        ],
        [
            lerp(SKY_NIGHT[0], SKY_DAY[0]),
            lerp(SKY_NIGHT[1], SKY_DAY[1]),
            lerp(SKY_NIGHT[2], SKY_DAY[2]),
        ],
    )
}

/// Свободная камера. float здесь законен: камера — чисто визуальное
/// состояние клиента, в симуляцию мира не входит (§6).
pub struct Camera {
    pub pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
}

impl Camera {
    const LOOK_SPEED: f32 = 0.0025;
    const FLY_SPEED: f32 = 24.0;

    /// Сырое смещение мыши → поворот. Pitch зажат чуть до зенита,
    /// чтобы матрица вида не вырождалась.
    pub fn look(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * Self::LOOK_SPEED;
        self.pitch = (self.pitch - dy * Self::LOOK_SPEED).clamp(-1.55, 1.55);
    }

    /// Полёт: forward/right — в плоскости взгляда (по yaw), up — мировой.
    pub fn fly(&mut self, forward: f32, right: f32, up: f32, dt: f32) {
        let (s, c) = self.yaw.sin_cos();
        let v = Self::FLY_SPEED * dt;
        self.pos[0] += (-s * forward + c * right) * v;
        self.pos[2] += (-c * forward - s * right) * v;
        self.pos[1] += up * v;
    }

    /// Единичный вектор взгляда.
    fn dir(&self) -> [f32; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        [-sy * cp, sp, -cy * cp]
    }
}

/// Режим передвижения: выживание (гравитация, коллизии) или полёт-носклип.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Mode {
    Walk,
    Fly,
}

/// Offscreen-цель: пересоздаётся при resize, поэтому выделена в свой тип
/// с единственным конструктором — нет способа обновить её наполовину.
struct Offscreen {
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
    /// Bind group блита держит ссылку на color — живут и умирают вместе.
    blit_bind: wgpu::BindGroup,
}

impl Offscreen {
    fn new(
        device: &wgpu::Device,
        surface_size: (u32, u32),
        blit_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) -> Self {
        let size = wgpu::Extent3d {
            width: (surface_size.0 / PIXEL_SCALE).max(1),
            height: (surface_size.1 / PIXEL_SCALE).max(1),
            depth_or_array_layers: 1,
        };
        let tex = |label, format, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let color = tex(
            "offscreen.color",
            OFFSCREEN_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let depth = tex(
            "offscreen.depth",
            DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let blit_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit.bind"),
            layout: blit_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&color),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        Self { color, depth, blit_bind }
    }
}

pub struct Gfx {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    offscreen: Offscreen,
    blit_layout: wgpu::BindGroupLayout,
    nearest: wgpu::Sampler,
    chunk_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    scene_bind: wgpu::BindGroup,
    origin_bind: wgpu::BindGroup,
    camera_buf: wgpu::Buffer,
    origins_buf: wgpu::Buffer,
    quad_indices: wgpu::Buffer,
    world: world::World,
    player: kb_core::Player,
    pub mode: Mode,
    pub camera: Camera,
    /// Игровое время суток, секунд. Стартуем утром.
    time: f32,
}

impl Gfx {
    /// Асинхронная инициализация — единственный async во всём рендере:
    /// так один и тот же код ждёт адаптер и на нативе (block_on),
    /// и в браузере (spawn_local).
    pub async fn new(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        size: (u32, u32),
    ) -> Result<Self, Box<dyn core::error::Error>> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(target)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                // WebGL2-fallback — обязательная платформа (§2): везде живём
                // в его лимитах, чтобы не выращивать рендер, который потом
                // не влезет в браузер.
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await?;

        let mut config = surface
            .get_default_config(&adapter, size.0.max(1), size.1.max(1))
            .ok_or("surface incompatible with adapter")?;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);

        // --- Texture array всех материалов (§5): печётся при старте ------
        let layers = kb_materials::TEXTURES.len() as u32;
        let pixels: Vec<u8> = kb_materials::TEXTURES
            .iter()
            // Сид 0 — фиксированный сид текстур (§5): одинаково у всех.
            .flat_map(|d| kb_materials::bake(d, 0))
            .collect();
        let texture = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("materials.array"),
                size: wgpu::Extent3d {
                    width: kb_materials::TEX_SIZE as u32,
                    height: kb_materials::TEX_SIZE as u32,
                    depth_or_array_layers: layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &pixels,
        );
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor::default());

        // --- Юниформы: камера + смещения чанков (динамический оффсет) ----
        let camera_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: 112, // mat4 + vec4(pos, fog) + vec4 sun + vec4 sky
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let origins_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("chunk.origins"),
            size: (MAX_DRAWS * UB_ALIGN) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Общий индексный буфер квадов: паттерн [0,1,2, 2,1,3] на все меши
        // сразу — чанкам остаются только вершинные буферы.
        let quad_indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad.ib"),
            contents: bytemuck::cast_slice(
                &(0..MAX_QUADS as u32)
                    .flat_map(|q| [0, 1, 2, 2, 1, 3].map(|i| q * 4 + i))
                    .collect::<Vec<u32>>(),
            ),
            usage: wgpu::BufferUsages::INDEX,
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("chunk.wgsl"));
        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene.layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                sampler_entry(2),
            ],
        });
        let origin_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("origin.layout"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX, true)],
        });

        let scene_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene.bind"),
            layout: &scene_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&nearest),
                },
            ],
        });
        let origin_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("origin.bind"),
            layout: &origin_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &origins_buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(16),
                }),
            }],
        });

        let chunk_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("chunk.pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&scene_layout), Some(&origin_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 8, // два u32: позиция/нормаль/слой + свет
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Uint32, 1 => Uint32],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(OFFSCREEN_FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });

        // --- Блит: offscreen → экран -------------------------------------
        let blit_shader = device.create_shader_module(wgpu::include_wgsl!("blit.wgsl"));
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit.layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                sampler_entry(1),
            ],
        });
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit.pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&blit_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(config.format.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });

        let offscreen = Offscreen::new(&device, size, &blit_layout, &nearest);

        // Спавн — на поверхности в начале координат.
        let spawn = [8.5, kb_worldgen::height(SEED, 8, 8) as f32 + 1.0, 8.5];
        let camera = Camera {
            pos: [spawn[0], spawn[1] + kb_core::EYE_HEIGHT, spawn[2]],
            yaw: 0.6,
            pitch: -0.1,
        };

        Ok(Self {
            surface,
            device,
            queue,
            config,
            offscreen,
            blit_layout,
            nearest,
            chunk_pipeline,
            blit_pipeline,
            scene_bind,
            origin_bind,
            camera_buf,
            origins_buf,
            quad_indices,
            world: world::World::new(SEED),
            player: kb_core::Player::new(spawn),
            mode: Mode::Walk,
            camera,
            // Утро; KB_TIME=<сек> — отладочная перемотка суток (native).
            #[cfg(not(target_arch = "wasm32"))]
            time: std::env::var("KB_TIME")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DAY_SECONDS * 0.1),
            #[cfg(target_arch = "wasm32")]
            time: DAY_SECONDS * 0.1,
        })
    }

    /// Шаг геймплея: в Walk — симуляция игрока (§3: физика в /core),
    /// в Fly — носклип-камера. Камера в Walk прибита к глазам игрока.
    pub fn tick(&mut self, dt: f32, (forward, right, up): (f32, f32, f32)) {
        self.time = (self.time + dt) % DAY_SECONDS;
        if self.mode == Mode::Fly {
            return self.camera.fly(forward, right, up, dt);
        }
        let (s, c) = self.camera.yaw.sin_cos();
        let wish = [
            (-s * forward + c * right) * WALK_SPEED,
            (-c * forward - s * right) * WALK_SPEED,
        ];
        let world = &mut self.world;
        self.player
            .step(dt, wish, up > 0.0, |x, y, z| world.block([x, y, z]).solid());
        self.camera.pos = self.player.pos;
        self.camera.pos[1] += kb_core::EYE_HEIGHT;
    }

    /// Переключение полёта; при посадке игрок продолжает с места камеры.
    pub fn toggle_fly(&mut self) {
        self.mode = match self.mode {
            Mode::Fly => {
                self.player = kb_core::Player::new([
                    self.camera.pos[0],
                    self.camera.pos[1] - kb_core::EYE_HEIGHT,
                    self.camera.pos[2],
                ]);
                Mode::Walk
            }
            Mode::Walk => Mode::Fly,
        };
    }

    /// Клик по миру: `place == None` — сломать блок под прицелом,
    /// `Some(b)` — поставить к его грани. Установка в собственный AABB
    /// запрещена — нельзя замуроваться.
    pub fn interact(&mut self, place: Option<kb_core::Block>) {
        let world = &mut self.world;
        let ray = kb_core::raycast(self.camera.pos, self.camera.dir(), REACH, |x, y, z| {
            world.block([x, y, z]).solid()
        });
        let Some((hit, prev)) = ray else { return };
        match place {
            None => self.world.set_block(hit, kb_core::Block::Air),
            Some(b) => {
                let feet = self.player.pos;
                let inside = |i: usize, p: i32| {
                    let half = kb_core::PLAYER_WIDTH / 2.0 + 0.01;
                    let (lo, hi) = match i {
                        1 => (feet[1], feet[1] + kb_core::PLAYER_HEIGHT),
                        _ => (feet[i] - half, feet[i] + half),
                    };
                    (p as f32) < hi && (p + 1) as f32 > lo
                };
                let overlaps_player = self.mode == Mode::Walk
                    && (0..3).all(|i| inside(i, prev[i]));
                if !overlaps_player {
                    self.world.set_block(prev, b);
                }
            }
        }
    }

    /// Сейв/загрузка мира (§4): прокидывается платформой в файл или URL.
    pub fn export_save(&self) -> Vec<u8> {
        self.world.save()
    }

    pub fn import_save(&mut self, bytes: &[u8]) -> bool {
        match world::World::restore(bytes) {
            Some(w) => {
                self.world = w;
                true
            }
            None => false,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return; // свёрнутое окно: держим старую конфигурацию
        }
        (self.config.width, self.config.height) = (width, height);
        self.surface.configure(&self.device, &self.config);
        self.offscreen =
            Offscreen::new(&self.device, (width, height), &self.blit_layout, &self.nearest);
    }

    /// Кадр: стриминг чанков → сцена в offscreen → nearest-блит на экран.
    /// Проблемы surface (Lost/Outdated/Timeout) лечатся или пропускаются
    /// здесь же — платформе не нужно знать типы wgpu.
    pub fn render(&mut self) {
        let aspect = self.config.width.max(1) as f32 / self.config.height.max(1) as f32;
        let vp = math::mul(
            &math::perspective(1.2, aspect, 0.1, FOG_END * 2.0),
            &math::view(self.camera.pos, self.camera.yaw, self.camera.pitch),
        );
        let (sun, sky) = daylight(self.time);
        let mut camera_data = [0f32; 28];
        camera_data[..16].copy_from_slice(bytemuck::cast_slice(&vp));
        camera_data[16..19].copy_from_slice(&self.camera.pos);
        camera_data[19] = FOG_END;
        camera_data[20..24].copy_from_slice(&sun);
        camera_data[24..27].copy_from_slice(&sky);
        self.queue
            .write_buffer(&self.camera_buf, 0, bytemuck::cast_slice(&camera_data));

        let planes = math::frustum(&vp);
        let draws = self.world.update(&self.device, self.camera.pos, &planes);

        // Смещения всех видимых чанков — одной записью в буфер,
        // по слоту UB_ALIGN на чанк (требование динамических оффсетов).
        let mut origins = vec![0u8; draws.len().min(MAX_DRAWS) * UB_ALIGN];
        for (i, d) in draws.iter().take(MAX_DRAWS).enumerate() {
            origins[i * UB_ALIGN..i * UB_ALIGN + 12]
                .copy_from_slice(bytemuck::cast_slice(&d.origin));
        }
        if !origins.is_empty() {
            self.queue.write_buffer(&self.origins_buf, 0, &origins);
        }

        use wgpu::CurrentSurfaceTexture as Cst;
        let frame = match self.surface.get_current_texture() {
            Cst::Success(f) | Cst::Suboptimal(f) => f,
            Cst::Lost | Cst::Outdated => {
                // Конфигурация устарела (resize, смена монитора):
                // перенастраиваем и спокойно ждём следующего кадра.
                return self.surface.configure(&self.device, &self.config);
            }
            Cst::Timeout | Cst::Occluded | Cst::Validation => return,
        };
        let screen = frame.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());

        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.offscreen.color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Небо = цвет тумана текущего часа: дальние чанки
                        // растворяются в небе в любое время суток.
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            // Чистка идёт до шейдеров — компенсируем sRGB.
                            r: (sky[0] as f64).powf(2.2),
                            g: (sky[1] as f64).powf(2.2),
                            b: (sky[2] as f64).powf(2.2),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.offscreen.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.chunk_pipeline);
            pass.set_bind_group(0, &self.scene_bind, &[]);
            pass.set_index_buffer(self.quad_indices.slice(..), wgpu::IndexFormat::Uint32);
            for (i, d) in draws.iter().take(MAX_DRAWS).enumerate() {
                pass.set_bind_group(1, &self.origin_bind, &[(i * UB_ALIGN) as u32]);
                pass.set_vertex_buffer(0, d.vertices.slice(..));
                pass.draw_indexed(0..d.quads.min(MAX_QUADS as u32) * 6, 0, 0..1);
            }
        }
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &screen,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &self.offscreen.blit_bind, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit([enc.finish()]);
        frame.present();
    }
}

fn uniform_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    dynamic: bool,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: None,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
