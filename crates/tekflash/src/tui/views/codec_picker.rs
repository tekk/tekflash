//! Modal that lets the user pick a compression codec + level before backup/archive.
//!
//! Each codec carries a short blurb and rough size/speed scores rendered as
//! ten-cell bars, so the trade-offs are visible at a glance. Levels are per-codec and
//! clamped to the codec's documented range.

use crate::tui::theme::Theme;
use crate::tui::views::action_picker::Action;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};
use tekflash_core::pipeline::compress::{Codec, CompressionLevel};

#[derive(Debug, Clone, Copy)]
pub struct CodecChoice {
    pub codec: Codec,
    pub name: &'static str,
    pub blurb: &'static str,
    /// Inclusive range; `min == max` means "no level".
    pub min_level: i32,
    pub max_level: i32,
    pub default_level: i32,
    pub extension: &'static str,
    /// Visual hint: 0 (no size win) … 10 (smallest output).
    pub size_score: u8,
    /// Visual hint: 0 (slow) … 10 (fastest).
    pub speed_score: u8,
}

pub const CODECS: &[CodecChoice] = &[
    CodecChoice {
        codec: Codec::Zstd,
        name: "zstd",
        blurb: "fast + small (recommended)",
        min_level: 1,
        max_level: 22,
        default_level: 3,
        extension: ".zst",
        size_score: 8,
        speed_score: 8,
    },
    CodecChoice {
        codec: Codec::Lz4,
        name: "lz4",
        blurb: "fastest, larger output",
        min_level: 0,
        max_level: 0,
        default_level: 0,
        extension: ".lz4",
        size_score: 4,
        speed_score: 10,
    },
    CodecChoice {
        codec: Codec::Brotli,
        name: "brotli",
        blurb: "high ratio on repetitive data, slower",
        min_level: 0,
        max_level: 11,
        default_level: 6,
        extension: ".br",
        size_score: 9,
        speed_score: 4,
    },
    CodecChoice {
        codec: Codec::Xz,
        name: "xz / lzma",
        blurb: "smallest output, slow CPU-bound",
        min_level: 0,
        max_level: 9,
        default_level: 6,
        extension: ".xz",
        size_score: 10,
        speed_score: 2,
    },
    CodecChoice {
        codec: Codec::Gzip,
        name: "gzip",
        blurb: "universal, decompresses everywhere",
        min_level: 1,
        max_level: 9,
        default_level: 6,
        extension: ".gz",
        size_score: 5,
        speed_score: 6,
    },
    CodecChoice {
        codec: Codec::Bzip2,
        name: "bzip2",
        blurb: "legacy (rarely the right choice today)",
        min_level: 1,
        max_level: 9,
        default_level: 6,
        extension: ".bz2",
        size_score: 7,
        speed_score: 3,
    },
    CodecChoice {
        codec: Codec::None,
        name: "none",
        blurb: "raw, no compression — bit-for-bit copy",
        min_level: 0,
        max_level: 0,
        default_level: 0,
        extension: "",
        size_score: 0,
        speed_score: 10,
    },
];

#[derive(Debug)]
pub struct CodecPicker {
    pub action: Action,
    pub device_idx: usize,
    pub cursor: usize,
    /// Current level per codec; lets the user audit zstd, bump to xz, come back, and
    /// see their zstd-19 still set.
    pub levels: Vec<i32>,
}

