use iced::widget::{column, scrollable, text, text_input};
use iced::{Element, Task};

use og_testdesk_core::jwt;

#[derive(Debug, Clone)]
pub enum JwtMessage {
    TokenChanged(String),
}

pub struct JwtTab {
    token: String,
    decoded: Option<Result<jwt::DecodedJwt, String>>,
}

impl JwtTab {
    pub fn new() -> (Self, Task<JwtMessage>) {
        (
            Self {
                token: String::new(),
                decoded: None,
            },
            Task::none(),
        )
    }

    pub fn update(&mut self, message: JwtMessage) -> Task<JwtMessage> {
        match message {
            JwtMessage::TokenChanged(v) => {
                self.decoded = if v.trim().is_empty() {
                    None
                } else {
                    Some(jwt::decode(&v))
                };
                self.token = v;
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, JwtMessage> {
        let body: Element<'_, JwtMessage> = match &self.decoded {
            None => text("Paste a JWT above to decode it").into(),
            Some(Err(err)) => text(format!("Error: {err}")).into(),
            Some(Ok(decoded)) => scrollable(
                column![
                    text("Header:"),
                    text(decoded.header_pretty.clone()),
                    text("Payload:"),
                    text(decoded.payload_pretty.clone()),
                    text("Signature (base64url):"),
                    text(decoded.signature_b64.clone()),
                ]
                .spacing(6),
            )
            .into(),
        };

        column![
            text("JWT Decoder").size(16),
            text_input("eyJhbGciOi...", &self.token).on_input(JwtMessage::TokenChanged),
            body,
        ]
        .spacing(8)
        .padding(16)
        .into()
    }
}
