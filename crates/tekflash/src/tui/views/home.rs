//! Home view: device table + action menu + status bar.

use crate::tui::{layout::Mode, widgets, AppState};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, mode: Mode, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);

    render_header(f, chunks[0], state);
    render_table(f, chunks[1], state, mode);
    render_footer(f, chunks[2], state);
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

fn render_footer(f: &mut Frame, area: Rect, state: &AppState) {
    let theme = &state.theme;
    let line = widgets::footer_keys(
        theme,
        &[
            ("↑↓", "select"),
            ("↵", "pick action"),
            ("Tab", "show-all"),
            ("r", "refresh"),
            ("?", "help"),
            ("q", "quit"),
        ],
    );
    let p = Paragraph::new(line);
    f.render_widget(p, area);
}
