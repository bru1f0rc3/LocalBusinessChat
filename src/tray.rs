//! System tray icon: left click restores the window, menu has Open / Exit.

use eframe::egui;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Arc;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub struct Tray {
    icon: TrayIcon,
    pub show_requested: Arc<AtomicBool>,
    pub quit_requested: Arc<AtomicBool>,
    /// main window handle (0 = unknown), used to leave "stealth" tray mode
    pub hwnd: Arc<AtomicIsize>,
}

fn restore(ctx: &egui::Context, hwnd: &AtomicIsize, show: &AtomicBool) {
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

impl Tray {
    pub fn new(ctx: &egui::Context) -> Option<Tray> {
        let data = crate::icons::Icons::app_icon()?;
        let icon = tray_icon::Icon::from_rgba(data.rgba, data.width, data.height).ok()?;
        let menu = Menu::new();
        let open = MenuItem::new("Открыть Nassi RS", true, None);
        let quit = MenuItem::new("Выход", true, None);
        menu.append(&open).ok()?;
        menu.append(&quit).ok()?;
        let tray = TrayIconBuilder::new()
            .with_tooltip("Nassi RS")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .ok()?;

        let show = Arc::new(AtomicBool::new(false));
        let quit_flag = Arc::new(AtomicBool::new(false));
        let hwnd = Arc::new(AtomicIsize::new(0));
        {
            let (ctx, show, hwnd) = (ctx.clone(), show.clone(), hwnd.clone());
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                if matches!(
                    e,
                    TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                        | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }
                ) {
                    restore(&ctx, &hwnd, &show);
                }
            }));
        }
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
        Some(Tray { icon: tray, show_requested: show, quit_requested: quit_flag, hwnd })
    }

    pub fn set_tooltip(&self, s: &str) {
        let _ = self.icon.set_tooltip(Some(s));
    }
}
