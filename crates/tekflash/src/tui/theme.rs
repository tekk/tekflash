//! Vivid dark + vivid light palettes, with OSC 11 background auto-detect and
//! truecolor / 256-color / 16-color tiers.

use crate::cli::{GlobalOpts, ThemeChoice};
use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Truecolor,
    Indexed256,
    Indexed16,
    Mono,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // fields used by views landing in follow-up commits
pub struct Theme {
    pub mode: Mode,
    pub tier: Tier,
    pub ascii: bool,

    pub fg: Color,
    pub bg: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub muted: Color,
    pub selected_bg: Color,
    pub progress_fill: Color,
}

#[allow(dead_code)]
impl Theme {
    pub fn title(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }
    pub fn body(&self) -> Style {
        Style::default().fg(self.fg)
    }
    pub fn muted_s(&self) -> Style {
        Style::default().fg(self.muted)
    }
    pub fn selected(&self) -> Style {
        Style::default()
            .fg(self.fg)
            .bg(self.selected_bg)
            .add_modifier(Modifier::BOLD)
    }
    pub fn danger_s(&self) -> Style {
        Style::default()
            .fg(self.danger)
            .add_modifier(Modifier::BOLD)
    }
    pub fn success_s(&self) -> Style {
        Style::default()
            .fg(self.success)
            .add_modifier(Modifier::BOLD)
    }
    pub fn warning_s(&self) -> Style {
        Style::default().fg(self.warning)
    }
}

pub fn resolve(global: &GlobalOpts) -> Theme {
    let mode = match global.theme {
        ThemeChoice::Dark => Mode::Dark,
        ThemeChoice::Light => Mode::Light,
        ThemeChoice::Auto => detect_mode(),
    };
    let tier = detect_tier();
    let ascii = global.ascii || is_dumb_terminal();
    palette(mode, tier, ascii)
}

fn detect_mode() -> Mode {
    // OSC 11 query is implemented by terminals like iTerm2, Kitty, WezTerm, foot,
    // Alacritty, Konsole. The TUI startup happens before we enter the alt screen, so the
    // response gets read on the main stdin. For a v1 we use a lightweight heuristic on
    // env vars; full OSC 11 round-trip can come later.
    if std::env::var("TEKFLASH_THEME").as_deref() == Ok("light") {
        return Mode::Light;
    }
    if std::env::var("TEKFLASH_THEME").as_deref() == Ok("dark") {
        return Mode::Dark;
    }
    if std::env::var("COLORFGBG").is_ok() {
        // COLORFGBG = "fg;bg" — bg index < 8 is dark, >= 8 light.
        if let Ok(v) = std::env::var("COLORFGBG") {
            if let Some(bg) = v.split(';').nth(1) {
                if let Ok(n) = bg.trim().parse::<u8>() {
                    return if n < 8 { Mode::Dark } else { Mode::Light };
                }
            }
        }
    }
    Mode::Dark
}

fn detect_tier() -> Tier {
    if std::env::var_os("NO_COLOR").is_some() {
        return Tier::Mono;
    }
    let term = std::env::var("TERM").unwrap_or_default();
    if term == "dumb" {
        return Tier::Mono;
    }
    let colorterm = std::env::var("COLORTERM").unwrap_or_default();
    if colorterm == "truecolor" || colorterm == "24bit" {
        return Tier::Truecolor;
    }
    if term.contains("256color") || term.contains("kitty") || term.contains("alacritty") {
        return Tier::Indexed256;
    }
    if term == "linux" {
        return Tier::Indexed16;
    }
    Tier::Indexed256
}

fn is_dumb_terminal() -> bool {
    matches!(std::env::var("TERM").as_deref(), Ok("linux") | Ok("dumb"))
}

fn palette(mode: Mode, tier: Tier, ascii: bool) -> Theme {
    use Color::*;
    let (fg, bg, accent, success, warning, danger, muted, sel) = match (mode, tier) {
        (Mode::Dark, Tier::Truecolor) => (
            Rgb(0xE6, 0xED, 0xF3),
            Rgb(0x0E, 0x11, 0x16),
            Rgb(0x7D, 0xF9, 0xFF),
            Rgb(0x39, 0xFF, 0x14),
            Rgb(0xFF, 0xB3, 0x00),
            Rgb(0xFF, 0x38, 0x60),
            Rgb(0x8B, 0x94, 0x9E),
            Rgb(0x26, 0x4F, 0x78),
        ),
        (Mode::Light, Tier::Truecolor) => (
            Rgb(0x1B, 0x1F, 0x23),
            Rgb(0xFA, 0xFA, 0xF7),
            Rgb(0x00, 0x6B, 0x7A),
            Rgb(0x0A, 0x7F, 0x1F),
            Rgb(0xA8, 0x5C, 0x00),
            Rgb(0xB0, 0x00, 0x20),
            Rgb(0x6B, 0x72, 0x80),
            Rgb(0xCD, 0xE7, 0xF0),
        ),
        (Mode::Dark, _) => (White, Black, Cyan, LightGreen, Yellow, LightRed, Gray, Blue),
        (Mode::Light, _) => (Black, White, Blue, Green, Yellow, Red, DarkGray, LightBlue),
    };
    Theme {
        mode,
        tier,
        ascii,
        fg,
        bg,
        accent,
        success,
        warning,
        danger,
        muted,
        selected_bg: sel,
        progress_fill: success,
    }
}
