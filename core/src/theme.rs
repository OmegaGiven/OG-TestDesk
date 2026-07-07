use serde::{Deserialize, Serialize};

pub fn default_font_family() -> String {
    "inherit".to_string()
}

pub fn default_theme_mode() -> String {
    "custom".to_string()
}

pub fn default_accent_color() -> String {
    "#4da6ff".to_string()
}

pub fn default_hover_window_accent() -> String {
    "#4da6ff".to_string()
}

pub fn default_cron_highlight_color() -> String {
    "#2f8cff".to_string()
}

pub fn default_element_margin() -> u32 {
    10
}

pub fn default_nav_height() -> u32 {
    50
}

pub fn default_corner_radius() -> u32 {
    4
}

pub fn default_panel_opacity() -> u32 {
    100
}

/// A named color/spacing palette applied to the desktop UI.
#[derive(Serialize, Deserialize, Clone)]
pub struct Theme {
    pub name: String,
    #[serde(default = "default_theme_mode")]
    pub mode: String,
    pub primary_bg: String,
    pub secondary_bg: String,
    pub tertiary_bg: String,
    pub text_color: String,
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    #[serde(default = "default_hover_window_accent")]
    pub hover_window_accent: String,
    #[serde(default = "default_cron_highlight_color")]
    pub cron_highlight_color: String,
    pub link_color: String,
    pub link_visited: String,
    pub link_hover: String,
    pub border_color: String,
    pub font_size_small: u32,
    pub font_size_medium: u32,
    pub font_size_large: u32,
    #[serde(default = "default_element_margin")]
    pub element_margin: u32,
    #[serde(default = "default_nav_height")]
    pub nav_height: u32,
    #[serde(default = "default_corner_radius")]
    pub corner_radius: u32,
    #[serde(default = "default_panel_opacity")]
    pub panel_opacity: u32,
    #[serde(default = "default_font_family")]
    pub font_family: String,
}

pub fn default_dark_theme() -> Theme {
    Theme {
        name: "Dark Default".to_string(),
        mode: "dark".to_string(),
        primary_bg: "#2e2e2e".to_string(),
        secondary_bg: "#222222".to_string(),
        tertiary_bg: "#3a3a3a".to_string(),
        text_color: "#eeeeee".to_string(),
        accent_color: "#4da6ff".to_string(),
        hover_window_accent: "#4da6ff".to_string(),
        cron_highlight_color: "#2f8cff".to_string(),
        link_color: "#4da6ff".to_string(),
        link_visited: "#b366ff".to_string(),
        link_hover: "#66ccff".to_string(),
        border_color: "#444444".to_string(),
        font_size_small: 14,
        font_size_medium: 16,
        font_size_large: 18,
        element_margin: 10,
        nav_height: 50,
        corner_radius: 4,
        panel_opacity: 100,
        font_family: "inherit".to_string(),
    }
}

/// Plain input for building a [`Theme`], replacing the original web-form parsing.
pub struct ThemeInput {
    pub name: String,
    pub mode: String,
    pub primary_bg: String,
    pub secondary_bg: String,
    pub tertiary_bg: String,
    pub text_color: String,
    pub accent_color: String,
    pub hover_window_accent: String,
    pub cron_highlight_color: String,
    pub link_color: String,
    pub link_visited: String,
    pub link_hover: String,
    pub border_color: String,
    pub font_size_small: u32,
    pub font_size_medium: u32,
    pub font_size_large: u32,
    pub element_margin: u32,
    pub nav_height: u32,
    pub corner_radius: u32,
    pub panel_opacity: u32,
    pub font_family: String,
}

pub fn theme_from_input(input: &ThemeInput) -> Theme {
    let mode = match input.mode.as_str() {
        "light" | "dark" | "custom" => input.mode.clone(),
        _ => "custom".to_string(),
    };
    let corner_radius = input.corner_radius.min(24);
    let panel_opacity = input.panel_opacity.clamp(20, 100);

    let mut theme = match mode.as_str() {
        "light" => Theme {
            name: input.name.clone(),
            mode: mode.clone(),
            primary_bg: "#f7f8fa".to_string(),
            secondary_bg: "#ffffff".to_string(),
            tertiary_bg: "#eef1f5".to_string(),
            text_color: "#1f2933".to_string(),
            accent_color: input.accent_color.clone(),
            hover_window_accent: input.hover_window_accent.clone(),
            cron_highlight_color: input.cron_highlight_color.clone(),
            link_color: input.accent_color.clone(),
            link_visited: "#7c3aed".to_string(),
            link_hover: input.accent_color.clone(),
            border_color: "#d8dee6".to_string(),
            font_size_small: input.font_size_small,
            font_size_medium: input.font_size_medium,
            font_size_large: input.font_size_large,
            element_margin: input.element_margin,
            nav_height: input.nav_height,
            corner_radius,
            panel_opacity,
            font_family: input.font_family.clone(),
        },
        "dark" => Theme {
            name: input.name.clone(),
            mode: mode.clone(),
            primary_bg: "#1c1c1c".to_string(),
            secondary_bg: "#111111".to_string(),
            tertiary_bg: "#292929".to_string(),
            text_color: "#ffffff".to_string(),
            accent_color: input.accent_color.clone(),
            hover_window_accent: input.hover_window_accent.clone(),
            cron_highlight_color: input.cron_highlight_color.clone(),
            link_color: input.accent_color.clone(),
            link_visited: "#b366ff".to_string(),
            link_hover: input.accent_color.clone(),
            border_color: "#444444".to_string(),
            font_size_small: input.font_size_small,
            font_size_medium: input.font_size_medium,
            font_size_large: input.font_size_large,
            element_margin: input.element_margin,
            nav_height: input.nav_height,
            corner_radius,
            panel_opacity,
            font_family: input.font_family.clone(),
        },
        _ => Theme {
            name: input.name.clone(),
            mode: mode.clone(),
            primary_bg: input.primary_bg.clone(),
            secondary_bg: input.secondary_bg.clone(),
            tertiary_bg: input.tertiary_bg.clone(),
            text_color: input.text_color.clone(),
            accent_color: input.accent_color.clone(),
            hover_window_accent: input.hover_window_accent.clone(),
            cron_highlight_color: input.cron_highlight_color.clone(),
            link_color: input.link_color.clone(),
            link_visited: input.link_visited.clone(),
            link_hover: input.link_hover.clone(),
            border_color: input.border_color.clone(),
            font_size_small: input.font_size_small,
            font_size_medium: input.font_size_medium,
            font_size_large: input.font_size_large,
            element_margin: input.element_margin,
            nav_height: input.nav_height,
            corner_radius,
            panel_opacity,
            font_family: input.font_family.clone(),
        },
    };

    if theme.accent_color.trim().is_empty() {
        theme.accent_color = "#4da6ff".to_string();
    }
    if theme.hover_window_accent.trim().is_empty() {
        theme.hover_window_accent = theme.accent_color.clone();
    }
    if theme.cron_highlight_color.trim().is_empty() {
        theme.cron_highlight_color = "#2f8cff".to_string();
    }
    theme
}
