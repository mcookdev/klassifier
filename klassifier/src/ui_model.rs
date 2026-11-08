//! The review window's data, exchanged with the plugin as tab-separated lines. Kept free of
//! Slint so it builds and is tested on any platform.
//!
//! Input, written by `Review.lua`:
//!
//! ```text
//! #klassifier-ui<TAB>review<TAB>1
//! WINDOW<TAB><x><TAB><y><TAB><width><TAB><height><TAB><maximized 0|1>   (optional)
//! NOTES<TAB><text>
//! ITEM<TAB><n><TAB><name><TAB><copy note><TAB><photo count><TAB><edited 0|1><TAB><stand-in thumbnail><TAB><Lightroom thumbnail>
//! LABEL<TAB><n><TAB><label><TAB><suggested 0|1><TAB><chosen 0|1><TAB><keyword "Parent > Child", or empty>
//! ```
//!
//! Items are numbered from 1, in order, and each item's LABEL lines follow its ITEM line, best
//! label first. WINDOW is where the window was last closed, as the output gave it: its
//! outer top-left corner and inner size, in physical pixels. In NOTES, `\n` stands for a line
//! break and `\\` for a backslash. The stand-in
//! thumbnail is the one `klassifier.exe --thumbs` writes; the Lightroom one, rendered with the
//! photo's edits, may appear later. Neither file has to exist yet.
//!
//! Output, on stdout:
//!
//! ```text
//! #klassifier-ui<TAB><version>
//! RESULT<TAB>accept|cancel
//! EDITED<TAB><n>              (for each edited item)
//! CHOSEN<TAB><n><TAB><k>      (for each ticked label of an edited item; k counts its LABEL lines from 1)
//! WINDOW<TAB>...               (where the window was, as in the input; not if minimized)
//! ```
//!
//! The no-results report (`--report`) reads:
//!
//! ```text
//! #klassifier-ui<TAB>report<TAB>1
//! WINDOW<TAB>...               (optional, as for the review)
//! SUMMARY<TAB><text, escaped as NOTES>
//! LINE<TAB><text>             (one per report line; the rest of the line, tabs included)
//! ```
//!
//! and writes `RESULT<TAB>ok` and a WINDOW line when closed.
//!
//! The options dialog (`--options`) reads:
//!
//! ```text
//! #klassifier-ui<TAB>options<TAB>1
//! WINDOW<TAB>...               (optional, as for the review)
//! COUNT<TAB><photos to classify>
//! WARN_ABOVE<TAB><n>           (optional: above n photos, warn that it takes a while)
//! SET<TAB><label set>          (one per label set, in menu order)
//! LABELSET<TAB><name>          (optional: the set chosen last time)
//! HINTS<TAB><text, escaped as NOTES>
//! ONLYHINTS<TAB><0|1>
//! ```
//!
//! and writes RESULT ok|cancel, and for ok the LABELSET, HINTS and ONLYHINTS chosen, plus a
//! WINDOW line.
//!
//! Lines starting with `#` are informational. Readers ignore unknown lines and extra fields.

use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub name: String,
    pub suggested: bool,
    /// The user's choice; only meaningful once the item is edited.
    pub chosen: bool,
    /// Where the keyword goes ("Wildlife > Bird > Egret"), or empty for a top-level keyword.
    pub keyword: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub copy_note: String,
    /// Photos sharing the file (an original and its virtual copies); each gets the keywords.
    pub photo_count: usize,
    pub edited: bool,
    pub standin: PathBuf,
    pub rendered: PathBuf,
    pub labels: Vec<Label>,
}

#[derive(Debug, Default, PartialEq)]
pub struct ReviewInput {
    pub notes: String,
    pub items: Vec<Item>,
    pub window: Option<Geometry>,
}

/// Where a window is: its outer top-left corner and inner size, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub rect: Rect,
    pub maximized: bool,
}

/// A WINDOW line's fields after the tag, or None if they don't make a window: a bad saved
/// position is ignored rather than stopping the window.
fn parse_window(fields: &[&str]) -> Option<Geometry> {
    let [x, y, w, h, maximized, ..] = fields else { return None };
    let rect = Rect { x: x.parse().ok()?, y: y.parse().ok()?, w: w.parse().ok()?, h: h.parse().ok()? };
    let maximized = match *maximized {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    (rect.w > 0 && rect.h > 0).then_some(Geometry { rect, maximized })
}

fn write_window(out: &mut impl Write, window: Option<Geometry>) -> io::Result<()> {
    match window {
        Some(Geometry { rect: Rect { x, y, w, h }, maximized }) => {
            writeln!(out, "WINDOW\t{x}\t{y}\t{w}\t{h}\t{}", u8::from(maximized))
        }
        None => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Accept,
    Cancel,
    /// The report was closed.
    Ok,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Accept => "accept",
            Outcome::Cancel => "cancel",
            Outcome::Ok => "ok",
        }
    }
}

