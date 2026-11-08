//! macOS: Lightroom's window from the window server's list, the displays from AppKit. See
//! `win.rs` for the interface.
//!
//! The window list and AppKit both work in points, so rectangles are converted to physical
//! pixels with their display's scale (2 on Retina), as Slint's window positions are. AppKit's
//! origin is the primary display's bottom-left corner, y up; the window list's, like Slint's,
//! is its top-left corner, y down. With displays of different scales, physical positions are
//! only approximate: Slint converts them with the window's own scale.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSScreen};
use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    kCGNullWindowID, kCGWindowBounds, kCGWindowLayer, kCGWindowOwnerName, kCGWindowOwnerPID,
    CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo, CGWindowListOption,
};

use crate::ui_model::Rect;

/// Lightroom's main window (else the main display's work area), its display's work area,
/// and the display's scale.
pub fn lightroom_window() -> Option<(Rect, Rect, f32)> {
    let screens = screens();
    let (target, screen) = match main_window(lightroom_pid()).and_then(|w| Some((w, screen_of(&screens, w)?))) {
        Some(found) => found,
        None => {
            let main = screens.first()?;
            (main.visible, main)
        }
    };
    Some((to_px(target, screen.scale), to_px(screen.visible, screen.scale), screen.scale as f32))
}

/// The work area and scale of the display `r` is (mostly) on, or None if it is on none:
/// that display was unplugged or moved.
pub fn monitor_of(r: Rect) -> Option<(Rect, f32)> {
    screens()
        .iter()
        .map(|s| (overlap(r, to_px(s.frame, s.scale)), s))
        .filter(|(area, _)| *area > 0)
        .max_by_key(|(area, _)| *area)
        .map(|(_, s)| (to_px(s.visible, s.scale), s.scale as f32))
}

/// The menus follow the system's appearance, which is what the user chose.
pub fn dark_menus() {}

/// Brings the app to the front: it is started by Lightroom through a shell, so macOS doesn't
/// activate it by itself. Since macOS 14 activation is cooperative, so macOS may still decline
/// (the Dock icon then bounces).
pub fn activate() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    if objc2::available!(macos = 14.0) {
        app.activate();
    } else {
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
}

/// A display, in points with a top-left origin.
struct Screen {
    frame: CGRect,
    /// The frame without the menu bar and the Dock.
    visible: CGRect,
    scale: f64,
}

/// The displays, the primary one (with the menu bar) first; none off the main thread.
fn screens() -> Vec<Screen> {
    let Some(mtm) = MainThreadMarker::new() else { return Vec::new() };
    let screens = NSScreen::screens(mtm);
    let Some(primary) = screens.firstObject() else { return Vec::new() };
    let height = primary.frame().size.height;
    let flip = |r: CGRect| CGRect::new(CGPoint::new(r.origin.x, height - r.origin.y - r.size.height), r.size);
    screens
        .iter()
        .map(|s| Screen { frame: flip(s.frame()), visible: flip(s.visibleFrame()), scale: s.backingScaleFactor() })
        .collect()
}

/// The display `r` (in points) is mostly on.
fn screen_of(screens: &[Screen], r: CGRect) -> Option<&Screen> {
    let r = to_px(r, 1.0);
    screens.iter().max_by_key(|s| overlap(r, to_px(s.frame, 1.0)))
}

fn to_px(r: CGRect, scale: f64) -> Rect {
    let px = |v: f64| (v * scale).round() as i32;
    Rect { x: px(r.origin.x), y: px(r.origin.y), w: px(r.size.width), h: px(r.size.height) }
}

fn overlap(a: Rect, b: Rect) -> i64 {
    let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    if w > 0 && h > 0 {
        i64::from(w) * i64::from(h)
    } else {
        0
    }
}

/// Whether a process or app name is Lightroom's ("Adobe Lightroom Classic").
fn is_lightroom(name: &str) -> bool {
    name.to_lowercase().contains("lightroom")
}

/// Lightroom's process: the nearest ancestor (Lightroom -> sh -> this app) named Lightroom.
fn lightroom_pid() -> Option<i32> {
    let mut pid = std::process::id() as i32;
    for _ in 0..4 {
        let parent = process_info(pid)?.pbi_ppid as i32;
        let info = process_info(parent)?;
        // pbi_name is the full name (up to 32 bytes); pbi_comm is cut to 16.
        let name: Vec<u8> = info.pbi_name.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
        if is_lightroom(&String::from_utf8_lossy(&name)) {
            return Some(parent);
        }
        pid = parent;
    }
    None
}

fn process_info(pid: i32) -> Option<libc::proc_bsdinfo> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: proc_pidinfo writes at most `size` bytes into `info`.
    let written = unsafe {
        libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size)
    };
    (written == size).then_some(info)
}

/// The largest on-screen normal window (layer 0) of Lightroom: of process `pid` if known,
/// else of the app named Lightroom. Its bounds and owner don't need Screen Recording
/// permission (window titles would).
fn main_window(pid: Option<i32>) -> Option<CGRect> {
    let list = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        kCGNullWindowID,
    )?;
    // SAFETY: the window list is an array of dictionaries with string keys.
    let list: CFRetained<CFArray<CFDictionary<CFString, CFType>>> = unsafe { CFRetained::cast_unchecked(list) };
    let number = |window: &CFDictionary<CFString, CFType>, key: &CFString| {
        window.get(key)?.downcast::<CFNumber>().ok()?.as_i64()
    };
    let mut best: Option<CGRect> = None;
    for window in list.iter() {
        let owner = match pid {
            Some(pid) => number(&window, unsafe { kCGWindowOwnerPID }) == Some(i64::from(pid)),
            None => window
                .get(unsafe { kCGWindowOwnerName })
                .and_then(|name| name.downcast::<CFString>().ok())
                .is_some_and(|name| is_lightroom(&name.to_string())),
        };
        if !owner || number(&window, unsafe { kCGWindowLayer }) != Some(0) {
            continue;
        }
        let Some(bounds) = window.get(unsafe { kCGWindowBounds }) else { continue };
        let Ok(bounds) = bounds.downcast::<CFDictionary>() else { continue };
        let mut r = CGRect::new(CGPoint::ZERO, CGSize::ZERO);
        // SAFETY: `r` is a valid CGRect to write to.
        if !unsafe { CGRectMakeWithDictionaryRepresentation(Some(&bounds), &mut r) } {
            continue;
        }
        let area = |r: &CGRect| r.size.width * r.size.height;
        if best.as_ref().is_none_or(|b| area(&r) > area(b)) {
            best = Some(r);
        }
    }
    best
}

/// Finder, through `open`.
pub fn file_browser() -> Option<&'static str> {
    Some("open")
}
