//! Live progress view for a running or just-completed backup.
//!
//! Layout (top to bottom):
//!
//! - header banner with title + source → destination
//! - gauge with bytes-read / total + percentage
//! - two-column stats panel (Stream / Throughput)
//! - block-device matrix map — each cell ≈ `total / cells` bytes, shaded by progress
//! - status line (Running / Finished / Failed)
//! - footer key hints
//!
//! The block map gets the largest share of vertical space when the terminal is tall
//! enough; on a 24-row terminal it gracefully collapses to a thinner band so the rest
//! of the panel still fits.

use crate::tui::progress_runner::{BackupProgress, BackupStatus};
use crate::tui::theme::Theme;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame,
};
use std::time::Duration;
use tekflash_core::pipeline::compress::Codec;

pub fn render(f: &mut Frame, area: Rect, p: &BackupProgress, theme: &Theme) {
    // Carve out vertical space adaptively. The block map gets whatever is left after
    // the fixed-size header / gauge / stats / status / footer — minimum 4 rows to be
    // useful, maximum is whatever the terminal gives us.
    let header_h = 3;
    let gauge_h = 3;
    let stats_h = 11;
    let status_h = 3;
    let footer_h = 2;
    let fixed = header_h + gauge_h + stats_h + status_h + footer_h;
    let map_h = area.height.saturating_sub(fixed);

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_h),
            Constraint::Length(gauge_h),
            Constraint::Length(stats_h),
            Constraint::Length(map_h),
            Constraint::Length(status_h),
            Constraint::Length(footer_h),
        ])
        .split(area);

    render_header(f, outer[0], p, theme);
    render_gauge(f, outer[1], p, theme);
    render_stats(f, outer[2], p, theme);
    if map_h >= 3 {
        render_block_map(f, outer[3], p, theme);
    }
    render_status(f, outer[4], p, theme);
    render_footer(f, outer[5], p, theme);
}

fn render_header(f: &mut Frame, area: Rect, p: &BackupProgress, theme: &Theme) {
    let title = match &p.status {
        BackupStatus::Running => " Backup running ",
        BackupStatus::Finished { .. } => " Backup finished ",
        BackupStatus::Failed { .. } => " Backup failed ",
    };
    let title_style = match &p.status {
        BackupStatus::Running => theme.title(),
        BackupStatus::Finished { .. } => theme.success_s(),
        BackupStatus::Failed { .. } => theme.danger_s(),
    };
    let p_widget = Paragraph::new(Line::from(vec![
        Span::styled(title, title_style),
        Span::raw("  "),
        Span::styled(p.params.source.display().to_string(), theme.body()),
        Span::styled("  →  ", theme.title()),
        Span::styled(p.params.dest.display().to_string(), theme.body()),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.muted_s()),
    );
    f.render_widget(p_widget, area);
}

fn render_gauge(f: &mut Frame, area: Rect, p: &BackupProgress, theme: &Theme) {
    let ratio = p.fraction().unwrap_or(0.0);
    let pct_int = ((ratio * 100.0).clamp(0.0, 100.0)).round() as u16;
    let label = match p.params.total_bytes {
        Some(total) => format!(
            "{} / {}  ({pct_int}%)",
            human_bytes(p.bytes_in),
            human_bytes(total)
        ),
        None => human_bytes(p.bytes_in),
    };
    let gauge_style = match &p.status {
        BackupStatus::Failed { .. } => theme.danger_s(),
        _ => theme.success_s(),
    };
    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.muted_s()),
        )
        .gauge_style(gauge_style)
        .ratio(ratio)
        .label(label);
    f.render_widget(gauge, area);
}