impl Item {
    /// Whether `label` will be written: the user's choice once edited, else the suggestion.
    pub fn keeps(&self, label: &Label) -> bool {
        if self.edited {
            label.chosen
        } else {
            label.suggested
        }
    }

    pub fn keyword_count(&self) -> usize {
        self.labels.iter().filter(|l| self.keeps(l)).count()
    }

    /// Ticks or unticks label `k`. The first edit starts from the suggestions, so the other
    /// labels keep what they showed. An item stays edited even with every label unticked.
    pub fn toggle(&mut self, k: usize) {
        if k >= self.labels.len() {
            return;
        }
        if !self.edited {
            for label in &mut self.labels {
                label.chosen = label.suggested;
            }
            self.edited = true;
        }
        self.labels[k].chosen = !self.labels[k].chosen;
    }

    /// "3 Keywords", "1 Keyword", "0 Keywords", "2 Keywords (edited)", as `Review.itemStatus`.
    pub fn status(&self) -> String {
        let text = plural(self.keyword_count(), "Keyword");
        if self.edited {
            text + " (edited)"
        } else {
            text
        }
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// Keyword assignments, and photos that would get at least one, over all items.
pub fn totals(items: &[Item]) -> (usize, usize) {
    let (mut keywords, mut photos) = (0, 0);
    for item in items {
        let n = item.keyword_count();
        if n > 0 {
            keywords += n * item.photo_count;
            photos += item.photo_count;
        }
    }
    (keywords, photos)
}

/// "12 photos to review. 30 keywords to add on 11 photos.", as `totalsLine` in `Review.lua`.
pub fn totals_line(items: &[Item]) -> String {
    let (keywords, photos) = totals(items);
    let reviewed = plural(items.len(), "photo");
    if keywords == 0 {
        format!("{reviewed} to review. No keywords to add.")
    } else {
        format!("{reviewed} to review. {} to add on {}.", plural(keywords, "keyword"), plural(photos, "photo"))
    }
}

/// One field that keeps its line breaks: backslash -> `\\`, line break -> `\n`, tab -> space
/// (as `Exec.escape` in the plugin).
pub fn escape(s: &str) -> String {
    s.replace('\r', "").replace('\\', "\\\\").replace('\n', "\\n").replace('\t', " ")
}

/// `\n` -> line break, `\\` -> backslash; any other backslash is kept as is.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn flag(field: &str, what: &str, line: usize) -> Result<bool> {
    match field {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => bail!("line {line}: {what} must be 0 or 1, not {field:?}"),
    }
}

fn number(field: &str, what: &str, line: usize) -> Result<usize> {
    field.parse().with_context(|| format!("line {line}: {what} must be a number, not {field:?}"))
}

/// The numbered lines after the `#klassifier-ui<TAB><kind>` header line.
fn body<'a>(text: &'a str, kind: &str) -> Result<impl Iterator<Item = (usize, &'a str)>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines().enumerate().map(|(i, l)| (i + 1, l.trim_end_matches('\r')));
    let header = format!("#klassifier-ui\t{kind}");
    match lines.next() {
        Some((_, first)) if first == header || first.starts_with(&format!("{header}\t")) => Ok(lines),
        _ => bail!("not a {kind} file (it must start with #klassifier-ui<TAB>{kind})"),
    }
}

