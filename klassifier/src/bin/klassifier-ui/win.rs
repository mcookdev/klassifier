//! Where the windows open: where the user last left them, if that is still on a monitor,
//! else centred over Lightroom's main window, on its monitor. Without this, Windows picks the
//! position and may use another monitor.

use crate::ui_model::{Geometry, Rect};

/// Smallest size a window opens at, in logical pixels.
#[cfg(windows)]
const MIN_SIZE: (f32, f32) = (900.0, 600.0);

/// How big a window is.
#[derive(Clone, Copy)]
pub enum Fit {
    /// Resizable: 80% of Lightroom's window, at least MIN_SIZE, or as the user left it.
    Large,
    /// A dialog that sizes itself: this many logical pixels, whatever was saved.
    Fixed(f32, f32),
}

/// Sizes and places `ui`'s window before it is shown: at `saved` (where it was last closed)
/// if that is on a monitor, else over Lightroom.
pub fn place<T: slint::ComponentHandle + 'static>(ui: &T, saved: Option<Geometry>, fit: Fit) {
    #[cfg(windows)]
    {
        use crate::ui_model::{center_window, place_window, restore_window};
        let window = ui.window();
        let px = |(w, h): (f32, f32), scale: f32| ((w * scale) as i32, (h * scale) as i32);
        let restored = saved.and_then(|saved| {
            let (work, scale) = imp::monitor_of(saved.rect)?;
            Some(match fit {
                Fit::Large => (restore_window(saved.rect, work, px(MIN_SIZE, scale)), saved.maximized),
                Fit::Fixed(w, h) => {
                    let (w, h) = px((w, h), scale);
                    (restore_window(Rect { w, h, ..saved.rect }, work, (w, h)), false)
                }
            })
        });
        let placed = restored.or_else(|| {
            let (target, work, scale) = imp::lightroom_window()?;
            Some(match fit {
                Fit::Large => (place_window(target, work, px(MIN_SIZE, scale)), false),
                Fit::Fixed(w, h) => (center_window(target, work, px((w, h), scale)), false),
            })
        });
        if let Some((r, maximized)) = placed {
            if let Fit::Large = fit {
                window.set_size(slint::PhysicalSize::new(r.w as u32, r.h as u32));
            }
            window.set_position(slint::PhysicalPosition::new(r.x, r.y));
            if maximized {
                // Once the window exists: before that there is nothing to maximize.
                let weak = ui.as_weak();
                slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.window().set_maximized(true);
                    }
                });
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (ui, saved, fit);
}

/// Makes the native menus (the label set list) dark, to match the windows. Call before any
/// menu is shown. Windows has no documented switch for this; uxtheme.dll's
/// SetPreferredAppMode (ordinal 135) and FlushMenuThemes (136), present since Windows 10
/// 1903, are what other apps use. Without them (older Windows) the menus stay light.
pub fn dark_menus() {
    #[cfg(windows)]
    imp::dark_menus();
}

/// Where `window` is now, to open there next time; None if it is minimized.
pub fn geometry(window: &slint::Window) -> Option<Geometry> {
    if window.is_minimized() {
        return None;
    }
    let (position, size) = (window.position(), window.size());
    let rect = Rect { x: position.x, y: position.y, w: size.width as i32, h: size.height as i32 };
    (rect.w > 0 && rect.h > 0).then_some(Geometry { rect, maximized: window.is_maximized() })
}

#[cfg(windows)]
mod imp {
    use std::collections::HashMap;

