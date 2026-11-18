//! Software rendering backend (Windows): winit windows + egui-winit input + the CPU
//! rasterizer from [raster], presented with GDI (SetDIBitsToDevice).
//!
//! Used when OpenGL 2+ is missing (clean Windows 7 with the standard VGA driver, virtual
//! machines without 3D) or broken (old video drivers drawing artefacts). It supports the
//! same features eframe gives the app: several immediate viewports (pop-up tips, the remote
//! access indicator, remote viewer windows), viewport commands, clipboard, IME, tray mode.

pub mod raster;

use crate::app::App;
use eframe::egui;
use egui::{ViewportBuilder, ViewportCommand, ViewportId, ViewportIdMap, ViewportInfo};
use egui_winit::winit;
use raster::{Canvas, Painter};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

struct Vp {
    builder: ViewportBuilder,
    window: Window,
    state: egui_winit::State,
    info: ViewportInfo,
    canvas: Canvas,
    actions: egui::ahash::HashSet<egui_winit::ActionRequested>,
}

struct Shared {
    vps: ViewportIdMap<Vp>,
    by_window: HashMap<WindowId, ViewportId>,
    painter: Painter,
    beginning: Instant,
    /// set when a child viewport needs the root pass to run again right away
    repaint_now: bool,
}

thread_local! {
    static EVENT_LOOP: Cell<*const ActiveEventLoop> = const { Cell::new(std::ptr::null()) };
}

/// Make the active event loop reachable from the immediate-viewport callback
/// (it runs inside egui::Context::run, deep inside the app's update).
fn with_event_loop<R>(el: &ActiveEventLoop, f: impl FnOnce() -> R) -> R {
    let prev = EVENT_LOOP.with(|c| c.replace(el as *const _));
    let r = f();
    EVENT_LOOP.with(|c| c.set(prev));
    r
}

fn current_event_loop<R>(f: impl FnOnce(&ActiveEventLoop) -> R) -> Option<R> {
    let p = EVENT_LOOP.with(|c| c.get());
    // SAFETY: the pointer is only set for the duration of with_event_loop on this thread.
    (!p.is_null()).then(|| f(unsafe { &*p }))
}

fn create_vp(ctx: &egui::Context, el: &ActiveEventLoop, id: ViewportId, builder: ViewportBuilder) -> Result<Vp, String> {
    let window = egui_winit::create_window(ctx, el, &builder).map_err(|e| e.to_string())?;
    let state = egui_winit::State::new(ctx.clone(), id, el, Some(window.scale_factor() as f32), window.theme(), Some(raster::MAX_TEXTURE_SIDE));
    let mut info = ViewportInfo::default();
    egui_winit::update_viewport_info(&mut info, ctx, &window, true);
    Ok(Vp { builder, window, state, info, canvas: Canvas::default(), actions: Default::default() })
}

fn take_input(ctx: &egui::Context, sh: &mut Shared, id: ViewportId) -> Option<egui::RawInput> {
    let vp = sh.vps.get_mut(&id)?;
    egui_winit::update_viewport_info(&mut vp.info, ctx, &vp.window, false);
    let mut raw = vp.state.take_egui_input(&vp.window);
    raw.time = Some(sh.beginning.elapsed().as_secs_f64());
    raw.viewports = sh.vps.iter().map(|(id, v)| (*id, v.info.clone())).collect();
    if let Some(vp) = sh.vps.get_mut(&id) {
        vp.info.events.clear(); // handed to egui now
    }
    Some(raw)
}

fn present(canvas: &Canvas, window: &Window) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Gdi::*;
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
        if canvas.w == 0 || canvas.h == 0 {
            return;
        }
        let hwnd = h.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        unsafe {
            let dc = GetDC(hwnd);
            if dc.is_null() {
                return;
            }
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = canvas.w as i32;
            bmi.bmiHeader.biHeight = -(canvas.h as i32); // top-down rows
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            SetDIBitsToDevice(dc, 0, 0, canvas.w as u32, canvas.h as u32, 0, 0, 0, canvas.h as u32, canvas.px.as_ptr() as *const _, &bmi, DIB_RGB_COLORS);
            ReleaseDC(hwnd, dc);
        }
    }
    #[cfg(not(windows))]
    let _ = (canvas, window);
}