pub fn parse_review(text: &str) -> Result<ReviewInput> {
    let mut input = ReviewInput::default();
    for (n, line) in body(text, "review")? {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[0] {
            "NOTES" => input.notes = unescape(fields.get(1).copied().unwrap_or("")),
            "WINDOW" => input.window = parse_window(&fields[1..]),
            "ITEM" => {
                if fields.len() < 8 {
                    bail!("line {n}: ITEM needs 8 fields, has {}", fields.len());
                }
                let id = number(fields[1], "the item number", n)?;
                if id != input.items.len() + 1 {
                    bail!("line {n}: expected item {}, found {id}", input.items.len() + 1);
                }
                input.items.push(Item {
                    name: fields[2].to_string(),
                    copy_note: fields[3].to_string(),
                    photo_count: number(fields[4], "the photo count", n)?,
                    edited: flag(fields[5], "edited", n)?,
                    standin: PathBuf::from(fields[6]),
                    rendered: PathBuf::from(fields[7]),
                    labels: Vec::new(),
                });
            }
            "LABEL" => {
                if fields.len() < 6 {
                    bail!("line {n}: LABEL needs 6 fields, has {}", fields.len());
                }
                let id = number(fields[1], "the item number", n)?;
                let count = input.items.len();
                let Some(item) = input.items.last_mut().filter(|_| id == count) else {
                    bail!("line {n}: LABEL for item {id} doesn't follow its ITEM line");
                };
                item.labels.push(Label {
                    name: fields[2].to_string(),
                    suggested: flag(fields[3], "suggested", n)?,
                    chosen: flag(fields[4], "chosen", n)?,
                    keyword: fields[5].to_string(),
                });
            }
            _ => {}
        }
    }
    Ok(input)
}

#[derive(Debug, Default, PartialEq)]
pub struct ReportInput {
    pub summary: String,
    pub lines: Vec<String>,
    pub window: Option<Geometry>,
}

pub fn parse_report(text: &str) -> Result<ReportInput> {
    let mut report = ReportInput::default();
    for (_, line) in body(text, "report")? {
        let (tag, rest) = line.split_once('\t').unwrap_or((line, ""));
        match tag {
            "SUMMARY" => report.summary = unescape(rest),
            "LINE" => report.lines.push(rest.to_string()),
            "WINDOW" => report.window = parse_window(&rest.split('\t').collect::<Vec<_>>()),
            _ => {}
        }
    }
    Ok(report)
}

/// The window's output: `outcome`, the edited `items`' choices, and where the window was.
#[derive(Debug, Default, PartialEq)]
pub struct OptionsInput {
    pub count: usize,
    pub warn_above: usize,
    pub sets: Vec<String>,
    /// The remembered label set's position in `sets` (0 if it is gone).
    pub set_index: usize,
    pub hints: String,
    pub only_hints: bool,
    pub window: Option<Geometry>,
}

pub fn parse_options(text: &str) -> Result<OptionsInput> {
    let mut input = OptionsInput::default();
    let (mut count, mut label_set) = (None, None);
    for (n, line) in body(text, "options")? {
        let fields: Vec<&str> = line.split('\t').collect();
        let value = fields.get(1).copied().unwrap_or("");
        match fields[0] {
            "WINDOW" => input.window = parse_window(&fields[1..]),
            "COUNT" => count = Some(number(value, "the photo count", n)?),
            "WARN_ABOVE" => input.warn_above = number(value, "WARN_ABOVE", n)?,
            "SET" => input.sets.push(value.to_string()),
            "LABELSET" => label_set = Some(value.to_string()),
            "HINTS" => input.hints = unescape(value),
            "ONLYHINTS" => input.only_hints = flag(value, "ONLYHINTS", n)?,
            _ => {}
        }
    }
    input.count = count.context("no COUNT line")?;
    if input.sets.is_empty() {
        bail!("no label sets (SET lines)");
    }
    input.set_index = label_set.and_then(|name| input.sets.iter().position(|s| *s == name)).unwrap_or(0);
    Ok(input)
}

/// "makeup, hair styling,,backstage" -> ["makeup", "hair styling", "backstage"], as
/// `parseHints` in the plugin: commas, semicolons and line breaks separate hints.
pub fn parse_hints(text: &str) -> Vec<&str> {
    text.split([',', ';', '\r', '\n']).map(str::trim).filter(|h| !h.is_empty()).collect()
}