    use crate::ui_model::Rect;
    use windows::core::BOOL;
    use windows::core::{w, PCSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromRect, MonitorFromWindow, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        MONITOR_DEFAULTTONULL,
    };
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, GW_OWNER,
    };

    pub fn dark_menus() {
        const FORCE_DARK: i32 = 2;
        unsafe {
            let Ok(uxtheme) = LoadLibraryExW(w!("uxtheme.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) else { return };
            // By ordinal: these functions have no exported names.
            let (Some(set_mode), Some(flush)) =
                (GetProcAddress(uxtheme, PCSTR(135 as *const u8)), GetProcAddress(uxtheme, PCSTR(136 as *const u8)))
            else {
                return;
            };
            let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set_mode);
            let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
            set_mode(FORCE_DARK);
            flush();
        }
    }

    /// Lightroom's main window (else the foreground window): its rectangle, its monitor's
    /// work area, and its display scale.
    pub fn lightroom_window() -> Option<(Rect, Rect, f32)> {
        let hwnd = lightroom_pid().and_then(main_window).or_else(|| {
            let fg = unsafe { GetForegroundWindow() };
            (!fg.is_invalid()).then_some(fg)
        })?;
        unsafe {
            let mut r = RECT::default();
            GetWindowRect(hwnd, &mut r).ok()?;
            let work = work_area(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST))?;
            let dpi = GetDpiForWindow(hwnd);
            let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            Some((rect(r), work, scale))
        }
    }

    /// The work area and display scale of the monitor `r` is on (most of), or None if it is
    /// on none: that monitor was unplugged or moved.
    pub fn monitor_of(r: Rect) -> Option<(Rect, f32)> {
        let r = RECT { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h };
        unsafe {
            let monitor = MonitorFromRect(&r, MONITOR_DEFAULTTONULL);
            if monitor.is_invalid() {
                return None;
            }
            let (mut dpi, mut unused) = (0, 0);
            let scale = match GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused) {
                Ok(()) if dpi > 0 => dpi as f32 / 96.0,
                _ => 1.0,
            };
            Some((work_area(monitor)?, scale))
        }
    }

    fn work_area(monitor: HMONITOR) -> Option<Rect> {
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        unsafe { GetMonitorInfoW(monitor, &mut info).as_bool() }.then(|| rect(info.rcWork))
    }

    fn rect(r: RECT) -> Rect {
        Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
    }

    /// Lightroom's process: the nearest ancestor (Lightroom -> cmd -> this exe) whose exe
    /// name starts with "lightroom".
    fn lightroom_pid() -> Option<u32> {
        let processes = processes()?;
        let mut pid = std::process::id();
        for _ in 0..4 {
            let (parent, _) = processes.get(&pid)?;
            let (_, exe) = processes.get(parent)?;
            if exe.starts_with("lightroom") {
                return Some(*parent);
            }
            pid = *parent;
        }
        None
    }

    /// pid -> (parent pid, lowercased exe name).
    fn processes() -> Option<HashMap<u32, (u32, String)>> {
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
            let mut entry =
                PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut map = HashMap::new();
            let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
            while more {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                let exe = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                map.insert(entry.th32ProcessID, (entry.th32ParentProcessID, exe));
                more = Process32NextW(snapshot, &mut entry).is_ok();
            }
            let _ = CloseHandle(snapshot);
            Some(map)
        }
    }

    struct Search {
        pid: u32,
        best: Option<(i64, HWND)>,
    }

    /// The largest visible, unowned, not minimized top-level window of `pid`.
    fn main_window(pid: u32) -> Option<HWND> {
        unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let search = unsafe { &mut *(lparam.0 as *mut Search) };
            let mut owner = 0;
            unsafe {
                GetWindowThreadProcessId(hwnd, Some(&mut owner));
                let unowned = GetWindow(hwnd, GW_OWNER).map_or(true, |o| o.is_invalid());
                let mut r = RECT::default();
                if owner == search.pid
                    && unowned
                    && IsWindowVisible(hwnd).as_bool()
                    && !IsIconic(hwnd).as_bool()
                    && GetWindowRect(hwnd, &mut r).is_ok()
                {
                    let area = i64::from(r.right - r.left) * i64::from(r.bottom - r.top);
                    if search.best.is_none_or(|(a, _)| area > a) {
                        search.best = Some((area, hwnd));
                    }
                }
            }
            BOOL(1)
        }
        let mut search = Search { pid, best: None };
        unsafe {
            let _ = EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize));
        }
        search.best.map(|(_, hwnd)| hwnd)
    }
}
