//! TUI entry point. Single-event-loop app over crossterm + ratatui.

pub mod layout;
pub mod progress_runner;
pub mod theme;
pub mod views;
pub mod widgets;

use crate::cli::GlobalOpts;
use color_eyre::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use progress_runner::{BackupParams, BackupProgress};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::time::Duration;
use tekflash_core::pipeline::compress::{Codec, CompressionLevel};
use views::action_picker::{Action, ActionPicker};
use views::codec_picker::CodecPicker;
use views::confirm::{Confirm, ConfirmFocus};
use views::file_browser::{BrowseMode, EnterResult, FileBrowser};

pub async fn run(global: GlobalOpts) -> Result<()> {
    let theme = theme::resolve(&global);
    let devices = tekflash_core::device::enumerate(global.show_all)?;

    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(out);
    let mut term = Terminal::new(backend)?;

    let mut state = AppState {
        global,
        theme,
        devices,
        selected: 0,
        show_help: false,
        show_all: false,
        action_picker: None,
        codec_picker: None,
        browser: None,
        browser_purpose: None,
        confirm: None,
        sessions: Vec::new(),
        viewing_session: None,
    };

    let result = event_loop(&mut term, &mut state).await;

    disable_raw_mode()?;
    execute!(
        term.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    term.show_cursor()?;

    result
}

/// What the file browser was opened for, so when a file is picked we know which screen
/// to advance into next.
#[derive(Debug, Clone)]
pub struct BrowserPurpose {
    pub action: Action,
    pub device_idx: usize,
    pub codec: Codec,
    pub level: CompressionLevel,
}

#[allow(dead_code)] // `global` is consumed by views/pipelines landing in follow-up commits
pub struct AppState {
    pub global: GlobalOpts,
    pub theme: theme::Theme,
    pub devices: Vec<tekflash_core::device::BlockDevice>,
    pub selected: usize,
    pub show_help: bool,
    pub show_all: bool,
    pub action_picker: Option<ActionPicker>,
    pub codec_picker: Option<CodecPicker>,
    pub browser: Option<FileBrowser>,
    pub browser_purpose: Option<BrowserPurpose>,
    pub confirm: Option<Confirm>,
    /// Every running, finished, or failed backup session. New sessions are appended;
    /// completed ones stay until the user dismisses them (Enter on the summary).
    pub sessions: Vec<BackupProgress>,
    /// Index into `sessions` of the one currently rendered full-screen. `None` means
    /// the home view is shown and any sessions appear as mini bars at the bottom.
    pub viewing_session: Option<usize>,
}

async fn event_loop<B: ratatui::backend::Backend>(
    term: &mut Terminal<B>,
    state: &mut AppState,
) -> Result<()> {
    loop {
        // Drain every session's progress events before drawing so the gauges and the
        // home-view mini bars all show the freshest numbers we have for this frame.
        for s in state.sessions.iter_mut() {
            s.poll();
        }

        term.draw(|f| {
            let area = f.area();
            if area.width < 80 || area.height < 24 {
                widgets::too_small(f, area, &state.theme);
                return;
            }

            // Showing a specific session full-screen replaces every other view (with
            // the keyboard help overlay as the only exception).
            if let Some(idx) = state.viewing_session {
                if let Some(s) = state.sessions.get(idx) {
                    views::progress::render(f, area, s, &state.theme);
                    return;
                }
            }
            if let Some(browser) = &state.browser {
                views::file_browser::render(f, area, browser, &state.theme);
                return;
            }
            let mode = layout::pick(area);
            views::home::render(f, area, mode, state);
            if let Some(picker) = &state.action_picker {
                if let Some(dev) = state.devices.get(picker.device_idx) {
                    views::action_picker::render(f, area, picker, dev, &state.theme);
                }
            } else if let Some(cp) = &state.codec_picker {
                views::codec_picker::render(f, area, cp, &state.theme);
            } else if let Some(confirm) = &state.confirm {
                if let Some(dev) = state.devices.get(confirm.device_idx) {
                    views::confirm::render(f, area, confirm, dev, &state.theme);
                }
            } else if state.show_help {
                views::help::overlay(f, area, &state.theme);
            }
        })?;

        // Shorter poll while any session is running so the rate readouts feel live.
        let poll_ms = if state.sessions.iter().any(|s| s.is_running()) {
            50
        } else {
            100
        };
        if event::poll(Duration::from_millis(poll_ms))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    if dispatch_key(state, k.code, k.modifiers) {
                        return Ok(());
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }
}

/// Returns `true` when the key should exit the TUI.
fn dispatch_key(state: &mut AppState, code: KeyCode, mods: KeyModifiers) -> bool {
    // Global Ctrl-C exits unconditionally.
    if matches!(code, KeyCode::Char('c')) && mods.contains(KeyModifiers::CONTROL) {
        return true;
    }

    // While a session is on screen, route keys to the progress handler. The home view
    // handles its own keys (including Tab to start cycling into the sessions).
    if state.viewing_session.is_some() {
        return handle_progress_key(state, code);
    }
    if state.confirm.is_some() {
        return handle_confirm_key(state, code);
    }
    if state.browser.is_some() {
        handle_browser_key(state, code, mods);
        return false;
    }
    if state.codec_picker.is_some() {
        return handle_codec_picker_key(state, code);
    }
    if state.action_picker.is_some() {
        return handle_action_picker_key(state, code);
    }
    handle_home_key(state, code)
}

fn handle_home_key(state: &mut AppState, code: KeyCode) -> bool {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => {
            if state.show_help {
                state.show_help = false;
                false
            } else {
                true
            }
        }
        KeyCode::Char('?') | KeyCode::F(1) => {
            state.show_help = !state.show_help;
            false
        }
        KeyCode::F(2) => {
            state.browser = Some(FileBrowser::open(
                views::file_browser::default_start_dir(),
                BrowseMode::Open,
            ));
            state.browser_purpose = None;
            false
        }
        KeyCode::Enter => {
            if !state.devices.is_empty() {
                state.action_picker = Some(ActionPicker::new(state.selected));
            }
            false
        }
        KeyCode::Char('a') => {
            // Toggle showing internal / system drives.
            state.show_all = !state.show_all;
            state.devices = tekflash_core::device::enumerate(state.show_all).unwrap_or_default();
            state.selected = state.selected.min(state.devices.len().saturating_sub(1));
            false
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.selected > 0 {
                state.selected -= 1;
            }
            false
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.selected + 1 < state.devices.len() {
                state.selected += 1;
            }
            false
        }
        KeyCode::Tab => {
            // Start the session cycle: jump into the first session. Subsequent Tab
            // presses (handled by handle_progress_key) walk forward through the
            // list and eventually return to home.
            if !state.sessions.is_empty() {
                state.viewing_session = Some(0);
            }
            false
        }
        KeyCode::Char('r') => {
            state.devices = tekflash_core::device::enumerate(state.show_all).unwrap_or_default();
            false
        }
        _ => false,
    }
}

fn handle_action_picker_key(state: &mut AppState, code: KeyCode) -> bool {
    let Some(picker) = state.action_picker.as_mut() else {
        return false;
    };
    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            state.action_picker = None;
        }
        KeyCode::Up | KeyCode::Char('k') => picker.move_up(),
        KeyCode::Down | KeyCode::Char('j') => picker.move_down(),
        KeyCode::Enter => {
            let action = picker.selected_action();
            let device_idx = picker.device_idx;
            state.action_picker = None;
            match action {
                Action::Flash => {
                    // Flash detects compression from the source magic bytes; no codec
                    // choice needed. Open the browser straight to a picker.
                    state.browser_purpose = Some(BrowserPurpose {
                        action,
                        device_idx,
                        codec: Codec::None,
                        level: CompressionLevel(0),
                    });
                    state.browser = Some(FileBrowser::open(
                        views::file_browser::default_start_dir(),
                        BrowseMode::Open,
                    ));
                }
                Action::Backup | Action::Archive => {
                    state.codec_picker = Some(CodecPicker::new(action, device_idx));
                }
            }
        }
        _ => {}
    }
    false
}