/// Why the options can't be used, as the plugin would say, or None if they can.
pub fn options_problem(hints: &str, only_hints: bool) -> Option<String> {
    let hints = parse_hints(hints);
    if only_hints && hints.is_empty() {
        return Some("Enter at least one hint, or untick \"Use only the hints\".".into());
    }
    // The hints file is read like a label set, so "=>" would map a hint to a nested keyword.
    hints.iter().find(|h| h.contains("=>")).map(|h| {
        format!(
            "Hints can't map to nested keywords: remove \"=>\" from \"{h}\". Put mappings in a label set file instead."
        )
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct OptionsChoice {
    pub label_set: String,
    pub hints: String,
    pub only_hints: bool,
}

/// The options dialog's output: `outcome` (Ok or Cancel), what was chosen, where it was.
pub fn write_options_result(
    out: &mut impl Write,
    outcome: Outcome,
    choice: Option<&OptionsChoice>,
    window: Option<Geometry>,
) -> io::Result<()> {
    writeln!(out, "#klassifier-ui\t{}", env!("CARGO_PKG_VERSION"))?;
    writeln!(out, "RESULT\t{}", outcome.as_str())?;
    if let Some(choice) = choice {
        writeln!(out, "LABELSET\t{}", choice.label_set.replace(['\t', '\r', '\n'], " "))?;
        writeln!(out, "HINTS\t{}", escape(&choice.hints))?;
        writeln!(out, "ONLYHINTS\t{}", u8::from(choice.only_hints))?;
    }
    write_window(out, window)?;
    out.flush()
}

pub fn write_result(
    out: &mut impl Write,
    outcome: Outcome,
    items: &[Item],
    window: Option<Geometry>,
) -> io::Result<()> {
    writeln!(out, "#klassifier-ui\t{}", env!("CARGO_PKG_VERSION"))?;
    writeln!(out, "RESULT\t{}", outcome.as_str())?;
    for (i, item) in items.iter().enumerate().filter(|(_, item)| item.edited) {
        writeln!(out, "EDITED\t{}", i + 1)?;
        for (k, _) in item.labels.iter().enumerate().filter(|(_, l)| l.chosen) {
            writeln!(out, "CHOSEN\t{}\t{}", i + 1, k + 1)?;
        }
    }
    write_window(out, window)?;
    out.flush()
}

/// Grid lines needed for `count` cells, `columns` across.
pub fn grid_rows(count: usize, columns: usize) -> usize {
    count.div_ceil(columns.max(1))
}

/// A keyboard move of the selected photo in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    /// Up or down this many lines (at least one), staying in the column.
    PageUp(usize),
    PageDown(usize),
}

/// The photo selected after `mv` from `index`, in a grid of `count` photos `columns` across.
/// Left and right run through the lines; up and down stay in the column, except that down
/// into a short last line lands on its last photo.
pub fn move_selection(index: usize, mv: Move, columns: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let columns = columns.max(1);
    let last = count - 1;
    let index = index.min(last);
    let up = |i: usize| if i >= columns { i - columns } else { i };
    let down = |i: usize| if i / columns < last / columns { (i + columns).min(last) } else { i };
    let repeat = |mut i: usize, lines: usize, step: &dyn Fn(usize) -> usize| {
        for _ in 0..lines.clamp(1, count) {
            i = step(i);
        }
        i
    };
    match mv {
        Move::Left => index.saturating_sub(1),
        Move::Right => (index + 1).min(last),
        Move::Up => up(index),
        Move::Down => down(index),
        Move::Home => 0,
        Move::End => last,
        Move::PageUp(lines) => repeat(index, lines, &up),
        Move::PageDown(lines) => repeat(index, lines, &down),
    }
}

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Where a window opens at its saved position `saved`: as it was, but at least `min` and at
/// most the work area `work` of the monitor it is on, and moved inside that work area.
pub fn restore_window(saved: Rect, work: Rect, min: (i32, i32)) -> Rect {
    let w = saved.w.max(min.0).min(work.w);
    let h = saved.h.max(min.1).min(work.h);
    let x = saved.x.clamp(work.x, work.x + work.w - w);
    let y = saved.y.clamp(work.y, work.y + work.h - h);
    Rect { x, y, w, h }
}

/// Where the review window opens: 80% of Lightroom's window `target` but at least `min`,
/// centred on it, and kept inside the monitor's work area `work`.
pub fn place_window(target: Rect, work: Rect, min: (i32, i32)) -> Rect {
    center_window(target, work, ((target.w * 4 / 5).max(min.0), (target.h * 4 / 5).max(min.1)))
}

/// A window of `size` (at most the work area `work`) centred on `target`, kept inside `work`.
pub fn center_window(target: Rect, work: Rect, size: (i32, i32)) -> Rect {
    let (w, h) = (size.0.min(work.w), size.1.min(work.h));
    let x = (target.x + (target.w - w) / 2).clamp(work.x, work.x + work.w - w);
    let y = (target.y + (target.h - h) / 2).clamp(work.y, work.y + work.h - h);
    Rect { x, y, w, h }
}