impl CodecPicker {
    pub fn new(action: Action, device_idx: usize) -> Self {
        let levels = CODECS.iter().map(|c| c.default_level).collect();
        Self {
            action,
            device_idx,
            cursor: 0,
            levels,
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < CODECS.len() {
            self.cursor += 1;
        }
    }

    pub fn level_up(&mut self) {
        let c = &CODECS[self.cursor];
        let l = &mut self.levels[self.cursor];
        if *l < c.max_level {
            *l += 1;
        }
    }

    pub fn level_down(&mut self) {
        let c = &CODECS[self.cursor];
        let l = &mut self.levels[self.cursor];
        if *l > c.min_level {
            *l -= 1;
        }
    }

    pub fn jump_max(&mut self) {
        self.levels[self.cursor] = CODECS[self.cursor].max_level;
    }

    pub fn jump_min(&mut self) {
        self.levels[self.cursor] = CODECS[self.cursor].min_level;
    }

    pub fn current(&self) -> &CodecChoice {
        &CODECS[self.cursor]
    }

    pub fn current_level(&self) -> i32 {
        self.levels[self.cursor]
    }

    /// `(codec, level, extension)` for the current selection.
    pub fn picked(&self) -> (Codec, CompressionLevel, String) {
        let c = self.current();
        (
            c.codec,
            CompressionLevel(self.current_level()),
            c.extension.to_string(),
        )
    }

    /// Output extension that the file browser should auto-append. For Backup the prefix
    /// is `.img`; for Archive it's `.tar`. Flash never reaches the codec picker.
    pub fn output_extension(&self, action: Action) -> String {
        let ext = self.current().extension;
        match action {
            Action::Backup => format!(".img{ext}"),
            Action::Archive => format!(".tar{ext}"),
            Action::Flash => String::new(),
        }
    }
}

pub fn render(f: &mut Frame, area: Rect, picker: &CodecPicker, theme: &Theme) {
    // Use almost the whole screen so the codec blurbs aren't truncated. The `centered`
    // helper already clamps to (area − 4), so on an 80-wide terminal this gracefully
    // shrinks; on a typical 120+ wide terminal it stretches to 140 with full blurbs.
    let popup = centered(area, 140, 32);
    f.render_widget(Clear, popup);

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(6),
            Constraint::Length(2),
        ])
        .split(popup);

    // Header
    let title = match picker.action {
        Action::Backup => " Choose compression for backup ",
        Action::Archive => " Choose compression for archive ",
        Action::Flash => " Choose compression ",
    };
    let header = Paragraph::new(Line::from(vec![
        Span::styled(title, theme.title()),
        Span::raw("  "),
        Span::styled("size & speed bars are rough hints", theme.muted_s()),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.muted_s()),
    );
    f.render_widget(header, outer[0]);

    // Codec list. Each item is two lines so the blurb has its own line and never
    // collides with the bars on narrow terminals.
    let items: Vec<ListItem<'_>> = CODECS
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let level = picker.levels[i];
            let level_str = if c.min_level == c.max_level {
                " — ".to_string()
            } else {
                format!("L{level:>2}")
            };
            let size_bar = bar(c.size_score, theme);
            let speed_bar = bar(c.speed_score, theme);
            let top = Line::from(vec![
                Span::styled(format!("  {:<11}", c.name), theme.body()),
                Span::styled(format!("{level_str:>4}  "), theme.title()),
                Span::styled("size  ", theme.muted_s()),
                size_bar,
                Span::raw("   "),
                Span::styled("speed ", theme.muted_s()),
                speed_bar,
            ]);
            let bottom = Line::from(vec![
                Span::raw("           "), // indent under codec name
                Span::styled(c.blurb, theme.muted_s()),
            ]);
            ListItem::new(vec![top, bottom])
        })
        .collect();
    let list = List::new(items)
        .highlight_style(theme.selected())
        .highlight_symbol("> ")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.muted_s())
                .title(Line::from(Span::styled(
                    " Codecs   (bars: 10 cells, 0 = worst, 10 = best) ",
                    theme.title(),
                ))),
        );
    let mut s = ListState::default();
    s.select(Some(picker.cursor));
    f.render_stateful_widget(list, outer[1], &mut s);

    // Level + summary detail panel
    let c = picker.current();
    let level_line = if c.min_level == c.max_level {
        Line::from(vec![
            Span::styled("level: ", theme.muted_s()),
            Span::styled("no level for this codec", theme.body()),
        ])
    } else {
        let level = picker.current_level();
        let span_len = c.max_level - c.min_level;
        let pos = if span_len == 0 {
            0
        } else {
            10 * (level - c.min_level) / span_len
        } as u8;
        Line::from(vec![
            Span::styled("level: ", theme.muted_s()),
            Span::styled(format!("{level:>2}"), theme.title()),
            Span::styled(
                format!("  (range {} – {})  ", c.min_level, c.max_level),
                theme.muted_s(),
            ),
            bar(pos, theme),
        ])
    };
    let output_name = picker.output_extension(picker.action);
    let summary = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("codec : ", theme.muted_s()),
            Span::styled(c.name, theme.title()),
            Span::raw("   "),
            Span::styled(c.blurb, theme.muted_s()),
        ]),
        level_line,
        Line::from(vec![
            Span::styled("output: ", theme.muted_s()),
            Span::styled(
                if output_name.is_empty() {
                    "(raw output for the chosen action)".to_string()
                } else {
                    format!("filename will end in {output_name}")
                },
                theme.body(),
            ),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.muted_s())
            .title(Line::from(Span::styled(" Selected ", theme.title()))),
    );
    f.render_widget(summary, outer[2]);

    // Footer
    let footer = crate::tui::widgets::footer_keys(
        theme,
        &[
            ("Up/Dn", "choose codec"),
            ("Left/Right or -/+", "adjust level"),
            ("0/9", "min/max"),
            ("Enter", "next"),
            ("Esc", "cancel"),
        ],
    );
    f.render_widget(Paragraph::new(footer), outer[3]);
}

