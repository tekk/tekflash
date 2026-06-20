//! Destructive-action confirmation modal.
//!
//! Shown after the user has picked an action AND a file. Displays everything that's
//! about to happen — device vendor / model / size / mountpoints, source-or-destination
//! file, the codec — and forces an explicit Confirm / Cancel choice.
//!
//! Borders go red (`theme.danger`) for any flash and for any action touching an
//! internal/system disk; subtler `accent`-coloured otherwise.

use crate::tui::theme::Theme;
use crate::tui::views::action_picker::Action;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use std::path::PathBuf;
use tekflash_core::device::BlockDevice;
use tekflash_core::pipeline::compress::{Codec, CompressionLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmFocus {
    Cancel,
    Confirm,
}

#[derive(Debug)]
pub struct Confirm {
    pub action: Action,
    pub device_idx: usize,
    pub file: PathBuf,
    pub focus: ConfirmFocus,
    /// Compression codec chosen for backup/archive. None for flash.
    pub codec: Option<Codec>,
    pub level: Option<CompressionLevel>,
    /// Result of the previous run, if any. Lets the user re-trigger or see what happened
    /// without losing the modal.
    pub result_message: Option<String>,
}

impl Confirm {
    pub fn new(action: Action, device_idx: usize, file: PathBuf) -> Self {
        // Default the focus to Cancel for any *flash* (the destructive direction) and
        // for actions on a system device — make the safe choice the resting state.
        let focus = ConfirmFocus::Cancel;
        Self {
            action,
            device_idx,
            file,
            focus,
            codec: None,
            level: None,
            result_message: None,
        }
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            ConfirmFocus::Cancel => ConfirmFocus::Confirm,
            ConfirmFocus::Confirm => ConfirmFocus::Cancel,
        };
    }
}

pub fn render(f: &mut Frame, area: Rect, confirm: &Confirm, device: &BlockDevice, theme: &Theme) {
    let popup = centered(area, 110, 20);
    f.render_widget(Clear, popup);

    let dangerous = matches!(confirm.action, Action::Flash) || device.is_system;
    let border = if dangerous {
        theme.danger_s()
    } else {
        Style::default().fg(theme.accent)
    };

    let title = match confirm.action {
        Action::Flash => " Confirm flash ",
        Action::Backup => " Confirm backup ",
        Action::Archive => " Confirm archive ",
    };

    let direction_line = match confirm.action {
        Action::Flash => Line::from(vec![
            Span::styled(confirm.file.display().to_string(), theme.body()),
            Span::styled("   ->   ", theme.title()),
            Span::styled(device.path.display().to_string(), theme.danger_s()),
        ]),
        Action::Backup | Action::Archive => Line::from(vec![
            Span::styled(device.path.display().to_string(), theme.body()),
            Span::styled("   ->   ", theme.title()),
            Span::styled(confirm.file.display().to_string(), theme.body()),
        ]),
    };

    let mounts = if device.mountpoints.is_empty() {
        "(none)".to_string()
    } else {
        device
            .mountpoints
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };

    let mut lines = vec![
        Line::from(Span::styled(title, theme.title())),
        Line::from(""),
        direction_line,
        Line::from(""),
        Line::from(vec![
            Span::styled("device : ", theme.muted_s()),
            Span::raw(device.name()),
        ]),
        Line::from(vec![
            Span::styled("size   : ", theme.muted_s()),
            Span::raw(device.size_human()),
        ]),
        Line::from(vec![
            Span::styled("bus    : ", theme.muted_s()),
            Span::raw(format!("{:?}", device.transport)),
        ]),
        Line::from(vec![
            Span::styled("mounts : ", theme.muted_s()),
            Span::raw(mounts),
        ]),
    ];
    if let (Some(codec), Some(level)) = (confirm.codec, confirm.level) {
        let level_str = if matches!(codec, Codec::None | Codec::Lz4) {
            "—".to_string()
        } else {
            level.0.to_string()
        };
        lines.push(Line::from(vec![
            Span::styled("codec  : ", theme.muted_s()),
            Span::raw(format!("{}  (level {level_str})", codec.human())),
        ]));
    }
    if device.is_system {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "⚠  This is the system / boot disk. Writing to it can break this machine.",
            theme.danger_s(),
        )));
    }
    if matches!(confirm.action, Action::Flash) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "⚠  Flashing will OVERWRITE every byte on the target device.",
            theme.danger_s(),
        )));
    }
    if let Some(msg) = &confirm.result_message {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(msg.clone(), theme.success_s())));
    }

    let body = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border)
            .title(Line::from(Span::styled(title, theme.title()))),
    );

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(popup);

    f.render_widget(body, inner[0]);

    let cancel_style = if confirm.focus == ConfirmFocus::Cancel {
        theme.selected()
    } else {
        theme.body()
    };
    let confirm_style = if confirm.focus == ConfirmFocus::Confirm {
        if dangerous {
            theme.danger_s()
        } else {
            theme.success_s()
        }
    } else {
        theme.body()
    };
    let buttons = Paragraph::new(Line::from(vec![
        Span::raw("   "),
        Span::styled("  Cancel  ", cancel_style),
        Span::raw("            "),
        Span::styled(
            match confirm.action {
                Action::Flash => "  Flash  ",
                Action::Backup => "  Back up  ",
                Action::Archive => "  Archive  ",
            },
            confirm_style,
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.muted_s())
            .title(Line::from(Span::styled(
                " Left/Right choose | Enter activate | Esc cancel ",
                theme.muted_s(),
            ))),
    );
    f.render_widget(buttons, inner[1]);
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
