//! klassifier-ui: the plugin's windows. The options dialog asks what to classify with; the
//! review window reads the photos and their suggested keywords from a file written by the
//! plugin, shows them, and prints what the user decided; the report window shows why no photo
//! could be classified. The exchange formats are described in `klassifier::ui_model`.
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
use std::time::Duration;

use anyhow::{bail, Context, Result};
use slint::platform::Key;
use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, SharedString, VecModel};
use ui_model::{
    move_selection, Geometry, Item, Label, LineReader, Move, OptionsChoice, OptionsInput, Outcome, ReportInput,
    ReviewInput, Save, SaveFiles,
};
use win::Fit;

const USAGE: &str = "\
Usage: klassifier-ui --options --in <file>
       klassifier-ui --review --in <file>
       klassifier-ui --report --in <file>
       klassifier-ui --demo <folder> [--count <n>] [--select <n>] [--screenshot <png>]

  --options --in <file> Ask for the Classify Photos options set out in <file>
  --review --in <file>  Review the photos in <file> (written by the plugin)
  --report --in <file>  Show the report in <file> (written by the plugin)
  --demo <folder>       Review the JPEGs in <folder> with made-up labels (for development);
                        --count repeats them up to <n> photos
  --select <n>          Start with photo <n> (from 1) selected
  --screenshot <png>    Render the window off screen to <png> instead of showing it";

/// How often the answers file is checked while a Save Keywords waits for the plugin.
const ANSWER_INTERVAL: Duration = Duration::from_millis(150);

/// Screenshot size, in pixels at scale 1.
const SCREENSHOT_SIZE: (u32, u32) = (1200, 800);

/// The options dialog's width, in logical pixels: `width` in ui/options.slint.
const OPTIONS_WIDTH: f32 = 560.0;

struct Args {
    input: Input,
    /// The photo selected first, from 0.
    select: usize,
    screenshot: Option<PathBuf>,
}

enum Input {
    Options(PathBuf),
    Review(PathBuf),
    Report(PathBuf),
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
    let (mut options, mut review, mut report) = (false, false, false);
    let (mut file, mut demo, mut count, mut screenshot) = (None, None, None, None);
    let mut select = 0;
    let mut it = std::env::args_os().skip(1);
    while let Some(arg) = it.next() {
        match arg.to_str() {
            Some("--options") => options = true,
            Some("--review") => review = true,
            Some("--report") => report = true,
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
    let input = match (options, review, report, file, demo) {
        (true, false, false, Some(file), None) => Input::Options(file),
        (false, true, false, Some(file), None) => Input::Review(file),
        (false, false, true, Some(file), None) => Input::Report(file),
        (false, false, false, None, Some(dir)) => Input::Demo(dir, count.unwrap_or(0)),
        _ => bail!("give --options, --review or --report with --in <file>, or --demo <folder>\n\n{USAGE}"),
    };
    Ok(Args { input, select, screenshot })
}

fn read(file: &Path) -> Result<String> {
    fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))
}

fn run() -> Result<()> {
    let args = parse_args()?;
    win::dark_menus();
    let input = match &args.input {
        Input::Review(file) => {
            ui_model::parse_review(&read(file)?).with_context(|| format!("cannot read {}", file.display()))?
        }
        Input::Options(file) => {
            let options =
                ui_model::parse_options(&read(file)?).with_context(|| format!("cannot read {}", file.display()))?;
            return show_options(options, args.screenshot.as_deref());
        }
        Input::Report(file) => {
            let report =
                ui_model::parse_report(&read(file)?).with_context(|| format!("cannot read {}", file.display()))?;
            return show_report(report, args.screenshot.as_deref());
        }
        Input::Demo(dir, count) => demo_input(dir, *count)?,
    };
    if input.items.is_empty() {
        bail!("no photos to review");
    }
    let select = args.select.min(input.items.len() - 1);
    let saver = match (&args.input, &input.save_files) {
        (Input::Demo(..), _) => Saver::Instant,
        (_, Some(files)) => Saver::Files(files.clone()),
        (_, None) => Saver::None,
    };
    if let Some(png) = &args.screenshot {
        // Off screen there is no event loop to run `changed` handlers, so pass the column
        // count on each frame.
        return screenshot(
            png,
            || Ok(build(input, select, saver, false)?.0),
            |_| SCREENSHOT_SIZE,
            |ui: &ReviewWindow| ui.invoke_columns_changed(ui.get_columns()),
        );
    }
    let (closed, items) = review(input, select, saver)?;
    ui_model::write_result(&mut io::stdout().lock(), closed.outcome, &items, closed.window)?;
    Ok(())
}

