//! OS integration (Windows). Non-Windows builds get harmless stubs.

use crate::proto::ProcInfo;

pub fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "UNKNOWN".into())
}

pub fn os_user() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "user".into())
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    use windows_sys::Win32::System::Registry::*;
    use windows_sys::Win32::System::SystemInformation::*;
    use windows_sys::Win32::System::Threading::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn idle_secs() -> u32 {
        unsafe {
            let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
            if GetLastInputInfo(&mut li) == 0 {
                return 0;
            }
            GetTickCount().wrapping_sub(li.dwTime) / 1000
        }
    }

    pub fn os_uptime_secs() -> u32 {
        unsafe { (GetTickCount64() / 1000) as u32 }
    }

    fn window_title(h: HWND) -> String {
        unsafe {
            let mut buf = [0u16; 512];
            let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
            String::from_utf16_lossy(&buf[..n.max(0) as usize])
        }
    }

    pub fn foreground_title() -> String {
        unsafe {
            let h = GetForegroundWindow();
            if h.is_null() {
                return String::new();
            }
            window_title(h)
        }
    }

    #[allow(dead_code)]
    pub fn cursor_pos() -> (i32, i32) {
        unsafe {
            let mut p = POINT { x: 0, y: 0 };
            GetCursorPos(&mut p);
            (p.x, p.y)
        }
    }

    pub fn os_name() -> String {
        let read = |name: &str| -> Option<String> {
            unsafe {
                let key = wide("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
                let val = wide(name);
                let mut buf = [0u16; 256];
                let mut size = (buf.len() * 2) as u32;
                let r = RegGetValueW(
                    HKEY_LOCAL_MACHINE,
                    key.as_ptr(),
                    val.as_ptr(),
                    RRF_RT_REG_SZ,
                    null_mut(),
                    buf.as_mut_ptr() as *mut _,
                    &mut size,
                );
                if r != 0 {
                    return None;
                }
                let n = (size as usize / 2).saturating_sub(1);
                Some(String::from_utf16_lossy(&buf[..n]))
            }
        };
        let mut product = read("ProductName").unwrap_or_else(|| "Windows".into());
        let build: u32 = read("CurrentBuild").and_then(|b| b.parse().ok()).unwrap_or(0);
        if build >= 22000 {
            product = product.replace("Windows 10", "Windows 11");
        }
        match read("DisplayVersion") {
            Some(v) => format!("{product} {v} (build {build})"),
            None => format!("{product} (build {build})"),
        }
    }

    unsafe extern "system" fn enum_titles(h: HWND, l: LPARAM) -> BOOL {
        let map = &mut *(l as *mut std::collections::HashMap<u32, String>);
        if IsWindowVisible(h) != 0 && GetWindow(h, GW_OWNER).is_null() {
            let t = window_title(h);
            if !t.is_empty() {
                let mut pid = 0u32;
                GetWindowThreadProcessId(h, &mut pid);
                map.entry(pid).or_insert(t);
            }
        }
        1
    }

    pub fn processes() -> Vec<ProcInfo> {
        let mut titles: std::collections::HashMap<u32, String> = Default::default();
        let mut out = Vec::new();
        unsafe {
            EnumWindows(Some(enum_titles), &mut titles as *mut _ as LPARAM);
            let mut fg_pid = 0u32;
            GetWindowThreadProcessId(GetForegroundWindow(), &mut fg_pid);
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return out;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snap, &mut e) != 0 {
                loop {
                    let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                    let name = String::from_utf16_lossy(&e.szExeFile[..len]);
                    let pid = e.th32ProcessID;
                    out.push(ProcInfo {
                        pid,
                        name,
                        title: titles.get(&pid).cloned().unwrap_or_default(),
                        active: pid == fg_pid,
                    });
                    if Process32NextW(snap, &mut e) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
        }
        out
    }

    pub fn kill(pid: u32) -> Result<(), String> {
        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if h.is_null() {
                return Err(format!("Не удалось открыть процесс {pid}"));
            }
            let ok = TerminateProcess(h, 1);
            CloseHandle(h);
            if ok == 0 {
                Err(format!("Не удалось завершить процесс {pid}"))
            } else {
                Ok(())
            }
        }
    }

    unsafe extern "system" fn enum_mon(_m: HMONITOR, _dc: HDC, r: *mut RECT, l: LPARAM) -> BOOL {
        let v = &mut *(l as *mut Vec<RECT>);
        v.push(*r);
        1
    }

    pub fn monitors() -> Vec<(i32, i32, i32, i32)> {
        let mut v: Vec<RECT> = Vec::new();
        unsafe {
            EnumDisplayMonitors(null_mut(), std::ptr::null(), Some(enum_mon), &mut v as *mut _ as LPARAM);
        }
        v.iter().map(|r| (r.left, r.top, r.right - r.left, r.bottom - r.top)).collect()
    }

    /// monitor 0 = whole virtual desktop, 1.. = individual monitor
    pub fn screenshot(monitor: usize) -> Result<(image::RgbaImage, usize), String> {
        let mons = monitors();
        let (x, y, w, h) = if monitor == 0 || monitor > mons.len() {
            unsafe {
                (
                    GetSystemMetrics(SM_XVIRTUALSCREEN),
                    GetSystemMetrics(SM_YVIRTUALSCREEN),
                    GetSystemMetrics(SM_CXVIRTUALSCREEN),
                    GetSystemMetrics(SM_CYVIRTUALSCREEN),
                )
            }
        } else {
            mons[monitor - 1]
        };
        if w <= 0 || h <= 0 {
            return Err("Пустой экран".into());
        }
        unsafe {
            let screen = GetDC(null_mut());
            let mem = CreateCompatibleDC(screen);
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bmp);
            let ok = BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY | CAPTUREBLT);
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bi.bmiHeader.biWidth = w;
            bi.bmiHeader.biHeight = -h;
            bi.bmiHeader.biPlanes = 1;
            bi.bmiHeader.biBitCount = 32;
            bi.bmiHeader.biCompression = BI_RGB;
            let mut buf = vec![0u8; (w * h * 4) as usize];
            SelectObject(mem, old);
            let lines = GetDIBits(mem, bmp, 0, h as u32, buf.as_mut_ptr() as *mut _, &mut bi, DIB_RGB_COLORS);
            DeleteObject(bmp);
            DeleteDC(mem);
            ReleaseDC(null_mut(), screen);
            if ok == 0 || lines == 0 {
                return Err("Не удалось снять экран".into());
            }
            for px in buf.chunks_exact_mut(4) {
                px.swap(0, 2);
                px[3] = 255;
            }
            let img = image::RgbaImage::from_raw(w as u32, h as u32, buf).ok_or("bad image")?;
            Ok((img, mons.len()))
        }
    }

    /// Work area (x, y, w, h) of the monitor containing the given point.
    pub fn work_area_at(x: i32, y: i32) -> (i32, i32, i32, i32) {
        unsafe {
            let m = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
            let mut mi: MONITORINFO = std::mem::zeroed();
            mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(m, &mut mi) == 0 {
                return (0, 0, 1920, 1080);
            }
            let r = mi.rcWork;
            (r.left, r.top, r.right - r.left, r.bottom - r.top)
        }
    }

    pub fn play_wav(path: &std::path::Path) {
        use windows_sys::Win32::Media::Audio::*;
        let w = wide(&path.to_string_lossy());
        unsafe {
            PlaySoundW(w.as_ptr(), null_mut(), SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
        }
    }

    unsafe extern "system" fn enum_main(h: HWND, l: LPARAM) -> BOOL {
        let out = &mut *(l as *mut isize);
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        if pid == GetCurrentProcessId() && window_title(h).starts_with("Nassi RS —") {
            *out = h as isize;
            return 0;
        }
        1
    }

    /// Capture a monitor (1-based; 0 = whole virtual desktop).
    /// Returns (image, origin x, origin y, monitor count).
    pub fn capture(monitor: usize) -> Result<(image::RgbaImage, i32, i32, usize), String> {
        let mons = monitors();
        let (x, y) = if monitor == 0 || monitor > mons.len() {
            unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN)) }
        } else {
            (mons[monitor - 1].0, mons[monitor - 1].1)
        };
        let (img, n) = screenshot(monitor)?;
        Ok((img, x, y, n))
    }

    fn send_inputs(inputs: &[INPUT]) {
        unsafe {
            SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
    }

    fn mouse_input(flags: u32, data: i32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: data as _, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        }
    }

    fn key_input(vk: u16, scan: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        }
    }

    /// Move the cursor to virtual-desktop coordinates.
    pub fn inject_mouse_move(x: i32, y: i32) {
        unsafe {
            SetCursorPos(x, y);
        }
        // a zero relative move makes apps that listen to raw input notice the cursor
        send_inputs(&[mouse_input(MOUSEEVENTF_MOVE, 0)]);
    }

    pub fn inject_button(b: u8, down: bool) {
        let f = match (b, down) {
            (0, true) => MOUSEEVENTF_LEFTDOWN,
            (0, false) => MOUSEEVENTF_LEFTUP,
            (1, true) => MOUSEEVENTF_RIGHTDOWN,
            (1, false) => MOUSEEVENTF_RIGHTUP,
            (_, true) => MOUSEEVENTF_MIDDLEDOWN,
            (_, false) => MOUSEEVENTF_MIDDLEUP,
        };
        send_inputs(&[mouse_input(f, 0)]);
    }

    pub fn inject_wheel(dx: i32, dy: i32) {
        if dy != 0 {
            send_inputs(&[mouse_input(MOUSEEVENTF_WHEEL, dy)]);
        }
        if dx != 0 {
            send_inputs(&[mouse_input(MOUSEEVENTF_HWHEEL, dx)]);
        }
    }

    pub fn inject_key(vk: u16, down: bool) {
        // arrows, ins/del, home/end, pgup/pgdn, win, apps, right ctrl/alt, numpad divide
        let extended = matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0x5D | 0xA3 | 0xA5 | 0x6F | 0x2C);
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
        let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
        if extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        send_inputs(&[key_input(vk, scan, flags)]);
    }

    pub fn inject_text(s: &str) {
        let mut v = Vec::new();
        for u in s.encode_utf16() {
            v.push(key_input(0, u, KEYEVENTF_UNICODE));
            v.push(key_input(0, u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
        }
        if !v.is_empty() {
            send_inputs(&v);
        }
    }

    // GetWindowLongPtrW returns isize on x64 but i32 on x86 — normalize to isize.
    unsafe fn get_ex(h: HWND) -> isize {
        GetWindowLongPtrW(h, GWL_EXSTYLE) as isize
    }

    unsafe fn set_ex(h: HWND, v: isize) {
        SetWindowLongPtrW(h, GWL_EXSTYLE, v as _);
    }

    /// winit rewrites the extended style on some state changes — check it is still applied.
    pub fn is_stealth(hwnd: isize) -> bool {
        unsafe {
            let h = hwnd as HWND;
            let ex = get_ex(h);
            ex & WS_EX_LAYERED as isize != 0 && ex & WS_EX_TOOLWINDOW as isize != 0 && IsWindowVisible(h) != 0
        }
    }

    /// The main window of this process (title "Nassi RS — …").
    pub fn find_main_window() -> Option<isize> {
        let mut h: isize = 0;
        unsafe {
            EnumWindows(Some(enum_main), &mut h as *mut isize as LPARAM);
        }
        (h != 0).then_some(h)
    }

    /// Blink the taskbar button and the caption until `on == false` (FLASHW_STOP).
    pub fn flash_window(hwnd: isize, on: bool) {
        let fi = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd: hwnd as HWND,
            dwFlags: if on { FLASHW_ALL | FLASHW_TIMER } else { FLASHW_STOP },
            uCount: if on { u32::MAX } else { 0 },
            dwTimeout: 0,
        };
        unsafe {
            FlashWindowEx(&fi);
        }
    }

    /// "Tray mode" without hiding the window: a hidden (SW_HIDE) window gets no
    /// WM_PAINT, which would freeze the egui loop and with it all message
    /// processing. Instead the window becomes fully transparent, click-through
    /// and disappears from the taskbar / Alt+Tab, while it keeps running.
    pub fn set_stealth(hwnd: isize, on: bool) {
        let h = hwnd as HWND;
        let bits = (WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW) as isize;
        unsafe {
            let ex = get_ex(h);
            ShowWindow(h, SW_HIDE);
            if on {
                set_ex(h, (ex | bits) & !(WS_EX_APPWINDOW as isize));
                SetLayeredWindowAttributes(h, 0, 0, LWA_ALPHA);
                ShowWindow(h, SW_SHOWNOACTIVATE);
                SetWindowPos(h, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            } else {
                set_ex(h, ex & !bits);
                if IsIconic(h) != 0 {
                    ShowWindow(h, SW_RESTORE);
                } else {
                    ShowWindow(h, SW_SHOW);
                }
                SetForegroundWindow(h);
            }
        }
    }
}

#[cfg(windows)]
pub use win::*;

/// What the OpenGL driver offers: (version, renderer, vendor) of a legacy WGL context,
/// i.e. the highest compatibility version. Err when there is no usable OpenGL at all.
#[cfg(windows)]
pub fn probe_opengl() -> Result<(String, String, String), String> {
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::Graphics::OpenGL::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe fn gl_str(name: u32) -> String {
        let p = glGetString(name);
        if p.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(p as *const std::ffi::c_char).to_string_lossy().trim().to_string()
    }
    let class: Vec<u16> = "STATIC".encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let hwnd = CreateWindowExW(0, class.as_ptr(), std::ptr::null(), WS_POPUP, 0, 0, 16, 16, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
        if hwnd.is_null() {
            return Err("CreateWindowEx failed".into());
        }
        let dc = GetDC(hwnd);
        let mut pfd: PIXELFORMATDESCRIPTOR = std::mem::zeroed();
        pfd.nSize = std::mem::size_of::<PIXELFORMATDESCRIPTOR>() as u16;
        pfd.nVersion = 1;
        pfd.dwFlags = PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER;
        pfd.iPixelType = PFD_TYPE_RGBA;
        pfd.cColorBits = 32;
        pfd.iLayerType = PFD_MAIN_PLANE as u8;
        let mut result = Err("нет подходящего формата пикселей OpenGL".to_string());
        let pf = ChoosePixelFormat(dc, &pfd);
        if pf != 0 && SetPixelFormat(dc, pf, &pfd) != 0 {
            let rc = wglCreateContext(dc);
            if rc.is_null() {
                result = Err("wglCreateContext failed".into());
            } else {
                if wglMakeCurrent(dc, rc) != 0 {
                    result = Ok((gl_str(GL_VERSION), gl_str(GL_RENDERER), gl_str(GL_VENDOR)));
                    wglMakeCurrent(std::ptr::null_mut(), std::ptr::null_mut());
                } else {
                    result = Err("wglMakeCurrent failed".into());
                }
                wglDeleteContext(rc);
            }
        }
        ReleaseDC(hwnd, dc);
        DestroyWindow(hwnd);
        result
    }
}

/// Modal error box (the release build has no console to print to).
pub fn error_box(title: &str, text: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let (t, m) = (w(title), w(text));
        unsafe {
            MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), MB_OK | MB_ICONERROR | MB_SETFOREGROUND);
        }
    }
    #[cfg(not(windows))]
    eprintln!("{title}: {text}");
}

