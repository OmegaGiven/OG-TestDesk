use std::collections::HashMap;
use std::sync::Arc;

use iced::widget::{button, column, container, mouse_area, row, scrollable, text, text_editor, text_input};
use iced::{Element, Length, Task};

use og_testdesk_core::requests::{
    self, GraphqlPayload, ProxyRequest, ProxyResponse, RequestVariableSet, RequestVariables, SavedRequest,
};

use crate::graphql_highlighter::{self, GraphqlHighlighter};
use crate::json_highlighter::{self, JsonHighlighter};
use crate::request_auth::{AuthMessage, AuthState};
use crate::request_env::{merge_scopes, scan_env_variables, substitute_env_variables};
use crate::request_kv_editor::{self, KvEditorMessage, KvRow};
use crate::request_url::{build_url_with_params, parse_query_params, scan_path_variables, substitute_path_variables};

const SPLIT_REFERENCE_HEIGHT: f32 = 700.0;

/// Coarse classification of an HTTP response outcome for status-line
/// coloring: successful ranges get their own class, and a request that
/// never got a real status (network/curl failure) gets a distinct class
/// rather than falling into some arbitrary numeric bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusClass {
    Success,
    Redirect,
    ClientError,
    ServerError,
    NetworkFailure,
    Other,
}

impl StatusClass {
    pub fn color(&self) -> iced::Color {
        match self {
            StatusClass::Success => iced::Color::from_rgb8(0x50, 0xfa, 0x7b),
            StatusClass::Redirect => iced::Color::from_rgb8(0x8b, 0xe9, 0xfd),
            StatusClass::ClientError => iced::Color::from_rgb8(0xff, 0xb8, 0x6c),
            StatusClass::ServerError => iced::Color::from_rgb8(0xff, 0x55, 0x55),
            StatusClass::NetworkFailure => iced::Color::from_rgb8(0xff, 0x55, 0x55),
            StatusClass::Other => iced::Color::from_rgb8(0xf8, 0xf8, 0xf2),
        }
    }
}