/// Shows the no-results report until closed (or saves a picture of it to `screenshot`).
fn show_report(report: ReportInput, screenshot_png: Option<&Path>) -> Result<()> {
    let closed_at = Rc::new(StdCell::new(None));
    let make = || -> Result<ReportWindow> {
        let ui = ReportWindow::new()?;
        ui.set_summary(report.summary.as_str().into());
        ui.set_report(report.lines.join("\n").into());
        let (weak, at) = (ui.as_weak(), closed_at.clone());
        ui.on_ok(move || {
            if let Some(ui) = weak.upgrade() {
                at.set(win::geometry(ui.window()));
                let _ = ui.hide();
            }
        });
        let (weak, at) = (ui.as_weak(), closed_at.clone());
        ui.window().on_close_requested(move || {
            if let Some(ui) = weak.upgrade() {
                at.set(win::geometry(ui.window()));
            }
            CloseRequestResponse::HideWindow
        });
        Ok(ui)
    };
    if let Some(png) = screenshot_png {
        return screenshot(png, make, |_| SCREENSHOT_SIZE, |_| {});
    }
    let ui = make()?;
    win::place(&ui, report.window, Fit::Large);
    ui.run()?;
    ui_model::write_result(&mut io::stdout().lock(), Outcome::Ok, &[], closed_at.get())?;
    Ok(())
}

/// What the review window ends with: the outcome, and where the window was.
#[derive(Clone, Copy)]
struct Closed {
    outcome: Outcome,
    window: Option<Geometry>,
}

/// How Save Keywords writes a photo's keywords.
#[derive(Clone)]
enum Saver {
    /// No Save Keywords.
    None,
    /// Through the plugin (see `ui_model`).
    Files(SaveFiles),
    /// At once, for `--demo`: nothing is written.
    Instant,
}

/// The review window, its items and how it closed (see build).
type Built = (ReviewWindow, Rc<RefCell<Vec<Item>>>, Rc<StdCell<Closed>>);