/// Paint one pass of one viewport and apply its output. Returns when the root should run
/// again (from the repaint delays egui reports), if the pass was the root one.
fn finish(ctx: &egui::Context, sh: &mut Shared, id: ViewportId, out: egui::FullOutput, is_root: bool) -> Option<Instant> {
    let egui::FullOutput { platform_output, textures_delta, shapes, pixels_per_point, viewport_output } = out;
    let Shared { vps, by_window, painter, repaint_now, .. } = sh;
    for (tid, delta) in &textures_delta.set {
        painter.set_texture(*tid, delta);
    }
    if let Some(vp) = vps.get_mut(&id) {
        let size = vp.window.inner_size();
        let shown = vp.window.is_visible() != Some(false) && vp.window.is_minimized() != Some(true);
        if shown && size.width > 0 && size.height > 0 {
            let prims = ctx.tessellate(shapes, pixels_per_point);
            let bg = ctx.style().visuals.panel_fill;
            vp.canvas.resize(size.width as usize, size.height as usize);
            vp.canvas.clear([bg.r(), bg.g(), bg.b()]);
            painter.paint(&mut vp.canvas, pixels_per_point, &prims);
            present(&vp.canvas, &vp.window);
        }
        vp.state.handle_platform_output(&vp.window, platform_output);
    }
    for tid in &textures_delta.free {
        painter.free(*tid);
    }

    let now = Instant::now();
    let mut next: Option<Instant> = None;
    for (vid, vo) in &viewport_output {
        if let Some(d) = now.checked_add(vo.repaint_delay) {
            next = Some(next.map_or(d, |n| n.min(d)));
        }
        let Some(vp) = vps.get_mut(vid) else { continue };
        egui_winit::process_viewport_commands(ctx, &mut vp.info, vo.commands.clone(), &vp.window, &mut vp.actions);
        for action in vp.actions.drain() {
            use egui_winit::ActionRequested as A;
            let ev = match action {
                A::Cut => Some(egui::Event::Cut),
                A::Copy => Some(egui::Event::Copy),
                A::Paste => vp.state.clipboard_text().map(|t| t.replace("\r\n", "\n")).filter(|t| !t.is_empty()).map(egui::Event::Paste),
                A::Screenshot => None,
            };
            if let Some(ev) = ev {
                vp.state.egui_input_mut().events.push(ev);
                *repaint_now = true;
            }
        }
        if !vp.info.events.is_empty() {
            *repaint_now = true; // e.g. ViewportCommand::Close: let the app see it
        }
    }
    if is_root {
        // viewports the app did not show in this pass are gone
        vps.retain(|vid, _| *vid == ViewportId::ROOT || viewport_output.contains_key(vid));
        by_window.retain(|_, vid| vps.contains_key(vid));
    }
    if *repaint_now {
        next = Some(now);
    }
    next
}

/// Called by egui for every ctx.show_viewport_immediate (from inside the root pass).
fn render_immediate(ctx: &egui::Context, shared: &RefCell<Shared>, imm: egui::ImmediateViewport<'_>) {
    let egui::ImmediateViewport { ids, mut builder, mut viewport_ui_cb } = imm;
    let id = ids.this;
    let raw = {
        let mut guard = shared.borrow_mut();
        let sh = &mut *guard;
        if builder.icon.is_none() {
            builder.icon = sh.vps.get(&ids.parent).and_then(|v| v.builder.icon.clone());
        }
        let mut recreate = false;
        if let Some(vp) = sh.vps.get_mut(&id) {
            let (cmds, rec) = vp.builder.patch(builder.clone());
            recreate = rec;
            if !rec {
                egui_winit::process_viewport_commands(ctx, &mut vp.info, cmds, &vp.window, &mut vp.actions);
            }
        }
        if recreate {
            sh.vps.remove(&id);
            sh.by_window.retain(|_, v| *v != id);
        }
        if !sh.vps.contains_key(&id) {
            match current_event_loop(|el| create_vp(ctx, el, id, builder)) {
                Some(Ok(vp)) => {
                    sh.by_window.insert(vp.window.id(), id);
                    sh.vps.insert(id, vp);
                }
                _ => return,
            }
        }
        match take_input(ctx, sh, id) {
            Some(raw) => raw,
            None => return,
        }
    };
    // no borrow may be held here: the ui code can show more immediate viewports
    let out = ctx.run(raw, |ctx| viewport_ui_cb(ctx));
    let mut sh = shared.borrow_mut();
    finish(ctx, &mut sh, id, out, false);
}

#[derive(Debug)]
struct Wake(Instant);

struct Runner {
    ctx: egui::Context,
    shared: Rc<RefCell<Shared>>,
    root_builder: Option<ViewportBuilder>,
    make_app: Option<Box<dyn FnOnce(&egui::Context) -> App>>,
    app: Option<App>,
    show_root: bool,
    next: Option<Instant>,
    error: Option<String>,
}

impl Runner {
    fn schedule(&mut self, t: Instant) {
        self.next = Some(self.next.map_or(t, |n| n.min(t)));
    }

