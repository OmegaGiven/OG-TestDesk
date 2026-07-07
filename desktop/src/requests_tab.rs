use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use iced::widget::{button, column, container, mouse_area, row, scrollable, text, text_editor, text_input};
use iced::{Element, Length, Task};

use og_testdesk_core::requests::{
    self, GraphqlPayload, PostmanImportResult, ProxyRequest, ProxyResponse, RequestVariableSet, RequestVariables,
    SavedRequest,
};

use crate::graphql_highlighter::{self, GraphqlHighlighter};
use crate::json_highlighter::{self, JsonHighlighter};
use crate::curl_import::{generate_curl_command, parse_curl_command};
use crate::request_auth::{AuthMessage, AuthState};
use crate::request_env::{merge_scopes, scan_env_variables, substitute_env_variables};
use crate::request_history::{self, HistoryEntry, RequestSnapshot, ResponseSnapshot};
use crate::request_kv_editor::{self, KvEditorMessage, KvRow};
use crate::request_url::{build_url_with_params, parse_query_params, scan_path_variables, substitute_path_variables};

fn current_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

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

    /// Opens a history entry as a new tab, restoring both the request that
    /// was sent and the response/error it produced, so the user can inspect
    /// what happened without re-sending.
    fn from_history(entry: &HistoryEntry) -> Self {
        let mut tab = Self::blank();
        let (base, rows) = parse_query_params(&entry.request.url);
        tab.name = format!("History: {} {}", entry.request.method, base);
        tab.method = entry.request.method.clone();
        tab.url_base = base;
        tab.params = rows;
        request_kv_editor::ensure_trailing_blank_row(&mut tab.params);
        tab.headers = headers_from_text(&entry.request.headers);
        request_kv_editor::ensure_trailing_blank_row(&mut tab.headers);
        tab.raw_body = text_editor::Content::with_text(&entry.request.body);
        sync_path_vars(&mut tab);

        if let Some(resp) = &entry.response {
            let proxy_resp = ProxyResponse {
                status: resp.status,
                headers: resp.headers.clone(),
                body: resp.body.clone(),
                body_truncated: false,
                body_bytes: resp.body.len() as u64,
                body_limit_bytes: 0,
                stderr: String::new(),
                curl_exit: 0,
                duration_ms: 0,
            };
            let display_text = match serde_json::from_str::<serde_json::Value>(&proxy_resp.body) {
                Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| proxy_resp.body.clone()),
                Err(_) => proxy_resp.body.clone(),
            };
            tab.response_body_editor = text_editor::Content::with_text(&display_text);
            tab.last_response = Some(Ok(proxy_resp));
        } else if let Some(err) = &entry.error {
            tab.last_response = Some(Err(err.clone()));
        }

        tab
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

/// State for the "Curl" toggle panel: import a pasted curl command into
/// the current tab, and view/copy the current tab's request as curl.
pub struct CurlPanelState {
    pub open: bool,
    pub import_text: String,
    pub import_error: Option<String>,
}

