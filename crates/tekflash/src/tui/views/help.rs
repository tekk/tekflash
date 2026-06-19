//! In-TUI help overlay shown on `?` / F1.

use crate::tui::theme::Theme;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

pub fn overlay(f: &mut Frame, area: Rect, theme: &Theme) {
    let popup = centered(area, 70, 24);
    f.render_widget(Clear, popup);

    let lines = vec![
        Line::from(Span::styled(
            " tekflash — keyboard shortcuts ",
            theme.title(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("↑↓ / j k", theme.title()),
            Span::raw("   move selection in tables"),
        ]),
        Line::from(vec![
            Span::styled("Enter", theme.title()),
            Span::raw("      pick the highlighted device -> Flash / Backup / Archive"),
        ]),
        Line::from(vec![
            Span::styled("a", theme.title()),
            Span::raw("          toggle showing internal / system disks"),
        ]),
        Line::from(vec![
            Span::styled("Tab", theme.title()),
            Span::raw("        switch to a running session (resumes a detached backup)"),
        ]),
        Line::from(vec![
            Span::styled("r", theme.title()),
            Span::raw("          refresh device list"),
        ]),
        Line::from(vec![
            Span::styled("F2", theme.title()),
            Span::raw("         open the file browser at the focused field"),
        ]),
        Line::from(""),
        Line::from(Span::styled(" File browser ", theme.title())),
        Line::from(""),
        Line::from(vec![
            Span::styled("type", theme.title()),
            Span::raw("       Open: type-ahead filter (case-insensitive)"),
        ]),
        Line::from(vec![
            Span::styled("type", theme.title()),
            Span::raw("       Save: type the output filename; auto-extension shown in grey"),
        ]),
        Line::from(vec![
            Span::styled("Backspace", theme.title()),
            Span::raw("  pop a typed character, or go up one directory"),
        ]),
        Line::from(vec![
            Span::styled("..", theme.title()),
            Span::raw("         row at the top of every listing — Enter to go up"),
        ]),
        Line::from(vec![
            Span::styled(".", theme.title()),
            Span::raw("          (Save mode) Enter to commit the save in the current dir"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl-H", theme.title()),
            Span::raw("     toggle hidden files"),
        ]),
        Line::from(vec![
            Span::styled("Tab", theme.title()),
            Span::raw("        toggle showing every file type (not just flashables)"),
        ]),
        Line::from(vec![
            Span::styled("? / F1", theme.title()),
            Span::raw("     toggle this help overlay"),
        ]),
        Line::from(vec![
            Span::styled("q / Esc / Ctrl-C", theme.title()),
            Span::raw("  quit / dismiss overlay"),
        ]),
        Line::from(""),
        Line::from(Span::styled(" Safety ", theme.title())),
        Line::from(""),
        Line::from("Internal/system disks are hidden by default. Pass --show-all or press"),
        Line::from("a to reveal them; they are highlighted in red and require explicit"),
        Line::from("confirmation before any write."),
        Line::from(""),
        Line::from("All destructive operations show a confirmation modal with the device"),
        Line::from("vendor, model, serial, size, and mounted volumes — verify carefully."),
    ];

    let p = Paragraph::new(lines)
        .style(theme.body())
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.title())
                .title(Line::from(Span::styled(" Help ", theme.title()))),
        );
    f.render_widget(p, popup);
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
