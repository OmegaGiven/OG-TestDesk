mod appdata;

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Task};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Sql,
    Requests,
    Inspector,
}

struct App {
    active_tab: Tab,
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(Tab),
}

impl App {
    fn new() -> (Self, Task<Message>) {
        (
            Self {
                active_tab: Tab::Sql,
            },
            Task::none(),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(tab) => self.active_tab = tab,
        }
        Task::none()
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

        let body = match self.active_tab {
            Tab::Sql => text("SQL workspace — coming soon"),
            Tab::Requests => text("Requests — coming soon"),
            Tab::Inspector => text("Inspector — coming soon"),
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
