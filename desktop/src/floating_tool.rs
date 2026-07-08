use serde::{Deserialize, Serialize};

/// Identifies one of the four hamburger-menu tools that can be presented
/// either as a real OS window or an in-app floating panel, per
/// `WindowMode`. Calculator is deliberately excluded — it predates this
/// mechanism and stays on the plain section-routing behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolKind {
    Appearance,
    Jwt,
    Ai,
    Scratchpad,
}

impl ToolKind {
    pub const ALL: [ToolKind; 4] = [ToolKind::Appearance, ToolKind::Jwt, ToolKind::Ai, ToolKind::Scratchpad];

    pub fn label(&self) -> &'static str {
        match self {
            ToolKind::Appearance => "Appearance",
            ToolKind::Jwt => "JWT Decoder",
            ToolKind::Ai => "AI Assistant",
            ToolKind::Scratchpad => "Scratch Pad",
        }
    }

    /// Staggered default position so opening several panels at once doesn't
    /// stack them exactly on top of each other.
    pub fn default_position(&self) -> (f32, f32) {
        let index = Self::ALL.iter().position(|kind| kind == self).unwrap_or(0) as f32;
        (140.0 + index * 36.0, 100.0 + index * 36.0)
    }

    pub fn default_window_size(&self) -> (f32, f32) {
        match self {
            ToolKind::Appearance => (420.0, 560.0),
            ToolKind::Jwt => (480.0, 420.0),
            ToolKind::Ai => (420.0, 560.0),
            ToolKind::Scratchpad => (420.0, 480.0),
        }
    }
}

/// Persisted app-wide setting controlling how the four tools above open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WindowMode {
    #[default]
    Native,
    Panel,
}

impl WindowMode {
    pub fn toggled(self) -> Self {
        match self {
            WindowMode::Native => WindowMode::Panel,
            WindowMode::Panel => WindowMode::Native,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            WindowMode::Native => "Native windows",
            WindowMode::Panel => "In-app panels",
        }
    }
}

/// Panel-mode state for one tool: whether it's open and where it's
/// currently positioned (top-left corner, in logical pixels within the
/// main window).
#[derive(Debug, Clone, Copy)]
pub struct PanelState {
    pub open: bool,
    pub position: (f32, f32),
}

impl PanelState {
    pub fn new(kind: ToolKind) -> Self {
        Self {
            open: false,
            position: kind.default_position(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_mode_toggles_both_ways() {
        assert_eq!(WindowMode::Native.toggled(), WindowMode::Panel);
        assert_eq!(WindowMode::Panel.toggled(), WindowMode::Native);
    }

    #[test]
    fn default_positions_are_staggered_not_identical() {
        let positions: Vec<(f32, f32)> = ToolKind::ALL.iter().map(|k| k.default_position()).collect();
        for i in 0..positions.len() {
            for j in (i + 1)..positions.len() {
                assert_ne!(positions[i], positions[j]);
            }
        }
    }
}
