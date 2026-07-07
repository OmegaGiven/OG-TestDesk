use std::sync::Arc;

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length, Task};

use og_testdesk_core::requests::{self, ProxyRequest, ProxyResponse, SavedRequest};

#[derive(Debug, Clone)]
pub enum RequestsMessage {
    MethodChanged(String),
    UrlChanged(String),
    HeadersChanged(String),
    BodyChanged(String),
    SendPressed,
    ResponseReceived(Arc<Result<ProxyResponse, String>>),

    SaveNameChanged(String),
    SavePressed,
    SavedReloaded(Vec<SavedRequest>),
    LoadSaved(String),
    DeleteSaved(String),
}

pub struct RequestsTab {
    method: String,
    url: String,
    headers: String,
    body: String,
    sending: bool,
    last_response: Option<Result<ProxyResponse, String>>,

    save_name: String,
    saved: Vec<SavedRequest>,
}

impl RequestsTab {
    pub fn new() -> (Self, Task<RequestsMessage>) {
        let tab = Self {
            method: "GET".to_string(),
            url: String::new(),
            headers: String::new(),
            body: String::new(),
            sending: false,
            last_response: None,
            save_name: String::new(),
            saved: Vec::new(),
        };
        (tab, reload_saved())
    }

    pub fn update(&mut self, message: RequestsMessage) -> Task<RequestsMessage> {
        match message {
            RequestsMessage::MethodChanged(v) => self.method = v,
            RequestsMessage::UrlChanged(v) => self.url = v,
            RequestsMessage::HeadersChanged(v) => self.headers = v,
            RequestsMessage::BodyChanged(v) => self.body = v,
            RequestsMessage::SendPressed => {
                if !self.url.trim().is_empty() {
                    self.sending = true;
                    let payload = ProxyRequest {
                        method: self.method.clone(),
                        url: self.url.clone(),
                        headers: parse_headers(&self.headers),
                        body: self.body.clone(),
                        body_mode: None,
                        form_data: vec![],
                        graphql: None,
                        request_id: None,
                    };
                    return Task::perform(
                        async move {
                            match tokio::task::spawn_blocking(move || {
                                requests::run_proxy_request(payload)
                            })
                            .await
                            {
                                Ok(Ok(resp)) => Ok(resp),
                                Ok(Err(io_err)) => Err(io_err.to_string()),
                                Err(join_err) => Err(format!("task join error: {join_err}")),
                            }
                        },
                        |result| RequestsMessage::ResponseReceived(Arc::new(result)),
                    );
                }
            }
            RequestsMessage::ResponseReceived(result) => {
                self.sending = false;
                self.last_response = Some((*result).clone());
            }

            RequestsMessage::SaveNameChanged(v) => self.save_name = v,
            RequestsMessage::SavePressed => {
                if !self.save_name.trim().is_empty() {
                    let name = self.save_name.clone();
                    let method = self.method.clone();
                    let url = self.url.clone();
                    let headers = self.headers.clone();
                    let body = self.body.clone();
                    self.save_name.clear();
                    return Task::perform(
                        async move {
                            let _ = requests::save_request(
                                &name, &method, &url, &headers, &body, None, None, None, None,
                                None, None,
                            )
                            .await;
                            requests::list_saved_requests().await
                        },
                        RequestsMessage::SavedReloaded,
                    );
                }
            }
            RequestsMessage::SavedReloaded(saved) => self.saved = saved,
            RequestsMessage::LoadSaved(name) => {
                if let Some(req) = self.saved.iter().find(|r| r.name == name) {
                    self.method = req.method.clone();
                    self.url = req.url.clone();
                    self.headers = req.headers.clone();
                    self.body = req.body.clone();
                }
            }
            RequestsMessage::DeleteSaved(name) => {
                let folder = self
                    .saved
                    .iter()
                    .find(|r| r.name == name)
                    .and_then(|r| r.folder.clone());
                return Task::perform(
                    async move {
                        let _ = requests::delete_request(&name, folder.as_deref()).await;
                        requests::list_saved_requests().await
                    },
                    RequestsMessage::SavedReloaded,
                );
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, RequestsMessage> {
        let method_row = row![
            method_button("GET", &self.method),
            method_button("POST", &self.method),
            method_button("PUT", &self.method),
            method_button("PATCH", &self.method),
            method_button("DELETE", &self.method),
        ]
        .spacing(6);

        let send_label = if self.sending { "Sending..." } else { "Send" };
        let builder = column![
            text("Request builder").size(16),
            method_row,
            text_input("https://example.com", &self.url).on_input(RequestsMessage::UrlChanged),
            text("Headers (Key: Value, one per line)"),
            text_input("Content-Type: application/json", &self.headers)
                .on_input(RequestsMessage::HeadersChanged),
            text("Body"),
            text_input("", &self.body).on_input(RequestsMessage::BodyChanged),
            row![
                button(text(send_label)).on_press(RequestsMessage::SendPressed),
                text_input("request name", &self.save_name)
                    .on_input(RequestsMessage::SaveNameChanged),
                button(text("Save request")).on_press(RequestsMessage::SavePressed),
            ]
            .spacing(8),
        ]
        .spacing(8);

        let saved_list = self.saved.iter().fold(
            column![text("Saved requests").size(16)].spacing(4),
            |col, r| {
                col.push(
                    row![
                        button(text(format!("{} {}", r.method, r.name)))
                            .on_press(RequestsMessage::LoadSaved(r.name.clone())),
                        button(text("delete"))
                            .on_press(RequestsMessage::DeleteSaved(r.name.clone())),
                    ]
                    .spacing(6),
                )
            },
        );

        let sidebar = scrollable(saved_list).width(Length::Fixed(240.0));

        let response_view: Element<'_, RequestsMessage> = match &self.last_response {
            Some(Ok(resp)) => {
                let error_note = if resp.curl_exit != 0 || !resp.stderr.trim().is_empty() {
                    format!(
                        "curl exit {}: {}",
                        resp.curl_exit,
                        resp.stderr.trim()
                    )
                } else {
                    String::new()
                };
                scrollable(
                    column![
                        text(format!(
                            "Status: {}  ({} ms, {} bytes{})",
                            resp.status,
                            resp.duration_ms,
                            resp.body_bytes,
                            if resp.body_truncated { ", truncated" } else { "" }
                        )),
                        text(error_note),
                        text("Headers:"),
                        text(resp.headers.clone()),
                        text("Body:"),
                        text(resp.body.clone()),
                    ]
                    .spacing(6),
                )
                .into()
            }
            Some(Err(err)) => text(format!("Request failed: {err}")).into(),
            None => text("Send a request to see the response").into(),
        };

        container(
            row![sidebar, column![builder, response_view].spacing(16).padding(16)].spacing(16),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

fn method_button(label: &'static str, current: &str) -> Element<'static, RequestsMessage> {
    let display = if current == label {
        format!("> {label}")
    } else {
        label.to_string()
    };
    button(text(display))
        .on_press(RequestsMessage::MethodChanged(label.to_string()))
        .into()
}

fn parse_headers(raw: &str) -> Vec<(String, String)> {
    raw.lines()
        .filter_map(|line| {
            let (k, v) = line.split_once(':')?;
            let k = k.trim();
            let v = v.trim();
            if k.is_empty() {
                None
            } else {
                Some((k.to_string(), v.to_string()))
            }
        })
        .collect()
}

fn reload_saved() -> Task<RequestsMessage> {
    Task::perform(requests::list_saved_requests(), RequestsMessage::SavedReloaded)
}