#[cfg(not(windows))]
mod stub {
    use super::*;
    pub fn idle_secs() -> u32 { 0 }
    pub fn os_uptime_secs() -> u32 { 0 }
    pub fn foreground_title() -> String { String::new() }
    #[allow(dead_code)]
    pub fn cursor_pos() -> (i32, i32) { (0, 0) }
    pub fn os_name() -> String { std::env::consts::OS.into() }
    pub fn processes() -> Vec<ProcInfo> { Vec::new() }
    pub fn kill(_pid: u32) -> Result<(), String> { Err("unsupported".into()) }
    pub fn monitors() -> Vec<(i32, i32, i32, i32)> { Vec::new() }
    pub fn screenshot(_m: usize) -> Result<(image::RgbaImage, usize), String> { Err("unsupported".into()) }
    pub fn work_area_at(_x: i32, _y: i32) -> (i32, i32, i32, i32) { (0, 0, 1920, 1080) }
    pub fn play_wav(_p: &std::path::Path) {}
    pub fn find_main_window() -> Option<isize> { None }
    pub fn set_stealth(_h: isize, _on: bool) {}
    pub fn flash_window(_h: isize, _on: bool) {}
    pub fn is_stealth(_h: isize) -> bool { true }
    pub fn capture(_m: usize) -> Result<(image::RgbaImage, i32, i32, usize), String> { Err("unsupported".into()) }
    pub fn inject_mouse_move(_x: i32, _y: i32) {}
    pub fn inject_button(_b: u8, _down: bool) {}
    pub fn inject_wheel(_dx: i32, _dy: i32) {}
    pub fn inject_key(_vk: u16, _down: bool) {}
    pub fn inject_text(_s: &str) {}
}
#[cfg(not(windows))]
pub use stub::*;