fn render_stats(f: &mut Frame, area: Rect, p: &BackupProgress, theme: &Theme) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let level_str = match p.params.codec {
        Codec::None | Codec::Lz4 => "—".to_string(),
        _ => p.params.level.0.to_string(),
    };
    let total = p
        .params
        .total_bytes
        .map(human_bytes)
        .unwrap_or_else(|| "unknown".to_string());

    let mut left_lines = vec![
        labelled(theme, "source     ", p.params.source.display().to_string()),
        labelled(theme, "dest       ", p.params.dest.display().to_string()),
        labelled(
            theme,
            "codec      ",
            format!("{}  (level {level_str})", p.params.codec.human()),
        ),
        labelled(theme, "total size ", total),
        labelled(
            theme,
            "read       ",
            format!(
                "{}  ({})",
                human_bytes(p.bytes_in),
                p.fraction()
                    .map(|r| format!("{:.1}%", r * 100.0))
                    .unwrap_or_else(|| "—".to_string())
            ),
        ),
        labelled(
            theme,
            "wrote      ",
            format!(
                "{}  (ratio {})",
                human_bytes(p.bytes_out),
                p.ratio()
                    .map(|r| format!("{r:.2}×"))
                    .unwrap_or_else(|| "—".to_string())
            ),
        ),
        labelled(
            theme,
            "est. output",
            estimated_output_size(p)
                .map(human_bytes)
                .unwrap_or_else(|| "—".to_string()),
        ),
    ];
    if let BackupStatus::Finished { hash_hex, .. } = &p.status {
        left_lines.push(labelled(theme, "BLAKE3     ", short_hash(hash_hex)));
    }

    let elapsed = p.elapsed();
    let right_lines = vec![
        labelled(theme, "rate (now) ", format_rate(p.instant_rate)),
        labelled(theme, "rate (avg) ", format_rate(p.average_rate())),
        labelled(theme, "elapsed    ", format_duration(elapsed)),
        labelled(
            theme,
            "ETA        ",
            p.eta()
                .map(format_duration)
                .unwrap_or_else(|| "—".to_string()),
        ),
        labelled(
            theme,
            "remaining  ",
            p.params
                .total_bytes
                .map(|t| human_bytes(t.saturating_sub(p.bytes_in)))
                .unwrap_or_else(|| "—".to_string()),
        ),
        labelled(theme, "buffer     ", "4 MiB read · 4 MiB write".to_string()),
        labelled(
            theme,
            "status     ",
            match &p.status {
                BackupStatus::Running => "streaming…".to_string(),
                BackupStatus::Finished { .. } => "complete".to_string(),
                BackupStatus::Failed { .. } => "error".to_string(),
            },
        ),
    ];

    let left = Paragraph::new(left_lines).block(
        Block::default()
            .title(Line::from(Span::styled(" Stream ", theme.title())))
            .borders(Borders::ALL)
            .border_style(theme.muted_s()),
    );
    let right = Paragraph::new(right_lines).block(
        Block::default()
            .title(Line::from(Span::styled(" Throughput ", theme.title())))
            .borders(Borders::ALL)
            .border_style(theme.muted_s()),
    );
    f.render_widget(left, cols[0]);
    f.render_widget(right, cols[1]);
}

/// Defrag.exe-style block map. Each cell is a 2-column-wide character pair representing
/// a fixed chunk of the source device. Cells fill left-to-right, top-to-bottom as the
/// backup reads. The currently-being-read cell uses a partial shade.
fn render_block_map(f: &mut Frame, area: Rect, p: &BackupProgress, theme: &Theme) {
    let inner = area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    let cell_w = 2usize; // each cell = 2 characters wide
    let cols = (inner.width as usize / cell_w).max(1);
    let rows = inner.height as usize;
    let total_cells = (cols * rows) as u64;

    let Some(total) = p.params.total_bytes else {
        let warn = Paragraph::new("(block map unavailable — source size unknown)")
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme.muted_s())
                    .title(Line::from(Span::styled(" Block map ", theme.title()))),
            );
        f.render_widget(warn, area);
        return;
    };
    let bytes_per_cell = (total / total_cells.max(1)).max(1);

    let filled_cells = (p.bytes_in / bytes_per_cell).min(total_cells);
    let progress_into_partial =
        p.bytes_in.saturating_sub(filled_cells * bytes_per_cell) as f64 / bytes_per_cell as f64;

    // Three shades: full / partial / empty. Partial gets one of four sub-shades by how
    // far into the current cell we are.
    let full_ch = "██";
    let empty_ch = "░░";
    let partial_chars = ["░░", "▒▒", "▓▓", "██"];
    let full_style = theme.success_s();
    let empty_style = Style::default().fg(theme.muted);
    let partial_style = Style::default().fg(theme.warning);

    let mut lines: Vec<Line<'_>> = Vec::with_capacity(rows);
    let mut idx: u64 = 0;
    for _ in 0..rows {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(cols);
        for _ in 0..cols {
            let (ch, style) = if idx < filled_cells {
                (full_ch, full_style)
            } else if idx == filled_cells {
                let bucket = (progress_into_partial * partial_chars.len() as f64) as usize;
                let bucket = bucket.min(partial_chars.len() - 1);
                (partial_chars[bucket], partial_style)
            } else {
                (empty_ch, empty_style)
            };
            spans.push(Span::styled(ch.to_string(), style));
            idx += 1;
        }
        lines.push(Line::from(spans));
    }

    let title = format!(
        " Block map   each cell ≈ {}   ({} cells)",
        human_bytes(bytes_per_cell),
        total_cells
    );
    let block = Paragraph::new(lines).block(
        Block::default()
            .title(Line::from(Span::styled(title, theme.title())))
            .borders(Borders::ALL)
            .border_style(theme.muted_s()),
    );
    f.render_widget(block, area);
}

