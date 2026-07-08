mod appdata;
mod ai_tab;
mod appearance_tab;
mod calculator_tab;
mod graphql_highlighter;
mod inspector_tab;
mod json_highlighter;
mod jwt_tab;
mod request_auth;
mod curl_import;
mod codegen;
mod request_env;
mod request_history;
mod request_kv_editor;
mod request_url;
mod requests_tab;
mod scratchpad_tab;
mod sql_erd;
mod sql_grid;
mod sql_highlighter;
mod sql_tab;

use std::sync::Arc;

use iced::widget::{button, column, container, row, scrollable, stack, text, Space};
use iced::{Element, Length, Task};

use ai_tab::{AiMessage, AiTab};
use appearance_tab::{AppearanceMessage, AppearanceTab};
use calculator_tab::{CalculatorMessage, CalculatorTab};
use inspector_tab::{InspectorMessage, InspectorTab};
use jwt_tab::{JwtMessage, JwtTab};
use og_testdesk_core::sql::engine::SqlEngineState;
use requests_tab::{RequestsMessage, RequestsTab};
use scratchpad_tab::{ScratchpadMessage, ScratchpadTab};
use sql_tab::{SqlMessage, SqlTab};

/// The three fixed carousel sections plus the tools currently reached via
/// the hamburger menu. Kept as one enum (rather than splitting
/// section/tool) since `active_tab` still drives a single "what's the main
/// body right now" switch — the hamburger menu just changes how you get to
/// Calculator/Jwt/Scratchpad/Appearance/Ai, not their own internal shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Sql,
    Requests,
    Inspector,
    Calculator,
    Jwt,
    Scratchpad,
    Appearance,
    Ai,
}

impl Tab {
    /// The three main carousel sections, in nav order.
    const SECTIONS: [Tab; 3] = [Tab::Sql, Tab::Requests, Tab::Inspector];

    fn label(&self) -> &'static str {
        match self {
            Tab::Sql => "SQL",
            Tab::Requests => "Requests",
            Tab::Inspector => "Inspector",
            Tab::Calculator => "Calculator",
            Tab::Jwt => "JWT Decoder",
            Tab::Scratchpad => "Scratch Pad",
            Tab::Appearance => "Appearance",
            Tab::Ai => "AI Assistant",
        }
    }
}

/// Which of the three overlay popups (if any) is currently showing. Only
/// one at a time — opening one closes the others, matching how a real menu
/// bar behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overlay {
    None,
    Notifications,
    Docs,
    Hamburger,
}

struct App {
    active_tab: Tab,
    overlay: Overlay,
    /// Total (sql history + requests history) entry count as of the last
    /// time the notifications popup was opened — the badge shows how many
    /// have arrived since. Session-scoped, not persisted (this is a tray,
    /// not a durable inbox, per the design doc).
    notifications_seen: usize,

