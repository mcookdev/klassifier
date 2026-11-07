//! klassifier-ui: the plugin's review window. Reads the photos and their suggested keywords
//! from a file written by the plugin, shows them, and prints what the user decided. The
//! exchange format is described in `klassifier::ui_model`.
#![windows_subsystem = "windows"]

mod grid;
mod thumbs;
// Compiled in directly rather than through the `klassifier` lib: linking the lib pulls the
// classifier's dependencies (rawler, ONNX Runtime, tokenizers) into this exe on MinGW.
#[path = "../../ui_model.rs"]
mod ui_model;
mod win;

slint::include_modules!();

use std::cell::{Cell as StdCell, RefCell};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

use anyhow::{bail, Context, Result};
use slint::platform::Key;
use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, SharedString};
use ui_model::{move_selection, Item, Label, Move, Outcome, ReviewInput};

const USAGE: &str = "\
Usage: klassifier-ui --review --in <file>
       klassifier-ui --demo <folder> [--count <n>] [--select <n>] [--screenshot <png>]

  --review --in <file>  Review the photos in <file> (written by the plugin)
  --demo <folder>       Review the JPEGs in <folder> with made-up labels (for development);
                        --count repeats them up to <n> photos
  --select <n>          Start with photo <n> (from 1) selected
  --screenshot <png>    Render the window off screen to <png> instead of showing it";

/// Screenshot size, in pixels at scale 1.
const SCREENSHOT_SIZE: (u32, u32) = (1200, 800);

struct Args {
    input: Input,
    /// The photo selected first, from 0.
    select: usize,
    screenshot: Option<PathBuf>,
}

