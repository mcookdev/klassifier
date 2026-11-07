//! Review window data: the exchange format with the plugin, and the texts that must match
//! what `Review.lua` showed.

use std::path::PathBuf;

use klassifier::ui_model::{
    center_window, escape, grid_rows, move_selection, options_problem, parse_hints, parse_options, parse_report,
    parse_review, place_window, restore_window, totals, totals_line, unescape, write_options_result, write_result,
    Geometry, Item, Label, Move, OptionsChoice, Outcome, Rect,
};

fn label(name: &str, suggested: bool) -> Label {
    Label { name: name.into(), suggested, chosen: false, keyword: String::new() }
}

fn item(labels: Vec<Label>, photo_count: usize) -> Item {
    Item {
        name: "a.cr3".into(),
        copy_note: String::new(),
        photo_count,
        edited: false,
        standin: PathBuf::from("1.jpg"),
        rendered: PathBuf::from("1.lr.jpg"),
        labels,
    }
}

const SAMPLE: &str = "#klassifier-ui\treview\t1\n\
NOTES\tLabels: General.\\nCancelled after 3 of 9 photos. C:\\\\x\n\
ITEM\t1\tDSC01.ARW\t+1 virtual copy\t2\t0\tC:\\t\\1.jpg\tC:\\t\\1.lr.jpg\n\
LABEL\t1\tegret\t1\t0\tWildlife > Bird > Egret\n\
LABEL\t1\theron\t0\t0\t\n\
ITEM\t2\tIMG_2.jpg\t\t1\t1\tC:\\t\\2.jpg\tC:\\t\\2.lr.jpg\textra field\n\
LABEL\t2\tbeach\t1\t0\t\n\
LABEL\t2\tsand\t0\t1\t\n\
FUTURE\tignored\n";

#[test]
fn parses_review_input() {
    let input = parse_review(SAMPLE).unwrap();
    assert_eq!(input.notes, "Labels: General.\nCancelled after 3 of 9 photos. C:\\x");
    assert_eq!(input.items.len(), 2);
    let first = &input.items[0];
    assert_eq!(first.name, "DSC01.ARW");
    assert_eq!(first.copy_note, "+1 virtual copy");
    assert_eq!(first.photo_count, 2);
    assert!(!first.edited);
    assert_eq!(first.standin, PathBuf::from("C:\\t\\1.jpg"));
    assert_eq!(first.rendered, PathBuf::from("C:\\t\\1.lr.jpg"));
    assert_eq!(first.labels.len(), 2);
    assert_eq!(first.labels[0].keyword, "Wildlife > Bird > Egret");
    assert!(first.labels[0].suggested && !first.labels[1].suggested);
    let second = &input.items[1];
    assert!(second.edited);
    assert!(second.labels[1].chosen);
}

#[test]
fn rejects_bad_input() {
    assert!(parse_review("ITEM\t1\n").is_err(), "missing header");
    let header = "#klassifier-ui\treview\t1\n";
    assert!(parse_review(&format!("{header}ITEM\t2\ta\t\t1\t0\ts\tr\n")).is_err(), "items out of order");
    assert!(parse_review(&format!("{header}LABEL\t1\tdog\t1\t0\t\n")).is_err(), "label before its item");
    assert!(parse_review(&format!("{header}ITEM\t1\ta\t\t1\tyes\ts\tr\n")).is_err(), "bad flag");
    assert!(parse_review(&format!("{header}ITEM\t1\ta\n")).is_err(), "too few fields");
}

#[test]
fn accepts_bom_and_crlf() {
    let input = parse_review("\u{feff}#klassifier-ui\treview\t1\r\nITEM\t1\ta\t\t1\t0\ts\tr\r\n").unwrap();
    assert_eq!(input.items[0].rendered, PathBuf::from("r"));
}

#[test]
fn unescapes_notes() {
    assert_eq!(unescape("a\\nb"), "a\nb");
    assert_eq!(unescape("C:\\\\dir"), "C:\\dir");
    assert_eq!(unescape("keep \\t and trailing \\"), "keep \\t and trailing \\");
}

#[test]
fn status_matches_review_lua() {
    assert_eq!(item(vec![], 1).status(), "0 Keywords");
    assert_eq!(item(vec![label("dog", true), label("cat", false)], 1).status(), "1 Keyword");
    let mut edited = item(vec![label("dog", true), label("cat", false), label("sofa", false)], 1);
    edited.edited = true;
    edited.labels[1].chosen = true;
    edited.labels[2].chosen = true;
    // Edited: the choices count, not the suggestions.
    assert_eq!(edited.status(), "2 Keywords (edited)");
    edited.labels[1].chosen = false;
    edited.labels[2].chosen = false;
    assert_eq!(edited.status(), "0 Keywords (edited)");
}

