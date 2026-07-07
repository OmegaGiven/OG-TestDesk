mod appdata;
mod sql_tab;

use std::sync::Arc;

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Task};

use og_testdesk_core::sql::engine::SqlEngineState;
use sql_tab::{SqlMessage, SqlTab};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Sql,
    Requests,
    Inspector,
}

struct App {
    active_tab: Tab,
    sql_tab: SqlTab,
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(Tab),
    Sql(SqlMessage),
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let engine = Arc::new(SqlEngineState::new());
        let (sql_tab, sql_task) = SqlTab::new(engine);
        (
            Self {
                active_tab: Tab::Sql,
                sql_tab,
            },
            sql_task.map(Message::Sql),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(tab) => {
                self.active_tab = tab;
                Task::none()
            }
            Message::Sql(msg) => self.sql_tab.update(msg).map(Message::Sql),
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
        ]
        .spacing(8);

        let body: Element<'_, Message> = match self.active_tab {
            Tab::Sql => self.sql_tab.view().map(Message::Sql),
            Tab::Requests => text("Requests — coming soon").into(),
            Tab::Inspector => text("Inspector — coming soon").into(),
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
