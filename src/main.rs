#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod emoticons;
mod gltest;
mod icons;
mod net;
mod platform;
mod proto;
mod received;
mod remote;
mod soft;
mod tray;
mod ui;
mod update;

use eframe::egui;
use std::sync::{Arc, OnceLock};

/// How the window is drawn this run (shown in the settings and in the event log).
pub static RENDER_INFO: OnceLock<String> = OnceLock::new();

/// True while eframe is starting OpenGL (before the app is created): a failure there is
/// answered by restarting with the software renderer instead of an error box.
static GL_STARTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static GL_PANIC: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn arg_value(args: &[String], name: &str) -> Option<String> {
    for (i, a) in args.iter().enumerate() {
        if let Some(v) = a.strip_prefix(&format!("{name}=")) {
            return Some(v.to_string());
        }
        if a == name {
            return args.get(i + 1).cloned();
        }
    }
    None
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates = [r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\tahoma.ttf", r"C:\Windows\Fonts\arial.ttf"];
    if let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert("system".into(), egui::FontData::from_owned(bytes));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "system".into());
    }
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguisym.ttf") {
        fonts.font_data.insert("symbols".into(), egui::FontData::from_owned(bytes));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().push("symbols".into());
    }
    ctx.set_fonts(fonts);
    ctx.style_mut(|s| {
        s.visuals = egui::Visuals::light();
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
    });
}

/// OpenGL good enough for egui_glow? Ok(description) or Err(reason to draw in software).
/// Old drivers with OpenGL 2.x are where egui_glow draws artefacts, and the standard VGA
/// driver of Windows 7 (and VMs without 3D) only has Microsoft's OpenGL 1.1 "GDI Generic".
fn check_opengl(broken_file: &std::path::Path) -> Result<String, String> {
    #[cfg(windows)]
    {
        let (ver, renderer, vendor) = platform::probe_opengl().map_err(|e| format!("OpenGL недоступен ({e})"))?;
        let desc = format!("OpenGL {ver}, {renderer} ({vendor})");
        if renderer.contains("GDI Generic") {
            return Err(format!("нет драйвера видеокарты с поддержкой OpenGL — {desc}"));
        }
        let num: String = ver.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        let mut it = num.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
        let (major, minor) = (it.next().unwrap_or(0), it.next().unwrap_or(0));
        if (major, minor) < (3, 0) {
            return Err(format!("старый драйвер OpenGL (нужен 3.0+) — {desc}"));
        }
        // Intel integrated graphics on Windows 7/8.x (HD 2000…4600: Sandy/Ivy Bridge, Haswell;
        // e.g. Pentium G2020): the drivers for these systems are no longer updated and
        // draw egui with artefacts — stretched triangles and stripes of text over a dark
        // window — whatever OpenGL version they report. The CPU renderer is fast enough.
        let intel = vendor.contains("Intel") || renderer.contains("Intel");
        if intel && platform::windows_build() < 10240 {
            return Err(format!("встроенная графика Intel на Windows 7/8 рисует через OpenGL с искажениями — {desc}"));
        }
        if intel && (major, minor) < (3, 3) {
            return Err(format!("драйвер Intel с OpenGL {major}.{minor} рисует с искажениями — {desc}"));
        }
        // the self-test (gltest) already failed on this driver: do not try it again
        if std::fs::read_to_string(broken_file).is_ok_and(|s| s.lines().next() == Some(desc.as_str())) {
            return Err(format!("драйвер OpenGL не прошёл проверку отрисовки — {desc}"));
        }
        Ok(desc)
    }
    #[cfg(not(windows))]
    {
        let _ = broken_file;
        Ok("OpenGL".into())
    }
}