enum Input {
    Review(PathBuf),
    Demo(PathBuf, usize),
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args() -> Result<Args> {
    let (mut review, mut file, mut demo, mut count, mut screenshot) = (false, None, None, None, None);
    let mut select = 0;
    let mut it = std::env::args_os().skip(1);
    while let Some(arg) = it.next() {
        match arg.to_str() {
            Some("--review") => review = true,
            Some("--in") => file = Some(PathBuf::from(it.next().context("--in needs a file")?)),
            Some("--demo") => demo = Some(PathBuf::from(it.next().context("--demo needs a folder")?)),
            Some("--count") => {
                count = Some(
                    it.next()
                        .and_then(|n| n.to_str().and_then(|s| s.parse().ok()))
                        .filter(|&n: &usize| n > 0)
                        .context("--count needs a number above 0")?,
                )
            }
            Some("--select") => {
                select = it
                    .next()
                    .and_then(|n| n.to_str().and_then(|s| s.parse().ok()))
                    .filter(|&n: &usize| n > 0)
                    .context("--select needs a number above 0")?
                    - 1
            }
            Some("--screenshot") => screenshot = Some(PathBuf::from(it.next().context("--screenshot needs a file")?)),
            _ => bail!("unknown argument {}\n\n{USAGE}", arg.to_string_lossy()),
        }
    }
    let input = match (review, file, demo) {
        (true, Some(file), None) => Input::Review(file),
        (false, None, Some(dir)) => Input::Demo(dir, count.unwrap_or(0)),
        _ => bail!("give --review --in <file>, or --demo <folder>\n\n{USAGE}"),
    };
    Ok(Args { input, select, screenshot })
}

fn run() -> Result<()> {
    let args = parse_args()?;
    let input = match &args.input {
        Input::Review(file) => {
            let text = fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
            ui_model::parse_review(&text).with_context(|| format!("cannot read {}", file.display()))?
        }
        Input::Demo(dir, count) => demo_input(dir, *count)?,
    };
    if input.items.is_empty() {
        bail!("no photos to review");
    }
    let select = args.select.min(input.items.len() - 1);
    if let Some(png) = &args.screenshot {
        return screenshot(input, select, png);
    }
    let (outcome, items) = review(input, select)?;
    ui_model::write_result(&mut io::stdout().lock(), outcome, &items)?;
    Ok(())
}

/// Builds the window for `input`, with photo `select` in the side panel. `threaded`: load
/// images on worker threads (false for screenshots, which have no event loop to deliver them).
fn build(
    input: ReviewInput,
    select: usize,
    threaded: bool,
) -> Result<(ReviewWindow, Rc<RefCell<Vec<Item>>>, Rc<StdCell<Outcome>>)> {
    let ui = ReviewWindow::new()?;
    let items = Rc::new(RefCell::new(input.items));
    let model = grid::GridModel::new(items.clone(), &ui, threaded);
    ui.set_rows(ModelRc::from(model.clone() as Rc<dyn Model<Data = GridRow>>));
    ui.set_notes(input.notes.into());
    show_totals(&ui, &items.borrow());
    ui.set_selected(select as i32);
    model.show_panel(&ui, select);

    let outcome = Rc::new(StdCell::new(Outcome::Cancel));
    let finish = |result: Outcome| {
        let (weak, outcome) = (ui.as_weak(), outcome.clone());
        move || {
            outcome.set(result);
            if let Some(ui) = weak.upgrade() {
                let _ = ui.hide();
            }
        }
    };
    ui.on_accept_all(finish(Outcome::Accept));
    ui.on_cancel(finish(Outcome::Cancel));

    let weak = ui.as_weak();
    let m = model.clone();
    ui.on_columns_changed(move |columns| {
        if let Some(ui) = weak.upgrade() {
            m.set_scale(ui.window().scale_factor());
        }
        m.set_columns(columns.max(1) as usize);
    });

    let weak = ui.as_weak();
    let m = model.clone();
    ui.on_select(move |index| {
        if let Some(ui) = weak.upgrade() {
            ui.set_selected(index);
            m.show_panel(&ui, index as usize);
        }
    });

    let (weak, m, its) = (ui.as_weak(), model.clone(), items.clone());
    ui.on_toggle(move |k| {
        let Some(ui) = weak.upgrade() else { return };
        let index = ui.get_selected() as usize;
        its.borrow_mut()[index].toggle(k as usize);
        m.item_changed(index);
        m.update_panel_labels(&ui, index);
        show_totals(&ui, &its.borrow());
    });

    let (weak, its) = (ui.as_weak(), items.clone());
    ui.on_key(move |text| {
        let Some(ui) = weak.upgrade() else { return false };
        if let Some(mv) = key_move(&text, ui.get_visible_rows().max(1) as usize) {
            let from = ui.get_selected() as usize;
            let to = move_selection(from, mv, model.columns(), its.borrow().len());
            if to != from {
                ui.invoke_select(to as i32);
            }
            ui.invoke_scroll_to_selected();
            return true;
        }
        match label_key(&text) {
            Some(k) if k < its.borrow()[ui.get_selected() as usize].labels.len() => {
                ui.invoke_toggle(k as i32);
                true
            }
            _ => false,
        }
    });
    Ok((ui, items, outcome))
}

fn show_totals(ui: &ReviewWindow, items: &[Item]) {
    ui.set_totals(ui_model::totals_line(items).into());
    ui.set_edited_count(items.iter().filter(|item| item.edited).count() as i32);
}

/// The grid move for a key, if it is one. `page`: grid lines per Page Up / Page Down.
fn key_move(text: &str, page: usize) -> Option<Move> {
    let is = |key: Key| text == SharedString::from(key).as_str();
    Some(if is(Key::LeftArrow) {
        Move::Left
    } else if is(Key::RightArrow) {
        Move::Right
    } else if is(Key::UpArrow) {
        Move::Up
    } else if is(Key::DownArrow) {
        Move::Down
    } else if is(Key::Home) {
        Move::Home
    } else if is(Key::End) {
        Move::End
    } else if is(Key::PageUp) {
        Move::PageUp(page)
    } else if is(Key::PageDown) {
        Move::PageDown(page)
    } else {
        return None;
    })
}

/// The label a key ticks: 1-9 the first nine, 0 the tenth.
fn label_key(text: &str) -> Option<usize> {
    match text.chars().collect::<Vec<_>>()[..] {
        ['0'] => Some(9),
        [c @ '1'..='9'] => Some(c as usize - '1' as usize),
        _ => None,
    }
}

/// Shows the window until the user decides. Closing it counts as Cancel, after asking if
/// anything was changed.
fn review(input: ReviewInput, select: usize) -> Result<(Outcome, Vec<Item>)> {
    let (ui, items, outcome) = build(input, select, true)?;
    let weak = ui.as_weak();
    ui.window().on_close_requested(move || match weak.upgrade() {
        Some(ui) if ui.get_edited_count() > 0 => {
            ui.invoke_request_cancel();
            CloseRequestResponse::KeepWindowShown
        }
        _ => CloseRequestResponse::HideWindow,
    });
    win::place(ui.window());
    ui.run()?;
    let items = items.borrow().clone();
    Ok((outcome.get(), items))
}

/// A review of the JPEGs in `dir` (repeated up to `count` photos, if more), with made-up
/// labels, so the window can be tried without Lightroom.
fn demo_input(dir: &Path, count: usize) -> Result<ReviewInput> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            (name.ends_with(".jpg") || name.ends_with(".jpeg")) && !name.ends_with(".lr.jpg")
        })
        .collect();
    files.sort();
    if files.is_empty() {
        bail!("no JPEGs in {}", dir.display());
    }
    let words = ["dog", "beach", "sunset", "portrait", "street", "bird", "food", "mountain", "city", "family"];
    let items = (0..count.max(files.len()))
        .map(|i| {
            let file = &files[i % files.len()];
            let labels = (0..words.len())
                .map(|k| {
                    let name = words[(i + k) % words.len()];
                    let keyword = if name == "bird" { "Wildlife > Bird".into() } else { String::new() };
                    Label { name: name.into(), suggested: k < i % 4, chosen: false, keyword }
                })
                .collect();
            Item {
                name: file.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                copy_note: if i % 7 == 3 { "+1 virtual copy".into() } else { String::new() },
                photo_count: if i % 7 == 3 { 2 } else { 1 },
                edited: false,
                standin: file.clone(),
                rendered: file.with_extension("lr.jpg"),
                labels,
            }
        })
        .collect();
    Ok(ReviewInput { notes: "Labels: demo. 0 failed, 0 unsupported.".into(), items })
}

