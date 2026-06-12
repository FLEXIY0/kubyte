//! kb-platform: winit-обвязка. Единственный крейт, знающий, на чём мы
//! запущены; вся разница платформ — в том, как дождаться async-инициализации
//! wgpu и куда воткнуть канвас.

use std::sync::Arc;

use kb_render::Gfx;
use web_time::Instant;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

/// Готовый рендер прилетает в событийный цикл как user event: на нативе —
/// сразу из `resumed`, в браузере — из `spawn_local`, когда адаптер
/// наконец выдан. Один и тот же путь доставки на обеих платформах.
struct GfxReady(Gfx);

struct App {
    /// Прокси создаётся до запуска цикла (требование winit) и служит
    /// почтовым ящиком для готового `Gfx`.
    proxy: winit::event_loop::EventLoopProxy<GfxReady>,
    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    started: Option<Instant>,
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
        self.started = Some(Instant::now());
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(s) => {
                if let Some(gfx) = &mut self.gfx {
                    gfx.resize(s.width, s.height);
                }
            }
            WindowEvent::RedrawRequested => {
                let (Some(gfx), Some(t0)) = (&mut self.gfx, self.started) else {
                    return;
                };
                gfx.render(t0.elapsed().as_secs_f32());
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
        started: None,
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
