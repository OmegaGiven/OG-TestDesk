use std::sync::Arc;

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length, Task};

use og_testdesk_core::ai::{self, AiChatRequest, AiSettingsInfo};

#[derive(Debug, Clone)]
pub enum AiMessage {
    SettingsLoaded(Arc<AiSettingsInfo>),
    ProviderChanged(String),
    ModelChanged(String),
    BaseUrlChanged(String),
    ApiKeyChanged(String),
    SaveSettingsPressed,
    SettingsSaved(Result<(), String>),
    TestConnectionPressed,
    TestConnectionResult(Result<String, String>),

    ChatInputChanged(String),
    SendPressed,
    ChatResponse(Result<String, String>),
}

pub struct AiTab {
    provider: String,
    model: String,
    base_url: String,
    api_key: String,
    has_api_key: bool,
    settings_status: String,

    chat_input: String,
    sending: bool,
    messages: Vec<(String, String)>,
}

impl AiTab {
    pub fn new() -> (Self, Task<AiMessage>) {
        let tab = Self {
            provider: "openai".to_string(),
            model: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            has_api_key: false,
            settings_status: String::new(),
            chat_input: String::new(),
            sending: false,
            messages: Vec::new(),
        };
        let task = Task::perform(
            async move { Arc::new(ai::get_settings_info().await) },
            AiMessage::SettingsLoaded,
        );
        (tab, task)
    }

    pub fn update(&mut self, message: AiMessage) -> Task<AiMessage> {
        match message {
            AiMessage::SettingsLoaded(info) => {
                self.provider = info.provider.clone();
                self.model = info.model.clone();
                self.base_url = info.base_url.clone();
                self.has_api_key = info.has_api_key;
            }
            AiMessage::ProviderChanged(v) => self.provider = v,
            AiMessage::ModelChanged(v) => self.model = v,
            AiMessage::BaseUrlChanged(v) => self.base_url = v,
            AiMessage::ApiKeyChanged(v) => self.api_key = v,
            AiMessage::SaveSettingsPressed => {
                let provider = self.provider.clone();
                let model = self.model.clone();
                let base_url = self.base_url.clone();
                let api_key = self.api_key.clone();
                self.settings_status.clear();
                return Task::perform(
                    async move {
                        let base_url = if base_url.trim().is_empty() {
                            None
                        } else {
                            Some(base_url)
                        };
                        let api_key = if api_key.trim().is_empty() {
                            None
                        } else {
                            Some(api_key)
                        };
                        ai::save_settings(
                            None,
                            None,
                            &provider,
                            &model,
                            base_url.as_deref(),
                            api_key.as_deref(),
                        )
                        .await
                        .map(|_| ())
                    },
                    AiMessage::SettingsSaved,
                );
            }
            AiMessage::SettingsSaved(result) => {
                self.settings_status = match result {
                    Ok(()) => {
                        self.api_key.clear();
                        self.has_api_key = true;
                        "Settings saved.".to_string()
                    }
                    Err(err) => format!("Failed to save: {err}"),
                };
            }
            AiMessage::TestConnectionPressed => {
                self.settings_status = "Testing...".to_string();
                return Task::perform(ai::test_connection(), AiMessage::TestConnectionResult);
            }
            AiMessage::TestConnectionResult(result) => {
                self.settings_status = match result {
                    Ok(reply) => format!("Connection OK: {reply}"),
                    Err(err) => format!("Connection failed: {err}"),
                };
            }

            AiMessage::ChatInputChanged(v) => self.chat_input = v,
            AiMessage::SendPressed => {
                let message = self.chat_input.trim().to_string();
                if !message.is_empty() {
                    self.messages.push(("user".to_string(), message.clone()));
                    self.chat_input.clear();
                    self.sending = true;
                    return Task::perform(
                        async move {
                            ai::chat(AiChatRequest {
                                message,
                                page: None,
                                context: None,
                                context_summary: None,
                            })
                            .await
                            .map(|response| response.message)
                        },
                        AiMessage::ChatResponse,
                    );
                }
            }
            AiMessage::ChatResponse(result) => {
                self.sending = false;
                match result {
                    Ok(reply) => self.messages.push(("assistant".to_string(), reply)),
                    Err(err) => self
                        .messages
                        .push(("error".to_string(), format!("AI request failed: {err}"))),
                }
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, AiMessage> {
        let key_hint = if self.has_api_key {
            "API key saved (leave blank to keep it)"
        } else {
            "API key"
        };

        let settings = column![
            text("AI Assistant settings").size(16),
            text_input("openai / ollama / gemini", &self.provider)
                .on_input(AiMessage::ProviderChanged),
            text_input("model", &self.model).on_input(AiMessage::ModelChanged),
            text_input("base url (optional)", &self.base_url)
                .on_input(AiMessage::BaseUrlChanged),
            text_input(key_hint, &self.api_key)
                .on_input(AiMessage::ApiKeyChanged)
                .secure(true),
            row![
                button(text("Save settings")).on_press(AiMessage::SaveSettingsPressed),
                button(text("Test connection")).on_press(AiMessage::TestConnectionPressed),
            ]
            .spacing(8),
            text(self.settings_status.clone()),
        ]
        .spacing(6);

        let chat_history = self.messages.iter().fold(column![].spacing(6), |col, (role, msg)| {
            col.push(text(format!("{role}: {msg}")))
        });

        let send_label = if self.sending { "Sending..." } else { "Send" };
        let chat = column![
            text("Chat").size(16),
            scrollable(chat_history).height(Length::Fixed(240.0)),
            row![
                text_input("ask something...", &self.chat_input)
                    .on_input(AiMessage::ChatInputChanged),
                button(text(send_label)).on_press(AiMessage::SendPressed),
            ]
            .spacing(8),
        ]
        .spacing(6);

        container(scrollable(
            column![settings, chat].spacing(16).padding(16),
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