fn main() {
    // the release build has no console: show what went wrong instead of vanishing silently
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        if GL_STARTING.load(std::sync::atomic::Ordering::SeqCst) {
            // eframe 0.29 panics (instead of returning an error) when it finds no usable
            // OpenGL configuration; run() catches it and switches to software rendering
            if let Ok(mut p) = GL_PANIC.lock() {
                *p = Some(info.payload().downcast_ref::<String>().cloned().or_else(|| info.payload().downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| info.to_string()));
            }
            return;
        }
        if std::thread::current().name() == Some("main") {
            platform::error_box("Nassi RS — ошибка", &format!("Программа аварийно завершилась:\n\n{info}"));
        }
    }));
    if let Err(e) = run() {
        platform::error_box("Nassi RS — ошибка запуска", &e);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(ms) = arg_value(&args, "--delay").and_then(|v| v.parse::<u64>().ok()) {
        std::thread::sleep(std::time::Duration::from_millis(ms.min(30_000)));
    }
    let profile = arg_value(&args, "--profile");
    // only one copy per profile: started again, it just shows the running one. After a
    // restart or an update (they pass --delay) wait until the old copy has closed.
    let instance_key = match &profile {
        Some(p) => format!("profile-{}", p.replace(['\\', '/', ':'], "_")),
        None => "main".to_string(),
    };
    let wait_ms = if arg_value(&args, "--delay").is_some() { 20_000 } else { 0 };
    let Ok(instance_event) = platform::single_instance(&instance_key, wait_ms) else {
        return Ok(());
    };
    let paths = config::Paths::new(profile.as_deref());
    let (cfg, _, _) = config::Config::load(&paths);

    let title = match &profile {
        Some(p) => format!("Nassi RS — {} [{p}]", cfg.nick),
        None => format!("Nassi RS — {}", cfg.nick),
    };
    let mut vp = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size([960.0, 640.0])
        .with_min_inner_size([560.0, 380.0]);
    if let Some(icon) = icons::Icons::app_icon() {
        vp = vp.with_icon(Arc::new(icon));
    }
    let pos = arg_value(&args, "--pos").and_then(|v| {
        let (x, y) = v.split_once(',')?;
        Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
    });
    if let Some(p) = pos {
        vp = vp.with_position(p);
    } else if let Some([x, y, w, h]) = cfg.window {
        vp = vp.with_position([x, y]).with_inner_size([w.max(560.0), (h - 30.0).max(380.0)]);
    }

    use config::RenderMode;
    let gl_broken_file = paths.data.join("opengl-broken.txt");
    let gl_error = std::env::var("NASSI_GL_ERROR").ok();
    let mode = if args.iter().any(|a| a == "--software") {
        RenderMode::Software
    } else if args.iter().any(|a| a == "--opengl") {
        RenderMode::OpenGl
    } else {
        cfg.general.render
    };
    let (software, info) = match (mode, gl_error) {
        (RenderMode::Software, Some(e)) => (true, format!("программная — OpenGL не запустился: {e}")),
        (RenderMode::Software, None) => (true, "программная (по умолчанию; OpenGL можно включить в Настройка → Общие → «Отрисовка окна»)".to_string()),
        (RenderMode::OpenGl, _) => (false, "OpenGL (выбран в настройках или ключом --opengl)".to_string()),
        (RenderMode::Auto, _) => match check_opengl(&gl_broken_file) {
            Ok(desc) => (false, desc),
            Err(why) => (true, format!("программная — {why}")),
        },
    };
    let _ = RENDER_INFO.set(info);

    let start_tray = args.iter().any(|a| a == "--tray");
    let make_app = move |ctx: &egui::Context| {
        setup_fonts(ctx);
        let mut app = app::App::new(ctx, profile);
        app.listen_for_second_start(instance_event);
        // --tray: restarted by an update while it was in the tray (src/update.rs)
        if (app.cfg.general.start_minimized || start_tray) && app.tray.is_some() && pos.is_none() {
            app.start_hidden = true;
        }
        app
    };

    if software {
        return soft::run(vp, Box::new(make_app)).map_err(|e| format!("Не удалось открыть окно программы: {e}"));
    }
    let self_test = mode == RenderMode::Auto;
    let options = eframe::NativeOptions { viewport: vp, ..Default::default() };
    use std::sync::atomic::Ordering::SeqCst;
    GL_STARTING.store(true, SeqCst);
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eframe::run_native(
            "nassi-rs",
            options,
            Box::new(move |cc| {
                // "Автоматически": make sure the driver really draws correctly; if not,
                // remember it and restart with the software renderer (below)
                if self_test {
                    if let Some(gl) = &cc.gl {
                        if let Err(e) = gltest::run(gl) {
                            let desc = RENDER_INFO.get().cloned().unwrap_or_default();
                            let _ = std::fs::write(&gl_broken_file, format!("{desc}\n{e}\n"));
                            return Err(e.into());
                        }
                    }
                }
                GL_STARTING.store(false, SeqCst);
                Ok(Box::new(make_app(&cc.egui_ctx)))
            }),
        )
    }));
    let starting = GL_STARTING.swap(false, SeqCst);
    let err = match res {
        Ok(Ok(())) => return Ok(()),
        Ok(Err(e)) if starting => e.to_string(),
        Ok(Err(e)) => return Err(format!("Ошибка OpenGL: {e}")),
        Err(panic) if starting => GL_PANIC.lock().ok().and_then(|mut p| p.take()).unwrap_or_else(|| format!("{panic:?}")),
        Err(panic) => std::panic::resume_unwind(panic),
    };
    // OpenGL did not start: run again in a new process with the software renderer
    // (winit allows only one event loop per process).
    let exe = std::env::current_exe().map_err(|x| format!("{err}\n{x}"))?;
    let mut a: Vec<String> = args.iter().skip(1).filter(|a| !a.starts_with("--delay") && *a != "--opengl" && *a != "--software").cloned().collect();
    a.push("--software".into());
    a.push("--delay=1000".into());
    std::process::Command::new(exe)
        .args(a)
        .env("NASSI_GL_ERROR", &err)
        .spawn()
        .map(|_| ())
        .map_err(|x| format!("Не удалось запустить OpenGL: {err}\nПерезапуск с программной отрисовкой тоже не удался: {x}"))
}
