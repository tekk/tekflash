//! In-TUI file browser.
#![allow(dead_code)] // landed before its call sites; wired up by Flash/Backup/Restore views
//!
//! Used both as a standalone "pick a file" view (launched from the home view's
//! Flash/Backup/Restore actions, from `F2` while focused on a path field, and from the
//! CLI when a path argument is omitted) and as the save-mode picker for backup/archive
//! destinations.

use crate::tui::theme::Theme;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tekflash_core::pipeline::format::{detect_by_extension, InputFormat};

/// Browser mode determines which extra UI affordances appear and what `Enter` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseMode {
    /// Pick an existing file to read from (flash source, restore archive).
    Open,
    /// Pick a directory + filename to write to (backup destination, archive destination).
    Save,
}

/// One row in the directory listing.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: Option<SystemTime>,
    /// Detected format from extension, if recognized. Magic-byte detection happens on
    /// the focused row only (in `update_focus_preview`), to keep listing cheap.
    pub format_hint: Option<InputFormat>,
}

#[derive(Debug)]
pub struct FileBrowser {
    pub cwd: PathBuf,
    pub mode: BrowseMode,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub show_hidden: bool,
    pub show_all_types: bool,
    /// Magic-byte-confirmed format for the focused entry, populated on demand.
    pub focused_magic: Option<InputFormat>,
    /// Save-mode filename input. Ignored in `Open` mode.
    pub save_name: String,
}

impl FileBrowser {
    pub fn open(start_dir: PathBuf, mode: BrowseMode) -> Self {
        let cwd = start_dir.canonicalize().unwrap_or(start_dir);
        let mut b = Self {
            cwd,
            mode,
            entries: Vec::new(),
            selected: 0,
            show_hidden: false,
            show_all_types: false,
            focused_magic: None,
            save_name: String::new(),
        };
        b.refresh();
        b
    }

    pub fn refresh(&mut self) {
        self.entries = read_dir(&self.cwd, self.show_hidden);
        // When show_all_types is false, sort flashable formats to the top, then dirs,
        // then everything else. When true, just dirs first then alphabetical.
        if self.show_all_types {
            self.entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            });
        } else {
            self.entries.sort_by(|a, b| {
                let a_rank = rank(a);
                let b_rank = rank(b);
                a_rank.cmp(&b_rank).then_with(|| a.name.cmp(&b.name))
            });
        }
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
        self.focused_magic = None;
    }

    pub fn focused(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.focused_magic = None;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
            self.focused_magic = None;
        }
    }

    pub fn enter_focused(&mut self) -> Option<PathBuf> {
        let entry = self.focused()?.clone();
        if entry.is_dir {
            self.cwd = entry.path;
            self.selected = 0;
            self.refresh();
            None
        } else {
            Some(entry.path)
        }
    }

    pub fn go_parent(&mut self) {
        if let Some(parent) = self.cwd.parent() {
            self.cwd = parent.to_path_buf();
            self.selected = 0;
            self.refresh();
        }
    }

    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.refresh();
    }

    pub fn toggle_all_types(&mut self) {
        self.show_all_types = !self.show_all_types;
        self.refresh();
    }

    /// Sniff magic bytes from the focused file. Cheap (reads at most 6 bytes); call
    /// each time the selection moves so the preview pane stays in sync.
    pub fn update_focus_preview(&mut self) {
        self.focused_magic = self
            .focused()
            .filter(|e| !e.is_dir)
            .and_then(|e| sniff_magic(&e.path));
    }
}

/// Where the browser should start, given the OS and an optional hint from upstream.
pub fn default_start_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn rank(e: &Entry) -> u8 {
    if e.is_dir {
        return 0;
    }
    match e.format_hint {
        Some(InputFormat::Raw | InputFormat::Zstd | InputFormat::Xz | InputFormat::Gzip) => 1,
        Some(_) => 2,
        None => 3,
    }
}

fn read_dir(dir: &Path, show_hidden: bool) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        out.push(Entry {
            path: path.clone(),
            name,
            is_dir: meta.is_dir(),
            size: meta.len(),
            mtime: meta.modified().ok(),
            format_hint: if meta.is_dir() {
                None
            } else {
                detect_by_extension(&path)
            },
        });
    }
    out
}

fn sniff_magic(path: &Path) -> Option<InputFormat> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 16];
    let n = f.read(&mut head).ok()?;
    Some(tekflash_core::pipeline::format::detect(&head[..n]))
}

pub fn render(f: &mut Frame, area: Rect, browser: &FileBrowser, theme: &Theme) {
    // Top: cwd path. Middle: list + preview pane. Bottom: footer with key hints.
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(if browser.mode == BrowseMode::Save {
                3
            } else {
                0
            }),
            Constraint::Length(2),
        ])
        .split(area);

    render_path_bar(f, outer[0], browser, theme);

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(outer[1]);
    render_list(f, mid[0], browser, theme);
    render_preview(f, mid[1], browser, theme);

    if browser.mode == BrowseMode::Save {
        render_save_input(f, outer[2], browser, theme);
    }

    render_footer(f, outer[3], browser, theme);
}