/// Builds the window for `input`, with photo `select` in the side panel. `threaded`: load
/// images on worker threads (false for screenshots, which have no event loop to deliver them).
fn build(input: ReviewInput, select: usize, saver: Saver, threaded: bool) -> Result<Built> {
    let ui = ReviewWindow::new()?;
    ui.set_version(env!("CARGO_PKG_VERSION").into());
    ui.set_can_save_keywords(!matches!(saver, Saver::None));
    let items = Rc::new(RefCell::new(input.items));
    let model = grid::GridModel::new(items.clone(), &ui, threaded);
    ui.set_rows(ModelRc::from(model.clone() as Rc<dyn Model<Data = GridRow>>));
    ui.set_notes(input.notes.into());
    show_totals(&ui, &items.borrow());
    ui.set_selected(select as i32);
    model.show_panel(&ui, select);

    let closed = Rc::new(StdCell::new(Closed { outcome: Outcome::Cancel, window: None }));
    let finish = |outcome: Outcome| {
        let (weak, closed) = (ui.as_weak(), closed.clone());
        move || {
            if let Some(ui) = weak.upgrade() {
                closed.set(Closed { outcome, window: win::geometry(ui.window()) });
                let _ = ui.hide();
            }
        }
    };
    ui.on_save_all(finish(Outcome::Accept));
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

    let (weak, m, its) = (ui.as_weak(), model.clone(), items.clone());
    ui.on_save_keywords(move || {
        let Some(ui) = weak.upgrade() else { return };
        let index = ui.get_selected() as usize;
        {
            let mut items = its.borrow_mut();
            let item = &mut items[index];
            if !item.can_save() {
                return;
            }
            item.save = match &saver {
                Saver::None => return,
                Saver::Instant => Save::Saved,
                Saver::Files(files) => match ui_model::append_line(&files.requests, &item.save_request(index + 1)) {
                    Ok(()) => Save::Saving,
                    Err(e) => Save::Failed(format!("cannot write {}: {e}", files.requests.display())),
                },
            };
        }
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
    Ok((ui, items, closed))
}

fn show_totals(ui: &ReviewWindow, items: &[Item]) {
    ui.set_totals(ui_model::totals_line(items).into());
    ui.set_edited_count(items.iter().filter(|item| item.unsaved_edit()).count() as i32);
    ui.set_saved_count(items.iter().filter(|item| item.save == Save::Saved).count() as i32);
}

/// Checks the answers file for the plugin's answers to Save Keywords while any is waiting, and
/// shows them. The timer runs until it is dropped.
fn watch_answers(ui: &ReviewWindow, items: Rc<RefCell<Vec<Item>>>, files: SaveFiles) -> slint::Timer {
    let timer = slint::Timer::default();
    let mut reader = LineReader::default();
    let weak = ui.as_weak();
    timer.start(slint::TimerMode::Repeated, ANSWER_INTERVAL, move || {
        let Some(ui) = weak.upgrade() else { return };
        if !items.borrow().iter().any(|item| item.save == Save::Saving) {
            return;
        }
        // Unreadable for now (the plugin writing it): try again next time.
        let Ok(lines) = reader.read_new(&files.answers) else { return };
        let answers: Vec<_> = lines.iter().filter_map(|line| ui_model::parse_answer(line)).collect();
        if answers.is_empty() {
            return;
        }
        let rows = ui.get_rows();
        let Some(model) = rows.as_any().downcast_ref::<grid::GridModel>() else { return };
        for (index, answer) in answers {
            ui_model::apply_answer(&mut items.borrow_mut(), index, answer);
            if index < items.borrow().len() {
                model.item_changed(index);
            }
        }
        model.update_panel_labels(&ui, ui.get_selected() as usize);
        show_totals(&ui, &items.borrow());
    });
    timer
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
fn review(input: ReviewInput, select: usize, saver: Saver) -> Result<(Closed, Vec<Item>)> {
    let saved = input.window;
    let files = match &saver {
        Saver::Files(files) => Some(files.clone()),
        _ => None,
    };
    let (ui, items, closed) = build(input, select, saver, true)?;
    let _answers = files.map(|files| watch_answers(&ui, items.clone(), files));
    let (weak, c) = (ui.as_weak(), closed.clone());
    ui.window().on_close_requested(move || match weak.upgrade() {
        Some(ui) if ui.get_edited_count() > 0 => {
            ui.invoke_request_cancel();
            CloseRequestResponse::KeepWindowShown
        }
        Some(ui) => {
            c.set(Closed { outcome: Outcome::Cancel, window: win::geometry(ui.window()) });
            CloseRequestResponse::HideWindow
        }
        None => CloseRequestResponse::HideWindow,
    });
    win::place(&ui, saved, Fit::Large);
    ui.run()?;
    let items = items.borrow().clone();
    Ok((closed.get(), items))
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
                save: Save::Unsaved,
            }
        })
        .collect();
    Ok(ReviewInput { notes: "Labels: demo. 0 failed, 0 unsupported.".into(), items, window: None, save_files: None })
}

