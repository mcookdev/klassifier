//! Review window data: the exchange format with the plugin, and the texts that must match
//! what `Review.lua` showed.

use std::path::PathBuf;

use klassifier::ui_model::{
    grid_rows, parse_review, place_window, totals, totals_line, unescape, write_result, Item, Label, Outcome, Rect,
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
    write_result(&mut out, Outcome::Accept, &items).unwrap();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("#klassifier-ui\t"));
    assert_eq!(&lines[1..], ["RESULT\taccept", "EDITED\t2", "CHOSEN\t2\t2"]);

    let mut out = Vec::new();
    write_result(&mut out, Outcome::Cancel, &[]).unwrap();
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
