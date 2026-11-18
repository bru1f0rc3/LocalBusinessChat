//! System tray icon: left click restores the window, menu has Open / Exit.

use eframe::egui;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Arc;
#[cfg(not(windows))]
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub struct Tray {
    icon: TrayIcon,
    normal: Option<tray_icon::Icon>,
    alert_icon: Option<tray_icon::Icon>,
    alert_on: AtomicBool,
    pub show_requested: Arc<AtomicBool>,
    pub quit_requested: Arc<AtomicBool>,
    /// main window handle (0 = unknown), used to leave "stealth" tray mode
    pub hwnd: Arc<AtomicIsize>,
}

/// Bring the main window back (from the tray, minimized or behind other windows).
pub fn restore(ctx: &egui::Context, hwnd: &AtomicIsize, show: &AtomicBool) {
    show.store(true, Ordering::Relaxed);
    let h = hwnd.load(Ordering::Relaxed);
    if h != 0 {
        crate::platform::set_stealth(h, false);
    } else {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}

/// The app icon with a big red "new message" dot in the bottom-right corner.
fn alert_rgba(src: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    if src.len() != (w * h * 4) as usize || w == 0 || h == 0 {
        return None;
    }
    let mut v = src.to_vec();
    let r = w.min(h) as f32 * 0.30;
    let (cx, cy) = (w as f32 - r - 0.5, h as f32 - r - 0.5);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let i = ((y * w + x) * 4) as usize;
            if d <= r - 1.0 {
                v[i..i + 4].copy_from_slice(&[230, 40, 40, 255]);
            } else if d <= r + 0.5 {
                v[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    Some(v)
}

impl Tray {
    pub fn new(ctx: &egui::Context) -> Option<Tray> {
        let data = crate::icons::Icons::app_icon()?;
        let alert_icon = alert_rgba(&data.rgba, data.width, data.height).and_then(|v| tray_icon::Icon::from_rgba(v, data.width, data.height).ok());
        let normal = tray_icon::Icon::from_rgba(data.rgba.clone(), data.width, data.height).ok();
        let icon = tray_icon::Icon::from_rgba(data.rgba, data.width, data.height).ok()?;
        #[cfg_attr(windows, allow(unused_mut))]
        let mut builder = TrayIconBuilder::new().with_tooltip("Nassi RS").with_icon(icon);
        // Windows: no tray-icon menu. It is opened from the library's hidden window with
        // WS_EX_NOACTIVATE, which on Windows 7 cannot become the foreground window: the menu
        // then does not close when you click elsewhere (e.g. on Start), and while it is open
        // the program's main thread sits in the menu loop, so the program hangs. We show our
        // own menu instead (platform::tray_popup_menu), the way Microsoft documents it.
        #[cfg(not(windows))]
        let (open, quit) = {
            let menu = Menu::new();
            let open = MenuItem::new("Открыть Nassi RS", true, None);
            let quit = MenuItem::new("Выход", true, None);
            menu.append(&open).ok()?;
            menu.append(&quit).ok()?;
            builder = builder.with_menu(Box::new(menu)).with_menu_on_left_click(false);
            (open, quit)
        };
        let tray = builder.build().ok()?;

        let show = Arc::new(AtomicBool::new(false));
        let quit_flag = Arc::new(AtomicBool::new(false));
        let hwnd = Arc::new(AtomicIsize::new(0));
        {
            let (ctx, show, hwnd, q) = (ctx.clone(), show.clone(), hwnd.clone(), quit_flag.clone());
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                match e {
                    TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                    | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => restore(&ctx, &hwnd, &show),
                    #[cfg(windows)]
                    TrayIconEvent::Click { button: MouseButton::Right, button_state: MouseButtonState::Up, .. } => {
                        match crate::platform::tray_popup_menu(&["Открыть Nassi RS", "Выход"]) {
                            Some(0) => restore(&ctx, &hwnd, &show),
                            Some(1) => {
                                q.store(true, Ordering::Relaxed);
                                ctx.request_repaint();
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
                let _ = &q;
            }));
        }
        #[cfg(not(windows))]
        {
            let (ctx, show, q, hwnd) = (ctx.clone(), show.clone(), quit_flag.clone(), hwnd.clone());
            let (open_id, quit_id) = (open.id().clone(), quit.id().clone());
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if e.id == open_id {
                    restore(&ctx, &hwnd, &show);
                } else if e.id == quit_id {
                    q.store(true, Ordering::Relaxed);
                }
                ctx.request_repaint();
            }));
        }
        Some(Tray { icon: tray, normal, alert_icon, alert_on: AtomicBool::new(false), show_requested: show, quit_requested: quit_flag, hwnd })
    }

    /// Switch between the normal icon and the "new message" icon (called on every
    /// blink phase; does nothing when the state does not change).
    pub fn set_alert(&self, on: bool) {
        if self.alert_on.swap(on, Ordering::Relaxed) == on {
            return;
        }
        let icon = if on { self.alert_icon.clone() } else { self.normal.clone() };
        if icon.is_some() {
            let _ = self.icon.set_icon(icon);
        }
    }

    pub fn set_tooltip(&self, s: &str) {
        let _ = self.icon.set_tooltip(Some(s));
    }
}
