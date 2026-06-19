//! In-TUI file browser.
//!
//! Two modes:
//!
//! - **Open** — pick an existing file (flash source, restore archive). Typing letters
//!   jumps the cursor to the first matching name (case-insensitive prefix, falling back
//!   to substring). Backspace shortens the filter; with an empty filter it goes up one
//!   directory.
//! - **Save** — type the output filename live; an auto-extension derived from the
//!   chosen action + codec is appended visibly. Backspace deletes typed characters;
//!   with empty input it goes up one directory.
//!
//! Every listing carries a synthetic `..` entry (parent dir) and, in Save mode, a `.`
//! entry that commits the save in the current directory.
#![allow(dead_code)]

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseMode {
    Open,
    Save,
}

/// Tags entries so the event handler knows the special rows (parent dir, save-here)
/// without re-matching on the literal name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Normal,
    /// Synthetic `..` row that walks up one directory.
    Parent,
    /// Synthetic `.` row (Save mode only) that commits the save in the current dir.
    SaveHere,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub kind: EntryKind,
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: Option<SystemTime>,
    pub format_hint: Option<InputFormat>,
}

/// Result of pressing Enter on the focused entry.
#[derive(Debug)]
pub enum EnterResult {
    /// Cursor moved into a different directory; the browser is still open.
    Navigated,
    /// Caller should receive this path and close the browser.
    Picked(PathBuf),
    /// Nothing happened (e.g. empty save name on `.` row).
    None,
}

#[derive(Debug)]
pub struct FileBrowser {
    pub cwd: PathBuf,
    pub mode: BrowseMode,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub show_hidden: bool,
    pub show_all_types: bool,
    pub focused_magic: Option<InputFormat>,
    /// Save-mode filename input. Auto-extension is appended at pick time.
    pub save_name: String,
    /// Auto-extension to append when saving (e.g. `.img.zst`). `None` means none.
    pub desired_extension: Option<String>,
    /// Open-mode type-ahead filter. Each keystroke appends; Backspace pops.
    pub filter: String,
}

impl FileBrowser {
    pub fn open(start_dir: PathBuf, mode: BrowseMode) -> Self {
        Self::open_with_extension(start_dir, mode, None)
    }

