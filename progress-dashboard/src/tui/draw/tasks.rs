use crate::tui::utils::{draw_text_nowrap, draw_text_nowrap_fn, rect, GraphemeCountWriter};
use crate::{Progress, ProgressStep, TreeKey, TreeValue};
use std::fmt;
use tui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};
use tui_react::fill_background;
use unicode_segmentation::UnicodeSegmentation;

const VERTICAL_LINE: &str = "│";
const MIN_TREE_WIDTH: u16 = 20;

pub fn pane(
    entries: Vec<(TreeKey, TreeValue)>,
    mut bound: Rect,
    buf: &mut Buffer,
) -> Vec<(TreeKey, TreeValue)> {
    let is_overflowing = if entries.len() > bound.height as usize {
        bound.height = bound.height.saturating_sub(1);
        true
    } else {
        false
    };

    if !entries.is_empty() {
        let column_width = bound.width / 2;
        let max_tree_draw_width = if column_width >= MIN_TREE_WIDTH {
            let prefix_area = Rect {
                width: column_width,
                ..bound
            };
            draw_tree(&entries, buf, prefix_area)
        } else {
            0
        };

        {
            let max_tree_draw_width = max_tree_draw_width;
            let progress_area = rect::offset_x(bound, max_tree_draw_width);
            draw_progress(
                &entries,
                buf,
                progress_area,
                if max_tree_draw_width == 0 {
                    false
                } else {
                    true
                },
            );
        }

        if is_overflowing {
            let overflow_rect = Rect {
                y: bound.height + 1,
                height: 1,
                ..bound
            };
            draw_overflow(
                entries.iter().skip(bound.height as usize),
                buf,
                overflow_rect,
                max_tree_draw_width,
            );
        }
    }
    entries
}

struct ProgressFormat<'a>(&'a Option<Progress>, u16);

impl<'a> fmt::Display for ProgressFormat<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(p) => {
                match p.done_at {
                    Some(done_at) => write!(f, "{} / {}", p.step, done_at),
                    None => write!(f, "{}", p.step),
                }?;
                if let Some(unit) = p.unit {
                    write!(f, " {}", unit)?;
                }
                Ok(())
            }
            None => write!(f, "{:─<width$}", '─', width = self.1 as usize),
        }
    }
}

pub fn draw_progress(
    entries: &[(TreeKey, TreeValue)],
    buf: &mut Buffer,
    bound: Rect,
    draw_column_line: bool,
) {
    let title_spacing = 2u16 + 1; // 2 on the left, 1 on the right
    let column_line_width = if draw_column_line { 1 } else { 0 };
    let max_progress_label_width = entries
        .iter()
        .take(bound.height as usize)
        .map(|(_, TreeValue { progress, .. })| progress)
        .fold(0, |state, progress| match progress {
            progress @ Some(_) => {
                use std::io::Write;
                let mut w = GraphemeCountWriter::default();
                write!(w, "{}", ProgressFormat(progress, 0)).expect("never fails");
                state.max(w.0)
            }
            None => state,
        });
    let max_title_width = entries.iter().take(bound.height as usize).fold(
        0,
        |state, (key, TreeValue { progress, title })| match progress {
            None => state
                .max(title.graphemes(true).count() + key.level() as usize + title_spacing as usize),
            Some(_) => state,
        },
    );

    for (line, (key, TreeValue { progress, title })) in
        entries.iter().take(bound.height as usize).enumerate()
    {
        let line_bound = line_bound(bound, line);
        let progress_text = format!(
            " {progress}",
            progress = ProgressFormat(progress, bound.width.saturating_sub(title_spacing))
        );

        let progress_bar_info = if let Some(fraction) = progress.and_then(|p| p.fraction()) {
            let bar_bound = rect::offset_x(line_bound, column_line_width);
            Some(draw_progress_bar(buf, bar_bound, fraction))
        } else {
            None
        };

        draw_text_nowrap(line_bound, buf, VERTICAL_LINE, None);

        let progress_rect = rect::offset_x(line_bound, column_line_width);
        match progress_bar_info.map(|(bound, style)| {
            move |_t: &str, x: u16, _y: u16| {
                if x < bound.right() {
                    style
                } else {
                    Style::default()
                }
            }
        }) {
            Some(style_fn) => {
                draw_text_nowrap_fn(progress_rect, buf, progress_text, style_fn);
            }
            None => {
                draw_text_nowrap(progress_rect, buf, progress_text, None);
                // we have progress, but no upper limit
                if let Some((step, None)) = progress.as_ref().map(|p| (p.step, p.done_at.as_ref()))
                {
                    let bar_rect = rect::offset_x(line_bound, max_progress_label_width as u16);
                    draw_spinner(buf, bar_rect, step, line);
                }
            }
        }

        if progress.is_none() {
            let center_rect = Rect {
                width: max_title_width as u16,
                ..rect::offset_x(
                    line_bound,
                    column_line_width + (bound.width.saturating_sub(max_title_width as u16)) / 2,
                )
            };
            let title_text = format!(
                " {:‧<prefix_count$} {} ",
                "",
                title,
                prefix_count = key.level() as usize
            );
            draw_text_nowrap(center_rect, buf, title_text, None);
        }
    }
}