    fn run_root(&mut self, el: &ActiveEventLoop) {
        let Some(app) = self.app.as_mut() else { return };
        let ctx = self.ctx.clone();
        let raw = {
            let mut sh = self.shared.borrow_mut();
            sh.repaint_now = false;
            match take_input(&ctx, &mut sh, ViewportId::ROOT) {
                Some(r) => r,
                None => return,
            }
        };
        let close_requested = raw.viewport().close_requested();
        let out = with_event_loop(el, || ctx.run(raw, |ctx| app.frame(ctx)));
        let cancel = out.viewport_output.get(&ViewportId::ROOT).is_some_and(|o| o.commands.contains(&ViewportCommand::CancelClose));
        if close_requested && !cancel {
            el.exit();
            return;
        }
        let next = with_event_loop(el, || finish(&ctx, &mut self.shared.borrow_mut(), ViewportId::ROOT, out, true));
        if self.show_root {
            self.show_root = false;
            if let Some(vp) = self.shared.borrow().vps.get(&ViewportId::ROOT) {
                vp.window.set_visible(true);
            }
        }
        self.next = next;
    }
}

impl ApplicationHandler<Wake> for Runner {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.app.is_some() || self.error.is_some() {
            return;
        }
        let Some(builder) = self.root_builder.take() else { return };
        // hidden until the first frame is painted (no white flash), like eframe
        self.show_root = builder.visible != Some(false);
        match create_vp(&self.ctx, el, ViewportId::ROOT, builder.with_visible(false)) {
            Ok(vp) => {
                let mut sh = self.shared.borrow_mut();
                sh.by_window.insert(vp.window.id(), ViewportId::ROOT);
                sh.vps.insert(ViewportId::ROOT, vp);
            }
            Err(e) => {
                self.error = Some(format!("Не удалось создать окно: {e}"));
                el.exit();
                return;
            }
        }
        if let Some(make) = self.make_app.take() {
            let ctx = self.ctx.clone();
            self.app = Some(with_event_loop(el, || make(&ctx)));
        }
        self.schedule(Instant::now());
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: Wake) {
        self.schedule(ev.0);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        let now = Instant::now();
        let mut run_now = false;
        {
            let mut sh = self.shared.borrow_mut();
            let Some(id) = sh.by_window.get(&window_id).copied() else { return };
            let Some(vp) = sh.vps.get_mut(&id) else { return };
            match &event {
                WindowEvent::RedrawRequested => {
                    // the OS asks to repaint (window uncovered): show the last frame
                    let size = vp.window.inner_size();
                    if vp.canvas.w == size.width as usize && vp.canvas.h == size.height as usize {
                        present(&vp.canvas, &vp.window);
                    } else {
                        run_now = true;
                    }
                }
                WindowEvent::CloseRequested => {
                    vp.info.events.push(egui::ViewportEvent::Close);
                    self.next = Some(now);
                }
                _ => {
                    let resp = vp.state.on_window_event(&vp.window, &event);
                    if matches!(event, WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }) {
                        run_now = true; // repaint while the user drags the border
                    } else if resp.repaint {
                        self.next = Some(now);
                    }
                }
            }
        }
        if run_now {
            self.run_root(el);
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.next.is_some_and(|t| t <= Instant::now()) {
            self.next = None;
            self.run_root(el);
        }
        el.set_control_flow(match self.next {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }
}

/// Run the app with the software renderer. Blocks until the main window is closed.
pub fn run(root: ViewportBuilder, make_app: Box<dyn FnOnce(&egui::Context) -> App>) -> Result<(), String> {
    let event_loop = EventLoop::<Wake>::with_user_event().build().map_err(|e| e.to_string())?;
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let proxy = Mutex::new(event_loop.create_proxy());
    ctx.set_request_repaint_callback(move |info| {
        let when = Instant::now().checked_add(info.delay).unwrap_or_else(|| Instant::now() + Duration::from_secs(3600));
        if let Ok(p) = proxy.lock() {
            let _ = p.send_event(Wake(when));
        }
    });
    let shared = Rc::new(RefCell::new(Shared {
        vps: ViewportIdMap::default(),
        by_window: HashMap::new(),
        painter: Painter::default(),
        beginning: Instant::now(),
        repaint_now: false,
    }));
    {
        let shared = shared.clone();
        egui::Context::set_immediate_viewport_renderer(move |ctx, imm| render_immediate(ctx, &shared, imm));
    }
    let mut runner = Runner {
        ctx,
        shared,
        root_builder: Some(root),
        make_app: Some(make_app),
        app: None,
        show_root: false,
        next: None,
        error: None,
    };
    event_loop.run_app(&mut runner).map_err(|e| e.to_string())?;
    drop(runner.app.take());
    match runner.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