    pub fn open_with_extension(
        start_dir: PathBuf,
        mode: BrowseMode,
        desired_extension: Option<String>,
    ) -> Self {
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
            desired_extension,
            filter: String::new(),
        };
        b.refresh();
        // In Save mode, start the cursor on `.` so Enter on a freshly-opened browser
        // saves with whatever filename the user types.
        if b.mode == BrowseMode::Save {
            if let Some(p) = b.entries.iter().position(|e| e.kind == EntryKind::SaveHere) {
                b.selected = p;
            }
        }
        b
    }

    pub fn refresh(&mut self) {
        let mut real = read_dir(&self.cwd, self.show_hidden);
        if self.show_all_types {
            real.sort_by(|a, b| match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            });
        } else {
            real.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name)));
        }

        // Synthetic entries first so they're always reachable.
        let mut entries: Vec<Entry> = Vec::with_capacity(real.len() + 2);
        if self.mode == BrowseMode::Save {
            entries.push(Entry {
                kind: EntryKind::SaveHere,
                path: self.cwd.clone(),
                name: ".".to_string(),
                is_dir: false,
                size: 0,
                mtime: None,
                format_hint: None,
            });
        }
        entries.push(Entry {
            kind: EntryKind::Parent,
            path: self
                .cwd
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.cwd.clone()),
            name: "..".to_string(),
            is_dir: true,
            size: 0,
            mtime: None,
            format_hint: None,
        });
        entries.extend(real);
        self.entries = entries;
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
        self.filter.clear();
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
            self.focused_magic = None;
        }
        self.filter.clear();
    }

    /// Activate the focused entry. Directories navigate; files / SaveHere yield a path.
    pub fn enter_focused(&mut self) -> EnterResult {
        let Some(entry) = self.focused().cloned() else {
            return EnterResult::None;
        };
        match entry.kind {
            EntryKind::Parent => {
                self.go_parent();
                EnterResult::Navigated
            }
            EntryKind::SaveHere => {
                let name = self.composed_save_name();
                if name.is_empty() {
                    EnterResult::None
                } else {
                    EnterResult::Picked(self.cwd.join(name))
                }
            }
            EntryKind::Normal => {
                if entry.is_dir {
                    self.cwd = entry.path;
                    self.selected = 0;
                    self.filter.clear();
                    self.refresh();
                    // Stay on `.` in Save mode after navigation.
                    if self.mode == BrowseMode::Save {
                        if let Some(p) = self
                            .entries
                            .iter()
                            .position(|e| e.kind == EntryKind::SaveHere)
                        {
                            self.selected = p;
                        }
                    }
                    EnterResult::Navigated
                } else if self.mode == BrowseMode::Save {
                    // Overwriting an existing file: lift its name into the save_name so
                    // the user sees what's about to be replaced.
                    self.save_name = entry.name.clone();
                    EnterResult::Picked(entry.path)
                } else {
                    EnterResult::Picked(entry.path)
                }
            }
        }
    }

    pub fn go_parent(&mut self) {
        if let Some(parent) = self.cwd.parent() {
            self.cwd = parent.to_path_buf();
            self.selected = 0;
            self.filter.clear();
            self.refresh();
            if self.mode == BrowseMode::Save {
                if let Some(p) = self
                    .entries
                    .iter()
                    .position(|e| e.kind == EntryKind::SaveHere)
                {
                    self.selected = p;
                }
            }
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

    pub fn update_focus_preview(&mut self) {
        self.focused_magic = self.focused().and_then(|e| {
            if matches!(e.kind, EntryKind::Normal) && !e.is_dir {
                sniff_magic(&e.path)
            } else {
                None
            }
        });
    }

    /// Accept a typed character. In Open mode it goes into the filter; in Save mode it
    /// extends the save_name.
    pub fn accept_char(&mut self, c: char) {
        if !is_valid_input_char(c) {
            return;
        }
        match self.mode {
            BrowseMode::Open => {
                self.filter.push(c);
                self.apply_filter();
            }
            BrowseMode::Save => {
                self.save_name.push(c);
                if let Some(p) = self
                    .entries
                    .iter()
                    .position(|e| e.kind == EntryKind::SaveHere)
                {
                    self.selected = p;
                }
            }
        }
    }

    /// Pop one character from the filter (Open) or save_name (Save). Returns `true` if
    /// a character was popped, `false` if there was nothing to pop (caller can then
    /// treat the Backspace as "go up").
    pub fn backspace(&mut self) -> bool {
        match self.mode {
            BrowseMode::Open => {
                if self.filter.pop().is_some() {
                    if !self.filter.is_empty() {
                        self.apply_filter();
                    }
                    return true;
                }
                false
            }
            BrowseMode::Save => {
                if self.save_name.pop().is_some() {
                    return true;
                }
                false
            }
        }
    }

    pub fn clear_typing(&mut self) {
        self.filter.clear();
    }

    /// `save_name` with the auto-extension appended, unless the user already typed a
    /// recognized suffix.
    pub fn composed_save_name(&self) -> String {
        if self.save_name.is_empty() {
            return String::new();
        }
        let Some(ext) = self.desired_extension.as_deref() else {
            return self.save_name.clone();
        };
        if ext.is_empty() {
            return self.save_name.clone();
        }
        let lower = self.save_name.to_ascii_lowercase();
        if lower.ends_with(&ext.to_ascii_lowercase()) || has_known_image_suffix(&lower) {
            self.save_name.clone()
        } else {
            format!("{}{}", self.save_name, ext)
        }
    }

    fn apply_filter(&mut self) {
        let f = self.filter.to_ascii_lowercase();
        if f.is_empty() {
            return;
        }
        let prefix_match = self.entries.iter().position(|e| {
            matches!(e.kind, EntryKind::Normal) && e.name.to_ascii_lowercase().starts_with(&f)
        });
        let pos = prefix_match.or_else(|| {
            self.entries.iter().position(|e| {
                matches!(e.kind, EntryKind::Normal) && e.name.to_ascii_lowercase().contains(&f)
            })
        });
        if let Some(p) = pos {
            self.selected = p;
            self.focused_magic = None;
        }
    }
}

/// Where the browser should start, given the OS.
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
            kind: EntryKind::Normal,
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

fn is_valid_input_char(c: char) -> bool {
    !c.is_control() && c != '/' && c != '\\' && c != '\0'
}

fn has_known_image_suffix(name_lower: &str) -> bool {
    const SUFFIXES: &[&str] = &[
        ".tar.zst",
        ".tar.zsd",
        ".tar.zstd",
        ".tar.xz",
        ".tar.gz",
        ".tar.bz2",
        ".tar.lz4",
        ".tar.br",
        ".img.zst",
        ".img.zsd",
        ".img.zstd",
        ".img.xz",
        ".img.gz",
        ".img.bz2",
        ".img.lz4",
        ".img.br",
        ".zst",
        ".zsd",
        ".zstd",
        ".xz",
        ".gz",
        ".bz2",
        ".lz4",
        ".br",
        ".iso",
        ".img",
        ".bin",
        ".raw",
        ".tar",
    ];
    SUFFIXES.iter().any(|s| name_lower.ends_with(s))
}

pub fn render(f: &mut Frame, area: Rect, browser: &FileBrowser, theme: &Theme) {
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
    let mut spans = vec![
        Span::styled(title, theme.title()),
        Span::raw(" "),
        Span::styled(browser.cwd.display().to_string(), theme.body()),
    ];
    if !browser.filter.is_empty() {
        spans.push(Span::raw("    "));
        spans.push(Span::styled("filter: ", theme.muted_s()));
        spans.push(Span::styled(browser.filter.clone(), theme.title()));
    }
    let p = Paragraph::new(Line::from(spans)).block(
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
            let icon = match e.kind {
                EntryKind::Parent => "[^]",
                EntryKind::SaveHere => "[*]",
                EntryKind::Normal if e.is_dir => "[D]",
                EntryKind::Normal => "   ",
            };
            let display_name = match e.kind {
                EntryKind::SaveHere => {
                    let composed = browser.composed_save_name();
                    if composed.is_empty() {
                        ".   (type a filename below to save here)".to_string()
                    } else {
                        format!(".   save here as {composed}")
                    }
                }
                EntryKind::Parent => "..  go up one directory".to_string(),
                EntryKind::Normal => e.name.clone(),
            };
            let size = if e.is_dir || matches!(e.kind, EntryKind::SaveHere) {
                String::new()
            } else {
                human_bytes(e.size)
            };
            let fmt = e
                .format_hint
                .map(|f| format!(" {}", f.human()))
                .unwrap_or_default();
            let line = format!("{icon} {display_name:<36}  {size:>10}{fmt}");
            let style = match e.kind {
                EntryKind::SaveHere => theme.success_s(),
                EntryKind::Parent => theme.title(),
                EntryKind::Normal if e.is_dir || e.format_hint.is_some() => theme.body(),
                EntryKind::Normal => theme.muted_s(),
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
        match focused.kind {
            EntryKind::Parent => {
                out.push(Line::from(Span::raw("walk up to the parent directory")));
            }
            EntryKind::SaveHere => {
                out.push(Line::from(Span::raw("commit the save in this directory")));
                out.push(Line::from(""));
                let composed = browser.composed_save_name();
                if composed.is_empty() {
                    out.push(Line::from(Span::styled(
                        "type a filename below first",
                        theme.muted_s(),
                    )));
                } else {
                    out.push(Line::from(format!(
                        "destination: {}",
                        browser.cwd.join(&composed).display()
                    )));
                }
            }
            EntryKind::Normal => {
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
    let composed = browser.composed_save_name();
    let auto_ext = composed
        .strip_prefix(browser.save_name.as_str())
        .unwrap_or("");
    let p = Paragraph::new(Line::from(vec![
        Span::styled(" filename ", theme.title()),
        Span::raw(" "),
        Span::styled(browser.save_name.clone(), theme.body()),
        Span::styled("▏", theme.title()),
        Span::styled(auto_ext.to_string(), theme.muted_s()),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.muted_s())
            .title(Line::from(Span::styled(
                " type filename · auto-extension shown in grey ",
                theme.muted_s(),
            ))),
    );
    f.render_widget(p, area);
}

fn render_footer(f: &mut Frame, area: Rect, browser: &FileBrowser, theme: &Theme) {
    let mut keys: Vec<(&str, &str)> = vec![
        ("↑↓", "move"),
        ("↵", "activate"),
        ("←/Backspace", "up"),
        ("Tab", "all-files"),
        ("Ctrl-H", "hidden"),
        ("Esc", "back"),
    ];
    if browser.mode == BrowseMode::Open {
        keys.insert(2, ("type", "filter"));
    } else {
        keys.insert(2, ("type", "filename"));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(label: &str) -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "tekflash-fbtest-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn populate(dir: &Path, names: &[&str]) {
        for n in names {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
    }

    #[test]
    fn listing_always_has_parent_entry_in_first_two_rows() {
        let d = tempdir("parent");
        populate(&d, &["a.iso", "b.txt"]);
        let b = FileBrowser::open(d.clone(), BrowseMode::Open);
        assert!(b
            .entries
            .iter()
            .take(2)
            .any(|e| e.kind == EntryKind::Parent && e.name == ".."));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn save_mode_adds_dot_and_starts_cursor_on_it() {
        let d = tempdir("savedot");
        populate(&d, &["existing.bin"]);
        let b = FileBrowser::open_with_extension(
            d.clone(),
            BrowseMode::Save,
            Some(".img.zst".to_string()),
        );
        assert_eq!(b.focused().unwrap().kind, EntryKind::SaveHere);
        assert_eq!(b.focused().unwrap().name, ".");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn type_ahead_jumps_to_first_prefix_match_case_insensitive() {
        let d = tempdir("ta");
        populate(&d, &["alpha", "Bravo", "charlie", "delta"]);
        let mut b = FileBrowser::open(d.clone(), BrowseMode::Open);
        // After the ".." row, real entries land alphabetically: Bravo, alpha, charlie, delta
        // (default sort puts dirs first; here everything is a file so they're sorted by
        // ASCII — capital letters first). We assert by matching: typing "b" → focuses Bravo.
        b.accept_char('b');
        assert_eq!(b.focused().unwrap().name, "Bravo");
        b.accept_char('r');
        assert_eq!(b.focused().unwrap().name, "Bravo");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn backspace_returns_false_when_filter_empty() {
        let d = tempdir("bs");
        populate(&d, &["a"]);
        let mut b = FileBrowser::open(d.clone(), BrowseMode::Open);
        assert!(!b.backspace(), "empty filter → caller should go_parent");
        b.accept_char('a');
        assert!(
            b.backspace(),
            "non-empty filter → just popped, no go_parent"
        );
        assert!(b.filter.is_empty());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn save_mode_auto_appends_extension_when_missing() {
        let d = tempdir("ext1");
        let mut b = FileBrowser::open_with_extension(
            d.clone(),
            BrowseMode::Save,
            Some(".img.zst".to_string()),
        );
        for c in "backup_2026".chars() {
            b.accept_char(c);
        }
        assert_eq!(b.composed_save_name(), "backup_2026.img.zst");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn save_mode_does_not_double_append_known_extensions() {
        let d = tempdir("ext2");
        let mut b = FileBrowser::open_with_extension(
            d.clone(),
            BrowseMode::Save,
            Some(".img.zst".to_string()),
        );
        for c in "backup.img.gz".chars() {
            b.accept_char(c);
        }
        // User typed a known suffix → leave alone, don't make backup.img.gz.img.zst
        assert_eq!(b.composed_save_name(), "backup.img.gz");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn save_mode_backspace_pops_before_navigating() {
        let d = tempdir("bs2");
        let mut b = FileBrowser::open_with_extension(
            d.clone(),
            BrowseMode::Save,
            Some(".img.zst".to_string()),
        );
        b.accept_char('a');
        b.accept_char('b');
        assert!(b.backspace(), "non-empty save_name → just popped");
        assert_eq!(b.save_name, "a");
        assert!(b.backspace());
        assert!(b.save_name.is_empty());
        assert!(!b.backspace(), "empty save_name → caller should go_parent");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn enter_on_save_here_with_typed_name_returns_full_path() {
        let d = tempdir("eh");
        let mut b = FileBrowser::open_with_extension(
            d.clone(),
            BrowseMode::Save,
            Some(".tar.zst".to_string()),
        );
        for c in "snapshot".chars() {
            b.accept_char(c);
        }
        match b.enter_focused() {
            EnterResult::Picked(p) => {
                assert!(p.to_string_lossy().ends_with("snapshot.tar.zst"));
                // On macOS the tempdir under /var/folders canonicalises to
                // /private/var/folders, so compare the resolved forms.
                assert_eq!(
                    p.parent().unwrap().canonicalize().unwrap(),
                    d.canonicalize().unwrap()
                );
            }
            other => panic!("expected Picked, got {other:?}"),
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn slash_and_control_chars_are_rejected_from_filename_input() {
        let d = tempdir("inv");
        let mut b =
            FileBrowser::open_with_extension(d.clone(), BrowseMode::Save, Some(".zst".to_string()));
        b.accept_char('a');
        b.accept_char('/');
        b.accept_char('\n');
        b.accept_char('b');
        assert_eq!(b.save_name, "ab");
        std::fs::remove_dir_all(&d).ok();
    }
}