    sql_tab: SqlTab,
    requests_tab: RequestsTab,
    inspector_tab: InspectorTab,
    calculator_tab: CalculatorTab,
    jwt_tab: JwtTab,
    scratchpad_tab: ScratchpadTab,
    appearance_tab: AppearanceTab,
    ai_tab: AiTab,
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(Tab),
    ToggleNotifications,
    ToggleDocs,
    ToggleHamburger,
    CloseOverlay,
    Sql(SqlMessage),
    Requests(RequestsMessage),
    Inspector(InspectorMessage),
    Calculator(CalculatorMessage),
    Jwt(JwtMessage),
    Scratchpad(ScratchpadMessage),
    Appearance(AppearanceMessage),
    Ai(AiMessage),
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let engine = Arc::new(SqlEngineState::new());
        let (sql_tab, sql_task) = SqlTab::new(engine);
        let (requests_tab, requests_task) = RequestsTab::new();
        let (inspector_tab, inspector_task) = InspectorTab::new();
        let (calculator_tab, calculator_task) = CalculatorTab::new();
        let (jwt_tab, jwt_task) = JwtTab::new();
        let (scratchpad_tab, scratchpad_task) = ScratchpadTab::new();
        let (appearance_tab, appearance_task) = AppearanceTab::new();
        let (ai_tab, ai_task) = AiTab::new();
        (
            Self {
                active_tab: Tab::Sql,
                overlay: Overlay::None,
                notifications_seen: 0,
                sql_tab,
                requests_tab,
                inspector_tab,
                calculator_tab,
                jwt_tab,
                scratchpad_tab,
                appearance_tab,
                ai_tab,
            },
            Task::batch([
                sql_task.map(Message::Sql),
                requests_task.map(Message::Requests),
                inspector_task.map(Message::Inspector),
                calculator_task.map(Message::Calculator),
                jwt_task.map(Message::Jwt),
                scratchpad_task.map(Message::Scratchpad),
                appearance_task.map(Message::Appearance),
                ai_task.map(Message::Ai),
            ]),
        )
    }

    fn notification_total(&self) -> usize {
        self.sql_tab.recent_history().len() + self.requests_tab.recent_history().len()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(tab) => {
                self.active_tab = tab;
                self.overlay = Overlay::None;
                Task::none()
            }
            Message::ToggleNotifications => {
                self.overlay = if self.overlay == Overlay::Notifications {
                    Overlay::None
                } else {
                    self.notifications_seen = self.notification_total();
                    Overlay::Notifications
                };
                Task::none()
            }
            Message::ToggleDocs => {
                self.overlay = if self.overlay == Overlay::Docs {
                    Overlay::None
                } else {
                    Overlay::Docs
                };
                Task::none()
            }
            Message::ToggleHamburger => {
                self.overlay = if self.overlay == Overlay::Hamburger {
                    Overlay::None
                } else {
                    Overlay::Hamburger
                };
                Task::none()
            }
            Message::CloseOverlay => {
                self.overlay = Overlay::None;
                Task::none()
            }
            Message::Sql(msg) => self.sql_tab.update(msg).map(Message::Sql),
            Message::Requests(msg) => self.requests_tab.update(msg).map(Message::Requests),
            Message::Inspector(msg) => self.inspector_tab.update(msg).map(Message::Inspector),
            Message::Calculator(msg) => {
                self.calculator_tab.update(msg).map(Message::Calculator)
            }
            Message::Jwt(msg) => self.jwt_tab.update(msg).map(Message::Jwt),
            Message::Scratchpad(msg) => {
                self.scratchpad_tab.update(msg).map(Message::Scratchpad)
            }
            Message::Appearance(msg) => {
                self.appearance_tab.update(msg).map(Message::Appearance)
            }
            Message::Ai(msg) => self.ai_tab.update(msg).map(Message::Ai),
        }
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        iced::Subscription::batch([
            self.sql_tab.subscription().map(Message::Sql),
            self.requests_tab.subscription().map(Message::Requests),
        ])
    }

    fn view_nav(&self) -> Element<'_, Message> {
        let section_button = |tab: Tab| {
            let active = self.active_tab == tab;
            let label = if active {
                format!("[ {} ]", tab.label())
            } else {
                tab.label().to_string()
            };
            button(text(label)).on_press(Message::TabSelected(tab))
        };

        let carousel = scrollable(
            row(Tab::SECTIONS.iter().map(|tab| section_button(*tab).into())).spacing(4),
        )
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::default(),
        ));

        let badge_count = self.notification_total().saturating_sub(self.notifications_seen);
        let bell_label = if badge_count > 0 {
            format!("\u{1F514} {badge_count}")
        } else {
            "\u{1F514}".to_string()
        };

        let right_cluster = row![
            button(text(bell_label)).on_press(Message::ToggleNotifications),
            button(text("?")).on_press(Message::ToggleDocs),
            button(text("\u{2630}")).on_press(Message::ToggleHamburger),
        ]
        .spacing(6);

        row![carousel, Space::with_width(Length::Fill), right_cluster]
            .spacing(12)
            .padding(8)
            .into()
    }

    fn view_notifications_popup(&self) -> Element<'_, Message> {
        let sql_lines = self.sql_tab.recent_history().iter().take(10).fold(
            column![text("Recent SQL runs").size(14)].spacing(2),
            |col, record| {
                let status = if record.error.is_some() { "error" } else { "ok" };
                let label = if record.query_name.trim().is_empty() {
                    record.sql.chars().take(60).collect::<String>()
                } else {
                    record.query_name.clone()
                };
                col.push(
                    button(text(format!("[{status}] {label}")))
                        .on_press(Message::TabSelected(Tab::Sql))
                        .width(Length::Fill),
                )
            },
        );

        let requests_lines = self.requests_tab.recent_history().iter().take(10).fold(
            column![text("Recent requests").size(14)].spacing(2),
            |col, entry| {
                let status = entry
                    .response
                    .as_ref()
                    .map(|resp| resp.status.to_string())
                    .unwrap_or_else(|| "failed".to_string());
                col.push(
                    button(text(format!(
                        "[{status}] {} {}",
                        entry.request.method, entry.request.url
                    )))
                    .on_press(Message::TabSelected(Tab::Requests))
                    .width(Length::Fill),
                )
            },
        );

        popup_card(
            column![sql_lines, requests_lines].spacing(12),
            Length::Fixed(360.0),
        )
    }

    fn view_docs_popup(&self) -> Element<'_, Message> {
        let body = column![
            text("OG TestDesk — quick reference").size(16),
            text(
                "SQL: manage database connections, write and run queries with \
                 syntax highlighting and autocomplete, browse and edit table \
                 data, view schema relationships, and schedule recurring \
                 queries. Ctrl/Cmd+F opens find-in-editor."
            ),
            text(
                "Requests: build and send HTTP requests across multiple tabs, \
                 with params/headers/auth/body editors, environment \
                 variables, saved collections, and history. Ctrl/Cmd+Enter \
                 sends the current request, Ctrl/Cmd+S saves it."
            ),
            text(
                "Inspector: paste raw JSON to pretty-print and get a quick \
                 structural summary."
            ),
            text(
                "The hamburger menu (\u{2630}) opens Appearance, JWT Decoder, \
                 AI Assistant, and Scratch Pad."
            ),
        ]
        .spacing(10);

        popup_card(body, Length::Fixed(420.0))
    }

    fn view_hamburger_popup(&self) -> Element<'_, Message> {
        // The written spec only names Appearance/JWT/AI/Scratch Pad for
        // this menu; Calculator predates this redesign and has no other
        // entry point, so it's folded in here too rather than left
        // unreachable — flagged as a judgment call, not a literal spec item.
        let tools = [Tab::Appearance, Tab::Jwt, Tab::Ai, Tab::Scratchpad, Tab::Calculator];
        let list = tools.iter().fold(column![].spacing(4), |col, tab| {
            col.push(
                button(text(tab.label()))
                    .on_press(Message::TabSelected(*tab))
                    .width(Length::Fill),
            )
        });
        popup_card(list, Length::Fixed(220.0))
    }

    fn view(&self) -> Element<'_, Message> {
        let nav = self.view_nav();

        let body: Element<'_, Message> = match self.active_tab {
            Tab::Sql => self.sql_tab.view().map(Message::Sql),
            Tab::Requests => self.requests_tab.view().map(Message::Requests),
            Tab::Inspector => self.inspector_tab.view().map(Message::Inspector),
            Tab::Calculator => self.calculator_tab.view().map(Message::Calculator),
            Tab::Jwt => self.jwt_tab.view().map(Message::Jwt),
            Tab::Scratchpad => self.scratchpad_tab.view().map(Message::Scratchpad),
            Tab::Appearance => self.appearance_tab.view().map(Message::Appearance),
            Tab::Ai => self.ai_tab.view().map(Message::Ai),
        };

        let main_content = container(column![nav, body].spacing(16).padding(16))
            .width(Length::Fill)
            .height(Length::Fill);

        if self.overlay == Overlay::None {
            return main_content.into();
        }

        let popup = match self.overlay {
            Overlay::Notifications => self.view_notifications_popup(),
            Overlay::Docs => self.view_docs_popup(),
            Overlay::Hamburger => self.view_hamburger_popup(),
            Overlay::None => unreachable!(),
        };

        // Full-window click-catcher behind the popup: click anywhere outside
        // the card to dismiss it, same as a native menu/popover.
        let backdrop = button(text(""))
            .on_press(Message::CloseOverlay)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme, _status| button::Style {
                background: Some(iced::Color::TRANSPARENT.into()),
                ..Default::default()
            });

        let overlay_layer = container(popup)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(16)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Top);

        stack![main_content, backdrop, overlay_layer].into()
    }
}

/// A small floating card used by all three nav popups (notifications, docs,
/// hamburger) — bordered/backgrounded so it reads as "on top of" the main
/// view when layered via `stack!`.
fn popup_card<'a>(content: impl Into<Element<'a, Message>>, width: Length) -> Element<'a, Message> {
    container(scrollable(content.into()).height(Length::Shrink))
        .width(width)
        .padding(16)
        .style(|theme: &iced::Theme| {
            let palette = theme.extended_palette();
            container::Style {
                background: Some(palette.background.weak.color.into()),
                border: iced::Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
}

fn main() -> iced::Result {
    appdata::configure();

    let runtime = tokio::runtime::Runtime::new().expect("build tokio runtime");
    runtime.block_on(async {
        if let Err(err) = og_testdesk_core::app_db::init().await {
            eprintln!("failed to init database: {err}");
        }
    });

    iced::application("OG TestDesk", App::update, App::view)
        .subscription(App::subscription)
        .run_with(App::new)
}