#[test]
fn totals_match_review_lua() {
    let none = vec![item(vec![label("dog", false)], 1)];
    assert_eq!(totals_line(&none), "1 photo to review. No keywords to add.");

    // An item with 2 keywords on 3 photos (original + 2 virtual copies) is 6 assignments.
    let items = vec![item(vec![label("dog", true), label("cat", true)], 3), item(vec![label("sea", true)], 1), item(vec![], 1)];
    assert_eq!(totals(&items), (7, 4));
    assert_eq!(totals_line(&items), "3 photos to review. 7 keywords to add on 4 photos.");
}

#[test]
fn writes_result_for_edited_items_only() {
    let mut items = parse_review(SAMPLE).unwrap().items;
    items[0].labels[0].chosen = true; // not edited: ignored
    let mut out = Vec::new();
    write_result(&mut out, Outcome::Accept, &items, None).unwrap();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("#klassifier-ui\t"));
    assert_eq!(&lines[1..], ["RESULT\taccept", "EDITED\t2", "CHOSEN\t2\t2"]);

    let mut out = Vec::new();
    write_result(&mut out, Outcome::Cancel, &[], None).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("RESULT\tcancel\n"));
}

#[test]
fn grid_rows_round_up() {
    assert_eq!(grid_rows(0, 5), 0);
    assert_eq!(grid_rows(1, 5), 1);
    assert_eq!(grid_rows(5, 5), 1);
    assert_eq!(grid_rows(6, 5), 2);
    assert_eq!(grid_rows(3, 0), 3, "zero columns counts as one");
}

#[test]
fn window_centred_on_lightroom() {
    let work = Rect { x: 0, y: 0, w: 2560, h: 1400 };
    let lr = Rect { x: 0, y: 0, w: 2560, h: 1400 };
    assert_eq!(place_window(lr, work, (900, 600)), Rect { x: 256, y: 140, w: 2048, h: 1120 });
}

#[test]
fn window_at_least_min_and_inside_work_area() {
    // A small Lightroom window near the right edge of a second monitor.
    let work = Rect { x: 1920, y: 0, w: 1920, h: 1040 };
    let lr = Rect { x: 3500, y: 900, w: 400, h: 300 };
    let r = place_window(lr, work, (900, 600));
    assert_eq!((r.w, r.h), (900, 600));
    assert_eq!((r.x, r.y), (1920 + 1920 - 900, 1040 - 600));
    // Larger than the monitor: clamped to it.
    let r = place_window(Rect { x: 0, y: 0, w: 5000, h: 5000 }, Rect { x: 0, y: 0, w: 1000, h: 800 }, (900, 600));
    assert_eq!(r, Rect { x: 0, y: 0, w: 1000, h: 800 });
}

#[test]
fn first_toggle_starts_from_the_suggestions() {
    let mut it = item(vec![label("dog", true), label("cat", false), label("sofa", true)], 1);
    it.labels[2].chosen = true; // stale input: ignored until edited
    it.toggle(1);
    assert!(it.edited);
    let chosen: Vec<bool> = it.labels.iter().map(|l| l.chosen).collect();
    assert_eq!(chosen, [true, true, true], "the untouched labels keep what they showed");
    it.toggle(0);
    it.toggle(2);
    assert_eq!(it.status(), "1 Keyword (edited)");
    // Unticking everything stays an edit: nothing is written for it.
    it.toggle(1);
    assert_eq!(it.status(), "0 Keywords (edited)");
    assert!(it.edited);
}

#[test]
fn toggle_out_of_range_changes_nothing() {
    let mut it = item(vec![label("dog", true)], 1);
    it.toggle(1);
    assert!(!it.edited);
    assert!(!it.labels[0].chosen);
}

#[test]
fn edits_reach_the_result_and_totals() {
    let mut items = vec![item(vec![label("dog", true), label("cat", false)], 2), item(vec![label("sea", true)], 1)];
    items[0].toggle(1); // dog + cat on 2 photos
    items[1].toggle(0); // nothing
    assert_eq!(totals(&items), (4, 2));
    let mut out = Vec::new();
    write_result(&mut out, Outcome::Accept, &items, None).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(&text.lines().collect::<Vec<_>>()[1..], ["RESULT\taccept", "EDITED\t1", "CHOSEN\t1\t1", "CHOSEN\t1\t2", "EDITED\t2"]);
}

