//! TUI entry point. Single-event-loop app over crossterm + ratatui.

pub mod layout;
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
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::time::Duration;
use views::action_picker::{Action, ActionPicker};
use views::confirm::{Confirm, ConfirmFocus};
use views::file_browser::{BrowseMode, FileBrowser};

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
        browser: None,
        browser_purpose: None,
        confirm: None,
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

/// What the file-browser was opened for, so when a file is picked we know which screen
/// to advance into next.
#[derive(Debug)]
pub struct BrowserPurpose {
    pub action: Action,
    pub device_idx: usize,
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
    pub browser: Option<FileBrowser>,
    pub browser_purpose: Option<BrowserPurpose>,
    pub confirm: Option<Confirm>,
}

async fn event_loop<B: ratatui::backend::Backend>(
    term: &mut Terminal<B>,
    state: &mut AppState,
) -> Result<()> {
    loop {
        term.draw(|f| {
            let area = f.area();
            if area.width < 80 || area.height < 24 {
                widgets::too_small(f, area, &state.theme);
                return;
            }

            // Render order: home is always the base; file browser fully replaces it;
            // action_picker and confirm are overlays on top of home.
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
            } else if let Some(confirm) = &state.confirm {
                if let Some(dev) = state.devices.get(confirm.device_idx) {
                    views::confirm::render(f, area, confirm, dev, &state.theme);
                }
            } else if state.show_help {
                views::help::overlay(f, area, &state.theme);
            }
        })?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    let exit = dispatch_key(state, k.code, k.modifiers);
                    if exit {
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

    if state.confirm.is_some() {
        return handle_confirm_key(state, code);
    }
    if state.browser.is_some() {
        handle_browser_key(state, code, mods);
        return false;
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
            // Open the action picker for the selected device.
            if !state.devices.is_empty() {
                state.action_picker = Some(ActionPicker::new(state.selected));
            }
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
            state.show_all = !state.show_all;
            state.devices = tekflash_core::device::enumerate(state.show_all).unwrap_or_default();
            state.selected = state.selected.min(state.devices.len().saturating_sub(1));
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
            // Flash reads a source file; backup and archive write a destination file.
            let mode = match action {
                Action::Flash => BrowseMode::Open,
                Action::Backup | Action::Archive => BrowseMode::Save,
            };
            state.browser_purpose = Some(BrowserPurpose { action, device_idx });
            state.browser = Some(FileBrowser::open(
                views::file_browser::default_start_dir(),
                mode,
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
        KeyCode::Char('q') | KeyCode::Esc => {
            state.browser = None;
            state.browser_purpose = None;
        }
        KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => {
            state.browser = None;
            state.browser_purpose = None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            browser.move_up();
            browser.update_focus_preview();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            browser.move_down();
            browser.update_focus_preview();
        }
        KeyCode::Left => browser.go_parent(),
        KeyCode::Right | KeyCode::Enter => {
            if let Some(picked) = browser.enter_focused() {
                state.browser = None;
                let purpose = state.browser_purpose.take();
                if let Some(p) = purpose {
                    state.confirm = Some(Confirm::new(p.action, p.device_idx, picked));
                }
            }
        }
        KeyCode::Char('.') => browser.toggle_hidden(),
        KeyCode::Tab => browser.toggle_all_types(),
        _ => {}
    }
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
        KeyCode::Enter => {
            match confirm.focus {
                ConfirmFocus::Cancel => {
                    state.confirm = None;
                }
                ConfirmFocus::Confirm => {
                    // Real pipeline execution under the TUI event loop is wired in a
                    // follow-up — long-running flash/backup work needs progress
                    // events plumbed through the redraw loop. For now we surface a
                    // clear message so the flow is visible end-to-end.
                    confirm.result_message = Some(format!(
                        "Queued: {} on {} (CLI mode supports this today: `sudo tekflash {} ...`).",
                        confirm.action.short_label(),
                        state
                            .devices
                            .get(confirm.device_idx)
                            .map(|d| d.path.display().to_string())
                            .unwrap_or_default(),
                        confirm.action.short_label().to_lowercase(),
                    ));
                }
            }
        }
        _ => {}
    }
    false
}
