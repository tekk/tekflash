//! Home view: device table + sessions panel + footer.

use crate::tui::progress_runner::{BackupProgress, BackupStatus};
use crate::tui::theme::Theme;
use crate::tui::{layout::Mode, widgets, AppState};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, mode: Mode, state: &AppState) {
    let session_count = state.sessions.len() as u16;
    // 2 borders + 1 line per session, capped at 6 visible rows so the panel never
    // squeezes the device table below its minimum.
    let session_panel_h: u16 = if session_count == 0 {
        0
    } else {
        (2 + session_count.min(6)).min(area.height.saturating_sub(13))
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(session_panel_h),
            Constraint::Length(2),
        ])
        .split(area);

    render_header(f, chunks[0], state);
    render_table(f, chunks[1], state, mode);
    if session_panel_h > 0 {
        render_sessions(f, chunks[2], state);
    }
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

/// One mini progress row per session: status icon, bar, percentage, rate, destination
/// filename. The block title shows the cycle-to-next-session hint.
fn render_sessions(f: &mut Frame, area: Rect, state: &AppState) {
    let theme = &state.theme;
    let running = state.sessions.iter().filter(|s| s.is_running()).count();
    let title = Line::from(vec![
        Span::styled(" Sessions ", theme.title()),
        Span::raw(" "),
        Span::styled(
            format!(
                "{} running · {} total · Tab to view",
                running,
                state.sessions.len()
            ),
            theme.muted_s(),
        ),
    ]);

    let inner_w = area.width.saturating_sub(2) as usize;
    let lines: Vec<Line<'_>> = state
        .sessions
        .iter()
        .take(6) // see render() — we only allocate room for up to 6 rows
        .enumerate()
        .map(|(i, s)| session_line(i, s, inner_w, theme))
        .collect();

    let widget = Paragraph::new(lines).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(theme.muted_s()),
    );
    f.render_widget(widget, area);
}

fn session_line<'a>(idx: usize, s: &'a BackupProgress, inner_w: usize, theme: &Theme) -> Line<'a> {
    let (icon, icon_style): (&str, Style) = match &s.status {
        BackupStatus::Running => (" ▶", theme.success_s()),
        BackupStatus::Finished { .. } => (" ✓", theme.success_s()),
        BackupStatus::Failed { .. } => (" ✗", theme.danger_s()),
    };
    let ratio = s.fraction().unwrap_or(0.0);
    let pct = format!("{:>3}%", (ratio * 100.0).round() as u32);
    let rate = match &s.status {
        BackupStatus::Running => format!("{}/s", human_bytes(s.instant_rate as u64)),
        BackupStatus::Finished { .. } => "complete".to_string(),
        BackupStatus::Failed { .. } => "failed".to_string(),
    };
    let dest = s
        .params
        .dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| s.params.dest.display().to_string());

    // Compose: " ▶ [1] ████████░░ 72%  152 MB/s  -> dest.zst"
    // Budget the bar to whatever's left after fixed-width fields.
    let prefix = format!(" {icon} [{idx}]  ");
    let suffix = format!("  {pct}  {rate:>12}  -> {dest}");
    let bar_w = inner_w
        .saturating_sub(prefix.chars().count())
        .saturating_sub(suffix.chars().count())
        .clamp(6, 40);
    let bar = mini_bar(ratio, bar_w);
    let bar_style = match &s.status {
        BackupStatus::Failed { .. } => theme.danger_s(),
        _ => theme.success_s(),
    };

    Line::from(vec![
        Span::styled(format!(" {icon}"), icon_style),
        Span::styled(format!(" [{idx}]  "), theme.muted_s()),
        Span::styled(bar, bar_style),
        Span::styled(format!("  {pct}  "), theme.body()),
        Span::styled(format!("{rate:>12}"), theme.body()),
        Span::styled("  -> ", theme.muted_s()),
        Span::styled(dest, theme.body()),
    ])
}

fn render_footer(f: &mut Frame, area: Rect, state: &AppState) {
    let theme = &state.theme;
    let mut keys: Vec<(&str, &str)> = vec![("↑↓", "select"), ("↵", "pick action")];
    keys.push(("a", "show-all"));
    if !state.sessions.is_empty() {
        let running = state.sessions.iter().filter(|s| s.is_running()).count();
        let label = if running == state.sessions.len() && running > 0 {
            "view sessions"
        } else if running > 0 {
            "view sessions (some finished)"
        } else {
            "view finished sessions"
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

fn mini_bar(ratio: f64, width: usize) -> String {
    let filled = ((ratio.clamp(0.0, 1.0)) * width as f64).round() as usize;
    let filled = filled.min(width);
    let mut s = String::with_capacity(width * 3); // unicode blocks are 3 bytes
    for _ in 0..filled {
        s.push('█');
    }
    for _ in filled..width {
        s.push('░');
    }
    s
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
