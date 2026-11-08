//! Other platforms (only built for the tests): the system places the windows.

use crate::ui_model::Rect;

pub fn lightroom_window() -> Option<(Rect, Rect, f32)> {
    None
}

pub fn monitor_of(_: Rect) -> Option<(Rect, f32)> {
    None
}

pub fn dark_menus() {}

pub fn activate() {}