#[test]
fn moves_within_the_grid() {
    // 4 across, 10 photos: rows 0-3, 4-7, 8-9.
    let m = |from, mv| move_selection(from, mv, 4, 10);
    assert_eq!(m(5, Move::Left), 4);
    assert_eq!(m(4, Move::Left), 3, "left wraps to the line above");
    assert_eq!(m(0, Move::Left), 0);
    assert_eq!(m(3, Move::Right), 4);
    assert_eq!(m(9, Move::Right), 9);
    assert_eq!(m(5, Move::Up), 1);
    assert_eq!(m(1, Move::Up), 1, "top line stays");
    assert_eq!(m(1, Move::Down), 5);
    assert_eq!(m(6, Move::Down), 9, "into a short last line: its last photo");
    assert_eq!(m(9, Move::Down), 9);
    assert_eq!(m(6, Move::Home), 0);
    assert_eq!(m(2, Move::End), 9);
    assert_eq!(m(1, Move::PageDown(2)), 9);
    assert_eq!(m(9, Move::PageUp(2)), 1);
    assert_eq!(m(5, Move::PageUp(5)), 1, "stays in its column");
    assert_eq!(m(7, Move::PageUp(0)), 3, "a page is at least a line");
}

#[test]
fn moves_with_nothing_or_one_column() {
    assert_eq!(move_selection(0, Move::Down, 4, 0), 0);
    assert_eq!(move_selection(2, Move::Down, 0, 5), 3, "zero columns counts as one");
    assert_eq!(move_selection(7, Move::Left, 4, 5), 3, "an index past the end is clamped to the last photo first");
}

#[test]
fn parses_report_input() {
    let text = "\u{feff}#klassifier-ui\treport\t1\r\n\
SUMMARY\t0 classified.\\nLabels: General.\r\n\
LINE\ta.mov: unsupported: video\r\n\
LINE\t\r\n\
LINE\tb.ARW: error\twith a tab\n\
FUTURE\tignored\n";
    let report = parse_report(text).unwrap();
    assert_eq!(report.summary, "0 classified.\nLabels: General.");
    assert_eq!(report.lines, ["a.mov: unsupported: video", "", "b.ARW: error\twith a tab"]);
}

#[test]
fn report_needs_its_header() {
    assert!(parse_report("#klassifier-ui\treview\t1\nLINE\tx\n").is_err(), "a review file");
    assert!(parse_report("LINE\tx\n").is_err());
    assert!(parse_review("#klassifier-ui\treport\t1\n").is_err(), "and the other way round");
}

#[test]
fn writes_ok_for_the_report() {
    let mut out = Vec::new();
    write_result(&mut out, Outcome::Ok, &[], None).unwrap();
    assert_eq!(String::from_utf8(out).unwrap().lines().nth(1), Some("RESULT\tok"));
}

#[test]
fn reads_the_saved_window() {
    let header = "#klassifier-ui\treview\t1\n";
    let input = parse_review(&format!("{header}WINDOW\t-1910\t40\t1600\t900\t1\nITEM\t1\ta\t\t1\t0\ts\tr\n")).unwrap();
    assert_eq!(input.window, Some(Geometry { rect: Rect { x: -1910, y: 40, w: 1600, h: 900 }, maximized: true }));
    let report = parse_report("#klassifier-ui\treport\t1\nWINDOW\t5\t6\t700\t500\t0\n").unwrap();
    assert_eq!(report.window, Some(Geometry { rect: Rect { x: 5, y: 6, w: 700, h: 500 }, maximized: false }));
}

#[test]
fn ignores_a_bad_saved_window() {
    let header = "#klassifier-ui\treview\t1\n";
    for line in ["WINDOW\t1\t2\t3", "WINDOW\ta\t2\t300\t300\t0", "WINDOW\t1\t2\t0\t300\t0", "WINDOW\t1\t2\t300\t300\t2"] {
        let input = parse_review(&format!("{header}{line}\nITEM\t1\ta\t\t1\t0\ts\tr\n")).unwrap();
        assert_eq!(input.window, None, "{line}");
        assert_eq!(input.items.len(), 1);
    }
}

#[test]
fn writes_the_window_on_close() {
    let mut out = Vec::new();
    let window = Geometry { rect: Rect { x: -8, y: 0, w: 2576, h: 1416 }, maximized: true };
    write_result(&mut out, Outcome::Cancel, &[], Some(window)).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(&text.lines().collect::<Vec<_>>()[1..], ["RESULT\tcancel", "WINDOW\t-8\t0\t2576\t1416\t1"]);
    // Read back as input, it is the same window.
    let input = parse_review(&format!("#klassifier-ui\treview\t1\n{}\n", text.lines().nth(2).unwrap())).unwrap();
    assert_eq!(input.window, Some(window));
}

#[test]
fn restored_window_kept_on_its_monitor() {
    let work = Rect { x: 1920, y: 0, w: 1920, h: 1040 };
    let min = (900, 600);
    // Where it was left: unchanged.
    let saved = Rect { x: 2000, y: 100, w: 1200, h: 800 };
    assert_eq!(restore_window(saved, work, min), saved);
    // Partly off the monitor (the taskbar side, or a resolution change): moved back on.
    let r = restore_window(Rect { x: 3500, y: 700, w: 1200, h: 800 }, work, min);
    assert_eq!(r, Rect { x: 1920 + 1920 - 1200, y: 1040 - 800, w: 1200, h: 800 });
    // Too small or too big: at least the minimum, at most the monitor.
    assert_eq!(restore_window(Rect { x: 2000, y: 100, w: 300, h: 200 }, work, min), Rect { x: 2000, y: 100, w: 900, h: 600 });
    assert_eq!(restore_window(Rect { x: 1800, y: -50, w: 4000, h: 3000 }, work, min), work);
}