impl CurlPanelState {
    fn blank() -> Self {
        Self {
            open: false,
            import_text: String::new(),
            import_error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum CurlMessage {
    ToggleOpen,
    ImportTextChanged(String),
    ImportPressed,
    CopyPressed,
}

/// State for the Postman collection import panel: pick a `.json` export,
/// choose how to handle name collisions, and import it via
/// `core::requests::import_postman_collection`.
pub struct PostmanImportState {
    pub open: bool,
    pub duplicate_mode: String,
    pub pending_collection: Option<serde_json::Value>,
    pub loading: bool,
    pub importing: bool,
    pub error: Option<String>,
    pub result: Option<PostmanImportResult>,
}

impl PostmanImportState {
    fn blank() -> Self {
        Self {
            open: false,
            duplicate_mode: "rename".to_string(),
            pending_collection: None,
            loading: false,
            importing: false,
            error: None,
            result: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PostmanImportMessage {
    ToggleOpen,
    DuplicateModeChanged(String),
    PickFilePressed,
    FileLoaded(Result<serde_json::Value, String>),
    ImportPressed,
    Imported(Result<PostmanImportResult, String>),
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
pub enum CollectionMessage {
    FilterChanged(String),
    ToggleFolderExpanded(String),
    NewFolderNameChanged(String),
    CreateFolderPressed,
    DeleteFolderPressed(String),
    RenameStart(String, Option<String>),
    RenameFieldChanged(String),
    RenameCancel,
    RenameConfirm(String, Option<String>),
    MoveFieldChanged(String, Option<String>, String),
    MovePressed(String, Option<String>),
}

#[derive(Debug, Clone)]
pub enum HistoryMessage {
    ToggleOpen,
    Loaded(serde_json::Value),
    Saved,
    FilterChanged(String),
    Load(String),
    Delete(String),
    ClearAll,
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
    SaveFolderChanged(String),
    SavePressed,
    CollectionsReloaded(Vec<SavedRequest>, Vec<String>),
    LoadSaved(String, Option<String>),
    DeleteSaved(String, Option<String>),
    Collection(CollectionMessage),
    History(HistoryMessage),
    Curl(CurlMessage),
    Postman(PostmanImportMessage),

    SplitDragStart,
    SplitCursorMoved(f32),
    SplitDragEnd,
}

pub struct RequestsTab {
    tabs: Vec<RequestTabState>,
    active: usize,
    save_name: String,
    save_folder: String,
    saved: Vec<SavedRequest>,
    saved_folders: Vec<String>,
    collections_filter: String,
    expanded_folders: HashSet<String>,
    new_folder_name: String,
    renaming: Option<(String, Option<String>, String)>,
    move_target: HashMap<(String, Option<String>), String>,
    history: Vec<HistoryEntry>,
    history_filter: String,
    history_open: bool,
    split_ratio: f32,
    resizing: bool,
    drag_last_y: Option<f32>,
    env: EnvManagerState,
    curl: CurlPanelState,
    postman: PostmanImportState,
}

impl RequestsTab {
    pub fn new() -> (Self, Task<RequestsMessage>) {
        let tab = Self {
            tabs: vec![RequestTabState::blank()],
            active: 0,
            save_name: String::new(),
            save_folder: String::new(),
            saved: Vec::new(),
            saved_folders: Vec::new(),
            collections_filter: String::new(),
            expanded_folders: HashSet::new(),
            new_folder_name: String::new(),
            renaming: None,
            move_target: HashMap::new(),
            history: Vec::new(),
            history_filter: String::new(),
            history_open: false,
            split_ratio: 0.55,
            resizing: false,
            drag_last_y: None,
            env: EnvManagerState::blank(),
            curl: CurlPanelState::blank(),
            postman: PostmanImportState::blank(),
        };
        let load_env = Task::perform(requests::get_request_variables(), |vars| {
            RequestsMessage::Env(EnvMessage::Loaded(vars))
        });
        let load_history = Task::perform(requests::get_request_history(), |value| {
            RequestsMessage::History(HistoryMessage::Loaded(value))
        });
        (tab, Task::batch([reload_collections(), load_env, load_history]))
    }

    /// Mirrors `SqlTab::subscription`'s split-drag tracking exactly (see
    /// `sql_tab.rs`) — same `mouse_area` + window-level `CursorMoved`/
    /// `ButtonReleased` pattern, not a new approach.
    /// Mirrors `SqlTab`'s split-drag tracking, plus two shortcuts scoped to
    /// this tab: Ctrl/Cmd+Enter sends the current request, Ctrl/Cmd+S saves
    /// it. Both require a modifier key, so they don't fire on plain typing
    /// (e.g. Enter inside a single-line text_input still behaves normally
    /// unless a modifier is held) — same reasoning as SQL's Ctrl/Cmd+F.
    pub fn subscription(&self) -> iced::Subscription<RequestsMessage> {
        iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                Some(RequestsMessage::SplitCursorMoved(position.y))
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(RequestsMessage::SplitDragEnd)
            }
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                if !modifiers.command() {
                    return None;
                }
                match key.as_ref() {
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter) => {
                        Some(RequestsMessage::SendPressed)
                    }
                    iced::keyboard::Key::Character("s") => Some(RequestsMessage::SavePressed),
                    _ => None,
                }
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
                let (method, url, headers_text, body_text) = {
                    let tab = self.current();
                    (tab.method.clone(), tab.full_url(), headers_as_text(&tab.headers), tab.raw_body.text())
                };
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

                let (response_snapshot, error) = match result.as_ref() {
                    Ok(resp) => (Some(ResponseSnapshot::new(resp.status, &resp.headers, &resp.body)), None),
                    Err(err) => (None, Some(err.clone())),
                };
                let entry = HistoryEntry {
                    id: format!("{}-{}", current_millis(), self.history.len()),
                    timestamp_ms: current_millis(),
                    request: RequestSnapshot::new(&method, &url, &headers_text, &body_text),
                    response: response_snapshot,
                    error,
                };
                request_history::push_capped(&mut self.history, entry, request_history::HISTORY_CAP);
                return self.save_history_task();
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
            RequestsMessage::SaveFolderChanged(v) => self.save_folder = v,
            RequestsMessage::SavePressed => return self.save_current(),
            RequestsMessage::CollectionsReloaded(saved, folders) => {
                self.saved = saved;
                self.saved_folders = folders;
            }
            RequestsMessage::LoadSaved(name, folder) => self.load_saved_into_current(&name, folder.as_deref()),
            RequestsMessage::DeleteSaved(name, folder) => {
                return reload_collections_after(async move {
                    let _ = requests::delete_request(&name, folder.as_deref()).await;
                });
            }
            RequestsMessage::Collection(msg) => return self.update_collection(msg),
            RequestsMessage::History(msg) => return self.update_history(msg),
            RequestsMessage::Curl(msg) => return self.update_curl(msg),
            RequestsMessage::Postman(msg) => return self.update_postman(msg),

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
        let folder_input = self.save_folder.trim().to_string();
        let folder = if folder_input.is_empty() { None } else { Some(folder_input) };
        self.save_name.clear();
        self.save_folder.clear();
        let tab = self.current();
        let method = tab.method.clone();
        let url = tab.full_url();
        let headers = headers_as_text(&tab.headers);
        let body = tab.raw_body.text();
        tab.saved_name = Some(name.clone());
        tab.saved_folder = folder.clone();
        tab.name = name.clone();
        tab.baseline = Some(tab.fingerprint());
        let folder_for_save = folder.clone();
        reload_collections_after(async move {
            let _ = requests::save_request(
                &name,
                &method,
                &url,
                &headers,
                &body,
                None,
                None,
                None,
                None,
                None,
                folder_for_save.as_deref(),
            )
            .await;
        })
    }

    fn load_saved_into_current(&mut self, name: &str, folder: Option<&str>) {
        let Some(saved) = self.saved.iter().find(|r| r.name == name && r.folder.as_deref() == folder).cloned() else {
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

    /// Mirrors `SqlTab`'s saved-query-folder-tree message handling
    /// (`ToggleFolderExpanded`/`CreateFolderPressed`/etc. in `sql_tab.rs`)
    /// against `core::requests`'s folder functions instead of the SQL
    /// engine's — same interaction pattern, no drag-and-drop (per the
    /// design doc's v1 simplification), move-to-folder via a text field.
    fn update_collection(&mut self, message: CollectionMessage) -> Task<RequestsMessage> {
        match message {
            CollectionMessage::FilterChanged(v) => {
                self.collections_filter = v;
                Task::none()
            }
            CollectionMessage::ToggleFolderExpanded(folder) => {
                if !self.expanded_folders.remove(&folder) {
                    self.expanded_folders.insert(folder);
                }
                Task::none()
            }
            CollectionMessage::NewFolderNameChanged(v) => {
                self.new_folder_name = v;
                Task::none()
            }
            CollectionMessage::CreateFolderPressed => {
                let folder = self.new_folder_name.trim().to_string();
                if folder.is_empty() {
                    return Task::none();
                }
                self.new_folder_name.clear();
                reload_collections_after(async move {
                    let _ = requests::create_request_folder(&folder).await;
                })
            }
            CollectionMessage::DeleteFolderPressed(folder) => reload_collections_after(async move {
                let _ = requests::delete_request_folder(&folder).await;
            }),
            CollectionMessage::RenameStart(name, folder) => {
                self.renaming = Some((name.clone(), folder, name));
                Task::none()
            }
            CollectionMessage::RenameFieldChanged(v) => {
                if let Some((_, _, editing)) = &mut self.renaming {
                    *editing = v;
                }
                Task::none()
            }
            CollectionMessage::RenameCancel => {
                self.renaming = None;
                Task::none()
            }
            CollectionMessage::RenameConfirm(original, folder) => {
                let Some((_, _, new_name)) = self.renaming.take() else {
                    return Task::none();
                };
                reload_collections_after(async move {
                    let _ = requests::rename_request(&original, folder.as_deref(), &new_name).await;
                })
            }
            CollectionMessage::MoveFieldChanged(name, folder, target) => {
                self.move_target.insert((name, folder), target);
                Task::none()
            }
            CollectionMessage::MovePressed(name, folder) => {
                let target = self.move_target.remove(&(name.clone(), folder.clone())).unwrap_or_default();
                let new_folder = if target.trim().is_empty() { None } else { Some(target) };
                reload_collections_after(async move {
                    let _ = requests::move_request(&name, folder.as_deref(), new_folder.as_deref()).await;
                })
            }
        }
    }

    fn update_history(&mut self, message: HistoryMessage) -> Task<RequestsMessage> {
        match message {
            HistoryMessage::ToggleOpen => {
                self.history_open = !self.history_open;
                Task::none()
            }
            HistoryMessage::Loaded(value) => {
                self.history = request_history::entries_from_value(&value);
                Task::none()
            }
            HistoryMessage::Saved => Task::none(),
            HistoryMessage::FilterChanged(v) => {
                self.history_filter = v;
                Task::none()
            }
            HistoryMessage::Load(id) => {
                if let Some(entry) = self.history.iter().find(|e| e.id == id).cloned() {
                    self.tabs.push(RequestTabState::from_history(&entry));
                    self.active = self.tabs.len() - 1;
                }
                Task::none()
            }
            HistoryMessage::Delete(id) => {
                self.history.retain(|e| e.id != id);
                self.save_history_task()
            }
            HistoryMessage::ClearAll => {
                self.history.clear();
                self.save_history_task()
            }
        }
    }

    fn save_history_task(&self) -> Task<RequestsMessage> {
        let value = request_history::entries_to_value(&self.history);
        Task::perform(
            async move {
                let _ = requests::save_request_history(&value).await;
            },
            |_| RequestsMessage::History(HistoryMessage::Saved),
        )
    }

    /// Imports a pasted curl command into the *current* tab (method, URL,
    /// headers, body). Replaces headers/body outright rather than merging,
    /// mirroring what pasting a curl command into Postman's importer does.
    fn update_curl(&mut self, message: CurlMessage) -> Task<RequestsMessage> {
        match message {
            CurlMessage::ToggleOpen => self.curl.open = !self.curl.open,
            CurlMessage::ImportTextChanged(v) => self.curl.import_text = v,
            CurlMessage::ImportPressed => match parse_curl_command(&self.curl.import_text) {
                Ok(parsed) => {
                    self.curl.import_error = None;
                    let tab = self.current();
                    tab.method = parsed.method;
                    let (base, rows) = parse_query_params(&parsed.url);
                    tab.url_base = base;
                    tab.params = rows;
                    request_kv_editor::ensure_trailing_blank_row(&mut tab.params);
                    tab.headers = parsed
                        .headers
                        .into_iter()
                        .map(|(k, v)| KvRow::new(k, v))
                        .collect();
                    request_kv_editor::ensure_trailing_blank_row(&mut tab.headers);
                    if let Some(body) = parsed.body {
                        tab.raw_body = text_editor::Content::with_text(&body);
                        tab.body_mode = BodyMode::Raw;
                    }
                    sync_path_vars(tab);
                }
                Err(err) => self.curl.import_error = Some(err),
            },
            CurlMessage::CopyPressed => {
                let tab = self.current();
                let headers = request_kv_editor::active_pairs(&tab.headers);
                let body = matches!(tab.body_mode, BodyMode::Raw | BodyMode::Binary)
                    .then(|| tab.raw_body.text())
                    .filter(|b| !b.is_empty());
                let command = generate_curl_command(&tab.method, &tab.full_url(), &headers, body.as_deref());
                return iced::clipboard::write(command);
            }
        }
        Task::none()
    }

    /// Picks a `.json` file, parses it, and imports it via
    /// `core::requests::import_postman_collection`, refreshing the
    /// collections tree/saved-requests list on success.
    fn update_postman(&mut self, message: PostmanImportMessage) -> Task<RequestsMessage> {
        match message {
            PostmanImportMessage::ToggleOpen => {
                self.postman.open = !self.postman.open;
            }
            PostmanImportMessage::DuplicateModeChanged(v) => self.postman.duplicate_mode = v,
            PostmanImportMessage::PickFilePressed => {
                self.postman.loading = true;
                self.postman.error = None;
                self.postman.result = None;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(|| {
                            let path = rfd::FileDialog::new()
                                .add_filter("Postman Collection", &["json"])
                                .pick_file()
                                .ok_or_else(|| "Import cancelled.".to_string())?;
                            let text = std::fs::read_to_string(&path)
                                .map_err(|err| format!("Failed to read file: {err}"))?;
                            serde_json::from_str::<serde_json::Value>(&text)
                                .map_err(|err| format!("File is not valid JSON: {err}"))
                        })
                        .await
                        .unwrap_or_else(|err| Err(format!("File dialog task failed: {err}")))
                    },
                    |result| RequestsMessage::Postman(PostmanImportMessage::FileLoaded(result)),
                );
            }
            PostmanImportMessage::FileLoaded(result) => {
                self.postman.loading = false;
                match result {
                    Ok(value) => self.postman.pending_collection = Some(value),
                    Err(err) => self.postman.error = Some(err),
                }
            }
            PostmanImportMessage::ImportPressed => {
                if let Some(collection) = self.postman.pending_collection.clone() {
                    self.postman.importing = true;
                    self.postman.error = None;
                    let duplicate_mode = self.postman.duplicate_mode.clone();
                    return Task::perform(
                        async move { requests::import_postman_collection(&collection, &duplicate_mode).await },
                        |result| RequestsMessage::Postman(PostmanImportMessage::Imported(result)),
                    );
                }
            }
            PostmanImportMessage::Imported(result) => {
                self.postman.importing = false;
                match result {
                    Ok(summary) => {
                        self.postman.result = Some(summary);
                        self.postman.pending_collection = None;
                        return reload_collections();
                    }
                    Err(err) => self.postman.error = Some(err),
                }
            }
        }
        Task::none()
    }

    /// Nested folder tree of saved requests — mirrors `SqlTab::view_saved_queries_tree`'s
    /// structure exactly (filter box, new-folder input, collapsible folder
    /// headers, per-request rename/delete/move-to-folder controls), just
    /// against `core::requests`'s folder functions and keyed by (name,
    /// folder) since request identity includes the folder, unlike SQL's
    /// saved queries.
    fn view_collections_tree(&self) -> Element<'_, RequestsMessage> {
        let filter = self.collections_filter.to_lowercase();
        let matches = |name: &str| filter.is_empty() || name.to_lowercase().contains(&filter);

        let mut col = column![
            text("Saved requests").size(16),
            text_input("filter requests", &self.collections_filter)
                .on_input(|v| RequestsMessage::Collection(CollectionMessage::FilterChanged(v))),
            row![
                text_input("new folder name", &self.new_folder_name)
                    .on_input(|v| RequestsMessage::Collection(CollectionMessage::NewFolderNameChanged(v))),
                button(text("+ Folder")).on_press(RequestsMessage::Collection(CollectionMessage::CreateFolderPressed)),
            ]
            .spacing(6),
        ]
        .spacing(4);

        let request_row = |r: &SavedRequest| -> Element<'_, RequestsMessage> {
            if let Some((original, folder, editing_name)) = &self.renaming {
                if original == &r.name && folder == &r.folder {
                    return row![
                        text_input("new name", editing_name)
                            .on_input(|v| RequestsMessage::Collection(CollectionMessage::RenameFieldChanged(v))),
                        button(text("OK")).on_press(RequestsMessage::Collection(CollectionMessage::RenameConfirm(
                            original.clone(),
                            folder.clone(),
                        ))),
                        button(text("x")).on_press(RequestsMessage::Collection(CollectionMessage::RenameCancel)),
                    ]
                    .spacing(4)
                    .into();
                }
            }
            let key = (r.name.clone(), r.folder.clone());
            let move_value = self.move_target.get(&key).cloned().unwrap_or_default();
            let (name_a, folder_a) = (r.name.clone(), r.folder.clone());
            let (name_b, folder_b) = (r.name.clone(), r.folder.clone());
            let (name_c, folder_c) = (r.name.clone(), r.folder.clone());
            let (name_d, folder_d) = (r.name.clone(), r.folder.clone());
            row![
                button(text(format!("{} {}", r.method, r.name))).on_press(RequestsMessage::LoadSaved(name_a, folder_a)),
                button(text("rename"))
                    .on_press(RequestsMessage::Collection(CollectionMessage::RenameStart(name_b, folder_b))),
                button(text("delete")).on_press(RequestsMessage::DeleteSaved(name_c, folder_c)),
                text_input("move to folder", &move_value)
                    .on_input(move |v| {
                        RequestsMessage::Collection(CollectionMessage::MoveFieldChanged(
                            name_d.clone(),
                            folder_d.clone(),
                            v,
                        ))
                    })
                    .width(Length::Fixed(110.0)),
                button(text("Move")).on_press(RequestsMessage::Collection(CollectionMessage::MovePressed(
                    r.name.clone(),
                    r.folder.clone(),
                ))),
            ]
            .spacing(4)
            .into()
        };

        for r in self.saved.iter().filter(|r| r.folder.is_none() && matches(&r.name)) {
            col = col.push(request_row(r));
        }

        for folder in &self.saved_folders {
            let folder_requests: Vec<&SavedRequest> =
                self.saved.iter().filter(|r| r.folder.as_deref() == Some(folder.as_str())).collect();
            let has_match = filter.is_empty() || folder_requests.iter().any(|r| matches(&r.name));
            if !has_match {
                continue;
            }
            let expanded = self.expanded_folders.contains(folder);
            let arrow = if expanded { "v" } else { ">" };
            col = col.push(
                row![
                    button(text(format!("{arrow} {folder}")))
                        .on_press(RequestsMessage::Collection(CollectionMessage::ToggleFolderExpanded(folder.clone()))),
                    button(text("delete folder"))
                        .on_press(RequestsMessage::Collection(CollectionMessage::DeleteFolderPressed(folder.clone()))),
                ]
                .spacing(6),
            );
            if expanded {
                for r in folder_requests.iter().filter(|r| matches(&r.name)) {
                    col = col.push(row![text("  "), request_row(r)].spacing(0));
                }
            }
        }

        col.into()
    }

    /// Searchable history list — replaces the original app's 12-entry
    /// `<select>` dropdown per the design doc's explicit UX-gap call-out.
    fn view_history_panel(&self) -> Element<'_, RequestsMessage> {
        let filter = self.history_filter.clone();
        let mut col = column![
            row![
                text("History").size(16),
                button(text("Clear all")).on_press(RequestsMessage::History(HistoryMessage::ClearAll)),
            ]
            .spacing(8),
            text_input("filter history", &self.history_filter)
                .on_input(|v| RequestsMessage::History(HistoryMessage::FilterChanged(v))),
        ]
        .spacing(4);

        for entry in self.history.iter().filter(|e| request_history::matches_filter(e, &filter)) {
            let status_label = match (&entry.response, &entry.error) {
                (Some(resp), _) => resp.status.to_string(),
                (None, Some(_)) => "error".to_string(),
                (None, None) => "?".to_string(),
            };
            let (id_load, id_delete) = (entry.id.clone(), entry.id.clone());
            col = col.push(
                row![
                    button(text(format!("{} {} [{}]", entry.request.method, entry.request.url, status_label)))
                        .on_press(RequestsMessage::History(HistoryMessage::Load(id_load))),
                    button(text("delete")).on_press(RequestsMessage::History(HistoryMessage::Delete(id_delete))),
                ]
                .spacing(6),
            );
        }

        container(scrollable(col).height(Length::Fixed(220.0)))
            .style(|theme: &iced::Theme| container::Style {
                background: Some(theme.extended_palette().background.weak.color.into()),
                ..Default::default()
            })
            .padding(8)
            .into()
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
        let history_toggle_label = if self.history_open { "Hide history" } else { "History" };
        let curl_toggle_label = if self.curl.open { "Hide curl" } else { "Curl" };
        let postman_toggle_label = if self.postman.open { "Hide Postman import" } else { "Postman import" };
        let url_row = row![
            method_row,
            text_input("https://example.com/{id}", &tab.full_url()).on_input(RequestsMessage::UrlChanged),
            env_indicator,
            button(text(env_toggle_label)).on_press(RequestsMessage::Env(EnvMessage::ToggleOpen)),
            button(text(history_toggle_label)).on_press(RequestsMessage::History(HistoryMessage::ToggleOpen)),
            button(text(curl_toggle_label)).on_press(RequestsMessage::Curl(CurlMessage::ToggleOpen)),
            button(text(postman_toggle_label)).on_press(RequestsMessage::Postman(PostmanImportMessage::ToggleOpen)),
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
        if self.history_open {
            builder = builder.push(self.view_history_panel());
        }
        if self.curl.open {
            builder = builder.push(view_curl_panel(&self.curl, tab));
        }
        if self.postman.open {
            builder = builder.push(view_postman_panel(&self.postman));
        }
        builder = builder.push(section_row).push(section_body);
        if let Some(err) = &tab.send_error {
            builder = builder.push(text(err.clone()).color(iced::Color::from_rgb8(0xe0, 0x5a, 0x5a)));
        }
        builder = builder.push(
            row![
                button(text(send_label)).on_press(RequestsMessage::SendPressed),
                text_input("request name", &self.save_name).on_input(RequestsMessage::SaveNameChanged),
                text_input("folder (optional)", &self.save_folder).on_input(RequestsMessage::SaveFolderChanged),
                button(text("Save request")).on_press(RequestsMessage::SavePressed),
            ]
            .spacing(8),
        );

        let sidebar = scrollable(self.view_collections_tree()).width(Length::Fixed(280.0));

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

/// Curl import (paste a command, populates the current tab) and export
/// ("View as curl" of the current tab's raw builder state, plus a Copy
/// button using `iced::clipboard::write`).
fn view_curl_panel<'a>(curl: &'a CurlPanelState, tab: &'a RequestTabState) -> Element<'a, RequestsMessage> {
    let import_section = column![
        text("Paste a curl command to populate this tab:").size(12),
        text_input("curl https://...", &curl.import_text)
            .on_input(|v| RequestsMessage::Curl(CurlMessage::ImportTextChanged(v))),
        button(text("Import")).on_press(RequestsMessage::Curl(CurlMessage::ImportPressed)),
    ]
    .spacing(6);

    let import_section: Element<'_, RequestsMessage> = if let Some(err) = &curl.import_error {
        column![import_section, text(err.clone()).color(iced::Color::from_rgb8(0xe0, 0x5a, 0x5a))]
            .spacing(6)
            .into()
    } else {
        import_section.into()
    };

    let headers = request_kv_editor::active_pairs(&tab.headers);
    let body = matches!(tab.body_mode, BodyMode::Raw | BodyMode::Binary)
        .then(|| tab.raw_body.text())
        .filter(|b| !b.is_empty());
    let generated = generate_curl_command(&tab.method, &tab.full_url(), &headers, body.as_deref());
    let export_section = column![
        text("This tab as curl:").size(12),
        text(generated).size(12),
        button(text("Copy")).on_press(RequestsMessage::Curl(CurlMessage::CopyPressed)),
    ]
    .spacing(6);

    container(column![import_section, export_section].spacing(16).padding(10))
        .style(|theme: &iced::Theme| container::Style {
            background: Some(theme.extended_palette().background.weak.color.into()),
            ..Default::default()
        })
        .into()
}

/// Postman collection import: pick a `.json` file, choose a duplicate-name
/// strategy, import via `core::requests::import_postman_collection`.
fn view_postman_panel(postman: &PostmanImportState) -> Element<'_, RequestsMessage> {
    let mode_row = row![
        text("On name conflict:").size(12),
        duplicate_mode_button("Rename", "rename", &postman.duplicate_mode),
        duplicate_mode_button("Overwrite", "overwrite", &postman.duplicate_mode),
        duplicate_mode_button("Skip", "skip", &postman.duplicate_mode),
    ]
    .spacing(8);

    let pick_label = if postman.loading { "Loading..." } else { "Choose file..." };
    let mut col = column![
        mode_row,
        button(text(pick_label)).on_press(RequestsMessage::Postman(PostmanImportMessage::PickFilePressed)),
    ]
    .spacing(8);

    if postman.pending_collection.is_some() {
        let import_label = if postman.importing { "Importing..." } else { "Import" };
        col = col.push(row![
            text("Collection loaded, ready to import.").size(12),
            button(text(import_label)).on_press(RequestsMessage::Postman(PostmanImportMessage::ImportPressed)),
        ].spacing(8));
    }

    if let Some(err) = &postman.error {
        col = col.push(text(err.clone()).color(iced::Color::from_rgb8(0xe0, 0x5a, 0x5a)));
    }

    if let Some(result) = &postman.result {
        col = col.push(text(format!(
            "Imported {} request(s), {} folder(s), {} variable(s).",
            result.imported, result.folders, result.variables
        )));
        for warning in &result.warnings {
            col = col.push(text(format!("Warning: {warning}")).size(12));
        }
    }

    container(col.padding(10))
        .style(|theme: &iced::Theme| container::Style {
            background: Some(theme.extended_palette().background.weak.color.into()),
            ..Default::default()
        })
        .into()
}

fn duplicate_mode_button<'a>(label: &'a str, value: &'static str, current: &str) -> Element<'a, RequestsMessage> {
    let display = if current == value { format!("[{label}]") } else { label.to_string() };
    button(text(display))
        .on_press(RequestsMessage::Postman(PostmanImportMessage::DuplicateModeChanged(value.to_string())))
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

fn reload_collections() -> Task<RequestsMessage> {
    Task::perform(
        async { (requests::list_saved_requests().await, requests::list_request_folders().await) },
        |(saved, folders)| RequestsMessage::CollectionsReloaded(saved, folders),
    )
}

/// Runs `action`, then reloads both the saved-requests list and the
/// folder list — mirrors `sql_tab.rs`'s `reload_saved_after`/
/// `reload_folders_after` pair, combined into one round trip since
/// requests don't have SQL's per-connection dimension to key on.
fn reload_collections_after(action: impl std::future::Future<Output = ()> + Send + 'static) -> Task<RequestsMessage> {
    Task::perform(
        async move {
            action.await;
            (requests::list_saved_requests().await, requests::list_request_folders().await)
        },
        |(saved, folders)| RequestsMessage::CollectionsReloaded(saved, folders),
    )
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