/// Classifies a `ProxyResponse` outcome. `status == 0` (with a non-zero
/// `curl_exit`) means curl never got a response at all — see
/// `core::requests::run_proxy_request`, which sets `status` to `0` via
/// `unwrap_or(0)` when curl's `%{http_code}` output can't be parsed.
pub fn classify_status(status: u16, curl_exit: i32) -> StatusClass {
    if status == 0 && curl_exit != 0 {
        return StatusClass::NetworkFailure;
    }
    match status {
        200..=299 => StatusClass::Success,
        300..=399 => StatusClass::Redirect,
        400..=499 => StatusClass::ClientError,
        500..=599 => StatusClass::ServerError,
        _ => StatusClass::Other,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuilderSection {
    Params,
    Path,
    Auth,
    Headers,
    Body,
    Variables,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyMode {
    Raw,
    FormData,
    UrlEncoded,
    Binary,
    GraphQl,
}

impl BodyMode {
    const ALL: [BodyMode; 5] = [
        BodyMode::Raw,
        BodyMode::FormData,
        BodyMode::UrlEncoded,
        BodyMode::Binary,
        BodyMode::GraphQl,
    ];

    fn label(&self) -> &'static str {
        match self {
            BodyMode::Raw => "Raw",
            BodyMode::FormData => "Form Data",
            BodyMode::UrlEncoded => "URL-encoded",
            BodyMode::Binary => "Binary",
            BodyMode::GraphQl => "GraphQL",
        }
    }
}

pub struct RequestTabState {
    pub name: String,
    pub saved_name: Option<String>,
    pub saved_folder: Option<String>,
    pub method: String,
    pub url_base: String,
    pub params: Vec<KvRow>,
    pub path_var_values: HashMap<String, String>,
    pub headers: Vec<KvRow>,
    pub local_vars: Vec<KvRow>,
    pub auth: AuthState,
    pub section: BuilderSection,
    pub body_mode: BodyMode,
    pub raw_body: text_editor::Content,
    pub form_data: Vec<KvRow>,
    pub urlencoded: Vec<KvRow>,
    pub graphql_query: text_editor::Content,
    pub graphql_vars: text_editor::Content,
    pub sending: bool,
    pub last_response: Option<Result<ProxyResponse, String>>,
    pub response_body_editor: text_editor::Content,
    pub send_error: Option<String>,
    pub baseline: Option<String>,
}

impl RequestTabState {
    fn blank() -> Self {
        Self {
            name: "Untitled Request".to_string(),
            saved_name: None,
            saved_folder: None,
            method: "GET".to_string(),
            url_base: String::new(),
            params: vec![KvRow::default()],
            path_var_values: HashMap::new(),
            headers: vec![KvRow::default()],
            local_vars: vec![KvRow::default()],
            auth: AuthState::default(),
            section: BuilderSection::Params,
            body_mode: BodyMode::Raw,
            raw_body: text_editor::Content::new(),
            form_data: vec![KvRow::default()],
            urlencoded: vec![KvRow::default()],
            graphql_query: text_editor::Content::new(),
            graphql_vars: text_editor::Content::new(),
            sending: false,
            last_response: None,
            response_body_editor: text_editor::Content::new(),
            send_error: None,
            baseline: None,
        }
    }

    fn full_url(&self) -> String {
        build_url_with_params(&self.url_base, &self.params)
    }

    fn fingerprint(&self) -> String {
        format!(
            "{}|{}|{:?}|{}|{:?}|{}|{}",
            self.method,
            self.full_url(),
            request_kv_editor::active_pairs(&self.headers),
            self.raw_body.text(),
            request_kv_editor::active_pairs(&self.form_data),
            self.auth.auth_type_index,
            self.auth.bearer_token,
        )
    }

    fn is_dirty(&self) -> bool {
        match &self.baseline {
            Some(baseline) => *baseline != self.fingerprint(),
            None => {
                !self.url_base.is_empty()
                    || !self.raw_body.text().trim().is_empty()
                    || request_kv_editor::active_pairs(&self.headers).len() > 0
                    || request_kv_editor::active_pairs(&self.params).len() > 0
            }
        }
    }
}

/// One named environment set as edited in the environment manager panel —
/// mirrors `core::requests::RequestVariableSet` but keeps its values as
/// editable [`KvRow`]s instead of a plain map.
pub struct EnvSetState {
    pub name: String,
    pub rows: Vec<KvRow>,
}

/// Workspace-level (not per-tab) environment manager: global variables, a
/// list of named environment sets, and which one is active. Persisted via
/// `core::requests::{get_request_variables, save_request_variables_value}`.
pub struct EnvManagerState {
    pub open: bool,
    pub global: Vec<KvRow>,
    pub sets: Vec<EnvSetState>,
    pub active_set: String,
    pub new_set_name: String,
}

impl EnvManagerState {
    fn blank() -> Self {
        Self {
            open: false,
            global: vec![KvRow::default()],
            sets: Vec::new(),
            active_set: String::new(),
            new_set_name: String::new(),
        }
    }

    fn load_from(&mut self, vars: RequestVariables) {
        self.global = vars
            .global
            .into_iter()
            .map(|(k, v)| KvRow::new(k, v))
            .collect();
        request_kv_editor::ensure_trailing_blank_row(&mut self.global);
        self.sets = vars
            .sets
            .into_iter()
            .map(|set| {
                let mut rows: Vec<KvRow> = set.values.into_iter().map(|(k, v)| KvRow::new(k, v)).collect();
                request_kv_editor::ensure_trailing_blank_row(&mut rows);
                EnvSetState { name: set.name, rows }
            })
            .collect();
        self.active_set = vars.active_set;
    }

    fn to_core(&self) -> RequestVariables {
        RequestVariables {
            active_set: self.active_set.clone(),
            sets: self
                .sets
                .iter()
                .map(|set| RequestVariableSet {
                    name: set.name.clone(),
                    values: request_kv_editor::active_pairs(&set.rows).into_iter().collect(),
                })
                .collect(),
            global: request_kv_editor::active_pairs(&self.global).into_iter().collect(),
        }
    }

    fn active_set_values(&self) -> Option<HashMap<String, String>> {
        self.sets
            .iter()
            .find(|s| s.name == self.active_set)
            .map(|s| request_kv_editor::active_pairs(&s.rows).into_iter().collect())
    }
}

#[derive(Debug, Clone)]
pub enum EnvMessage {
    ToggleOpen,
    Loaded(RequestVariables),
    Saved,
    Global(KvEditorMessage),
    SetRows(usize, KvEditorMessage),
    NewSetNameChanged(String),
    CreateSet,
    DeleteSet(usize),
    RenameSet(usize, String),
    SetActive(String),
}

#[derive(Debug, Clone)]
pub enum RequestsMessage {
    NewTab,
    CloseTab(usize),
    SelectTab(usize),

    SectionSelected(BuilderSection),
    MethodChanged(String),
    UrlChanged(String),
    Params(KvEditorMessage),
    Headers(KvEditorMessage),
    FormData(KvEditorMessage),
    UrlEncoded(KvEditorMessage),
    LocalVars(KvEditorMessage),
    PathVarChanged(String, String),
    Auth(AuthMessage),
    Env(EnvMessage),
    BodyModeSelected(BodyMode),
    RawBodyAction(text_editor::Action),
    GraphqlQueryAction(text_editor::Action),
    GraphqlVarsAction(text_editor::Action),

    SendPressed,
    ResponseReceived(Arc<Result<ProxyResponse, String>>),
    OAuthTokenReceived(Result<String, String>),

    SaveNameChanged(String),
    SavePressed,
    SavedReloaded(Vec<SavedRequest>),
    LoadSaved(String),
    DeleteSaved(String),

    SplitDragStart,
    SplitCursorMoved(f32),
    SplitDragEnd,
}

pub struct RequestsTab {
    tabs: Vec<RequestTabState>,
    active: usize,
    save_name: String,
    saved: Vec<SavedRequest>,
    split_ratio: f32,
    resizing: bool,
    drag_last_y: Option<f32>,
    env: EnvManagerState,
}

impl RequestsTab {
    pub fn new() -> (Self, Task<RequestsMessage>) {
        let tab = Self {
            tabs: vec![RequestTabState::blank()],
            active: 0,
            save_name: String::new(),
            saved: Vec::new(),
            split_ratio: 0.55,
            resizing: false,
            drag_last_y: None,
            env: EnvManagerState::blank(),
        };
        let load_env = Task::perform(requests::get_request_variables(), |vars| {
            RequestsMessage::Env(EnvMessage::Loaded(vars))
        });
        (tab, Task::batch([reload_saved(), load_env]))
    }

    /// Mirrors `SqlTab::subscription`'s split-drag tracking exactly (see
    /// `sql_tab.rs`) — same `mouse_area` + window-level `CursorMoved`/
    /// `ButtonReleased` pattern, not a new approach.
    pub fn subscription(&self) -> iced::Subscription<RequestsMessage> {
        iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                Some(RequestsMessage::SplitCursorMoved(position.y))
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(RequestsMessage::SplitDragEnd)
            }
            _ => None,
        })
    }

    fn current(&mut self) -> &mut RequestTabState {
        &mut self.tabs[self.active]
    }

    pub fn update(&mut self, message: RequestsMessage) -> Task<RequestsMessage> {
        match message {
            RequestsMessage::NewTab => {
                self.tabs.push(RequestTabState::blank());
                self.active = self.tabs.len() - 1;
            }
            RequestsMessage::CloseTab(i) => {
                if self.tabs.len() > 1 && i < self.tabs.len() {
                    self.tabs.remove(i);
                    if self.active >= self.tabs.len() {
                        self.active = self.tabs.len() - 1;
                    } else if self.active > i {
                        self.active -= 1;
                    }
                }
            }
            RequestsMessage::SelectTab(i) => {
                if i < self.tabs.len() {
                    self.active = i;
                }
            }

            RequestsMessage::SectionSelected(section) => self.current().section = section,
            RequestsMessage::MethodChanged(v) => self.current().method = v,
            RequestsMessage::UrlChanged(v) => {
                let (base, rows) = parse_query_params(&v);
                let tab = self.current();
                tab.url_base = base;
                tab.params = rows;
                request_kv_editor::ensure_trailing_blank_row(&mut tab.params);
                sync_path_vars(tab);
            }
            RequestsMessage::Params(msg) => {
                let tab = self.current();
                request_kv_editor::update(&mut tab.params, msg);
                sync_path_vars(tab);
            }
            RequestsMessage::Headers(msg) => request_kv_editor::update(&mut self.current().headers, msg),
            RequestsMessage::FormData(msg) => request_kv_editor::update(&mut self.current().form_data, msg),
            RequestsMessage::UrlEncoded(msg) => request_kv_editor::update(&mut self.current().urlencoded, msg),
            RequestsMessage::LocalVars(msg) => request_kv_editor::update(&mut self.current().local_vars, msg),
            RequestsMessage::PathVarChanged(name, value) => {
                self.current().path_var_values.insert(name, value);
            }
            RequestsMessage::Auth(msg) => {
                if matches!(msg, AuthMessage::FetchTokenPressed) {
                    return self.fetch_oauth_token();
                }
                crate::request_auth::update(&mut self.current().auth, msg);
            }
            RequestsMessage::Env(msg) => return self.update_env(msg),
            RequestsMessage::BodyModeSelected(mode) => self.current().body_mode = mode,
            RequestsMessage::RawBodyAction(action) => self.current().raw_body.perform(action),
            RequestsMessage::GraphqlQueryAction(action) => self.current().graphql_query.perform(action),
            RequestsMessage::GraphqlVarsAction(action) => self.current().graphql_vars.perform(action),

            RequestsMessage::SendPressed => return self.send_request(),
            RequestsMessage::ResponseReceived(result) => {
                let tab = self.current();
                tab.sending = false;
                if let Ok(resp) = result.as_ref() {
                    let display_text = match serde_json::from_str::<serde_json::Value>(&resp.body) {
                        Ok(value) => {
                            serde_json::to_string_pretty(&value).unwrap_or_else(|_| resp.body.clone())
                        }
                        Err(_) => resp.body.clone(),
                    };
                    tab.response_body_editor = text_editor::Content::with_text(&display_text);
                }
                tab.last_response = Some((*result).clone());
            }
            RequestsMessage::OAuthTokenReceived(result) => {
                let tab = self.current();
                tab.auth.oauth_fetching = false;
                match result {
                    Ok(token) => {
                        tab.auth.oauth_fetched_token = Some(token);
                        tab.auth.oauth_error = None;
                    }
                    Err(err) => tab.auth.oauth_error = Some(err),
                }
            }

            RequestsMessage::SaveNameChanged(v) => self.save_name = v,
            RequestsMessage::SavePressed => return self.save_current(),
            RequestsMessage::SavedReloaded(saved) => self.saved = saved,
            RequestsMessage::LoadSaved(name) => self.load_saved_into_current(&name),
            RequestsMessage::DeleteSaved(name) => {
                let folder = self.saved.iter().find(|r| r.name == name).and_then(|r| r.folder.clone());
                return Task::perform(
                    async move {
                        let _ = requests::delete_request(&name, folder.as_deref()).await;
                        requests::list_saved_requests().await
                    },
                    RequestsMessage::SavedReloaded,
                );
            }

            RequestsMessage::SplitDragStart => {
                self.resizing = true;
                self.drag_last_y = None;
            }
            RequestsMessage::SplitCursorMoved(y) => {
                if self.resizing {
                    if let Some(last_y) = self.drag_last_y {
                        let delta = (y - last_y) / SPLIT_REFERENCE_HEIGHT;
                        self.split_ratio = (self.split_ratio + delta).clamp(0.15, 0.85);
                    }
                    self.drag_last_y = Some(y);
                }
            }
            RequestsMessage::SplitDragEnd => {
                self.resizing = false;
                self.drag_last_y = None;
            }
        }
        Task::none()
    }

    fn send_request(&mut self) -> Task<RequestsMessage> {
        let global = request_kv_editor::active_pairs(&self.env.global).into_iter().collect::<HashMap<_, _>>();
        let active_set_values = self.env.active_set_values();
        let tab = self.current();
        tab.send_error = None;
        let local = request_kv_editor::active_pairs(&tab.local_vars).into_iter().collect::<HashMap<_, _>>();
        let env_values = merge_scopes(&global, active_set_values.as_ref(), &local);
        match build_proxy_request(tab, &env_values) {
            Ok(payload) => {
                tab.sending = true;
                Task::perform(
                    async move {
                        match tokio::task::spawn_blocking(move || requests::run_proxy_request(payload)).await {
                            Ok(Ok(resp)) => Ok(resp),
                            Ok(Err(io_err)) => Err(io_err.to_string()),
                            Err(join_err) => Err(format!("task join error: {join_err}")),
                        }
                    },
                    |result| RequestsMessage::ResponseReceived(Arc::new(result)),
                )
            }
            Err(err) => {
                tab.send_error = Some(err);
                Task::none()
            }
        }
    }

    fn fetch_oauth_token(&mut self) -> Task<RequestsMessage> {
        let tab = self.current();
        let token_url = tab.auth.oauth_token_url.clone();
        let client_id = tab.auth.oauth_client_id.clone();
        let client_secret = tab.auth.oauth_client_secret.clone();
        let scope = tab.auth.oauth_scope.clone();
        if token_url.trim().is_empty() {
            tab.auth.oauth_error = Some("Token URL is required.".to_string());
            return Task::none();
        }
        tab.auth.oauth_fetching = true;
        tab.auth.oauth_error = None;

        let body = format!(
            "grant_type=client_credentials&client_id={}&client_secret={}&scope={}",
            client_id, client_secret, scope
        );
        let payload = ProxyRequest {
            method: "POST".to_string(),
            url: token_url,
            headers: vec![("Content-Type".to_string(), "application/x-www-form-urlencoded".to_string())],
            body,
            body_mode: Some("raw".to_string()),
            form_data: vec![],
            graphql: None,
            request_id: None,
        };
        Task::perform(
            async move {
                let result = tokio::task::spawn_blocking(move || requests::run_proxy_request(payload)).await;
                match result {
                    Ok(Ok(resp)) if resp.status >= 200 && resp.status < 300 => {
                        match serde_json::from_str::<serde_json::Value>(&resp.body) {
                            Ok(json) => json
                                .get("access_token")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .ok_or_else(|| "Response had no access_token field.".to_string()),
                            Err(_) => Err("Token response was not valid JSON.".to_string()),
                        }
                    }
                    Ok(Ok(resp)) => Err(format!("Token request failed with status {}", resp.status)),
                    Ok(Err(io_err)) => Err(io_err.to_string()),
                    Err(join_err) => Err(format!("task join error: {join_err}")),
                }
            },
            RequestsMessage::OAuthTokenReceived,
        )
    }

    /// Applies an [`EnvMessage`], then (for anything that changes persisted
    /// state) saves via `save_request_variables_value` — fire-and-forget,
    /// same pattern as `scratchpad_tab.rs`'s save-on-every-change.
    fn update_env(&mut self, message: EnvMessage) -> Task<RequestsMessage> {
        match message {
            EnvMessage::ToggleOpen => {
                self.env.open = !self.env.open;
                return Task::none();
            }
            EnvMessage::Loaded(vars) => {
                self.env.load_from(vars);
                return Task::none();
            }
            EnvMessage::Saved => return Task::none(),
            EnvMessage::Global(msg) => request_kv_editor::update(&mut self.env.global, msg),
            EnvMessage::SetRows(i, msg) => {
                if let Some(set) = self.env.sets.get_mut(i) {
                    request_kv_editor::update(&mut set.rows, msg);
                }
            }
            EnvMessage::NewSetNameChanged(v) => {
                self.env.new_set_name = v;
                return Task::none();
            }
            EnvMessage::CreateSet => {
                let name = self.env.new_set_name.trim().to_string();
                if name.is_empty() || self.env.sets.iter().any(|s| s.name == name) {
                    return Task::none();
                }
                self.env.sets.push(EnvSetState {
                    name,
                    rows: vec![KvRow::default()],
                });
                self.env.new_set_name.clear();
            }
            EnvMessage::DeleteSet(i) => {
                if i < self.env.sets.len() {
                    let removed_name = self.env.sets.remove(i).name;
                    if self.env.active_set == removed_name {
                        self.env.active_set.clear();
                    }
                }
            }
            EnvMessage::RenameSet(i, new_name) => {
                if let Some(set) = self.env.sets.get_mut(i) {
                    let old_name = set.name.clone();
                    set.name = new_name.clone();
                    if self.env.active_set == old_name {
                        self.env.active_set = new_name;
                    }
                }
            }
            EnvMessage::SetActive(name) => self.env.active_set = name,
        }
        let vars = self.env.to_core();
        Task::perform(
            async move {
                let _ = requests::save_request_variables_value(vars).await;
            },
            |_| RequestsMessage::Env(EnvMessage::Saved),
        )
    }

    fn save_current(&mut self) -> Task<RequestsMessage> {
        if self.save_name.trim().is_empty() {
            return Task::none();
        }
        let name = self.save_name.clone();
        self.save_name.clear();
        let tab = self.current();
        let method = tab.method.clone();
        let url = tab.full_url();
        let headers = headers_as_text(&tab.headers);
        let body = tab.raw_body.text();
        tab.saved_name = Some(name.clone());
        tab.name = name.clone();
        tab.baseline = Some(tab.fingerprint());
        Task::perform(
            async move {
                let _ = requests::save_request(&name, &method, &url, &headers, &body, None, None, None, None, None, None).await;
                requests::list_saved_requests().await
            },
            RequestsMessage::SavedReloaded,
        )
    }

    fn load_saved_into_current(&mut self, name: &str) {
        let Some(saved) = self.saved.iter().find(|r| r.name == name).cloned() else {
            return;
        };
        let (base, rows) = parse_query_params(&saved.url);
        let tab = self.current();
        tab.name = saved.name.clone();
        tab.saved_name = Some(saved.name.clone());
        tab.saved_folder = saved.folder.clone();
        tab.method = saved.method.clone();
        tab.url_base = base;
        tab.params = rows;
        request_kv_editor::ensure_trailing_blank_row(&mut tab.params);
        tab.headers = headers_from_text(&saved.headers);
        request_kv_editor::ensure_trailing_blank_row(&mut tab.headers);
        tab.raw_body = text_editor::Content::with_text(&saved.body);
        sync_path_vars(tab);
        tab.baseline = Some(tab.fingerprint());
    }

    pub fn view(&self) -> Element<'_, RequestsMessage> {
        let tab_bar = self.tabs.iter().enumerate().fold(row![].spacing(4), |r, (i, t)| {
            let label = if t.is_dirty() { format!("* {}", t.name) } else { t.name.clone() };
            let active_marker = if i == self.active { "> " } else { "" };
            r.push(
                row![
                    button(text(format!("{active_marker}{label}"))).on_press(RequestsMessage::SelectTab(i)),
                    button(text("x")).on_press(RequestsMessage::CloseTab(i)),
                ]
                .spacing(2),
            )
        });
        let tab_bar = row![tab_bar, button(text("+")).on_press(RequestsMessage::NewTab)].spacing(8);

        let tab = &self.tabs[self.active];

        let method_row = row![
            method_button("GET", &tab.method),
            method_button("POST", &tab.method),
            method_button("PUT", &tab.method),
            method_button("PATCH", &tab.method),
            method_button("DELETE", &tab.method),
        ]
        .spacing(6);

        let env_indicator = if self.env.active_set.is_empty() {
            text("No environment active").size(12)
        } else {
            text(format!("Env: {}", self.env.active_set)).size(12)
        };
        let env_toggle_label = if self.env.open { "Hide environments" } else { "Environments" };
        let url_row = row![
            method_row,
            text_input("https://example.com/{id}", &tab.full_url()).on_input(RequestsMessage::UrlChanged),
            env_indicator,
            button(text(env_toggle_label)).on_press(RequestsMessage::Env(EnvMessage::ToggleOpen)),
        ]
        .spacing(8);

        let section_row = row![
            section_button("Params", BuilderSection::Params, tab.section),
            section_button("Path", BuilderSection::Path, tab.section),
            section_button("Auth", BuilderSection::Auth, tab.section),
            section_button("Headers", BuilderSection::Headers, tab.section),
            section_button("Body", BuilderSection::Body, tab.section),
            section_button("Vars", BuilderSection::Variables, tab.section),
        ]
        .spacing(6);

        let section_body: Element<'_, RequestsMessage> = match tab.section {
            BuilderSection::Params => request_kv_editor::view(&tab.params).map(RequestsMessage::Params),
            BuilderSection::Path => view_path_vars(tab),
            BuilderSection::Auth => crate::request_auth::view(&tab.auth).map(RequestsMessage::Auth),
            BuilderSection::Headers => request_kv_editor::view(&tab.headers).map(RequestsMessage::Headers),
            BuilderSection::Body => view_body(tab),
            BuilderSection::Variables => {
                let detected = scan_env_variables(&format!(
                    "{} {} {}",
                    tab.full_url(),
                    request_kv_editor::active_pairs(&tab.headers)
                        .iter()
                        .map(|(_, v)| v.clone())
                        .collect::<Vec<_>>()
                        .join(" "),
                    tab.raw_body.text(),
                ));
                let detected_line = if detected.is_empty() {
                    "No {{env}} variables detected in this request yet.".to_string()
                } else {
                    format!("Detected: {}", detected.join(", "))
                };
                column![
                    text(detected_line).size(12),
                    text("Local overrides (this request tab only, not saved):").size(12),
                    request_kv_editor::view(&tab.local_vars).map(RequestsMessage::LocalVars),
                ]
                .spacing(6)
                .into()
            }
        };

        let send_label = if tab.sending { "Sending..." } else { "Send" };
        let mut builder = column![
            tab_bar,
            url_row,
        ]
        .spacing(10);
        if self.env.open {
            builder = builder.push(view_env_manager(&self.env));
        }
        builder = builder.push(section_row).push(section_body);
        if let Some(err) = &tab.send_error {
            builder = builder.push(text(err.clone()).color(iced::Color::from_rgb8(0xe0, 0x5a, 0x5a)));
        }
        builder = builder.push(
            row![
                button(text(send_label)).on_press(RequestsMessage::SendPressed),
                text_input("request name", &self.save_name).on_input(RequestsMessage::SaveNameChanged),
                button(text("Save request")).on_press(RequestsMessage::SavePressed),
            ]
            .spacing(8),
        );

        let saved_list = self.saved.iter().fold(column![text("Saved requests").size(16)].spacing(4), |col, r| {
            col.push(
                row![
                    button(text(format!("{} {}", r.method, r.name))).on_press(RequestsMessage::LoadSaved(r.name.clone())),
                    button(text("delete")).on_press(RequestsMessage::DeleteSaved(r.name.clone())),
                ]
                .spacing(6),
            )
        });
        let sidebar = scrollable(saved_list).width(Length::Fixed(240.0));

        let response_view: Element<'_, RequestsMessage> = match &tab.last_response {
            Some(Ok(resp)) => view_response(resp, &tab.response_body_editor),
            Some(Err(err)) => text(format!("Request failed: {err}"))
                .color(StatusClass::NetworkFailure.color())
                .into(),
            None => text("Send a request to see the response").into(),
        };

        let divider = mouse_area(
            container(text(""))
                .width(Length::Fill)
                .height(Length::Fixed(6.0))
                .style(|theme: &iced::Theme| container::Style {
                    background: Some(theme.extended_palette().background.strong.color.into()),
                    ..Default::default()
                }),
        )
        .on_press(RequestsMessage::SplitDragStart);

        let builder_portion = (self.split_ratio * 1000.0) as u16;
        let response_portion = ((1.0 - self.split_ratio) * 1000.0) as u16;

        let main_area = column![
            container(builder).height(Length::FillPortion(builder_portion.max(1))),
            divider,
            container(response_view).height(Length::FillPortion(response_portion.max(1))),
        ];

        container(row![sidebar, main_area.width(Length::Fill).padding(16)].spacing(16))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

