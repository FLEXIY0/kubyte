//! kb-platform: winit-обвязка. Единственный крейт, знающий, на чём мы
//! запущены; вся разница платформ — в том, как дождаться async-инициализации
//! wgpu, куда воткнуть канвас и когда можно захватить курсор.

use std::sync::Arc;

use kb_render::Gfx;
use web_time::Instant;
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

/// Готовый рендер прилетает в событийный цикл как user event: на нативе —
/// сразу из `resumed`, в браузере — из `spawn_local`, когда адаптер
/// наконец выдан. Один и тот же путь доставки на обеих платформах.
struct GfxReady(Gfx);

/// Зажатые клавиши движения — битовая маска: состояние ввода целиком
/// в одном байте, опрос — без ветвлений по типам событий.
#[derive(Default, Clone, Copy)]
struct Keys(u8);

impl Keys {
    const BINDS: [(KeyCode, u8); 8] = [
        (KeyCode::KeyW, 1),
        (KeyCode::ArrowUp, 1),
        (KeyCode::KeyS, 2),
        (KeyCode::ArrowDown, 2),
        (KeyCode::KeyA, 4),
        (KeyCode::KeyD, 8),
        (KeyCode::Space, 16),
        (KeyCode::ShiftLeft, 32),
    ];

    fn set(&mut self, code: KeyCode, pressed: bool) {
        for (k, bit) in Self::BINDS {
            if k == code {
                self.0 = if pressed { self.0 | bit } else { self.0 & !bit };
            }
        }
    }

    /// (вперёд, вправо, вверх) ∈ {-1, 0, 1} — готовые оси для камеры.
    fn axes(self) -> (f32, f32, f32) {
        let axis = |pos, neg| (self.0 & pos != 0) as i8 - (self.0 & neg != 0) as i8;
        (axis(1, 2) as f32, axis(8, 4) as f32, axis(16, 32) as f32)
    }
}

struct App {
    /// Прокси создаётся до запуска цикла (требование winit) и служит
    /// почтовым ящиком для готового `Gfx`.
    proxy: winit::event_loop::EventLoopProxy<GfxReady>,
    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    keys: Keys,
    grabbed: bool,
    last_frame: Option<Instant>,
}

impl App {
    /// Захват курсора: Locked, где умеют (X11/Windows/web), иначе Confined
    /// (macOS). В браузере законен только из обработчика клика.
    fn grab(&mut self, on: bool) {
        let Some(w) = &self.window else { return };
        let mode = if on { CursorGrabMode::Locked } else { CursorGrabMode::None };
        self.grabbed = on
            && w.set_cursor_grab(mode)
                .or_else(|_| w.set_cursor_grab(CursorGrabMode::Confined))
                .is_ok();
        w.set_cursor_visible(!self.grabbed);
    }
}

impl ApplicationHandler<GfxReady> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return; // Android/web могут вызывать resumed повторно
        }
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("kb"))
                .expect("create window"),
        );

        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowExtWebSys;
            web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.body())
                .and_then(|body| body.append_child(window.canvas()?.as_ref()).ok())
                .expect("attach canvas to <body>");
        }

        let size = window.inner_size();
        let gfx = Gfx::new(window.clone(), (size.width, size.height));

        #[cfg(not(target_arch = "wasm32"))]
        {
            let gfx = pollster::block_on(gfx).expect("init wgpu");
            self.proxy.send_event(GfxReady(gfx)).ok();
        }
        #[cfg(target_arch = "wasm32")]
        {
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let gfx = gfx.await.expect("init wgpu");
                let _ = proxy.send_event(GfxReady(gfx));
            });
        }

        self.window = Some(window);
    }

    fn user_event(&mut self, _: &ActiveEventLoop, GfxReady(mut gfx): GfxReady) {
        // В браузере канвас мог изменить размер, пока wgpu просыпался.
        if let Some(w) = &self.window {
            let s = w.inner_size();
            gfx.resize(s.width, s.height);
        }
        self.gfx = Some(gfx);
        self.last_frame = Some(Instant::now());
        // На нативе курсор можно брать сразу; в браузере — только по клику.
        #[cfg(not(target_arch = "wasm32"))]
        self.grab(true);
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        // Сырая дельта мыши, не позиция курсора: работает при захвате
        // и не упирается в края экрана.
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            if self.grabbed {
                if let Some(gfx) = &mut self.gfx {
                    gfx.camera.look(dx as f32, dy as f32);
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(s) => {
                if let Some(gfx) = &mut self.gfx {
                    gfx.resize(s.width, s.height);
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, .. } => self.grab(true),
            WindowEvent::KeyboardInput { event: key, .. } => {
                if let PhysicalKey::Code(code) = key.physical_key {
                    if code == KeyCode::Escape {
                        self.grab(false);
                    }
                    self.keys.set(code, key.state.is_pressed());
                }
            }
            WindowEvent::Focused(false) => {
                self.keys = Keys::default(); // не «залипать» при потере фокуса
                self.grab(false);
            }
            WindowEvent::RedrawRequested => {
                let (Some(gfx), Some(last)) = (&mut self.gfx, &mut self.last_frame) else {
                    return;
                };
                let now = Instant::now();
                // Кап dt: после паузы/сворачивания камера не телепортируется.
                let dt = (now - *last).as_secs_f32().min(0.1);
                *last = now;

                let (f, r, u) = self.keys.axes();
                gfx.camera.fly(f, r, u, dt);
                gfx.render();

                // Непрерывная анимация: следующий кадр сразу по vsync.
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        // Первый кадр после готовности gfx; дальше цикл сам поддерживает
        // request_redraw из RedrawRequested.
        if self.gfx.is_some() {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}

pub fn run() {
    let event_loop = EventLoop::<GfxReady>::with_user_event()
        .build()
        .expect("event loop");
    let app = App {
        proxy: event_loop.create_proxy(),
        window: None,
        gfx: None,
        keys: Keys::default(),
        grabbed: false,
        last_frame: None,
    };

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut app = app;
        event_loop.run_app(&mut app).expect("run");
    }

    // В браузере блокировать нельзя: spawn_app отдаёт управление JS,
    // приложение живёт в обработчиках событий.
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(app);
    }
}

/// Точка входа браузера: вызывается автоматически при загрузке модуля.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_main() {
    console_error_panic_hook::set_once();
    run();
}