fn draw_spinner(buf: &mut Buffer, bound: Rect, step: ProgressStep, seed: usize) {
    if bound.width == 0 {
        return;
    }
    let step = step as usize;
    let x = bound.x + ((step + seed) % bound.width as usize) as u16;
    let width = 5;
    let bound = rect::intersect(Rect { x, width, ..bound }, bound);
    tui_react::fill_background(bound, buf, Color::White);
}

fn draw_progress_bar(buf: &mut Buffer, bound: Rect, fraction: f32) -> (Rect, Style) {
    draw_progress_bar_fn(buf, bound, fraction, |fraction| {
        if fraction >= 0.8 {
            Color::Green
        } else {
            Color::Yellow
        }
    })
}
fn draw_progress_bar_fn(
    buf: &mut Buffer,
    bound: Rect,
    fraction: f32,
    style: impl FnOnce(f32) -> Color,
) -> (Rect, Style) {
    if bound.width == 0 {
        return (Rect::default(), Style::default());
    }
    let fractional_progress_rect = Rect {
        width: ((bound.width as f32 * fraction).ceil() as u16).min(bound.width),
        ..bound
    };
    let color = style(fraction);
    tui_react::fill_background(fractional_progress_rect, buf, color);
    (
        fractional_progress_rect,
        Style::default().bg(color).fg(Color::Black),
    )
}

pub fn draw_tree(entries: &[(TreeKey, TreeValue)], buf: &mut Buffer, bound: Rect) -> u16 {
    let mut max_prefix_len = 0;
    for (line, (key, TreeValue { progress, title })) in
        entries.iter().take(bound.height as usize).enumerate()
    {
        let line_bound = line_bound(bound, line);
        let tree_prefix = format!(
            "{:>width$} {} ",
            if key.level() == 1 {
                "‧"
            } else {
                if progress.is_none() {
                    "…"
                } else {
                    "└"
                }
            },
            if progress.is_none() { "" } else { &title },
            width = key.level() as usize
        );
        max_prefix_len = max_prefix_len.max(draw_text_nowrap(line_bound, buf, tree_prefix, None));
    }
    max_prefix_len
}

fn line_bound(bound: Rect, line: usize) -> Rect {
    Rect {
        y: bound.y + line as u16,
        height: 1,
        ..bound
    }
}

pub fn draw_overflow<'a>(
    entries: impl Iterator<Item = &'a (TreeKey, TreeValue)>,
    buf: &mut Buffer,
    bound: Rect,
    label_offset: u16,
) {
    let (count, mut progress_percent) = entries.fold(
        (0usize, 0f32),
        |(count, progress_fraction), (_key, value)| {
            let progress = value
                .progress
                .and_then(|p| p.fraction())
                .unwrap_or_default();
            (count + 1, progress_fraction + progress)
        },
    );
    progress_percent /= count as f32;
    let label = format!(
        "{} …and {} more",
        if label_offset == 0 { "" } else { VERTICAL_LINE },
        count
    );
    let (progress_rect, style) =
        draw_progress_bar_fn(buf, bound, progress_percent, |_| Color::Green);

    let bg_color = Color::Red;
    fill_background(
        rect::offset_x(bound, progress_rect.right() - 1),
        buf,
        bg_color,
    );
    draw_text_nowrap_fn(
        rect::offset_x(bound, label_offset),
        buf,
        label,
        move |_g, x, _y| {
            if x < progress_rect.right() {
                style
            } else {
                style.bg(bg_color)
            }
        },
    );
}