/// Renders a successful `ProxyResponse`: graduated status-code coloring,
/// headers, and a highlighted pretty-printed body when it parses as JSON
/// (falls back to plain text otherwise, e.g. HTML/plain-text responses).
fn view_response<'a>(
    resp: &'a ProxyResponse,
    response_body_editor: &'a text_editor::Content,
) -> Element<'a, RequestsMessage> {
    let class = classify_status(resp.status, resp.curl_exit);
    let status_label = if resp.status == 0 {
        "No response".to_string()
    } else {
        format!("Status: {}", resp.status)
    };

    let error_note = if resp.curl_exit != 0 || !resp.stderr.trim().is_empty() {
        format!("curl exit {}: {}", resp.curl_exit, resp.stderr.trim())
    } else {
        String::new()
    };

    let body_is_json = serde_json::from_str::<serde_json::Value>(&resp.body).is_ok();
    let body_view: Element<'_, RequestsMessage> = if body_is_json {
        text_editor(response_body_editor)
            .highlight_with::<JsonHighlighter>((), json_highlighter::format_for)
            .into()
    } else {
        text(resp.body.clone()).into()
    };

    scrollable(
        column![
            text(format!(
                "{status_label}  ({} ms, {} bytes{})",
                resp.duration_ms,
                resp.body_bytes,
                if resp.body_truncated { ", truncated" } else { "" }
            ))
            .color(class.color()),
            text(error_note).color(StatusClass::NetworkFailure.color()),
            text("Headers:"),
            text(resp.headers.clone()),
            text("Body:"),
            body_view,
        ]
        .spacing(6),
    )
    .into()
}