fn bar(score: u8, theme: &Theme) -> Span<'static> {
    const W: usize = 10;
    let filled = (score as usize).min(W);
    let mut s = String::with_capacity(W);
    for _ in 0..filled {
        s.push('█');
    }
    for _ in filled..W {
        s.push('░');
    }
    Span::styled(s, theme.success_s())
}

fn centered(area: Rect, want_w: u16, want_h: u16) -> Rect {
    let w = want_w.min(area.width.saturating_sub(4));
    let h = want_h.min(area.height.saturating_sub(4));
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((area.width.saturating_sub(w)) / 2),
            Constraint::Length(w),
            Constraint::Min(0),
        ])
        .split(area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(h)) / 2),
            Constraint::Length(h),
            Constraint::Min(0),
        ])
        .split(cols[1]);
    rows[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_clamped_to_range() {
        let p = CodecPicker::new(Action::Backup, 0);
        for (i, c) in CODECS.iter().enumerate() {
            let l = p.levels[i];
            assert!(
                (c.min_level..=c.max_level).contains(&l),
                "{} default {l} outside [{}, {}]",
                c.name,
                c.min_level,
                c.max_level
            );
        }
    }

    #[test]
    fn level_clamps_at_codec_bounds() {
        let mut p = CodecPicker::new(Action::Backup, 0);
        // Cursor is on zstd by default.
        p.jump_min();
        assert_eq!(p.current_level(), 1);
        p.level_down();
        assert_eq!(p.current_level(), 1, "should clamp at min");
        p.jump_max();
        assert_eq!(p.current_level(), 22);
        p.level_up();
        assert_eq!(p.current_level(), 22, "should clamp at max");
    }

    #[test]
    fn per_codec_levels_are_independent() {
        let mut p = CodecPicker::new(Action::Backup, 0);
        p.jump_max(); // zstd -> 22
        p.move_down(); // -> lz4
        p.move_down(); // -> brotli
        p.jump_max(); // brotli -> 11
        p.move_up(); // -> lz4 (no level — clamped to 0)
        p.move_up(); // -> zstd
        assert_eq!(p.current_level(), 22, "zstd should remember its choice");
    }

    #[test]
    fn output_extension_combines_action_and_codec() {
        let mut p = CodecPicker::new(Action::Backup, 0);
        assert_eq!(p.output_extension(Action::Backup), ".img.zst");
        assert_eq!(p.output_extension(Action::Archive), ".tar.zst");
        p.move_down();
        assert_eq!(p.output_extension(Action::Backup), ".img.lz4");
    }

    #[test]
    fn cursor_clamps_at_codec_list_bounds() {
        let mut p = CodecPicker::new(Action::Backup, 0);
        p.move_up();
        assert_eq!(p.cursor, 0);
        for _ in 0..50 {
            p.move_down();
        }
        assert_eq!(p.cursor, CODECS.len() - 1);
    }
}