fn handle_codec_picker_key(state: &mut AppState, code: KeyCode) -> bool {
    let Some(picker) = state.codec_picker.as_mut() else {
        return false;
    };
    match code {
        KeyCode::Esc => {
            // Go back to the action picker for the same device.
            let action = picker.action;
            let device_idx = picker.device_idx;
            state.codec_picker = None;
            let mut ap = ActionPicker::new(device_idx);
            for _ in 0..Action::ALL.iter().position(|a| *a == action).unwrap_or(0) {
                ap.move_down();
            }
            state.action_picker = Some(ap);
        }
        KeyCode::Up => picker.move_up(),
        KeyCode::Down => picker.move_down(),
        KeyCode::Left | KeyCode::Char('-') | KeyCode::Char('_') => picker.level_down(),
        KeyCode::Right | KeyCode::Char('+') | KeyCode::Char('=') | KeyCode::Char(']') => {
            picker.level_up()
        }
        KeyCode::Char('0') => picker.jump_min(),
        KeyCode::Char('9') => picker.jump_max(),
        KeyCode::Enter => {
            let action = picker.action;
            let device_idx = picker.device_idx;
            let (codec, level, _ext) = picker.picked();
            let ext_suffix = picker.output_extension(action);
            state.codec_picker = None;
            state.browser_purpose = Some(BrowserPurpose {
                action,
                device_idx,
                codec,
                level,
            });
            state.browser = Some(FileBrowser::open_with_extension(
                views::file_browser::default_start_dir(),
                BrowseMode::Save,
                if ext_suffix.is_empty() {
                    None
                } else {
                    Some(ext_suffix)
                },
            ));
        }
        _ => {}
    }
    false
}