fn sync_path_vars(tab: &mut RequestTabState) {
    let names = scan_path_variables(&tab.full_url());
    tab.path_var_values.retain(|k, _| names.contains(k));
    for name in &names {
        tab.path_var_values.entry(name.clone()).or_default();
    }
}

/// Workspace-level environment manager panel: global variables, named
/// environment sets (create/rename/delete/activate), each with its own
/// key/value rows via the shared `request_kv_editor`.
fn view_env_manager(env: &EnvManagerState) -> Element<'_, RequestsMessage> {
    let global_section = column![
        text("Global variables").size(14),
        request_kv_editor::view(&env.global).map(|m| RequestsMessage::Env(EnvMessage::Global(m))),
    ]
    .spacing(6);

    let new_set_row = row![
        text_input("new environment name", &env.new_set_name)
            .on_input(|v| RequestsMessage::Env(EnvMessage::NewSetNameChanged(v))),
        button(text("+ New environment")).on_press(RequestsMessage::Env(EnvMessage::CreateSet)),
    ]
    .spacing(8);

    let sets_section = env.sets.iter().enumerate().fold(
        column![text("Environments").size(14), new_set_row].spacing(8),
        |col, (i, set)| {
            let is_active = env.active_set == set.name;
            let activate_label = if is_active { "Active" } else { "Make active" };
            let header = row![
                text_input("environment name", &set.name)
                    .on_input(move |v| RequestsMessage::Env(EnvMessage::RenameSet(i, v)))
                    .width(Length::FillPortion(1)),
                button(text(activate_label)).on_press(RequestsMessage::Env(EnvMessage::SetActive(set.name.clone()))),
                button(text("Delete")).on_press(RequestsMessage::Env(EnvMessage::DeleteSet(i))),
            ]
            .spacing(8);
            col.push(
                column![
                    header,
                    request_kv_editor::view(&set.rows).map(move |m| RequestsMessage::Env(EnvMessage::SetRows(i, m))),
                ]
                .spacing(4)
                .padding(8),
            )
        },
    );

    container(column![global_section, sets_section].spacing(16).padding(10))
        .style(|theme: &iced::Theme| container::Style {
            background: Some(theme.extended_palette().background.weak.color.into()),
            ..Default::default()
        })
        .into()
}

