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
        browser: None,
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

#[allow(dead_code)] // `global` is used by views landing in follow-up commits
pub struct AppState {
    pub global: GlobalOpts,
    pub theme: theme::Theme,
    pub devices: Vec<tekflash_core::device::BlockDevice>,
    pub selected: usize,
    pub show_help: bool,
    pub show_all: bool,
    pub browser: Option<views::file_browser::FileBrowser>,
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
            if let Some(browser) = &state.browser {
                views::file_browser::render(f, area, browser, &state.theme);
                return;
            }
            let mode = layout::pick(area);
            views::home::render(f, area, mode, state);
            if state.show_help {
                views::help::overlay(f, area, &state.theme);
            }
        })?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    if state.browser.is_some() {
                        handle_browser_key(state, k.code, k.modifiers);
                        continue;
                    }
                    match k.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            if state.show_help {
                                state.show_help = false;
                            } else {
                                return Ok(());
                            }
                        }
                        KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                            return Ok(());
                        }
                        KeyCode::Char('?') | KeyCode::F(1) => {
                            state.show_help = !state.show_help;
                        }
                        KeyCode::F(2) => {
                            state.browser = Some(views::file_browser::FileBrowser::open(
                                views::file_browser::default_start_dir(),
                                views::file_browser::BrowseMode::Open,
                            ));
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if state.selected > 0 {
                                state.selected -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if state.selected + 1 < state.devices.len() {
                                state.selected += 1;
                            }
                        }
                        KeyCode::Tab => {
                            state.show_all = !state.show_all;
                            state.devices = tekflash_core::device::enumerate(state.show_all)
                                .unwrap_or_default();
                            state.selected =
                                state.selected.min(state.devices.len().saturating_sub(1));
                        }
                        KeyCode::Char('r') => {
                            state.devices = tekflash_core::device::enumerate(state.show_all)
                                .unwrap_or_default();
                        }
                        _ => {}
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }
}

fn handle_browser_key(state: &mut AppState, code: KeyCode, mods: KeyModifiers) {
    let Some(browser) = state.browser.as_mut() else {
        return;
    };
    match code {
        KeyCode::Char('q') | KeyCode::Esc => {
            state.browser = None;
        }
        KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => {
            state.browser = None;
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
            if browser.enter_focused().is_some() {
                // File picked — for now, drop back to the home view. A future
                // commit hands the chosen path to whichever caller opened the
                // browser (Flash source, Backup dest, etc.).
                state.browser = None;
            }
        }
        KeyCode::Char('.') => browser.toggle_hidden(),
        KeyCode::Tab => browser.toggle_all_types(),
        _ => {}
    }
}