/// Renders the window off screen with the software renderer and saves it as a PNG.
fn screenshot(input: ReviewInput, select: usize, png: &Path) -> Result<()> {
    use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
    use slint::Rgb8Pixel;
    use slint::platform::{Platform, WindowAdapter};

    struct Offscreen(Rc<MinimalSoftwareWindow>);
    impl Platform for Offscreen {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
    }

    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Offscreen(window.clone())))
        .map_err(|e| anyhow::anyhow!("cannot set the off-screen platform: {e}"))?;
    let (w, h) = SCREENSHOT_SIZE;
    window.set_size(slint::PhysicalSize::new(w, h));
    let (ui, _items, _outcome) = build(input, select, false)?;
    ui.show()?;

    let mut pixels = vec![Rgb8Pixel::default(); (w * h) as usize];
    // A few frames: the first lays out the list, which then reports its column count.
    // `changed` handlers run from the event loop, which this has none of, so pass it on here.
    for _ in 0..4 {
        slint::platform::update_timers_and_animations();
        ui.invoke_columns_changed(ui.get_columns());
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, w as usize);
        });
    }
    let bytes: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
    image::RgbImage::from_raw(w, h, bytes)
        .context("screenshot buffer has the wrong size")?
        .save(png)
        .with_context(|| format!("cannot save {}", png.display()))?;
    Ok(())
}
