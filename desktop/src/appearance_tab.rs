use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length, Task};

use og_testdesk_core::app_db::{get_current_theme_value, save_current_theme_value};
use og_testdesk_core::theme::{default_dark_theme, theme_from_input, Theme, ThemeInput};

#[derive(Debug, Clone)]
pub enum AppearanceMessage {
    Loaded(Theme),
    ModeSelected(String),
    NameChanged(String),
    PrimaryBgChanged(String),
    SecondaryBgChanged(String),
    TertiaryBgChanged(String),
    TextColorChanged(String),
    AccentColorChanged(String),
    HoverWindowAccentChanged(String),
    CronHighlightColorChanged(String),
    LinkColorChanged(String),
    LinkVisitedChanged(String),
    LinkHoverChanged(String),
    BorderColorChanged(String),
    FontSizeSmallChanged(String),
    FontSizeMediumChanged(String),
    FontSizeLargeChanged(String),
    ElementMarginChanged(String),
    NavHeightChanged(String),
    CornerRadiusChanged(String),
    PanelOpacityChanged(String),
    FontFamilyChanged(String),
    ApplyPressed,
    Saved,
}

pub struct AppearanceTab {
    name: String,
    mode: String,
    primary_bg: String,
    secondary_bg: String,
    tertiary_bg: String,
    text_color: String,
    accent_color: String,
    hover_window_accent: String,
    cron_highlight_color: String,
    link_color: String,
    link_visited: String,
    link_hover: String,
    border_color: String,
    font_size_small: String,
    font_size_medium: String,
    font_size_large: String,
    element_margin: String,
    nav_height: String,
    corner_radius: String,
    panel_opacity: String,
    font_family: String,
    saved_note: String,
}

impl AppearanceTab {
    pub fn new() -> (Self, Task<AppearanceMessage>) {
        let tab = Self::from_theme(default_dark_theme());
        let task = Task::perform(
            async move {
                match get_current_theme_value().await {
                    Some(theme) => theme,
                    None => default_dark_theme(),
                }
            },
            AppearanceMessage::Loaded,
        );
        (tab, task)
    }

    fn from_theme(theme: Theme) -> Self {
        Self {
            name: theme.name,
            mode: theme.mode,
            primary_bg: theme.primary_bg,
            secondary_bg: theme.secondary_bg,
            tertiary_bg: theme.tertiary_bg,
            text_color: theme.text_color,
            accent_color: theme.accent_color,
            hover_window_accent: theme.hover_window_accent,
            cron_highlight_color: theme.cron_highlight_color,
            link_color: theme.link_color,
            link_visited: theme.link_visited,
            link_hover: theme.link_hover,
            border_color: theme.border_color,
            font_size_small: theme.font_size_small.to_string(),
            font_size_medium: theme.font_size_medium.to_string(),
            font_size_large: theme.font_size_large.to_string(),
            element_margin: theme.element_margin.to_string(),
            nav_height: theme.nav_height.to_string(),
            corner_radius: theme.corner_radius.to_string(),
            panel_opacity: theme.panel_opacity.to_string(),
            font_family: theme.font_family,
            saved_note: String::new(),
        }
    }

    fn parse_u32(value: &str, fallback: u32) -> u32 {
        value.trim().parse::<u32>().unwrap_or(fallback)
    }

    fn to_input(&self) -> ThemeInput {
        let defaults = default_dark_theme();
        ThemeInput {
            name: self.name.clone(),
            mode: self.mode.clone(),
            primary_bg: self.primary_bg.clone(),
            secondary_bg: self.secondary_bg.clone(),
            tertiary_bg: self.tertiary_bg.clone(),
            text_color: self.text_color.clone(),
            accent_color: self.accent_color.clone(),
            hover_window_accent: self.hover_window_accent.clone(),
            cron_highlight_color: self.cron_highlight_color.clone(),
            link_color: self.link_color.clone(),
            link_visited: self.link_visited.clone(),
            link_hover: self.link_hover.clone(),
            border_color: self.border_color.clone(),
            font_size_small: Self::parse_u32(&self.font_size_small, defaults.font_size_small),
            font_size_medium: Self::parse_u32(&self.font_size_medium, defaults.font_size_medium),
            font_size_large: Self::parse_u32(&self.font_size_large, defaults.font_size_large),
            element_margin: Self::parse_u32(&self.element_margin, defaults.element_margin),
            nav_height: Self::parse_u32(&self.nav_height, defaults.nav_height),
            corner_radius: Self::parse_u32(&self.corner_radius, defaults.corner_radius),
            panel_opacity: Self::parse_u32(&self.panel_opacity, defaults.panel_opacity),
            font_family: self.font_family.clone(),
        }
    }

