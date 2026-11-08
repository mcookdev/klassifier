//! Where the windows open: where the user last left them, if that is still on a monitor,
//! else centred over Lightroom's main window, on its monitor. Without this, the system picks
//! the position and may use another monitor. The positioning rules are shared (and tested in
//! `ui_model`); finding Lightroom's window and the monitors is per platform, in `platform`.
//!
//! Each `platform` module provides, in physical pixels with a top-left origin (as Slint's
//! window positions are):
//! - `lightroom_window()`: Lightroom's main window, its monitor's work area, and its scale;
//! - `monitor_of(rect)`: the work area and scale of the monitor `rect` is (mostly) on, or None;
//! - `dark_menus()` and `activate()`: see the functions below;
//! - `file_browser()`: the program that opens a folder in the system's file browser.

use std::time::Duration;

use crate::ui_model::{center_window, place_window, restore_window, Geometry, Rect};

#[cfg(windows)]
#[path = "win/windows.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "win/macos.rs"]
mod platform;

#[cfg(not(any(windows, target_os = "macos")))]
#[path = "win/other.rs"]
mod platform;

/// Smallest size a window opens at, in logical pixels.
const MIN_SIZE: (f32, f32) = (900.0, 600.0);

/// How big a window is.
#[derive(Clone, Copy)]
pub enum Fit {
    /// Resizable: 80% of Lightroom's window, at least MIN_SIZE, or as the user left it.
    Large,
    /// A dialog that sizes itself: this many logical pixels, whatever was saved.
    Fixed(f32, f32),
}

/// Moves `window` back into its monitor's work area after it grew. `place` clamps with the
/// height measured before the window is shown, which is too short, so a dialog that later
/// grows (text wraps, a warning appears) could end up with its bottom, and its buttons, off
/// screen. `size` is in logical pixels.
pub fn keep_on_screen(window: &slint::Window, size: (f32, f32)) {
    let scale = window.scale_factor();
    let (w, h) = ((size.0 * scale) as i32, (size.1 * scale) as i32);
    let pos = window.position();
    let rect = Rect { x: pos.x, y: pos.y, w, h };
    let Some((work, _)) = platform::monitor_of(rect) else { return };
    let moved = restore_window(rect, work, (w, h));
    if (moved.x, moved.y) != (pos.x, pos.y) {
        window.set_position(slint::PhysicalPosition::new(moved.x, moved.y));
    }
}

/// Sizes and places `ui`'s window before it is shown: at `saved` (where it was last closed)
/// if that is on a monitor, else over Lightroom. Once shown, it is brought to the front.
pub fn place<T: slint::ComponentHandle + 'static>(ui: &T, saved: Option<Geometry>, fit: Fit) {
    let window = ui.window();
    let px = |(w, h): (f32, f32), scale: f32| ((w * scale) as i32, (h * scale) as i32);
    let restored = saved.and_then(|saved| {
        let (work, scale) = platform::monitor_of(saved.rect)?;
        Some(match fit {
            Fit::Large => (restore_window(saved.rect, work, px(MIN_SIZE, scale)), saved.maximized),
            Fit::Fixed(w, h) => {
                let (w, h) = px((w, h), scale);
                (restore_window(Rect { w, h, ..saved.rect }, work, (w, h)), false)
            }
        })
    });
    let placed = restored.or_else(|| {
        let (target, work, scale) = platform::lightroom_window()?;
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
            slint::Timer::single_shot(Duration::ZERO, move || {
                if let Some(ui) = weak.upgrade() {
                    ui.window().set_maximized(true);
                }
            });
        }
    }
    // Runs once the event loop has started, so after the window is shown.
    slint::Timer::single_shot(Duration::ZERO, platform::activate);
}

/// Makes the native menus (the label set list) dark, to match the windows. Call before any
/// menu is shown. Only Windows needs this (see `win/windows.rs`).
pub fn dark_menus() {
    platform::dark_menus();
}

/// Opens `dir` in the system's file browser (Finder, Explorer), without waiting for it.
pub fn open_folder(dir: &std::path::Path) {
    let Some(program) = platform::file_browser() else { return };
    if let Err(e) = std::process::Command::new(program).arg(dir).spawn() {
        eprintln!("cannot open {} with {program}: {e}", dir.display());
    }
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
