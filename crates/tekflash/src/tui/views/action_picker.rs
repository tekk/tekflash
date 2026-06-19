//! Modal that appears when the user presses Enter on a device in the home view.
//!
//! Asks "what do you want to do with this device?" and offers Flash / Backup / Archive.
//! Selecting one drops back to the event loop with the chosen action set, so the next
//! step (the file browser) can open with the right purpose.

use crate::tui::theme::Theme;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};
use tekflash_core::device::BlockDevice;

/// What the user wants to do with the selected device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Write an image from disk to this device.
    Flash,
    /// Bit-exact backup of this device to an image file.
    Backup,
    /// File-level tar.zst archive of this device's filesystem (must be mounted).
    Archive,
}

impl Action {
    pub const ALL: [Action; 3] = [Action::Flash, Action::Backup, Action::Archive];

    pub fn label(self) -> &'static str {
        match self {
            Action::Flash => "Flash an image to this device",
            Action::Backup => "Back up this device to an image file",
            Action::Archive => "Archive this device's filesystem (.tar.zst)",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Action::Flash => "Flash",
            Action::Backup => "Backup",
            Action::Archive => "Archive",
        }
    }

    pub fn explanation(self) -> &'static str {
        match self {
            Action::Flash => "Overwrites the device. The image source is decompressed on the fly.",
            Action::Backup => "Reads the device bit-for-bit, compressed via the chosen codec.",
            Action::Archive => "Reads files (not blocks); the device must be mounted.",
        }
    }
}

#[derive(Debug)]
pub struct ActionPicker {
    /// Index into `AppState.devices` for the device the user picked.
    pub device_idx: usize,
    /// Cursor inside `Action::ALL`.
    pub cursor: usize,
}

impl ActionPicker {
    pub fn new(device_idx: usize) -> Self {
        Self {
            device_idx,
            cursor: 0,
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < Action::ALL.len() {
            self.cursor += 1;
        }
    }

    pub fn selected_action(&self) -> Action {
        Action::ALL[self.cursor]
    }
}

pub fn render(
    f: &mut Frame,
    area: Rect,
    picker: &ActionPicker,
    device: &BlockDevice,
    theme: &Theme,
) {
    let popup = centered(area, 90, 14);
    f.render_widget(Clear, popup);

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(Action::ALL.len() as u16 + 2),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(popup);

    // Header: "[vendor model] · [size] · [path]"
    let header_lines = vec![
        Line::from(Span::styled(" Choose an action ", theme.title())),
        Line::from(vec![
            Span::styled(device.name(), theme.body()),
            Span::raw("  "),
            Span::styled(device.size_human(), theme.muted_s()),
            Span::raw("  "),
            Span::styled(device.path.display().to_string(), theme.muted_s()),
        ]),
    ];
    let header = ratatui::widgets::Paragraph::new(header_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(if device.is_system {
                theme.danger_s()
            } else {
                theme.muted_s()
            }),
    );
    f.render_widget(header, outer[0]);

    // Action list.
    let items: Vec<ListItem<'_>> = Action::ALL
        .iter()
        .map(|a| ListItem::new(Line::from(Span::styled(a.label(), theme.body()))))
        .collect();
    let list = List::new(items)
        .highlight_style(theme.selected())
        .highlight_symbol("> ")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.muted_s()),
        );
    let mut s = ListState::default();
    s.select(Some(picker.cursor));
    f.render_stateful_widget(list, outer[1], &mut s);

    // Explanation of the focused action.
    let exp = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
        picker.selected_action().explanation(),
        theme.muted_s(),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.muted_s()),
    );
    f.render_widget(exp, outer[2]);

    let footer = crate::tui::widgets::footer_keys(
        theme,
        &[("↑↓", "choose"), ("↵", "next"), ("Esc", "cancel")],
    );
    f.render_widget(ratatui::widgets::Paragraph::new(footer), outer[3]);
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
    fn cursor_starts_at_flash() {
        let p = ActionPicker::new(0);
        assert_eq!(p.selected_action(), Action::Flash);
    }

    #[test]
    fn cursor_clamps_at_bounds() {
        let mut p = ActionPicker::new(0);
        p.move_up();
        assert_eq!(p.cursor, 0, "moving up at the top should clamp");
        for _ in 0..10 {
            p.move_down();
        }
        assert_eq!(
            p.cursor,
            Action::ALL.len() - 1,
            "moving down past end should clamp"
        );
        assert_eq!(p.selected_action(), Action::Archive);
    }

    #[test]
    fn each_action_advertises_a_label_and_explanation() {
        for a in Action::ALL {
            assert!(!a.label().is_empty());
            assert!(!a.explanation().is_empty());
            assert!(!a.short_label().is_empty());
        }
    }
}
