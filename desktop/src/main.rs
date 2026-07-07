mod appdata;
mod calculator_tab;
mod jwt_tab;
mod requests_tab;
mod scratchpad_tab;
mod sql_tab;

use std::sync::Arc;

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Task};

use calculator_tab::{CalculatorMessage, CalculatorTab};
use jwt_tab::{JwtMessage, JwtTab};
use og_testdesk_core::sql::engine::SqlEngineState;
use requests_tab::{RequestsMessage, RequestsTab};
use scratchpad_tab::{ScratchpadMessage, ScratchpadTab};
use sql_tab::{SqlMessage, SqlTab};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Sql,
    Requests,
    Inspector,
    Calculator,
    Jwt,
    Scratchpad,
}

struct App {
    active_tab: Tab,
    sql_tab: SqlTab,
    requests_tab: RequestsTab,
    calculator_tab: CalculatorTab,
    jwt_tab: JwtTab,
    scratchpad_tab: ScratchpadTab,
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(Tab),
    Sql(SqlMessage),
    Requests(RequestsMessage),
    Calculator(CalculatorMessage),
    Jwt(JwtMessage),
    Scratchpad(ScratchpadMessage),
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let engine = Arc::new(SqlEngineState::new());
        let (sql_tab, sql_task) = SqlTab::new(engine);
        let (requests_tab, requests_task) = RequestsTab::new();
        let (calculator_tab, calculator_task) = CalculatorTab::new();
        let (jwt_tab, jwt_task) = JwtTab::new();
        let (scratchpad_tab, scratchpad_task) = ScratchpadTab::new();
        (
            Self {
                active_tab: Tab::Sql,
                sql_tab,
                requests_tab,
                calculator_tab,
                jwt_tab,
                scratchpad_tab,
            },
            Task::batch([
                sql_task.map(Message::Sql),
                requests_task.map(Message::Requests),
                calculator_task.map(Message::Calculator),
                jwt_task.map(Message::Jwt),
                scratchpad_task.map(Message::Scratchpad),
            ]),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(tab) => {
                self.active_tab = tab;
                Task::none()
            }
            Message::Sql(msg) => self.sql_tab.update(msg).map(Message::Sql),
            Message::Requests(msg) => self.requests_tab.update(msg).map(Message::Requests),
            Message::Calculator(msg) => {
                self.calculator_tab.update(msg).map(Message::Calculator)
            }
            Message::Jwt(msg) => self.jwt_tab.update(msg).map(Message::Jwt),
            Message::Scratchpad(msg) => {
                self.scratchpad_tab.update(msg).map(Message::Scratchpad)
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let nav_button = |label: &'static str, tab: Tab| {
            button(text(label)).on_press(Message::TabSelected(tab))
        };

        let nav = row![
            nav_button("SQL", Tab::Sql),
            nav_button("Requests", Tab::Requests),
            nav_button("Inspector", Tab::Inspector),
            nav_button("Calculator", Tab::Calculator),
            nav_button("JWT", Tab::Jwt),
            nav_button("Scratch Pad", Tab::Scratchpad),
        ]
        .spacing(8);

        let body: Element<'_, Message> = match self.active_tab {
            Tab::Sql => self.sql_tab.view().map(Message::Sql),
            Tab::Requests => self.requests_tab.view().map(Message::Requests),
            Tab::Inspector => text("Inspector — coming soon").into(),
            Tab::Calculator => self.calculator_tab.view().map(Message::Calculator),
            Tab::Jwt => self.jwt_tab.view().map(Message::Jwt),
            Tab::Scratchpad => self.scratchpad_tab.view().map(Message::Scratchpad),
        };

        container(column![nav, body].spacing(16).padding(16))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn main() -> iced::Result {
    appdata::configure();

    let runtime = tokio::runtime::Runtime::new().expect("build tokio runtime");
    runtime.block_on(async {
        if let Err(err) = og_testdesk_core::app_db::init().await {
            eprintln!("failed to init database: {err}");
        }
    });

    iced::application("OG TestDesk", App::update, App::view).run_with(App::new)
}