const OPTIONS: &str = "#klassifier-ui\toptions\t1\n\
WINDOW\t100\t200\t560\t300\t0\n\
COUNT\t812\n\
WARN_ABOVE\t500\n\
SET\tGeneral\n\
SET\tFashion\n\
SET\tWildlife\n\
LABELSET\tFashion\n\
HINTS\tmakeup, hair styling\\nbackstage\n\
ONLYHINTS\t1\n";

#[test]
fn parses_options_input() {
    let input = parse_options(OPTIONS).unwrap();
    assert_eq!(input.count, 812);
    assert_eq!(input.warn_above, 500);
    assert_eq!(input.sets, ["General", "Fashion", "Wildlife"]);
    assert_eq!(input.set_index, 1);
    assert_eq!(input.hints, "makeup, hair styling\nbackstage");
    assert!(input.only_hints);
    assert_eq!(input.window.unwrap().rect, Rect { x: 100, y: 200, w: 560, h: 300 });
}

#[test]
fn options_need_sets_and_a_count() {
    let header = "#klassifier-ui\toptions\t1\n";
    assert!(parse_options(&format!("{header}COUNT\t3\n")).is_err(), "no label sets");
    assert!(parse_options(&format!("{header}SET\tGeneral\n")).is_err(), "no count");
    assert!(parse_options(&format!("{header}COUNT\tmany\nSET\tGeneral\n")).is_err(), "bad count");
    // An unknown remembered set (deleted since) falls back to the first one.
    let input = parse_options(&format!("{header}COUNT\t3\nSET\tGeneral\nSET\tFashion\nLABELSET\tGone\n")).unwrap();
    assert_eq!((input.set_index, input.hints.as_str(), input.only_hints, input.warn_above), (0, "", false, 0));
}

#[test]
fn hints_split_like_parse_hints_in_lua() {
    assert_eq!(parse_hints("makeup, hair styling,,backstage"), ["makeup", "hair styling", "backstage"]);
    assert_eq!(parse_hints(" a ;b\r\n c\n,"), ["a", "b", "c"]);
    assert!(parse_hints(" , ;").is_empty());
}

#[test]
fn options_problems_match_lua() {
    assert_eq!(options_problem("", false), None);
    assert_eq!(options_problem("dog", true), None);
    assert_eq!(options_problem(" , ", true).as_deref(), Some("Enter at least one hint, or untick \"Use only the hints\"."));
    assert_eq!(
        options_problem("cat, egret => Wildlife > Bird", false).as_deref(),
        Some("Hints can't map to nested keywords: remove \"=>\" from \"egret => Wildlife > Bird\". Put mappings in a label set file instead.")
    );
}

#[test]
fn escape_is_undone_by_unescape() {
    for s in ["plain", "two\nlines", "C:\\dir\\", "a\\nb (a backslash, then n)"] {
        assert_eq!(unescape(&escape(s)), s.replace('\t', " "));
    }
    assert_eq!(escape("a\tb\r\nc"), "a b\\nc");
}

#[test]
fn writes_the_options_chosen() {
    let choice = OptionsChoice { label_set: "Fashion".into(), hints: "a,\nb".into(), only_hints: false };
    let mut out = Vec::new();
    write_options_result(&mut out, Outcome::Ok, Some(&choice), None).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(&text.lines().collect::<Vec<_>>()[1..], ["RESULT\tok", "LABELSET\tFashion", "HINTS\ta,\\nb", "ONLYHINTS\t0"]);
    let mut out = Vec::new();
    write_options_result(&mut out, Outcome::Cancel, None, None).unwrap();
    assert_eq!(&String::from_utf8(out).unwrap().lines().collect::<Vec<_>>()[1..], ["RESULT\tcancel"]);
}

#[test]
fn dialog_centred_at_its_size() {
    let work = Rect { x: 0, y: 0, w: 2560, h: 1400 };
    assert_eq!(center_window(Rect { x: 0, y: 0, w: 2560, h: 1400 }, work, (560, 300)), Rect { x: 1000, y: 550, w: 560, h: 300 });
    // Over a Lightroom window near the edge: kept on screen.
    assert_eq!(center_window(Rect { x: 2400, y: 0, w: 400, h: 300 }, work, (560, 300)), Rect { x: 2000, y: 0, w: 560, h: 300 });
}