fn view_path_vars(tab: &RequestTabState) -> Element<'_, RequestsMessage> {
    let names = scan_path_variables(&tab.full_url());
    if names.is_empty() {
        return text("No {path} variables detected in the URL.").into();
    }
    names
        .iter()
        .fold(column![].spacing(6), |col, name| {
            let value = tab.path_var_values.get(name).cloned().unwrap_or_default();
            let name_for_msg = name.clone();
            col.push(
                row![
                    text(format!("{name}:")).width(Length::Fixed(120.0)),
                    text_input("value", &value)
                        .on_input(move |v| RequestsMessage::PathVarChanged(name_for_msg.clone(), v)),
                ]
                .spacing(8),
            )
        })
        .into()
}

fn view_body(tab: &RequestTabState) -> Element<'_, RequestsMessage> {
    let mode_row = BodyMode::ALL.iter().fold(row![].spacing(6), |r, mode| {
        let selected = *mode == tab.body_mode;
        let label = if selected { format!("> {}", mode.label()) } else { mode.label().to_string() };
        r.push(button(text(label)).on_press(RequestsMessage::BodyModeSelected(*mode)))
    });

    let editor: Element<'_, RequestsMessage> = match tab.body_mode {
        BodyMode::Raw => text_editor(&tab.raw_body)
            .on_action(RequestsMessage::RawBodyAction)
            .highlight_with::<JsonHighlighter>((), json_highlighter::format_for)
            .height(Length::Fixed(200.0))
            .into(),
        // Binary bodies aren't JSON — no highlighter applied, matches the
        // original textarea-with-no-highlighting behavior for this mode.
        BodyMode::Binary => {
            text_editor(&tab.raw_body).on_action(RequestsMessage::RawBodyAction).height(Length::Fixed(200.0)).into()
        }
        BodyMode::FormData => request_kv_editor::view(&tab.form_data).map(RequestsMessage::FormData),
        BodyMode::UrlEncoded => request_kv_editor::view(&tab.urlencoded).map(RequestsMessage::UrlEncoded),
        BodyMode::GraphQl => column![
            text("Query:"),
            text_editor(&tab.graphql_query)
                .on_action(RequestsMessage::GraphqlQueryAction)
                .highlight_with::<GraphqlHighlighter>((), graphql_highlighter::format_for)
                .height(Length::Fixed(150.0)),
            text("Variables (JSON):"),
            text_editor(&tab.graphql_vars)
                .on_action(RequestsMessage::GraphqlVarsAction)
                .highlight_with::<JsonHighlighter>((), json_highlighter::format_for)
                .height(Length::Fixed(100.0)),
        ]
        .spacing(6)
        .into(),
    };

    column![mode_row, editor].spacing(8).into()
}

