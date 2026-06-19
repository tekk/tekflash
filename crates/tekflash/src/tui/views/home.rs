//! Home view: device table + action menu + status bar.

use crate::tui::progress_runner::{BackupProgress, BackupStatus};
use crate::tui::{layout::Mode, widgets, AppState};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, mode: Mode, state: &AppState) {
    let has_detached_backup = state.backup_progress.is_some() && state.backup_detached;
    let banner_h: u16 = if has_detached_backup { 3 } else { 0 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(banner_h),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);

    render_header(f, chunks[0], state);
    if has_detached_backup {
        if let Some(p) = state.backup_progress.as_ref() {
            render_backup_banner(f, chunks[1], p, &state.theme);
        }
    }
    render_table(f, chunks[2], state, mode);
    render_footer(f, chunks[3], state);
}

fn render_header(f: &mut Frame, area: Rect, state: &AppState) {
    let theme = &state.theme;
    let title = Line::from(vec![
        Span::styled(" tekflash ", theme.title()),
        Span::raw(" "),
        Span::styled(format!("v{}", env!("CARGO_PKG_VERSION")), theme.muted_s()),
        Span::raw("   "),
        Span::styled(
            if state.show_all {
                "[showing internal disks]"
            } else {
                "[removable only]"
            },
            if state.show_all {
                theme.warning_s()
            } else {
                theme.muted_s()
            },
        ),
    ]);
    let p = Paragraph::new(title).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.muted_s()),
    );
    f.render_widget(p, area);
}

/// One-line summary of the detached backup with a "press b to resume" cue.
fn render_backup_banner(
    f: &mut Frame,
    area: Rect,
    p: &BackupProgress,
    theme: &crate::tui::theme::Theme,
) {
    let (label, label_style, border_style): (&str, Style, Style) = match &p.status {
        BackupStatus::Running => (" Backup running ", theme.success_s(), theme.success_s()),
        BackupStatus::Finished { .. } => {
            (" Backup finished ", theme.success_s(), theme.success_s())
        }
        BackupStatus::Failed { .. } => (" Backup failed ", theme.danger_s(), theme.danger_s()),
    };

    let pct = p
        .fraction()
        .map(|f| format!("{:.0}%", f * 100.0))
        .unwrap_or_else(|| "—".to_string());
    let rate = format!("{}/s", human_bytes(p.instant_rate as u64));
    let dest = p
        .params
        .dest
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| p.params.dest.display().to_string());

    let resume_cue = match &p.status {
        BackupStatus::Running => "press Tab to resume",
        BackupStatus::Finished { .. } => "press Tab to view summary",
        BackupStatus::Failed { .. } => "press Tab for error details",
    };

    let line = Line::from(vec![
        Span::styled(label, label_style),
        Span::raw("  "),
        Span::styled(format!("{} {pct}", human_bytes(p.bytes_in)), theme.body()),
        Span::styled("  ·  ", theme.muted_s()),
        Span::styled(rate, theme.body()),
        Span::styled("  ·  ", theme.muted_s()),
        Span::styled(format!("-> {dest}"), theme.body()),
        Span::styled("  ·  ", theme.muted_s()),
        Span::styled(resume_cue, theme.title()),
    ]);
    let widget = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style),
    );
    f.render_widget(widget, area);
}

fn render_table(f: &mut Frame, area: Rect, state: &AppState, _mode: Mode) {
    let theme = &state.theme;
    let header_cells = ["PATH", "MODEL", "SIZE", "BUS", "RM", "MOUNT"]
        .iter()
        .map(|h| Cell::from(*h).style(theme.title()));
    let header = Row::new(header_cells).height(1).bottom_margin(0);

    let rows: Vec<Row> = state
        .devices
        .iter()
        .map(|d| {
            let mount = d
                .mountpoints
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let mut cells = vec![
                Cell::from(d.path.display().to_string()),
                Cell::from(d.name()),
                Cell::from(d.size_human()),
                Cell::from(format!("{:?}", d.transport)),
                Cell::from(if d.removable { "yes" } else { "no " }),
                Cell::from(mount),
            ];
            if d.is_system {
                for c in &mut cells {
                    *c = c.clone().style(theme.danger_s());
                }
            }
            Row::new(cells)
        })
        .collect();

    let widths = [
        Constraint::Length(24),
        Constraint::Min(20),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(4),
        Constraint::Min(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .highlight_style(theme.selected())
        .highlight_symbol("> ")
        .block(
            Block::default()
                .title(Line::from(Span::styled(" Devices ", theme.title())))
                .borders(Borders::ALL)
                .border_style(theme.muted_s()),
        );

    let mut ts = TableState::default();
    if !state.devices.is_empty() {
        ts.select(Some(state.selected.min(state.devices.len() - 1)));
    }
    f.render_stateful_widget(table, area, &mut ts);
}

fn render_footer(f: &mut Frame, area: Rect, state: &AppState) {
    let theme = &state.theme;
    let mut keys: Vec<(&str, &str)> = vec![("↑↓", "select"), ("↵", "pick action")];
    let has_detached = state.backup_progress.is_some() && state.backup_detached;
    keys.push(("a", "show-all"));
    if has_detached {
        // Tab switches to / resumes the detached session. The label adapts so the user
        // can tell from the footer whether the backup is still running or it finished.
        let label = match state.backup_progress.as_ref().map(|p| &p.status) {
            Some(BackupStatus::Finished { .. }) => "view backup summary",
            Some(BackupStatus::Failed { .. }) => "view backup error",
            _ => "resume session",
        };
        keys.push(("Tab", label));
    }
    keys.push(("r", "refresh"));
    keys.push(("?", "help"));
    keys.push(("q", "quit"));
    let line = widgets::footer_keys(theme, &keys);
    let p = Paragraph::new(line);
    f.render_widget(p, area);
}

fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut idx = 0;
    while v >= 1000.0 && idx + 1 < UNITS.len() {
        v /= 1000.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[idx])
    }
}