fn handle_browser_key(state: &mut AppState, code: KeyCode, mods: KeyModifiers) {
    let Some(browser) = state.browser.as_mut() else {
        return;
    };
    match code {
        KeyCode::Esc => {
            state.browser = None;
            state.browser_purpose = None;
        }
        KeyCode::Tab => browser.toggle_all_types(),
        KeyCode::Char('h') if mods.contains(KeyModifiers::CONTROL) => {
            browser.toggle_hidden();
        }
        KeyCode::Up => {
            browser.move_up();
            browser.update_focus_preview();
        }
        KeyCode::Down => {
            browser.move_down();
            browser.update_focus_preview();
        }
        KeyCode::Left => browser.go_parent(),
        KeyCode::Backspace => {
            // Pop a typed character first; only walk up if the buffer was already empty.
            if !browser.backspace() {
                browser.go_parent();
            }
        }
        KeyCode::Right | KeyCode::Enter => match browser.enter_focused() {
            EnterResult::Picked(picked) => {
                state.browser = None;
                let purpose = state.browser_purpose.take();
                if let Some(p) = purpose {
                    match p.action {
                        // Backup and Archive both start immediately with a live
                        // progress view — both are read-only on the source side.
                        Action::Backup => {
                            let original_source = state
                                .devices
                                .get(p.device_idx)
                                .map(|d| d.path.clone())
                                .unwrap_or_default();
                            // Resolve to the per-OS fast device node before opening:
                            // /dev/diskN -> /dev/rdiskN on macOS (often 5–10× faster on
                            // USB / SD because it bypasses the buffered block layer);
                            // no-op on Linux and Windows.
                            let fast_source =
                                tekflash_core::device::resolve_fast_path(&original_source);
                            let total_bytes = state
                                .devices
                                .get(p.device_idx)
                                .map(|d| d.size_bytes)
                                .filter(|n| *n > 0)
                                .or_else(|| std::fs::metadata(&fast_source).ok().map(|m| m.len()));
                            let params = BackupParams {
                                kind: progress_runner::OperationKind::Backup,
                                source: fast_source,
                                dest: picked,
                                codec: p.codec,
                                level: p.level,
                                total_bytes,
                                archive_format: tekflash_core::archive::ArchiveFormat::Tar,
                                archive_selection: None,
                            };
                            state
                                .sessions
                                .push(BackupProgress::new(p.device_idx, params));
                            state.viewing_session = Some(state.sessions.len() - 1);
                        }
                        Action::Archive => {
                            // Archive reads files from a mounted directory, so the
                            // source is a mountpoint of the chosen device. If the
                            // device isn't mounted the worker reports the failure
                            // through the same UI as a normal session.
                            let source = state
                                .devices
                                .get(p.device_idx)
                                .and_then(|d| d.mountpoints.first().cloned())
                                .unwrap_or_else(|| {
                                    state
                                        .devices
                                        .get(p.device_idx)
                                        .map(|d| d.path.clone())
                                        .unwrap_or_default()
                                });
                            let params = BackupParams {
                                kind: progress_runner::OperationKind::Archive,
                                source,
                                dest: picked,
                                codec: p.codec,
                                level: p.level,
                                total_bytes: None, // worker walks the tree and reports it
                                archive_format: tekflash_core::archive::ArchiveFormat::Tar,
                                archive_selection: None,
                            };
                            state
                                .sessions
                                .push(BackupProgress::new(p.device_idx, params));
                            state.viewing_session = Some(state.sessions.len() - 1);
                        }
                        // Flash keeps the confirm step (it's the destructive direction).
                        Action::Flash => {
                            state.confirm = Some(Confirm::new(p.action, p.device_idx, picked));
                            if let Some(c) = state.confirm.as_mut() {
                                c.codec = Some(p.codec);
                                c.level = Some(p.level);
                            }
                        }
                    }
                }
            }
            EnterResult::Navigated | EnterResult::None => {
                browser.update_focus_preview();
            }
        },
        KeyCode::Char(c) => browser.accept_char(c),
        _ => {}
    }
}