fn render_status(f: &mut Frame, area: Rect, p: &BackupProgress, theme: &Theme) {
    let (text, style) = match &p.status {
        BackupStatus::Running => (
            "Backup is streaming. Press Esc to detach (the worker keeps running in the background).".to_string(),
            theme.muted_s(),
        ),
        BackupStatus::Finished { hash_hex, elapsed } => (
            format!(
                "Backup OK in {}.  BLAKE3 = {}",
                format_duration(*elapsed),
                short_hash(hash_hex)
            ),
            theme.success_s(),
        ),
        BackupStatus::Failed { message } => (format!("Backup FAILED: {message}"), theme.danger_s()),
    };
    let p_widget = Paragraph::new(Line::from(Span::styled(text, style)))
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.muted_s()),
        );
    f.render_widget(p_widget, area);
}

fn render_footer(f: &mut Frame, area: Rect, p: &BackupProgress, theme: &Theme) {
    let keys: Vec<(&str, &str)> = if p.is_running() {
        vec![("Esc / q", "detach (backup keeps running)")]
    } else {
        vec![("Esc / q / Enter", "back to home")]
    };
    let line = crate::tui::widgets::footer_keys(theme, &keys);
    f.render_widget(Paragraph::new(line), area);
}

fn labelled<'a>(theme: &Theme, label: &'a str, value: String) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!(" {label}: "), theme.muted_s()),
        Span::styled(value, theme.body()),
    ])
}

/// Projected total output size based on the running compression ratio.
fn estimated_output_size(p: &BackupProgress) -> Option<u64> {
    let total = p.params.total_bytes?;
    if p.bytes_in == 0 {
        return None;
    }
    let ratio = p.bytes_out as f64 / p.bytes_in as f64;
    Some((total as f64 * ratio) as u64)
}

fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = n as f64;
    let mut idx = 0;
    while v >= 1000.0 && idx + 1 < UNITS.len() {
        v /= 1000.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{n} {}", UNITS[0])
    } else {
        format!("{v:.2} {}", UNITS[idx])
    }
}

/// Format a byte-per-second rate as "152.30 MB/s  (9.14 GB/min)". Showing both gives a
/// feel for "per second" granularity and a "per minute" intuition for long jobs.
fn format_rate(bytes_per_sec: f64) -> String {
    let per_sec = human_bytes(bytes_per_sec.max(0.0) as u64);
    let per_min = human_bytes((bytes_per_sec.max(0.0) * 60.0) as u64);
    format!("{per_sec}/s  ({per_min}/min)")
}

fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{h:02}:{m:02}:{sec:02}")
    } else {
        format!("{m:02}:{sec:02}")
    }
}

fn short_hash(h: &str) -> String {
    if h.len() <= 24 {
        h.to_string()
    } else {
        format!("{}…{}", &h[..12], &h[h.len() - 12..])
    }
}
