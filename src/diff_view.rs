//! Read-only GPUI CE adaptation of GPUI Box Kit's MIT-licensed DiffView.
//! Source: https://github.com/fran0220/gpui-box/blob/24afaaf1b2f34827342b5ad04a4efba95e705c45/crates/gpui-kit/src/content/diff_view.rs
//! Original license: licenses/GPUI-Box-Kit-MIT.txt.
//! Diff production and filesystem access belong to the caller.

use crate::theme::{ACCENT, BG, BORDER, MUTED, SURFACE, TEXT};
use gpui::{AnyElement, IntoElement, ListState, div, prelude::*, px, rgb};
use std::sync::Arc;

const ADDED_BG: u32 = 0x19382e;
const REMOVED_BG: u32 = 0x422a31;
const ADDED_TEXT: u32 = 0x9cdbb5;
const REMOVED_TEXT: u32 = 0xf0aaaa;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentation {
    Unified,
    Split,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug)]
pub struct Side {
    pub number: usize,
    pub text: String,
    pub mark: Mark,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub old: Option<Side>,
    pub new: Option<Side>,
}

#[derive(Clone, Debug)]
pub struct Hunk {
    pub header: String,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug)]
pub struct File {
    pub path: String,
    pub hunks: Vec<Hunk>,
    pub note: Option<String>,
}

#[derive(Clone)]
pub enum Row {
    File(String),
    Hunk(String),
    Unified {
        side: Side,
        old_number: Option<usize>,
        new_number: Option<usize>,
    },
    Split {
        old: Option<Side>,
        new: Option<Side>,
    },
    Note(String),
}

pub fn flatten(files: &[File], presentation: Presentation) -> Vec<Row> {
    let mut rows = Vec::new();
    for file in files {
        rows.push(Row::File(file.path.clone()));
        if let Some(note) = &file.note {
            rows.push(Row::Note(note.clone()));
        }
        for hunk in &file.hunks {
            rows.push(Row::Hunk(hunk.header.clone()));
            for line in &hunk.lines {
                match presentation {
                    Presentation::Split => rows.push(Row::Split {
                        old: line.old.clone(),
                        new: line.new.clone(),
                    }),
                    Presentation::Unified => match (&line.old, &line.new) {
                        (Some(old), Some(new)) if old.mark == Mark::Context => {
                            rows.push(Row::Unified {
                                side: old.clone(),
                                old_number: Some(old.number),
                                new_number: Some(new.number),
                            });
                        }
                        (Some(old), Some(new)) => {
                            rows.push(Row::Unified {
                                side: old.clone(),
                                old_number: Some(old.number),
                                new_number: None,
                            });
                            rows.push(Row::Unified {
                                side: new.clone(),
                                old_number: None,
                                new_number: Some(new.number),
                            });
                        }
                        (Some(old), None) => rows.push(Row::Unified {
                            side: old.clone(),
                            old_number: Some(old.number),
                            new_number: None,
                        }),
                        (None, Some(new)) => rows.push(Row::Unified {
                            side: new.clone(),
                            old_number: None,
                            new_number: Some(new.number),
                        }),
                        (None, None) => {}
                    },
                }
            }
        }
    }
    rows
}

pub fn view(state: ListState, rows: Arc<Vec<Row>>) -> impl IntoElement {
    gpui::list(state, move |index, _, _| render_row(&rows[index]))
        .w_full()
        .h_full()
}

fn render_row(row: &Row) -> AnyElement {
    match row {
        Row::File(path) => div()
            .h(px(32.))
            .px_3()
            .flex()
            .items_center()
            .bg(rgb(SURFACE))
            .text_sm()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(TEXT))
            .child(path.clone())
            .into_any_element(),
        Row::Hunk(header) => div()
            .h(px(28.))
            .px_3()
            .flex()
            .items_center()
            .bg(rgb(BORDER))
            .text_xs()
            .font_family("monospace")
            .text_color(rgb(ACCENT))
            .child(header.clone())
            .into_any_element(),
        Row::Note(note) => div()
            .h(px(30.))
            .px_3()
            .flex()
            .items_center()
            .text_sm()
            .text_color(rgb(MUTED))
            .child(note.clone())
            .into_any_element(),
        Row::Unified {
            side,
            old_number,
            new_number,
        } => div()
            .h(px(24.))
            .flex()
            .items_center()
            .bg(rgb(mark_bg(side.mark)))
            .font_family("monospace")
            .text_xs()
            .child(number(*old_number))
            .child(number(*new_number))
            .child(prefix(side.mark))
            .child(code(&side.text))
            .into_any_element(),
        Row::Split { old, new } => div()
            .h(px(24.))
            .flex()
            .items_center()
            .font_family("monospace")
            .text_xs()
            .child(split_side(old.as_ref()))
            .child(div().h_full().w(px(1.)).bg(rgb(BORDER)))
            .child(split_side(new.as_ref()))
            .into_any_element(),
    }
}

fn split_side(side: Option<&Side>) -> impl IntoElement {
    let mark = side.map_or(Mark::Context, |side| side.mark);
    div()
        .flex()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .items_center()
        .bg(rgb(mark_bg(mark)))
        .child(number(side.map(|side| side.number)))
        .child(prefix(mark))
        .child(code(side.map_or("", |side| side.text.as_str())))
}

fn number(value: Option<usize>) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .w(px(42.))
        .pr_2()
        .text_right()
        .text_color(rgb(MUTED))
        .child(value.map_or_else(String::new, |number| number.to_string()))
}

fn prefix(mark: Mark) -> impl IntoElement {
    let (symbol, color) = match mark {
        Mark::Context => (" ", MUTED),
        Mark::Added => ("+", ADDED_TEXT),
        Mark::Removed => ("-", REMOVED_TEXT),
    };
    div()
        .flex_shrink_0()
        .w(px(18.))
        .text_color(rgb(color))
        .child(symbol)
}

fn code(value: &str) -> impl IntoElement {
    div()
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_color(rgb(TEXT))
        .child(value.to_owned())
}

fn mark_bg(mark: Mark) -> u32 {
    match mark {
        Mark::Context => BG,
        Mark::Added => ADDED_BG,
        Mark::Removed => REMOVED_BG,
    }
}