/// Open a file / folder / URL with the system shell.
pub fn shell_open(target: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(target).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(target).spawn();
    }
}

/// Run a command line (cmd /C) with UTF-8 output, waiting up to `timeout`.
pub fn exec(cmd: &str, timeout: std::time::Duration) -> String {
    let mut c = std::process::Command::new("cmd");
    c.arg("/C").arg(format!("chcp 65001>nul & {cmd}"));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(c.output());
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(o)) => {
            let mut s = String::from_utf8_lossy(&o.stdout).to_string();
            let e = String::from_utf8_lossy(&o.stderr);
            if !e.trim().is_empty() {
                s.push_str(&e);
            }
            if let Some(code) = o.status.code() {
                if code != 0 {
                    s.push_str(&format!("\n[код завершения {code}]"));
                }
            }
            if s.len() > 200_000 {
                let mut cut = 200_000;
                while !s.is_char_boundary(cut) {
                    cut -= 1;
                }
                s.truncate(cut);
                s.push_str("\n…(вывод обрезан)");
            }
            s
        }
        Ok(Err(e)) => format!("Ошибка запуска: {e}"),
        Err(_) => format!("Команда выполняется дольше {} с — ответ не дождались (процесс продолжает работать)", timeout.as_secs()),
    }
}

/// 0 shutdown, 1 reboot, 2 log off, 3 lock
pub fn power(action: u8) -> Result<(), String> {
    let (exe, args): (&str, &[&str]) = match action {
        0 => ("shutdown", &["/s", "/t", "5", "/c", "Выключение по команде администратора (Nassi RS)"]),
        1 => ("shutdown", &["/r", "/t", "5", "/c", "Перезагрузка по команде администратора (Nassi RS)"]),
        2 => ("shutdown", &["/l"]),
        _ => ("rundll32.exe", &["user32.dll,LockWorkStation"]),
    };
    std::process::Command::new(exe).args(args).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// English <-> Russian keyboard layout text revert (Alt+R).
pub fn revert_layout(s: &str) -> String {
    const EN: &str = "`qwertyuiop[]asdfghjkl;'zxcvbnm,./~QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>?@#$^&";
    const RU: &str = "ёйцукенгшщзхъфывапролджэячсмитьбю.ЁЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,\"№;:?";
    let en: Vec<char> = EN.chars().collect();
    let ru: Vec<char> = RU.chars().collect();
    let ru_count = s.chars().filter(|c| ru.contains(c) && !en.contains(c)).count();
    let en_count = s.chars().filter(|c| en.contains(c) && !ru.contains(c)).count();
    let (from, to) = if ru_count > en_count { (&ru, &en) } else { (&en, &ru) };
    s.chars()
        .map(|c| match from.iter().position(|&x| x == c) {
            Some(i) if i < to.len() => to[i],
            _ => c,
        })
        .collect()
}

#[cfg(all(test, windows))]
mod tests {
    /// Output longer than the limit must be cut on a UTF-8 char boundary (no panic).
    #[test]
    fn exec_truncates_long_utf8_output() {
        let p = std::env::temp_dir().join(format!("nassi-exec-{:x}.txt", rand::random::<u64>()));
        let mut s = String::from("a");
        s.push_str(&"я".repeat(150_000)); // byte 200 000 falls inside a 2-byte char
        std::fs::write(&p, &s).unwrap();
        let out = super::exec(&format!("type {}", p.display()), std::time::Duration::from_secs(30));
        let _ = std::fs::remove_file(&p);
        assert!(out.starts_with("aя"));
        assert!(out.ends_with("(вывод обрезан)"));
    }
}