fn render_path_bar(f: &mut Frame, area: Rect, browser: &FileBrowser, theme: &Theme) {
    let title = match browser.mode {
        BrowseMode::Open => " Open file ",
        BrowseMode::Save => " Save to ",
    };
    let p = Paragraph::new(Line::from(vec![
        Span::styled(title, theme.title()),
        Span::raw(" "),
        Span::styled(browser.cwd.display().to_string(), theme.body()),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.muted_s()),
    );
    f.render_widget(p, area);
}

fn render_list(f: &mut Frame, area: Rect, browser: &FileBrowser, theme: &Theme) {
    let items: Vec<ListItem<'_>> = browser
        .entries
        .iter()
        .map(|e| {
            let icon = if e.is_dir { "[D]" } else { "   " };
            let size = if e.is_dir {
                String::new()
            } else {
                tekflash_core::device::BlockDevice {
                    path: PathBuf::new(),
                    vendor: None,
                    model: None,
                    serial: None,
                    size_bytes: e.size,
                    block_size: 0,
                    transport: tekflash_core::device::Transport::Unknown,
                    removable: false,
                    is_system: false,
                    read_only: false,
                    mountpoints: vec![],
                }
                .size_human()
            };
            let fmt = e
                .format_hint
                .map(|f| format!(" {}", f.human()))
                .unwrap_or_default();
            let line = format!("{icon} {:<28}  {:>10}{}", e.name, size, fmt);
            let style = if e.is_dir || e.format_hint.is_some() {
                theme.body()
            } else {
                theme.muted_s()
            };
            ListItem::new(Line::from(Span::styled(line, style)))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(theme.selected())
        .highlight_symbol("> ")
        .block(
            Block::default()
                .title(Line::from(Span::styled(" Files ", theme.title())))
                .borders(Borders::ALL)
                .border_style(theme.muted_s()),
        );

    let mut state = ListState::default();
    if !browser.entries.is_empty() {
        state.select(Some(browser.selected.min(browser.entries.len() - 1)));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn render_preview(f: &mut Frame, area: Rect, browser: &FileBrowser, theme: &Theme) {
    let lines: Vec<Line<'_>> = if let Some(focused) = browser.focused() {
        let mut out = vec![Line::from(Span::styled(
            focused.name.clone(),
            theme.title(),
        ))];
        out.push(Line::from(""));
        if focused.is_dir {
            out.push(Line::from(Span::raw("directory")));
        } else {
            out.push(Line::from(format!("size: {}", human_bytes(focused.size))));
            if let Some(magic) = browser.focused_magic {
                out.push(Line::from(format!("format (magic): {}", magic.human())));
            }
            if let Some(hint) = focused.format_hint {
                out.push(Line::from(format!("format (ext): {}", hint.human())));
            }
            let manifest = focused
                .path
                .parent()
                .map(|p| p.join(format!("{}.tfmanifest.json", focused.name)));
            if let Some(m) = manifest {
                if m.exists() {
                    out.push(Line::from(""));
                    out.push(Line::from(Span::styled(
                        "tekflash manifest sidecar present",
                        theme.success_s(),
                    )));
                }
            }
        }
        out
    } else {
        vec![Line::from(Span::styled("no selection", theme.muted_s()))]
    };

    let p = Paragraph::new(lines).block(
        Block::default()
            .title(Line::from(Span::styled(" Preview ", theme.title())))
            .borders(Borders::ALL)
            .border_style(theme.muted_s()),
    );
    f.render_widget(p, area);
}

fn render_save_input(f: &mut Frame, area: Rect, browser: &FileBrowser, theme: &Theme) {
    let p = Paragraph::new(Line::from(vec![
        Span::styled(" filename ", theme.title()),
        Span::raw(" "),
        Span::styled(&browser.save_name, theme.body()),
        Span::styled("_", theme.title()),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.muted_s()),
    );
    f.render_widget(p, area);
}

fn render_footer(f: &mut Frame, area: Rect, browser: &FileBrowser, theme: &Theme) {
    let mut keys: Vec<(&str, &str)> = vec![
        ("↑↓", "move"),
        ("↵", "select"),
        ("←", "parent"),
        (".", "hidden"),
        ("Tab", "all-files"),
    ];
    if browser.mode == BrowseMode::Save {
        keys.push(("type", "filename"));
    }
    keys.push(("q", "back"));
    let line = crate::tui::widgets::footer_keys(theme, &keys);
    f.render_widget(Paragraph::new(line), area);
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