    pub fn update(&mut self, message: AppearanceMessage) -> Task<AppearanceMessage> {
        match message {
            AppearanceMessage::Loaded(theme) => *self = Self::from_theme(theme),
            AppearanceMessage::ModeSelected(mode) => {
                self.mode = mode;
                self.saved_note.clear();
            }
            AppearanceMessage::NameChanged(v) => self.name = v,
            AppearanceMessage::PrimaryBgChanged(v) => self.primary_bg = v,
            AppearanceMessage::SecondaryBgChanged(v) => self.secondary_bg = v,
            AppearanceMessage::TertiaryBgChanged(v) => self.tertiary_bg = v,
            AppearanceMessage::TextColorChanged(v) => self.text_color = v,
            AppearanceMessage::AccentColorChanged(v) => self.accent_color = v,
            AppearanceMessage::HoverWindowAccentChanged(v) => self.hover_window_accent = v,
            AppearanceMessage::CronHighlightColorChanged(v) => self.cron_highlight_color = v,
            AppearanceMessage::LinkColorChanged(v) => self.link_color = v,
            AppearanceMessage::LinkVisitedChanged(v) => self.link_visited = v,
            AppearanceMessage::LinkHoverChanged(v) => self.link_hover = v,
            AppearanceMessage::BorderColorChanged(v) => self.border_color = v,
            AppearanceMessage::FontSizeSmallChanged(v) => self.font_size_small = v,
            AppearanceMessage::FontSizeMediumChanged(v) => self.font_size_medium = v,
            AppearanceMessage::FontSizeLargeChanged(v) => self.font_size_large = v,
            AppearanceMessage::ElementMarginChanged(v) => self.element_margin = v,
            AppearanceMessage::NavHeightChanged(v) => self.nav_height = v,
            AppearanceMessage::CornerRadiusChanged(v) => self.corner_radius = v,
            AppearanceMessage::PanelOpacityChanged(v) => self.panel_opacity = v,
            AppearanceMessage::FontFamilyChanged(v) => self.font_family = v,
            AppearanceMessage::ApplyPressed => {
                let theme = theme_from_input(&self.to_input());
                self.saved_note.clear();
                return Task::perform(
                    async move {
                        let _ = save_current_theme_value(&theme).await;
                    },
                    |_| AppearanceMessage::Saved,
                );
            }
            AppearanceMessage::Saved => self.saved_note = "Theme saved.".to_string(),
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, AppearanceMessage> {
        let mode_row = row![
            mode_button("light", &self.mode),
            mode_button("dark", &self.mode),
            mode_button("custom", &self.mode),
        ]
        .spacing(6);

        let mut form = column![
            text("Appearance").size(16),
            text_input("theme name", &self.name).on_input(AppearanceMessage::NameChanged),
            text("Mode"),
            mode_row,
            text("Accent color"),
            text_input("#4da6ff", &self.accent_color).on_input(AppearanceMessage::AccentColorChanged),
            text("Hover window accent"),
            text_input("#4da6ff", &self.hover_window_accent)
                .on_input(AppearanceMessage::HoverWindowAccentChanged),
            text("Cron highlight color"),
            text_input("#2f8cff", &self.cron_highlight_color)
                .on_input(AppearanceMessage::CronHighlightColorChanged),
        ]
        .spacing(6);

        if self.mode == "custom" {
            form = form.push(column![
                text("Primary background"),
                text_input("", &self.primary_bg).on_input(AppearanceMessage::PrimaryBgChanged),
                text("Secondary background"),
                text_input("", &self.secondary_bg).on_input(AppearanceMessage::SecondaryBgChanged),
                text("Tertiary background"),
                text_input("", &self.tertiary_bg).on_input(AppearanceMessage::TertiaryBgChanged),
                text("Text color"),
                text_input("", &self.text_color).on_input(AppearanceMessage::TextColorChanged),
                text("Border color"),
                text_input("", &self.border_color).on_input(AppearanceMessage::BorderColorChanged),
                text("Link color"),
                text_input("", &self.link_color).on_input(AppearanceMessage::LinkColorChanged),
                text("Link visited"),
                text_input("", &self.link_visited).on_input(AppearanceMessage::LinkVisitedChanged),
                text("Link hover"),
                text_input("", &self.link_hover).on_input(AppearanceMessage::LinkHoverChanged),
            ]
            .spacing(6));
        }

        form = form.push(column![
            text("Font size (small/medium/large)"),
            row![
                text_input("14", &self.font_size_small).on_input(AppearanceMessage::FontSizeSmallChanged),
                text_input("16", &self.font_size_medium).on_input(AppearanceMessage::FontSizeMediumChanged),
                text_input("18", &self.font_size_large).on_input(AppearanceMessage::FontSizeLargeChanged),
            ]
            .spacing(6),
            text("Element margin / nav height"),
            row![
                text_input("10", &self.element_margin).on_input(AppearanceMessage::ElementMarginChanged),
                text_input("50", &self.nav_height).on_input(AppearanceMessage::NavHeightChanged),
            ]
            .spacing(6),
            text("Corner radius / panel opacity"),
            row![
                text_input("4", &self.corner_radius).on_input(AppearanceMessage::CornerRadiusChanged),
                text_input("100", &self.panel_opacity).on_input(AppearanceMessage::PanelOpacityChanged),
            ]
            .spacing(6),
            text("Font family"),
            text_input("inherit", &self.font_family).on_input(AppearanceMessage::FontFamilyChanged),
            row![
                button(text("Apply")).on_press(AppearanceMessage::ApplyPressed),
                text(self.saved_note.clone()),
            ]
            .spacing(8),
            text("Note: applying a theme here saves it, but the running app's own colors are not re-skinned live yet — that needs a separate pass wiring this data into iced's styling system."),
        ]
        .spacing(6));

        container(scrollable(form.padding(16)))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn mode_button(label: &'static str, current: &str) -> Element<'static, AppearanceMessage> {
    let display = if current == label {
        format!("> {label}")
    } else {
        label.to_string()
    };
    button(text(display))
        .on_press(AppearanceMessage::ModeSelected(label.to_string()))
        .into()
}
