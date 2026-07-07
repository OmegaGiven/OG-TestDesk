use iced::widget::{column, text, text_input};
use iced::{Element, Task};
use serde_json::json;

use og_testdesk_core::requests::{get_scratchpads, save_scratchpads};

#[derive(Debug, Clone)]
pub enum ScratchpadMessage {
    Loaded(String),
    TextChanged(String),
    Saved,
}

pub struct ScratchpadTab {
    text: String,
}

impl ScratchpadTab {
    pub fn new() -> (Self, Task<ScratchpadMessage>) {
        let task = Task::perform(
            async move {
                let value = get_scratchpads().await;
                value
                    .get("main")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            },
            ScratchpadMessage::Loaded,
        );
        (
            Self {
                text: String::new(),
            },
            task,
        )
    }

    pub fn update(&mut self, message: ScratchpadMessage) -> Task<ScratchpadMessage> {
        match message {
            ScratchpadMessage::Loaded(text) => {
                self.text = text;
            }
            ScratchpadMessage::TextChanged(text) => {
                self.text = text;
                let pads = json!({ "main": self.text });
                return Task::perform(
                    async move {
                        let _ = save_scratchpads(&pads).await;
                    },
                    |_| ScratchpadMessage::Saved,
                );
            }
            ScratchpadMessage::Saved => {}
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, ScratchpadMessage> {
        column![
            text("Scratch Pad").size(16),
            text_input("jot something down...", &self.text)
                .on_input(ScratchpadMessage::TextChanged),
        ]
        .spacing(8)
        .padding(16)
        .into()
    }
}
