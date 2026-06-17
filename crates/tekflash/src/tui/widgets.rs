//! Shared widget helpers.

use crate::tui::theme::Theme;
use ratatui::{
    layout::{Alignment, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn too_small(f: &mut Frame, area: Rect, theme: &Theme) {
    let body = format!(
        "Terminal too small — tekflash needs at least 80x24.\nCurrent: {}x{}.\nResize the window and tekflash will redraw automatically.",
        area.width, area.height
    );
    let p = Paragraph::new(body)
        .style(theme.body())
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .title(Line::from(Span::styled(" tekflash ", theme.title())))
                .borders(Borders::ALL)
                .border_style(theme.danger_s()),
        );
    f.render_widget(p, area);
}

pub fn footer_keys(theme: &Theme, keys: &[(&str, &str)]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(keys.len() * 4);
    for (i, (k, v)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("   ", theme.muted_s()));
        }
        spans.push(Span::styled(k.to_string(), theme.title()));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(v.to_string(), theme.muted_s()));
    }
    Line::from(spans)
}
