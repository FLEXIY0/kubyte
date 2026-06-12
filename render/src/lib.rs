//! kb-render: wgpu-рендер с ретро-пайплайном (§6).
//!
//! Архитектурный инвариант M0, который не меняется до конца проекта:
//! сцена ВСЕГДА рисуется в offscreen-буфер пониженного разрешения и
//! растягивается на экран nearest-блитом. Весь будущий рендер (чанки,
//! туман, небо) идёт только через этот буфер — раздельная пикселизация
//! слоёв запрещена ТЗ.

mod math;

use wgpu::util::DeviceExt;

/// Во сколько раз offscreen-буфер меньше экрана. Целое — пиксели обязаны
/// быть одинаковой ширины (integer scaling, §6). Станет настройкой в M4.
const PIXEL_SCALE: u32 = 3;

const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Вершина куба M0. Чанки в M1 перейдут на упакованный u32 (§6) и свой
/// пайплайн; кубу-демонстратору хватает простого формата.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
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
    scene_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    scene_bind: wgpu::BindGroup,
    camera_buf: wgpu::Buffer,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
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

        // --- Сцена: куб с процедурной текстурой -------------------------
        let texture = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("material.stone"),
                size: wgpu::Extent3d {
                    width: kb_materials::TEX_SIZE as u32,
                    height: kb_materials::TEX_SIZE as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            // Сид 0 — фиксированный сид текстур (§5): одинаково у всех.
            &kb_materials::bake(&kb_materials::STONE, 0),
        );
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor::default());

        let camera_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let scene_shader = device.create_shader_module(wgpu::include_wgsl!("scene.wgsl"));
        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene.layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1),
                sampler_entry(2),
            ],
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

        let (verts, idx) = cube_mesh();
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cube.vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cube.ib"),
            contents: bytemuck::cast_slice(&idx),
            usage: wgpu::BufferUsages::INDEX,
        });

        let scene_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene.pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&scene_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
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
            entries: &[texture_entry(0), sampler_entry(1)],
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

        Ok(Self {
            surface,
            device,
            queue,
            config,
            offscreen,
            blit_layout,
            nearest,
            scene_pipeline,
            blit_pipeline,
            scene_bind,
            camera_buf,
            vertices,
            indices,
            index_count: idx.len() as u32,
        })
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

    /// Кадр: сцена в offscreen → nearest-блит на экран. `t` — секунды
    /// от старта, единственное «состояние» анимации (§1: всё — функция).
    ///
    /// Проблемы surface (Lost/Outdated/Timeout) лечатся или пропускаются
    /// здесь же — платформе не нужно знать типы wgpu.
    pub fn render(&mut self, t: f32) {
        let aspect = self.config.width.max(1) as f32 / self.config.height.max(1) as f32;
        let mvp = math::mul(
            &math::perspective(1.0, aspect, 0.1, 100.0),
            &math::spin_view(t * 0.7, 3.0),
        );
        self.queue
            .write_buffer(&self.camera_buf, 0, bytemuck::cast_slice(&mvp));

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
                        // Глубокий сине-серый — заготовка неба «приятной тревоги» (§14).
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.012,
                            g: 0.014,
                            b: 0.022,
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
            pass.set_pipeline(&self.scene_pipeline);
            pass.set_bind_group(0, &self.scene_bind, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..self.index_count, 0, 0..1);
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

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
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

/// Куб 1×1×1 вокруг начала координат: 6 граней × 4 вершины, развёрнутые
/// из таблицы нормалей — данные вместо шести скопированных кусков кода (§1).
fn cube_mesh() -> ([Vertex; 24], [u16; 36]) {
    const N: [[f32; 3]; 6] = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    let mut verts = [Vertex { pos: [0.0; 3], normal: [0.0; 3], uv: [0.0; 2] }; 24];
    let mut idx = [0u16; 36];
    for (f, n) in N.iter().enumerate() {
        // Базис грани: u = ось, циклически следующая за нормалью, v = n × u.
        let a = n.iter().position(|&c| c != 0.0).unwrap();
        let (u, v) = ((a + 1) % 3, (a + 2) % 3);
        for corner in 0..4 {
            let (su, sv) = ((corner & 1) as f32 - 0.5, (corner >> 1) as f32 - 0.5);
            let mut pos = [0.0f32; 3];
            pos[a] = n[a] * 0.5;
            pos[u] = su * n[a]; // знак держит обход CCW наружу для обеих сторон
            pos[v] = sv;
            verts[f * 4 + corner] = Vertex {
                pos,
                normal: *n,
                uv: [su + 0.5, 0.5 - sv],
            };
        }
        let base = (f * 4) as u16;
        let quad = [0, 1, 2, 2, 1, 3].map(|i| base + i);
        idx[f * 6..f * 6 + 6].copy_from_slice(&quad);
    }
    (verts, idx)
}
