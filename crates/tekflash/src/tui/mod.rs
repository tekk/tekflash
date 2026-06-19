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

            // Browser fully replaces the base view; the rest are modal overlays on home.
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

        if event::poll(Duration::from_millis(100))? {
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
                    state.confirm = Some(Confirm::new(p.action, p.device_idx, picked));
                    if let Some(c) = state.confirm.as_mut() {
                        c.codec = Some(p.codec);
                        c.level = Some(p.level);
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
                confirm.result_message = Some(format!(
                        "Queued: {} on {} (today: run `sudo tekflash {} ...` in your shell to execute).",
                        confirm.action.short_label(),
                        state
                            .devices
                            .get(confirm.device_idx)
                            .map(|d| d.path.display().to_string())
                            .unwrap_or_default(),
                        confirm.action.short_label().to_lowercase(),
                    ));
            }
        },
        _ => {}
    }
    false
}
