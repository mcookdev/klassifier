//! Opens the window centred over Lightroom's main window, on its monitor. Without this,
//! Windows picks the position and may use another monitor.

/// Smallest size the window opens at, in logical pixels.
#[cfg(windows)]
const MIN_SIZE: (f32, f32) = (900.0, 600.0);

pub fn place(window: &slint::Window) {
    #[cfg(windows)]
    if let Some((target, work, scale)) = imp::lightroom_window() {
        let min = ((MIN_SIZE.0 * scale) as i32, (MIN_SIZE.1 * scale) as i32);
        let r = crate::ui_model::place_window(target, work, min);
        window.set_size(slint::PhysicalSize::new(r.w as u32, r.h as u32));
        window.set_position(slint::PhysicalPosition::new(r.x, r.y));
    }
    #[cfg(not(windows))]
    let _ = window;
}

#[cfg(windows)]
mod imp {
    use std::collections::HashMap;

    use crate::ui_model::Rect;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        GW_OWNER,
    };

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
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return None;
            }
            let dpi = GetDpiForWindow(hwnd);
            let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            Some((rect(r), rect(info.rcWork), scale))
        }
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
            let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
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
