//! Responsive layout selection based on terminal dimensions.

use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// ≥ 120×30: sidebar + main + status + log tail.
    Full,
    /// 100–119 wide: sidebar collapses to icons; log tail hides.
    Compact,
    /// 80×24 to 99 wide: single column, tabbed views.
    Minimal,
}

pub fn pick(area: Rect) -> Mode {
    if area.width >= 120 && area.height >= 30 {
        Mode::Full
    } else if area.width >= 100 {
        Mode::Compact
    } else {
        Mode::Minimal
    }
}