/// Asks for the options until the user classifies or cancels (or saves a picture of the
/// dialog to `screenshot`).
fn show_options(input: OptionsInput, screenshot_png: Option<&Path>) -> Result<()> {
    type Closed = (Outcome, Option<OptionsChoice>, Option<Geometry>);
    let closed: Rc<RefCell<Closed>> = Rc::new(RefCell::new((Outcome::Cancel, None, None)));
    let make = || -> Result<OptionsWindow> {
        let ui = OptionsWindow::new()?;
        let names: Vec<SharedString> = input.sets.iter().map(|s| s.as_str().into()).collect();
        ui.set_sets(ModelRc::new(VecModel::from(names)));
        ui.set_set_index(input.set_index as i32);
        ui.set_hints(input.hints.as_str().into());
        ui.set_only_hints(input.only_hints);
        ui.set_count(input.count as i32);
        ui.set_warn_above(input.warn_above as i32);
        ui.on_problem_with(|hints, only| ui_model::options_problem(&hints, only).unwrap_or_default().into());

        let (weak, c, sets) = (ui.as_weak(), closed.clone(), input.sets.clone());
        ui.on_classify(move || {
            let Some(ui) = weak.upgrade() else { return };
            let (hints, only_hints) = (ui.get_hints().to_string(), ui.get_only_hints());
            if ui_model::options_problem(&hints, only_hints).is_some() {
                return;
            }
            let label_set = sets.get(ui.get_set_index().max(0) as usize).unwrap_or(&sets[0]).clone();
            *c.borrow_mut() =
                (Outcome::Ok, Some(OptionsChoice { label_set, hints, only_hints }), win::geometry(ui.window()));
            let _ = ui.hide();
        });
        let (weak, c) = (ui.as_weak(), closed.clone());
        ui.on_cancel(move || {
            if let Some(ui) = weak.upgrade() {
                *c.borrow_mut() = (Outcome::Cancel, None, win::geometry(ui.window()));
                let _ = ui.hide();
            }
        });
        let (weak, c) = (ui.as_weak(), closed.clone());
        ui.window().on_close_requested(move || {
            if let Some(ui) = weak.upgrade() {
                *c.borrow_mut() = (Outcome::Cancel, None, win::geometry(ui.window()));
            }
            CloseRequestResponse::HideWindow
        });
        let weak = ui.as_weak();
        ui.on_natural_size_changed(move || {
            let weak = weak.clone();
            // Next turn of the event loop, so the new size is applied first.
            slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                if let Some(ui) = weak.upgrade() {
                    win::keep_on_screen(ui.window(), (OPTIONS_WIDTH, ui.get_natural_height()));
                }
            });
        });
        Ok(ui)
    };
    let natural = |ui: &OptionsWindow| (OPTIONS_WIDTH, ui.get_natural_height());
    if let Some(png) = screenshot_png {
        return screenshot(png, make, |ui| (natural(ui).0 as u32, natural(ui).1.ceil() as u32), |_| {});
    }
    let ui = make()?;
    let (w, h) = natural(&ui);
    // Measured before the window is shown, text a little short: off centre by a few pixels.
    win::place(&ui, input.window, Fit::Fixed(w, h));
    ui.run()?;
    let (outcome, choice, window) = closed.borrow().clone();
    ui_model::write_options_result(&mut io::stdout().lock(), outcome, choice.as_ref(), window)?;
    Ok(())
}

/// Renders the window `make` builds off screen with the software renderer, `size` pixels at
/// scale 1, and saves it as a PNG. `frame` runs before each of the few frames drawn.
fn screenshot<T: ComponentHandle>(
    png: &Path,
    make: impl FnOnce() -> Result<T>,
    size: impl Fn(&T) -> (u32, u32),
    frame: impl Fn(&T),
) -> Result<()> {
    use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
    use slint::platform::{Platform, WindowAdapter};
    use slint::Rgb8Pixel;

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
    let ui = make()?;
    ui.show()?;
    // A few frames: the first lays the window out, which later ones may depend on, and its
    // size (text can only be measured once the window is shown).
    let mut pixels = Vec::new();
    let (mut w, mut h) = (w, h);
    for _ in 0..4 {
        slint::platform::update_timers_and_animations();
        frame(&ui);
        (w, h) = size(&ui);
        window.set_size(slint::PhysicalSize::new(w, h));
        pixels.resize((w * h) as usize, Rgb8Pixel::default());
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