fn method_button(label: &'static str, current: &str) -> Element<'static, RequestsMessage> {
    let display = if current == label { format!("> {label}") } else { label.to_string() };
    button(text(display)).on_press(RequestsMessage::MethodChanged(label.to_string())).into()
}

fn section_button(label: &'static str, section: BuilderSection, current: BuilderSection) -> Element<'static, RequestsMessage> {
    let display = if section == current { format!("[{label}]") } else { label.to_string() };
    button(text(display)).on_press(RequestsMessage::SectionSelected(section)).into()
}

fn headers_as_text(rows: &[KvRow]) -> String {
    request_kv_editor::active_pairs(rows)
        .into_iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn headers_from_text(raw: &str) -> Vec<KvRow> {
    raw.lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| KvRow::new(k.trim(), v.trim()))
        .collect()
}

/// Assembles a `ProxyRequest` from a tab's builder state, resolving query
/// params, path variables, auth contributions, and the active body mode.
/// Returns an error message (rather than sending) if a `{path}` variable or
/// an `{{env}}` variable (resolved against `env_values`, the merged
/// local/active-set/global scopes) is left unresolved.
fn build_proxy_request(tab: &RequestTabState, env_values: &HashMap<String, String>) -> Result<ProxyRequest, String> {
    let mut query_pairs = request_kv_editor::active_pairs(&tab.params);
    query_pairs.extend(tab.auth.effective_query_params());
    let base_with_query = if query_pairs.is_empty() {
        tab.url_base.clone()
    } else {
        let query = query_pairs
            .iter()
            .map(|(k, v)| format!("{}={}", crate::request_url::urlencode(k), crate::request_url::urlencode(v)))
            .collect::<Vec<_>>()
            .join("&");
        format!("{}?{}", tab.url_base, query)
    };

    let names = scan_path_variables(&base_with_query);
    let missing: Vec<&String> = names
        .iter()
        .filter(|n| tab.path_var_values.get(*n).map(|v| v.trim().is_empty()).unwrap_or(true))
        .collect();
    if !missing.is_empty() {
        let list = missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
        return Err(format!("Missing path variable value(s): {list}"));
    }
    let path_resolved_url = substitute_path_variables(&base_with_query, &tab.path_var_values);

    let mut headers = request_kv_editor::active_pairs(&tab.headers);
    headers.extend(tab.auth.effective_headers());

    let (raw_body_text, body_mode, form_data, graphql_query, graphql_vars_text) = match tab.body_mode {
        BodyMode::Raw => (tab.raw_body.text(), Some("raw".to_string()), vec![], None, None),
        BodyMode::Binary => (tab.raw_body.text(), Some("binary".to_string()), vec![], None, None),
        BodyMode::FormData => (String::new(), Some("form-data".to_string()), request_kv_editor::active_pairs(&tab.form_data), None, None),
        BodyMode::UrlEncoded => {
            let pairs = request_kv_editor::active_pairs(&tab.urlencoded);
            let encoded = pairs
                .iter()
                .map(|(k, v)| format!("{}={}", crate::request_url::urlencode(k), crate::request_url::urlencode(v)))
                .collect::<Vec<_>>()
                .join("&");
            (encoded, Some("raw".to_string()), vec![], None, None)
        }
        BodyMode::GraphQl => (
            String::new(),
            Some("graphql".to_string()),
            vec![],
            Some(tab.graphql_query.text()),
            Some(tab.graphql_vars.text()),
        ),
    };

    // {{env}} substitution: merge every string that reaches the network
    // (URL, header values, body/GraphQL text) so unresolved names surface
    // as one combined error rather than sending literal `{{name}}` text.
    let mut missing = Vec::new();
    let (resolved_url, url_missing) = substitute_env_variables(&path_resolved_url, env_values);
    missing.extend(url_missing);

    let mut resolved_headers = Vec::with_capacity(headers.len());
    for (k, v) in headers {
        let (resolved_v, header_missing) = substitute_env_variables(&v, env_values);
        missing.extend(header_missing);
        resolved_headers.push((k, resolved_v));
    }

    let (resolved_body, body_missing) = substitute_env_variables(&raw_body_text, env_values);
    missing.extend(body_missing);

    let resolved_graphql_query = graphql_query.map(|q| {
        let (resolved, q_missing) = substitute_env_variables(&q, env_values);
        missing.extend(q_missing);
        resolved
    });
    let resolved_graphql_vars_text = graphql_vars_text.map(|v| {
        let (resolved, v_missing) = substitute_env_variables(&v, env_values);
        missing.extend(v_missing);
        resolved
    });

    missing.sort();
    missing.dedup();
    if !missing.is_empty() {
        let list = missing.join(", ");
        return Err(format!("Missing environment variable value(s): {list}"));
    }

    let graphql = resolved_graphql_query.map(|query| GraphqlPayload {
        variables: resolved_graphql_vars_text
            .as_deref()
            .and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok()),
        query,
        operation_name: None,
    });

    Ok(ProxyRequest {
        method: tab.method.clone(),
        url: resolved_url,
        headers: resolved_headers,
        body: resolved_body,
        body_mode,
        form_data,
        graphql,
        request_id: None,
    })
}

fn reload_saved() -> Task<RequestsMessage> {
    Task::perform(requests::list_saved_requests(), RequestsMessage::SavedReloaded)
}

#[cfg(test)]
mod phase2_tests {
    use super::*;

    #[test]
    fn classify_status_2xx_is_success() {
        assert_eq!(classify_status(200, 0), StatusClass::Success);
        assert_eq!(classify_status(201, 0), StatusClass::Success);
        assert_eq!(classify_status(299, 0), StatusClass::Success);
    }

    #[test]
    fn classify_status_3xx_is_redirect() {
        assert_eq!(classify_status(301, 0), StatusClass::Redirect);
        assert_eq!(classify_status(304, 0), StatusClass::Redirect);
    }

    #[test]
    fn classify_status_4xx_is_client_error() {
        assert_eq!(classify_status(404, 0), StatusClass::ClientError);
        assert_eq!(classify_status(401, 0), StatusClass::ClientError);
    }

    #[test]
    fn classify_status_5xx_is_server_error() {
        assert_eq!(classify_status(500, 0), StatusClass::ServerError);
        assert_eq!(classify_status(503, 0), StatusClass::ServerError);
    }

    #[test]
    fn classify_status_zero_with_nonzero_curl_exit_is_network_failure() {
        assert_eq!(classify_status(0, 7), StatusClass::NetworkFailure);
        assert_eq!(classify_status(0, 28), StatusClass::NetworkFailure);
    }

    #[test]
    fn classify_status_zero_with_zero_curl_exit_is_other() {
        // curl succeeded (exit 0) but somehow reported status 0 - an edge
        // case, not a network failure, falls through to Other rather than
        // being misclassified as a failure.
        assert_eq!(classify_status(0, 0), StatusClass::Other);
    }

    #[test]
    fn classify_status_1xx_is_other() {
        assert_eq!(classify_status(100, 0), StatusClass::Other);
    }
}
