//! Windows: Lightroom's window and the monitors from Win32. See `win.rs` for the interface.

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
    EnumWindows, GetForegroundWindow, GetWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    GW_OWNER,
};

/// Windows has no documented switch for dark menus; uxtheme.dll's SetPreferredAppMode
/// (ordinal 135) and FlushMenuThemes (136), present since Windows 10 1903, are what other
/// apps use. Without them (older Windows) the menus stay light.
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

/// Windows brings a new window to the front by itself.
pub fn activate() {}

/// File Explorer.
pub fn file_browser() -> Option<&'static str> {
    Some("explorer.exe")
}