fn handle_progress_key(state: &mut AppState, code: KeyCode) -> bool {
    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            // Detach back to the home view. Sessions stay in the vec; the worker
            // threads keep running, and the home view shows their mini bars.
            state.viewing_session = None;
        }
        KeyCode::Enter => {
            // Enter on a finished / failed summary dismisses that session (removes
            // it from the vec). On a running one Enter does nothing — avoids
            // accidentally killing a session the user is watching.
            let Some(idx) = state.viewing_session else {
                return false;
            };
            let is_done = state.sessions.get(idx).is_some_and(|s| !s.is_running());
            if is_done {
                state.sessions.remove(idx);
                state.viewing_session = if state.sessions.is_empty() {
                    None
                } else {
                    Some(idx.min(state.sessions.len() - 1))
                };
            }
        }
        KeyCode::Tab => {
            // Cycle to the next session, or back to the home view when past the end.
            if let Some(idx) = state.viewing_session {
                let next = idx + 1;
                state.viewing_session = if next < state.sessions.len() {
                    Some(next)
                } else {
                    None
                };
            }
        }
        _ => {}
    }
    false
}

fn handle_confirm_key(state: &mut AppState, code: KeyCode) -> bool {
    let Some(confirm) = state.confirm.as_mut() else {
        return false;
    };
    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            state.confirm = None;
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
            confirm.toggle_focus();
        }
        KeyCode::Char('h') => confirm.focus = ConfirmFocus::Cancel,
        KeyCode::Char('l') => confirm.focus = ConfirmFocus::Confirm,
        KeyCode::Enter => match confirm.focus {
            ConfirmFocus::Cancel => {
                state.confirm = None;
            }
            ConfirmFocus::Confirm => {
                // Snapshot the modal's data before we drop it from state.
                let action = confirm.action;
                let device_idx = confirm.device_idx;
                let file = confirm.file.clone();
                let codec = confirm.codec.unwrap_or(Codec::None);
                let level = confirm.level.unwrap_or(CompressionLevel(3));
                state.confirm = None;

                // Flash is the only action that reaches the Confirm step today; the
                // others (Backup, Archive) start straight from the file browser. If
                // Backup / Archive ever get routed through Confirm, extend this match.
                if !matches!(action, Action::Flash) {
                    return false;
                }

                // Resolve the device to the per-OS fast path (/dev/rdiskN on macOS) so
                // the write goes through the unbuffered character device.
                let device_path = state
                    .devices
                    .get(device_idx)
                    .map(|d| d.path.clone())
                    .unwrap_or_default();
                let fast_dest = tekflash_core::device::resolve_fast_path(&device_path);
                // total_bytes tracks *compressed* bytes consumed from the source —
                // that's what drives the gauge.
                let total_bytes = std::fs::metadata(&file).ok().map(|m| m.len());
                let params = BackupParams {
                    kind: progress_runner::OperationKind::Flash,
                    source: file,
                    dest: fast_dest,
                    codec,
                    level,
                    total_bytes,
                    archive_format: tekflash_core::archive::ArchiveFormat::Tar,
                    archive_selection: None,
                };
                state.sessions.push(BackupProgress::new(device_idx, params));
                state.viewing_session = Some(state.sessions.len() - 1);
            }
        },
        _ => {}
    }
    false
}
