use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use iced::widget::{button, column, container, mouse_area, row, scrollable, text, text_editor, text_input};
use iced::{Element, Length, Task};

use og_testdesk_core::app_db::{self, SqlRunHistoryRecord};
use og_testdesk_core::sql::engine::{
    self, SqlEngineState, TableBrowseResult, TableWriteResult,
};
use og_testdesk_core::sql::helpers::find_connection;
use og_testdesk_core::sql::models::{
    AddConnForm, DbConnection, DbFunctionInfo, EditConnectionForm, SavedQuery, SqlAlertRule,
    SqlExecution, SqlForm, SqlRelationshipSchema, SqlTimezoneInfo, TableBrowseFilter,
    TableUpdateChange,
};

const HISTORY_LIMIT: i64 = 200;

use crate::sql_erd::{self, CardLayout, ErdMessage, ErdProgram};
use crate::sql_grid::{GridMessage, ResultsGrid};
use crate::sql_highlighter::{
    self, SqlHighlighter, SqlHighlighterSettings, VariableFormat, format_for,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SqlViewMode {
    /// Connection manager shown before a connection is opened, or reachable
    /// via "Manage connections" from the editor. See App Shell Phase 3:
    /// selecting a connection here is a simplified "select + switch view"
    /// rather than spawning a real per-connection tab group (that's a
    /// larger `SqlTab` restructuring left for a future phase).
    ConnectionLanding,
    QueryEditor,
    RelationshipDiagram,
}

/// In-progress edit of a saved connection, prefilled from its current
/// values. Password is left blank in the draft (matches
/// `update_connection`'s "blank means keep existing" semantics) rather
/// than round-tripping the real secret back into a plain text field.
#[derive(Clone)]
struct EditConnDraft {
    original_nickname: String,
    db_type: String,
    host: String,
    db_name: String,
    user: String,
    password: String,
    nickname: String,
}

const SPLIT_REFERENCE_HEIGHT: f32 = 700.0;

/// What's currently displayed below the editor: either the last ad-hoc
/// query's results, or a live browse of one table's rows.
struct BrowseState {
    table: String,
    page: u32,
    has_next: bool,
    filter: Option<TableBrowseFilter>,
}

/// A periodically-run SQL task with an optional row-count alert rule.
/// `core` stores these as opaque `serde_json::Value` blobs (a single
/// app-wide list, not scoped per connection) — this type and its
/// `to_value`/`from_value` conversions define the shape we read/write.
#[derive(Clone, Debug)]
struct CronTask {
    id: String,
    name: String,
    sql: String,
    connection: String,
    interval_ms: i64,
    alert: Option<SqlAlertRule>,
    enabled: bool,
    last_run_ms: i64,
}

impl CronTask {
    fn to_value(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "sql": self.sql,
            "connection": self.connection,
            "interval_ms": self.interval_ms,
            "alert": self.alert.as_ref().map(|a| serde_json::json!({
                "comparator": a.comparator,
                "value": a.value,
            })),
            "enabled": self.enabled,
            "last_run_ms": self.last_run_ms,
        })
    }

    fn from_value(value: &serde_json::Value) -> Option<Self> {
        let alert = value.get("alert").and_then(|a| {
            if a.is_null() {
                return None;
            }
            Some(SqlAlertRule {
                comparator: a.get("comparator")?.as_str()?.to_string(),
                value: a.get("value")?.as_i64()?,
            })
        });
        Some(CronTask {
            id: value.get("id")?.as_str()?.to_string(),
            name: value.get("name")?.as_str()?.to_string(),
            sql: value.get("sql")?.as_str()?.to_string(),
            connection: value.get("connection")?.as_str()?.to_string(),
            interval_ms: value.get("interval_ms")?.as_i64()?,
            alert,
            enabled: value.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
            last_run_ms: value.get("last_run_ms").and_then(|v| v.as_i64()).unwrap_or(0),
        })
    }

    fn is_due(&self, now_ms: i64) -> bool {
        self.enabled && self.interval_ms > 0 && now_ms - self.last_run_ms >= self.interval_ms
    }
}

const ALERT_COMPARATORS: &[&str] = &["=", "!=", "<", "<=", ">", ">="];

/// Cycles through the available alert comparators, wrapping to the first
/// after the last — used by a single toggle button rather than a dropdown.
fn next_comparator(current: &str) -> String {
    let index = ALERT_COMPARATORS.iter().position(|c| *c == current).unwrap_or(0);
    ALERT_COMPARATORS[(index + 1) % ALERT_COMPARATORS.len()].to_string()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Quotes a CSV field per RFC 4180 (wrap in quotes and double any embedded
/// quote) only when the field actually contains a character that requires it.
fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Builds a CSV document (CRLF line endings per RFC 4180) from grid-style
/// headers/rows already held in memory — works for both ad-hoc query
/// results and table-browse results, unlike `core`'s `export_results_csv`
/// which only covers the last ad-hoc query run (see Phase 6 notes).
fn build_csv(headers: &[String], rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    out.push_str(&headers.iter().map(|h| csv_field(h)).collect::<Vec<_>>().join(","));
    out.push_str("\r\n");
    for row in rows {
        out.push_str(&row.iter().map(|c| csv_field(c)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

#[derive(Debug, Clone)]
pub enum SqlMessage {
    ConnectionSelected(String),
    EditorAction(text_editor::Action),
    RunPressed,
    QueryFinished(Arc<SqlExecution>),
    Grid(GridMessage),

    NewConnNickname(String),
    NewConnDbType(String),
    NewConnHost(String),
    NewConnDbName(String),
    NewConnUser(String),
    NewConnPassword(String),
    AddConnectionPressed,
    DeleteConnection(String),
    ConnectionsReloaded(Vec<DbConnection>),

    EditConnectionStart(String),
    EditConnDbTypeChanged(String),
    EditConnHostChanged(String),
    EditConnDbNameChanged(String),
    EditConnUserChanged(String),
    EditConnPasswordChanged(String),
    EditConnNicknameChanged(String),
    EditConnectionSave,
    EditConnectionCancel,

    CreateSqliteDbPressed,
    SqliteDbPathChosen(Option<String>),

    GoToLanding,

    SaveQueryNameChanged(String),
    SaveQueryFolderChanged(String),
    SaveQueryPressed,
    SavedQueriesReloaded(Vec<SavedQuery>),
    SavedFoldersReloaded(Vec<String>),
    LoadSavedQuery(String),
    QueryFilterChanged(String),
    ToggleFolderExpanded(String),
    NewFolderNameChanged(String),
    CreateFolderPressed,
    DeleteFolderPressed(String),
    RenameQueryStart(String),
    RenameQueryFieldChanged(String),
    RenameQueryConfirm(String),
    RenameQueryCancel,
    DeleteQueryPressed(String),
    MoveQueryFieldChanged(String, String),
    MoveQueryPressed(String),
    ExportQueriesPressed,
    ImportQueriesPressed,
    ImportExportFinished(Result<String, String>),
    FileDroppedForImport(std::path::PathBuf),

    HistoryReloaded(Vec<SqlRunHistoryRecord>),
    HistoryFilterChanged(String),
    LoadHistoryEntry(String),
    HistoryEntryLoaded(Option<SqlRunHistoryRecord>),
    DeleteHistoryEntry(String),
    ClearHistoryPressed,

    SplitDragStart,
    SplitCursorMoved(f32),
    SplitDragEnd,

    RefreshSchema,
    SchemaLoaded(Result<SqlRelationshipSchema, String>),
    FunctionsLoaded(Result<Vec<DbFunctionInfo>, String>),
    LoadFunctionDefinition(String),
    ToggleTableExpanded(String),
    BrowseTable(String),
    BrowseFiltered(String, String, String),
    BrowseResultLoaded(Result<TableBrowseResult, String>),
    NextPage,
    PrevPage,

    StartEditRow(usize),
    EditFieldChanged(String, String),
    SaveRowEdit,
    CancelRowEdit,

    StartAddRow,
    NewRowFieldChanged(String, String),
    SaveNewRow,
    CancelAddRow,

    VariableValueChanged(String, String),
    VariableFormatChanged(String, VariableFormat),

    AutocompleteSelected(String),

    ToggleFind,
    FindTermChanged(String),
    FindNext,
    FindPrev,
    CloseFind,

    ToggleErdView,
    ErdFilterChanged(String),
    Erd(ErdMessage),

    ExportCsvPressed,
    ExportCsvFinished(Result<String, String>),

    TimezoneLoaded(Result<SqlTimezoneInfo, String>),

    CronTasksLoaded(Vec<serde_json::Value>),
    CronNameChanged(String),
    CronSqlAction(text_editor::Action),
    CronIntervalValueChanged(String),
    CronIntervalUnitChanged(String),
    CronAlertComparatorChanged(String),
    CronAlertValueChanged(String),
    CreateCronTaskPressed,
    DeleteCronTask(String),
    ToggleCronEnabled(String),
    RunCronNow(String),
    CronTick,
    CronRunFinished(String, Arc<SqlExecution>),

    ToggleSchedulePanel,
    ToggleRunningQueries,
    SaveToFilePressed,
    SaveToFileFinished(Result<String, String>),
}

pub struct SqlTab {
    engine: Arc<SqlEngineState>,
    connections: Vec<DbConnection>,
    selected_connection: Option<String>,
    editor: text_editor::Content,
    last_error: Option<String>,
    grid: ResultsGrid,
    running: bool,

    new_conn_nickname: String,
    new_conn_db_type: String,
    new_conn_host: String,
    new_conn_db_name: String,
    new_conn_user: String,
    new_conn_password: String,

    editing_connection: Option<EditConnDraft>,
    sqlite_create_status: Option<String>,

    saved_queries: Vec<SavedQuery>,
    saved_folders: Vec<String>,
    save_query_name: String,
    save_query_folder: String,
    query_filter: String,
    expanded_folders: HashSet<String>,
    new_folder_name: String,
    renaming_query: Option<(String, String)>,
    move_target: HashMap<String, String>,

    history: Vec<SqlRunHistoryRecord>,
    history_filter: String,

    split_ratio: f32,
    resizing: bool,
    drag_last_y: Option<f32>,

    schema: Option<SqlRelationshipSchema>,
    schema_error: Option<String>,
    expanded_tables: HashSet<String>,
    browse: Option<BrowseState>,

    functions: Option<Vec<DbFunctionInfo>>,
    function_error: Option<String>,

    import_export_status: Option<Result<String, String>>,

    editing_row: Option<(usize, HashMap<String, String>)>,
    new_row: Option<HashMap<String, String>>,

    variables: HashMap<String, (String, VariableFormat)>,
    variable_order: Vec<String>,

    autocomplete: Vec<String>,

    find_open: bool,
    find_term: String,
    find_matches: Vec<(usize, usize)>,
    find_current: usize,

    view_mode: SqlViewMode,
    erd_filter: String,
    erd_highlighted: Option<String>,

    csv_status: Option<Result<String, String>>,

    timezone: Option<Result<SqlTimezoneInfo, String>>,

    cron_tasks: Vec<CronTask>,
    cron_activity: Vec<String>,
    cron_name: String,
    cron_sql: text_editor::Content,
    cron_interval_value: String,
    cron_interval_unit: String,
    cron_alert_comparator: String,
    cron_alert_value: String,

    schedule_panel_open: bool,
    running_queries_open: bool,
    save_to_file_status: Option<Result<String, String>>,
}

impl SqlTab {
    /// Read-only view of recent run history for the notifications popup
    /// (main.rs shell, App Shell Phase 1) — most-recent-first, already the
    /// order `history` is stored/rendered in elsewhere in this file.
    pub fn recent_history(&self) -> &[SqlRunHistoryRecord] {
        &self.history
    }

    pub fn new(engine: Arc<SqlEngineState>) -> (Self, Task<SqlMessage>) {
        let tab = Self {
            engine: engine.clone(),
            connections: Vec::new(),
            selected_connection: None,
            editor: text_editor::Content::new(),
            last_error: None,
            grid: ResultsGrid::new(Vec::new(), Vec::new()),
            running: false,

            new_conn_nickname: String::new(),
            new_conn_db_type: "postgres".to_string(),
            new_conn_host: String::new(),
            new_conn_db_name: String::new(),
            new_conn_user: String::new(),
            new_conn_password: String::new(),

            editing_connection: None,
            sqlite_create_status: None,

            saved_queries: Vec::new(),
            saved_folders: Vec::new(),
            save_query_name: String::new(),
            save_query_folder: String::new(),
            query_filter: String::new(),
            expanded_folders: HashSet::new(),
            new_folder_name: String::new(),
            renaming_query: None,
            move_target: HashMap::new(),

            history: Vec::new(),
            history_filter: String::new(),

            split_ratio: 0.4,
            resizing: false,
            drag_last_y: None,

            schema: None,
            schema_error: None,
            expanded_tables: HashSet::new(),
            browse: None,

            functions: None,
            function_error: None,

            import_export_status: None,

            editing_row: None,
            new_row: None,

            variables: HashMap::new(),
            variable_order: Vec::new(),

            autocomplete: Vec::new(),

            find_open: false,
            find_term: String::new(),
            find_matches: Vec::new(),
            find_current: 0,

            view_mode: SqlViewMode::ConnectionLanding,
            erd_filter: String::new(),
            erd_highlighted: None,

            csv_status: None,

            timezone: None,

            cron_tasks: Vec::new(),
            cron_activity: Vec::new(),
            cron_name: String::new(),
            cron_sql: text_editor::Content::new(),
            cron_interval_value: String::new(),
            cron_interval_unit: "minutes".to_string(),
            cron_alert_comparator: ">".to_string(),
            cron_alert_value: String::new(),

            schedule_panel_open: false,
            running_queries_open: false,
            save_to_file_status: None,
        };
        let task = Task::batch([reload_connections(engine), load_cron_tasks()]);
        (tab, task)
    }

    pub fn subscription(&self) -> iced::Subscription<SqlMessage> {
        iced::Subscription::batch([
            self.event_subscription(),
            iced::time::every(std::time::Duration::from_secs(30))
                .map(|_instant| SqlMessage::CronTick),
        ])
    }

    fn event_subscription(&self) -> iced::Subscription<SqlMessage> {
        iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                Some(SqlMessage::SplitCursorMoved(position.y))
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(SqlMessage::SplitDragEnd)
            }
            // Real OS-level drag-and-drop import for the saved-queries panel
            // (App Shell Phase 4). iced 0.13 only reports FileDropped at the
            // window level (no per-widget drop zones), so any file dropped
            // while a connection is selected is treated as a saved-query
            // import — see `SqlMessage::FileDroppedForImport`'s handler for
            // the actual scoping (it's a no-op with no connection selected).
            iced::Event::Window(iced::window::Event::FileDropped(path)) => {
                Some(SqlMessage::FileDroppedForImport(path))
            }
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                if modifiers.command() {
                    if let iced::keyboard::Key::Character(c) = key.as_ref() {
                        if c == "f" {
                            return Some(SqlMessage::ToggleFind);
                        }
                    }
                }
                None
            }
            _ => None,
        })
    }

    pub fn update(&mut self, message: SqlMessage) -> Task<SqlMessage> {
        match message {
            SqlMessage::ConnectionSelected(nickname) => {
                self.selected_connection = Some(nickname.clone());
                self.view_mode = SqlViewMode::QueryEditor;
                self.schema = None;
                self.schema_error = None;
                self.functions = None;
                self.function_error = None;
                self.browse = None;
                self.editing_row = None;
                self.new_row = None;
                self.timezone = None;
                return Task::batch([
                    reload_saved_queries(nickname.clone()),
                    reload_saved_folders(nickname.clone()),
                    reload_schema(self.engine.clone(), nickname.clone()),
                    reload_functions(self.engine.clone(), nickname.clone()),
                    reload_history(nickname.clone()),
                    reload_timezone(self.engine.clone(), nickname),
                ]);
            }
            SqlMessage::EditorAction(action) => {
                self.editor.perform(action);
                self.sync_variables();
                self.update_autocomplete();
                if self.find_open {
                    self.recompute_find_matches();
                }
            }
            SqlMessage::RunPressed => {
                if let Some(connection) = self.selected_connection.clone() {
                    self.running = true;
                    self.last_error = None;
                    self.browse = None;
                    self.editing_row = None;
                    self.new_row = None;
                    let engine = self.engine.clone();
                    let sql = self.editor.text();
                    let history_connection = connection.clone();
                    let variables = self.resolved_variables();
                    return Task::perform(
                        async move {
                            let form = SqlForm {
                                sql: sql.clone(),
                                connection,
                                variables,
                                tab_id: None,
                                query_name: None,
                                query_folder: None,
                                run_source: None,
                                cron_task_id: None,
                                cron_task_name: None,
                                alert: None,
                            };
                            let execution = engine::execute_sql(form, &engine).await;
                            let created_at = engine::now_isoish();
                            let record = SqlRunHistoryRecord {
                                id: history_record_id(),
                                connection: history_connection,
                                tab_id: String::new(),
                                sql,
                                query_name: String::new(),
                                query_folder: String::new(),
                                run_source: "manual".to_string(),
                                cron_task_id: String::new(),
                                cron_task_name: String::new(),
                                status: if execution.error.is_some() {
                                    "error".to_string()
                                } else {
                                    "completed".to_string()
                                },
                                created_at: created_at.clone(),
                                completed_at: Some(created_at),
                                row_count_text: Some(format!("{} rows", execution.rows.len())),
                                result_json: None,
                                error: execution.error.clone(),
                                alert_triggered: false,
                                alert_message: None,
                            };
                            if let Err(err) = app_db::upsert_sql_run_history(&record).await {
                                eprintln!("Failed to persist SQL run history: {err}");
                            }
                            execution
                        },
                        |execution| SqlMessage::QueryFinished(Arc::new(execution)),
                    );
                }
            }
            SqlMessage::QueryFinished(execution) => {
                self.running = false;
                if let Some(err) = &execution.error {
                    self.last_error = Some(err.clone());
                    self.grid = ResultsGrid::new(Vec::new(), Vec::new());
                } else {
                    self.last_error = None;
                    self.grid = ResultsGrid::new(execution.headers.clone(), execution.rows.clone());
                }
                if let Some(connection) = self.selected_connection.clone() {
                    return reload_history(connection);
                }
            }
            SqlMessage::Grid(grid_message) => {
                if let GridMessage::CellClicked(display_row_index, col_index) = grid_message {
                    if let Some(row_index) = self.grid.original_row_index(display_row_index) {
                        if let Some(task) = self.handle_cell_clicked(row_index, col_index) {
                            return task;
                        }
                        if self.browse.is_some() {
                            return self.update(SqlMessage::StartEditRow(row_index));
                        }
                    }
                    return Task::none();
                }
                return self.grid.update(grid_message).map(SqlMessage::Grid);
            }

            SqlMessage::NewConnNickname(v) => self.new_conn_nickname = v,
            SqlMessage::NewConnDbType(v) => self.new_conn_db_type = v,
            SqlMessage::NewConnHost(v) => self.new_conn_host = v,
            SqlMessage::NewConnDbName(v) => self.new_conn_db_name = v,
            SqlMessage::NewConnUser(v) => self.new_conn_user = v,
            SqlMessage::NewConnPassword(v) => self.new_conn_password = v,
            SqlMessage::AddConnectionPressed => {
                if !self.new_conn_nickname.trim().is_empty() {
                    let form = AddConnForm {
                        db_type: Some(self.new_conn_db_type.clone()),
                        host: self.new_conn_host.clone(),
                        db_name: self.new_conn_db_name.clone(),
                        user: self.new_conn_user.clone(),
                        password: self.new_conn_password.clone(),
                        nickname: self.new_conn_nickname.clone(),
                    };
                    engine::add_connection(&self.engine, form);
                    self.new_conn_nickname.clear();
                    self.new_conn_host.clear();
                    self.new_conn_db_name.clear();
                    self.new_conn_user.clear();
                    self.new_conn_password.clear();
                    return reload_connections(self.engine.clone());
                }
            }
            SqlMessage::DeleteConnection(nickname) => {
                engine::delete_connection(&self.engine, &nickname);
                if self.selected_connection.as_deref() == Some(nickname.as_str()) {
                    self.selected_connection = None;
                }
                return reload_connections(self.engine.clone());
            }
            SqlMessage::ConnectionsReloaded(connections) => self.connections = connections,

            SqlMessage::EditConnectionStart(nickname) => {
                if let Some(conn) = self.connections.iter().find(|c| c.nickname == nickname) {
                    self.editing_connection = Some(EditConnDraft {
                        original_nickname: conn.nickname.clone(),
                        db_type: conn.db_type.clone(),
                        host: conn.host.clone(),
                        db_name: conn.db_name.clone(),
                        user: conn.user.clone(),
                        password: String::new(),
                        nickname: conn.nickname.clone(),
                    });
                }
            }
            SqlMessage::EditConnDbTypeChanged(v) => {
                if let Some(draft) = &mut self.editing_connection {
                    draft.db_type = v;
                }
            }
            SqlMessage::EditConnHostChanged(v) => {
                if let Some(draft) = &mut self.editing_connection {
                    draft.host = v;
                }
            }
            SqlMessage::EditConnDbNameChanged(v) => {
                if let Some(draft) = &mut self.editing_connection {
                    draft.db_name = v;
                }
            }
            SqlMessage::EditConnUserChanged(v) => {
                if let Some(draft) = &mut self.editing_connection {
                    draft.user = v;
                }
            }
            SqlMessage::EditConnPasswordChanged(v) => {
                if let Some(draft) = &mut self.editing_connection {
                    draft.password = v;
                }
            }
            SqlMessage::EditConnNicknameChanged(v) => {
                if let Some(draft) = &mut self.editing_connection {
                    draft.nickname = v;
                }
            }
            SqlMessage::EditConnectionCancel => self.editing_connection = None,
            SqlMessage::EditConnectionSave => {
                if let Some(draft) = self.editing_connection.take() {
                    let form = EditConnectionForm {
                        original_nickname: draft.original_nickname,
                        db_type: Some(draft.db_type),
                        host: draft.host,
                        db_name: draft.db_name,
                        user: draft.user,
                        password: draft.password,
                        nickname: draft.nickname,
                    };
                    let _ = engine::update_connection(&self.engine, form);
                    return reload_connections(self.engine.clone());
                }
            }

            SqlMessage::CreateSqliteDbPressed => {
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            rfd::FileDialog::new()
                                .add_filter("SQLite database", &["sqlite", "db"])
                                .set_file_name("new.sqlite")
                                .save_file()
                        })
                        .await
                        .ok()
                        .flatten()
                        .map(|path| path.display().to_string())
                    },
                    SqlMessage::SqliteDbPathChosen,
                );
            }
            SqlMessage::SqliteDbPathChosen(path) => {
                let Some(path) = path else {
                    self.sqlite_create_status = Some("Cancelled.".to_string());
                    return Task::none();
                };
                let nickname = std::path::Path::new(&path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or("New SQLite DB")
                    .to_string();
                let form = AddConnForm {
                    db_type: Some("sqlite".to_string()),
                    host: path,
                    db_name: String::new(),
                    user: String::new(),
                    password: String::new(),
                    nickname,
                };
                engine::add_connection(&self.engine, form);
                self.sqlite_create_status = None;
                return reload_connections(self.engine.clone());
            }

            SqlMessage::GoToLanding => self.view_mode = SqlViewMode::ConnectionLanding,

            SqlMessage::SaveQueryNameChanged(v) => self.save_query_name = v,
            SqlMessage::SaveQueryFolderChanged(v) => self.save_query_folder = v,
            SqlMessage::SaveQueryPressed => {
                if let Some(connection) = self.selected_connection.clone() {
                    if !self.save_query_name.trim().is_empty() {
                        let name = self.save_query_name.clone();
                        let sql = self.editor.text();
                        let folder = self.save_query_folder.clone();
                        self.save_query_name.clear();
                        return Task::perform(
                            async move {
                                let folder_arg = if folder.trim().is_empty() {
                                    None
                                } else {
                                    Some(folder.as_str())
                                };
                                let _ = engine::save_query(&connection, &name, &sql, folder_arg).await;
                                engine::list_saved_queries(&connection).await
                            },
                            SqlMessage::SavedQueriesReloaded,
                        );
                    }
                }
            }
            SqlMessage::SavedQueriesReloaded(queries) => self.saved_queries = queries,
            SqlMessage::SavedFoldersReloaded(folders) => self.saved_folders = folders,
            SqlMessage::LoadSavedQuery(sql) => self.editor = text_editor::Content::with_text(&sql),
            SqlMessage::QueryFilterChanged(v) => self.query_filter = v,
            SqlMessage::ToggleFolderExpanded(folder) => {
                if !self.expanded_folders.remove(&folder) {
                    self.expanded_folders.insert(folder);
                }
            }
            SqlMessage::NewFolderNameChanged(v) => self.new_folder_name = v,
            SqlMessage::CreateFolderPressed => {
                if let Some(connection) = self.selected_connection.clone() {
                    if !self.new_folder_name.trim().is_empty() {
                        let folder = self.new_folder_name.clone();
                        self.new_folder_name.clear();
                        return reload_folders_after(connection.clone(), async move {
                            let _ = engine::create_query_folder(&connection, &folder).await;
                        });
                    }
                }
            }
            SqlMessage::DeleteFolderPressed(folder) => {
                if let Some(connection) = self.selected_connection.clone() {
                    return reload_saved_after(connection.clone(), async move {
                        let _ = engine::delete_query_folder(&connection, &folder).await;
                    });
                }
            }
            SqlMessage::RenameQueryStart(name) => {
                self.renaming_query = Some((name.clone(), name));
            }
            SqlMessage::RenameQueryFieldChanged(v) => {
                if let Some((_, new_name)) = &mut self.renaming_query {
                    *new_name = v;
                }
            }
            SqlMessage::RenameQueryCancel => self.renaming_query = None,
            SqlMessage::RenameQueryConfirm(original) => {
                if let (Some(connection), Some((_, new_name))) =
                    (self.selected_connection.clone(), self.renaming_query.take())
                {
                    return reload_saved_after(connection.clone(), async move {
                        let _ = engine::rename_query(&connection, &original, &new_name).await;
                    });
                }
            }
            SqlMessage::DeleteQueryPressed(name) => {
                if let Some(connection) = self.selected_connection.clone() {
                    return reload_saved_after(connection.clone(), async move {
                        let _ = engine::delete_query(&connection, &name).await;
                    });
                }
            }
            SqlMessage::MoveQueryFieldChanged(name, folder) => {
                self.move_target.insert(name, folder);
            }
            SqlMessage::MoveQueryPressed(name) => {
                if let Some(connection) = self.selected_connection.clone() {
                    let folder = self.move_target.remove(&name).unwrap_or_default();
                    return reload_saved_after(connection.clone(), async move {
                        let folder_arg = if folder.trim().is_empty() {
                            None
                        } else {
                            Some(folder.as_str())
                        };
                        let _ = engine::move_query(&connection, &name, folder_arg).await;
                    });
                }
            }

            SqlMessage::ExportQueriesPressed => {
                if let Some(connection) = self.selected_connection.clone() {
                    return Task::perform(
                        async move {
                            let export = engine::export_queries(&connection).await;
                            let json = serde_json::to_string_pretty(&export)
                                .map_err(|err| format!("Failed to serialize queries: {err}"))?;
                            let path = tokio::task::spawn_blocking(move || {
                                rfd::FileDialog::new()
                                    .set_file_name("saved_queries.json")
                                    .add_filter("JSON", &["json"])
                                    .save_file()
                            })
                            .await
                            .map_err(|err| format!("File dialog task failed: {err}"))?;
                            let Some(path) = path else {
                                return Err("Export cancelled.".to_string());
                            };
                            std::fs::write(&path, json)
                                .map_err(|err| format!("Failed to write file: {err}"))?;
                            Ok(format!("Exported to {}", path.display()))
                        },
                        SqlMessage::ImportExportFinished,
                    );
                }
            }
            SqlMessage::ImportQueriesPressed => {
                if let Some(connection) = self.selected_connection.clone() {
                    return Task::perform(
                        async move { pick_and_import_queries(connection).await },
                        SqlMessage::ImportExportFinished,
                    );
                }
            }
            SqlMessage::FileDroppedForImport(path) => {
                if let Some(connection) = self.selected_connection.clone() {
                    return Task::perform(
                        async move { import_queries_from_path(connection, path).await },
                        SqlMessage::ImportExportFinished,
                    );
                }
            }
            SqlMessage::ImportExportFinished(result) => {
                let connection = self.selected_connection.clone();
                self.import_export_status = Some(result.clone());
                if result.is_ok() {
                    if let Some(connection) = connection {
                        return Task::batch([
                            reload_saved_queries(connection.clone()),
                            reload_saved_folders(connection),
                        ]);
                    }
                }
            }

            SqlMessage::HistoryReloaded(history) => self.history = history,
            SqlMessage::HistoryFilterChanged(v) => self.history_filter = v,
            SqlMessage::LoadHistoryEntry(id) => {
                return Task::perform(
                    async move { app_db::get_sql_run_history_by_id(&id).await },
                    SqlMessage::HistoryEntryLoaded,
                );
            }
            SqlMessage::HistoryEntryLoaded(Some(record)) => {
                self.editor = text_editor::Content::with_text(&record.sql);
            }
            SqlMessage::HistoryEntryLoaded(None) => {}
            SqlMessage::DeleteHistoryEntry(id) => {
                if let Some(connection) = self.selected_connection.clone() {
                    return Task::perform(
                        async move {
                            let _ = app_db::delete_sql_run_history(&id).await;
                            app_db::get_sql_run_history_summaries(&connection, None, HISTORY_LIMIT).await
                        },
                        SqlMessage::HistoryReloaded,
                    );
                }
            }
            SqlMessage::ClearHistoryPressed => {
                if let Some(connection) = self.selected_connection.clone() {
                    return Task::perform(
                        async move {
                            let _ = app_db::clear_sql_run_history(&connection, None).await;
                            app_db::get_sql_run_history_summaries(&connection, None, HISTORY_LIMIT).await
                        },
                        SqlMessage::HistoryReloaded,
                    );
                }
            }

            SqlMessage::SplitDragStart => {
                self.resizing = true;
                self.drag_last_y = None;
            }
            SqlMessage::SplitCursorMoved(y) => {
                if self.resizing {
                    if let Some(last_y) = self.drag_last_y {
                        let delta = (y - last_y) / SPLIT_REFERENCE_HEIGHT;
                        self.split_ratio = (self.split_ratio + delta).clamp(0.15, 0.85);
                    }
                    self.drag_last_y = Some(y);
                }
            }
            SqlMessage::SplitDragEnd => {
                self.resizing = false;
                self.drag_last_y = None;
            }

            SqlMessage::RefreshSchema => {
                if let Some(connection) = self.selected_connection.clone() {
                    return Task::batch([
                        reload_schema(self.engine.clone(), connection.clone()),
                        reload_functions(self.engine.clone(), connection),
                    ]);
                }
            }
            SqlMessage::SchemaLoaded(Ok(schema)) => {
                self.schema = Some(schema);
                self.schema_error = None;
            }
            SqlMessage::SchemaLoaded(Err(err)) => {
                self.schema = None;
                self.schema_error = Some(err);
            }
            SqlMessage::FunctionsLoaded(Ok(functions)) => {
                self.functions = Some(functions);
                self.function_error = None;
            }
            SqlMessage::FunctionsLoaded(Err(err)) => {
                self.functions = None;
                self.function_error = Some(err);
            }
            SqlMessage::LoadFunctionDefinition(signature) => {
                if let Some(functions) = &self.functions {
                    if let Some(function) = functions.iter().find(|f| f.signature == signature) {
                        self.editor = text_editor::Content::with_text(&function.definition);
                    }
                }
            }
            SqlMessage::ToggleTableExpanded(table) => {
                if !self.expanded_tables.remove(&table) {
                    self.expanded_tables.insert(table);
                }
            }
            SqlMessage::BrowseTable(table) => {
                self.editing_row = None;
                self.new_row = None;
                self.last_error = None;
                self.browse = Some(BrowseState {
                    table: table.clone(),
                    page: 1,
                    has_next: false,
                    filter: None,
                });
                if let Some(connection) = self.selected_connection.clone() {
                    return browse_task(self.engine.clone(), connection, table, 1, None);
                }
            }
            SqlMessage::BrowseFiltered(table, column, value) => {
                self.editing_row = None;
                self.new_row = None;
                self.last_error = None;
                let filter = TableBrowseFilter {
                    column: Some(column),
                    op: "eq".to_string(),
                    value: Some(value),
                };
                self.browse = Some(BrowseState {
                    table: table.clone(),
                    page: 1,
                    has_next: false,
                    filter: Some(filter.clone()),
                });
                if let Some(connection) = self.selected_connection.clone() {
                    return browse_task(self.engine.clone(), connection, table, 1, Some(filter));
                }
            }
            SqlMessage::BrowseResultLoaded(Ok(result)) => {
                self.last_error = None;
                if let Some(browse) = &mut self.browse {
                    browse.page = result.page;
                    browse.has_next = result.has_next;
                }
                let fk_columns = self.current_table_fk_columns();
                self.grid =
                    ResultsGrid::with_fk_columns(result.headers, result.rows, &fk_columns);
            }
            SqlMessage::BrowseResultLoaded(Err(err)) => {
                self.last_error = Some(err);
            }
            SqlMessage::NextPage => {
                if let Some(browse) = &self.browse {
                    if browse.has_next {
                        let next_page = browse.page + 1;
                        if let Some(connection) = self.selected_connection.clone() {
                            return browse_task(
                                self.engine.clone(),
                                connection,
                                browse.table.clone(),
                                next_page,
                                browse.filter.clone(),
                            );
                        }
                    }
                }
            }
            SqlMessage::PrevPage => {
                if let Some(browse) = &self.browse {
                    if browse.page > 1 {
                        let prev_page = browse.page - 1;
                        if let Some(connection) = self.selected_connection.clone() {
                            return browse_task(
                                self.engine.clone(),
                                connection,
                                browse.table.clone(),
                                prev_page,
                                browse.filter.clone(),
                            );
                        }
                    }
                }
            }

            SqlMessage::StartEditRow(row_index) => {
                if let Some(row) = self.grid.rows.get(row_index) {
                    let mut values = HashMap::new();
                    for (col, val) in self.grid.columns.iter().zip(row.iter()) {
                        values.insert(col.title.clone(), val.clone());
                    }
                    self.editing_row = Some((row_index, values));
                    self.new_row = None;
                }
            }
            SqlMessage::EditFieldChanged(column, value) => {
                if let Some((_, values)) = &mut self.editing_row {
                    values.insert(column, value);
                }
            }
            SqlMessage::CancelRowEdit => self.editing_row = None,
            SqlMessage::SaveRowEdit => {
                if let (Some((row_index, current)), Some(browse), Some(connection)) = (
                    self.editing_row.clone(),
                    &self.browse,
                    self.selected_connection.clone(),
                ) {
                    if let Some(row) = self.grid.rows.get(row_index) {
                        let mut original = HashMap::new();
                        for (col, val) in self.grid.columns.iter().zip(row.iter()) {
                            original.insert(col.title.clone(), val.clone());
                        }
                        let table = browse.table.clone();
                        let page = browse.page;
                        let filter = browse.filter.clone();
                        let engine = self.engine.clone();
                        self.editing_row = None;
                        return Task::perform(
                            async move {
                                let change = TableUpdateChange { original, current };
                                let write =
                                    engine::update_table_rows(&engine, &connection, None, &table, &[change])
                                        .await;
                                let write: Result<TableWriteResult, String> = write;
                                if let Err(err) = write {
                                    return Err(err);
                                }
                                let filters = filter.map(|f| vec![f]).unwrap_or_default();
                                engine::browse_table(
                                    &engine,
                                    &connection,
                                    None,
                                    &table,
                                    Some(page),
                                    None,
                                    &filters,
                                )
                                .await
                            },
                            SqlMessage::BrowseResultLoaded,
                        );
                    }
                }
            }

            SqlMessage::StartAddRow => {
                let mut values = HashMap::new();
                for col in &self.grid.columns {
                    values.insert(col.title.clone(), String::new());
                }
                self.new_row = Some(values);
                self.editing_row = None;
            }
            SqlMessage::NewRowFieldChanged(column, value) => {
                if let Some(values) = &mut self.new_row {
                    values.insert(column, value);
                }
            }
            SqlMessage::CancelAddRow => self.new_row = None,
            SqlMessage::SaveNewRow => {
                if let (Some(values), Some(browse), Some(connection)) = (
                    self.new_row.clone(),
                    &self.browse,
                    self.selected_connection.clone(),
                ) {
                    let table = browse.table.clone();
                    let page = browse.page;
                    let filter = browse.filter.clone();
                    let engine = self.engine.clone();
                    self.new_row = None;
                    return Task::perform(
                        async move {
                            let write =
                                engine::insert_table_rows(&engine, &connection, None, &table, &[values])
                                    .await;
                            let write: Result<TableWriteResult, String> = write;
                            if let Err(err) = write {
                                return Err(err);
                            }
                            let filters = filter.map(|f| vec![f]).unwrap_or_default();
                            engine::browse_table(
                                &engine,
                                &connection,
                                None,
                                &table,
                                Some(page),
                                None,
                                &filters,
                            )
                            .await
                        },
                        SqlMessage::BrowseResultLoaded,
                    );
                }
            }

            SqlMessage::VariableValueChanged(name, value) => {
                if let Some((raw, _)) = self.variables.get_mut(&name) {
                    *raw = value;
                }
            }
            SqlMessage::VariableFormatChanged(name, mode) => {
                if let Some((_, format)) = self.variables.get_mut(&name) {
                    *format = mode;
                }
            }

            SqlMessage::AutocompleteSelected(name) => {
                self.editor.perform(text_editor::Action::Select(text_editor::Motion::WordLeft));
                self.editor.perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                    std::sync::Arc::new(name),
                )));
                self.autocomplete.clear();
                self.sync_variables();
            }

            SqlMessage::ToggleFind => {
                self.find_open = !self.find_open;
                if self.find_open {
                    self.recompute_find_matches();
                } else {
                    self.find_term.clear();
                    self.find_matches.clear();
                    self.find_current = 0;
                }
            }
            SqlMessage::FindTermChanged(v) => {
                self.find_term = v;
                self.find_current = 0;
                self.recompute_find_matches();
            }
            SqlMessage::FindNext => {
                if !self.find_matches.is_empty() {
                    self.find_current = (self.find_current + 1) % self.find_matches.len();
                }
            }
            SqlMessage::FindPrev => {
                if !self.find_matches.is_empty() {
                    self.find_current =
                        (self.find_current + self.find_matches.len() - 1) % self.find_matches.len();
                }
            }
            SqlMessage::CloseFind => {
                self.find_open = false;
                self.find_term.clear();
                self.find_matches.clear();
                self.find_current = 0;
            }

            SqlMessage::ToggleErdView => {
                self.view_mode = match self.view_mode {
                    SqlViewMode::QueryEditor => SqlViewMode::RelationshipDiagram,
                    SqlViewMode::RelationshipDiagram | SqlViewMode::ConnectionLanding => {
                        SqlViewMode::QueryEditor
                    }
                };
            }
            SqlMessage::ErdFilterChanged(v) => self.erd_filter = v,
            SqlMessage::Erd(ErdMessage::TableClicked(table)) => {
                self.erd_highlighted = Some(table);
            }
            SqlMessage::Erd(ErdMessage::FkColumnClicked(table, _column)) => {
                // Clicking an FK column in the diagram (not a specific row's
                // cell, which is Phase 2's grid FK-navigation) has no value
                // to filter on — just switch to browsing the referenced
                // table, mirroring "jump to this table" behavior.
                self.erd_highlighted = Some(table.clone());
                self.view_mode = SqlViewMode::QueryEditor;
                return Task::done(SqlMessage::BrowseTable(table));
            }

            SqlMessage::ExportCsvPressed => {
                let csv = build_csv(
                    &self.grid.columns.iter().map(|c| c.title.clone()).collect::<Vec<_>>(),
                    &self.grid.rows,
                );
                return Task::perform(
                    async move {
                        let path = tokio::task::spawn_blocking(move || {
                            rfd::FileDialog::new()
                                .set_file_name("results.csv")
                                .add_filter("CSV", &["csv"])
                                .save_file()
                        })
                        .await
                        .map_err(|err| format!("File dialog task failed: {err}"))?;
                        let Some(path) = path else {
                            return Err("Export cancelled.".to_string());
                        };
                        std::fs::write(&path, csv)
                            .map_err(|err| format!("Failed to write CSV: {err}"))?;
                        Ok(path.display().to_string())
                    },
                    SqlMessage::ExportCsvFinished,
                );
            }
            SqlMessage::ExportCsvFinished(result) => {
                self.csv_status = Some(result);
            }

            SqlMessage::TimezoneLoaded(result) => self.timezone = Some(result),

            SqlMessage::CronTasksLoaded(values) => {
                self.cron_tasks = values.iter().filter_map(CronTask::from_value).collect();
            }
            SqlMessage::CronNameChanged(v) => self.cron_name = v,
            SqlMessage::CronSqlAction(action) => self.cron_sql.perform(action),
            SqlMessage::CronIntervalValueChanged(v) => self.cron_interval_value = v,
            SqlMessage::CronIntervalUnitChanged(v) => self.cron_interval_unit = v,
            SqlMessage::CronAlertComparatorChanged(v) => self.cron_alert_comparator = v,
            SqlMessage::CronAlertValueChanged(v) => self.cron_alert_value = v,
            SqlMessage::CreateCronTaskPressed => {
                if let Some(connection) = self.selected_connection.clone() {
                    let interval_units: f64 = self.cron_interval_value.trim().parse().unwrap_or(0.0);
                    let interval_ms = match self.cron_interval_unit.as_str() {
                        "hours" => (interval_units * 3_600_000.0) as i64,
                        _ => (interval_units * 60_000.0) as i64,
                    };
                    if !self.cron_name.trim().is_empty() && interval_ms > 0 {
                        let alert = self.cron_alert_value.trim().parse::<i64>().ok().map(|value| {
                            SqlAlertRule {
                                comparator: self.cron_alert_comparator.clone(),
                                value,
                            }
                        });
                        let task = CronTask {
                            id: format!("cron-{}-{}", now_ms(), self.cron_tasks.len()),
                            name: self.cron_name.clone(),
                            sql: self.cron_sql.text(),
                            connection,
                            interval_ms,
                            alert,
                            enabled: true,
                            last_run_ms: 0,
                        };
                        self.cron_tasks.push(task);
                        self.cron_name.clear();
                        self.cron_sql = text_editor::Content::new();
                        self.cron_interval_value.clear();
                        self.cron_alert_value.clear();
                        return save_cron_tasks(self.cron_tasks.clone());
                    }
                }
            }
            SqlMessage::DeleteCronTask(id) => {
                self.cron_tasks.retain(|t| t.id != id);
                return save_cron_tasks(self.cron_tasks.clone());
            }
            SqlMessage::ToggleCronEnabled(id) => {
                if let Some(task) = self.cron_tasks.iter_mut().find(|t| t.id == id) {
                    task.enabled = !task.enabled;
                }
                return save_cron_tasks(self.cron_tasks.clone());
            }
            SqlMessage::RunCronNow(id) => {
                if let Some(task) = self.cron_tasks.iter().find(|t| t.id == id).cloned() {
                    return run_cron_task(self.engine.clone(), task);
                }
            }
            SqlMessage::CronTick => {
                let now = now_ms();
                let due: Vec<CronTask> = self.cron_tasks.iter().filter(|t| t.is_due(now)).cloned().collect();
                if due.is_empty() {
                    return Task::none();
                }
                for task in &due {
                    if let Some(t) = self.cron_tasks.iter_mut().find(|t| t.id == task.id) {
                        t.last_run_ms = now;
                    }
                }
                let save = save_cron_tasks(self.cron_tasks.clone());
                let runs = Task::batch(
                    due.into_iter().map(|task| run_cron_task(self.engine.clone(), task)),
                );
                return Task::batch([save, runs]);
            }
            SqlMessage::CronRunFinished(task_id, execution) => {
                let name = self
                    .cron_tasks
                    .iter()
                    .find(|t| t.id == task_id)
                    .map(|t| t.name.clone())
                    .unwrap_or(task_id);
                let summary = if let Some(err) = &execution.error {
                    format!("{name}: error - {err}")
                } else {
                    format!("{name}: {} rows", execution.rows.len())
                };
                self.cron_activity.insert(0, summary);
                self.cron_activity.truncate(50);
            }

            SqlMessage::ToggleSchedulePanel => {
                self.schedule_panel_open = !self.schedule_panel_open;
            }
            SqlMessage::ToggleRunningQueries => {
                self.running_queries_open = !self.running_queries_open;
            }
            SqlMessage::SaveToFilePressed => {
                let sql = self.editor.text();
                return Task::perform(
                    async move {
                        let path = tokio::task::spawn_blocking(move || {
                            rfd::FileDialog::new()
                                .set_file_name("query.sql")
                                .add_filter("SQL", &["sql"])
                                .save_file()
                        })
                        .await
                        .map_err(|err| format!("File dialog task failed: {err}"))?;
                        let Some(path) = path else {
                            return Err("Save cancelled.".to_string());
                        };
                        std::fs::write(&path, sql)
                            .map_err(|err| format!("Failed to write file: {err}"))?;
                        Ok(path.display().to_string())
                    },
                    SqlMessage::SaveToFileFinished,
                );
            }
            SqlMessage::SaveToFileFinished(result) => {
                self.save_to_file_status = Some(result);
            }
        }
        Task::none()
    }

    /// Rescans the editor text for `{{name}}` tokens, adding/removing
    /// entries in `self.variables` to match what's currently present
    /// while preserving already-entered values for names still in use.
    fn sync_variables(&mut self) {
        let names = sql_highlighter::scan_variable_names(&self.editor.text());
        self.variables.retain(|name, _| names.contains(name));
        for name in &names {
            self.variables
                .entry(name.clone())
                .or_insert_with(|| (String::new(), VariableFormat::Raw));
        }
        self.variable_order = names;
    }

    /// Builds the final `{{name}} -> value` map to send with the query,
    /// applying each variable's selected format transform.
    fn resolved_variables(&self) -> Option<HashMap<String, String>> {
        if self.variables.is_empty() {
            return None;
        }
        Some(
            self.variables
                .iter()
                .map(|(name, (raw, mode))| {
                    (name.clone(), sql_highlighter::format_variable_value(raw, *mode))
                })
                .collect(),
        )
    }

    /// Recomputes autocomplete suggestions for the word currently being
    /// typed at the cursor, against known table names from the cached
    /// schema for the selected connection.
    fn update_autocomplete(&mut self) {
        self.autocomplete.clear();
        let Some(schema) = &self.schema else {
            return;
        };
        let (line_index, column) = self.editor.cursor_position();
        let Some(line) = self.editor.line(line_index) else {
            return;
        };
        let line: &str = &line;
        let column = column.min(line.len());
        let prefix_start = line[..column]
            .rfind(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|i| i + 1)
            .unwrap_or(0);
        let word = &line[prefix_start..column];
        if word.is_empty() {
            return;
        }
        let word_lower = word.to_lowercase();
        for table in &schema.tables {
            let table_lower = table.name.to_lowercase();
            if table_lower.starts_with(&word_lower) && table_lower != word_lower {
                self.autocomplete.push(table.name.clone());
            }
        }
        self.autocomplete.truncate(8);
    }

    /// Recomputes the flat list of `(line, byte_start)` matches for the
    /// current find term across the whole document.
    fn recompute_find_matches(&mut self) {
        self.find_matches.clear();
        if self.find_term.is_empty() {
            return;
        }
        for (line_index, line) in self.editor.lines().enumerate() {
            for range in sql_highlighter::find_search_matches(&line, &self.find_term) {
                self.find_matches.push((line_index, range.start));
            }
        }
        if self.find_current >= self.find_matches.len() {
            self.find_current = 0;
        }
    }

    /// Connection manager landing screen (App Shell Phase 3): list/add/
    /// edit/delete saved connections, "Create SQLite database", and
    /// "Open" to select + switch into the query editor. See the
    /// `SqlViewMode::ConnectionLanding` doc comment for the scoping note
    /// on why "Open" doesn't yet spawn a real per-connection tab group.
    fn view_connection_landing(&self) -> Element<'_, SqlMessage> {
        let mut list = column![text("Saved connections").size(20)].spacing(8);

        if self.connections.is_empty() {
            list = list.push(text("No saved connections yet.").size(13));
        }

        for conn in &self.connections {
            if let Some(draft) = &self.editing_connection {
                if draft.original_nickname == conn.nickname {
                    list = list.push(self.view_edit_connection_form(draft));
                    continue;
                }
            }
            list = list.push(
                row![
                    text(format!("{} ({})", conn.nickname, conn.db_type)).width(Length::Fixed(240.0)),
                    button(text("Open")).on_press(SqlMessage::ConnectionSelected(conn.nickname.clone())),
                    button(text("Edit")).on_press(SqlMessage::EditConnectionStart(conn.nickname.clone())),
                    button(text("Delete")).on_press(SqlMessage::DeleteConnection(conn.nickname.clone())),
                ]
                .spacing(8),
            );
        }

        let add_connection_form = column![
            text("Add connection").size(16),
            text_input("nickname", &self.new_conn_nickname).on_input(SqlMessage::NewConnNickname),
            row![
                button(text("postgres")).on_press(SqlMessage::NewConnDbType("postgres".into())),
                button(text("sqlite")).on_press(SqlMessage::NewConnDbType("sqlite".into())),
                text(format!("selected: {}", self.new_conn_db_type)),
            ]
            .spacing(6),
            text_input("host / sqlite path", &self.new_conn_host).on_input(SqlMessage::NewConnHost),
            text_input("db name", &self.new_conn_db_name).on_input(SqlMessage::NewConnDbName),
            text_input("user", &self.new_conn_user).on_input(SqlMessage::NewConnUser),
            text_input("password", &self.new_conn_password)
                .on_input(SqlMessage::NewConnPassword)
                .secure(true),
            button(text("+ Add connection")).on_press(SqlMessage::AddConnectionPressed),
        ]
        .spacing(6);

        let sqlite_status: Element<'_, SqlMessage> = match &self.sqlite_create_status {
            Some(status) => text(status.clone()).size(12).into(),
            None => text("").size(12).into(),
        };
        let create_sqlite = column![
            text("Create SQLite database").size(16),
            text("Pick a path for a brand-new .sqlite file — it's created on first connect.").size(12),
            button(text("Create SQLite database...")).on_press(SqlMessage::CreateSqliteDbPressed),
            sqlite_status,
        ]
        .spacing(6);

        scrollable(
            column![list, row![add_connection_form, create_sqlite].spacing(32)]
                .spacing(24)
                .padding(16),
        )
        .into()
    }

    fn view_edit_connection_form(&self, draft: &EditConnDraft) -> Element<'_, SqlMessage> {
        column![
            text(format!("Editing: {}", draft.original_nickname)).size(14),
            row![
                button(text("postgres")).on_press(SqlMessage::EditConnDbTypeChanged("postgres".into())),
                button(text("sqlite")).on_press(SqlMessage::EditConnDbTypeChanged("sqlite".into())),
                text(format!("selected: {}", draft.db_type)),
            ]
            .spacing(6),
            text_input("nickname", &draft.nickname).on_input(SqlMessage::EditConnNicknameChanged),
            text_input("host / sqlite path", &draft.host).on_input(SqlMessage::EditConnHostChanged),
            text_input("db name", &draft.db_name).on_input(SqlMessage::EditConnDbNameChanged),
            text_input("user", &draft.user).on_input(SqlMessage::EditConnUserChanged),
            text_input("password (leave blank to keep existing)", &draft.password)
                .on_input(SqlMessage::EditConnPasswordChanged)
                .secure(true),
            row![
                button(text("Save")).on_press(SqlMessage::EditConnectionSave),
                button(text("Cancel")).on_press(SqlMessage::EditConnectionCancel),
            ]
            .spacing(8),
        ]
        .spacing(6)
        .into()
    }

    /// Returns `Some(task)` if the click was on an FK cell and should
    /// navigate to the referenced table instead of the default grid
    /// handling (sort etc). Returns `None` for a plain-cell click.
    fn view_schema_tree(&self) -> Element<'_, SqlMessage> {
        let diagram_label = match self.view_mode {
            SqlViewMode::QueryEditor | SqlViewMode::ConnectionLanding => "Open relationships",
            SqlViewMode::RelationshipDiagram => "Back to editor",
        };
        let mut col = column![row![
            text("Tables & Functions").size(16),
            button(text("Refresh")).on_press(SqlMessage::RefreshSchema),
            button(text(diagram_label)).on_press(SqlMessage::ToggleErdView),
        ]
        .spacing(8)]
        .spacing(4);

        if let Some(err) = &self.schema_error {
            col = col.push(text(err.clone()).color(iced::Color::from_rgb8(0xe0, 0x5a, 0x5a)).size(12));
        }

        let Some(schema) = &self.schema else {
            return col.push(text("Select a connection to load schema").size(12)).into();
        };

        for table in &schema.tables {
            let expanded = self.expanded_tables.contains(&table.name);
            let arrow = if expanded { "v" } else { ">" };
            col = col.push(
                row![
                    button(text(format!("{arrow} {}", table.name)))
                        .on_press(SqlMessage::ToggleTableExpanded(table.name.clone())),
                    button(text("browse")).on_press(SqlMessage::BrowseTable(table.name.clone())),
                ]
                .spacing(6),
            );
            if expanded {
                for column_info in &table.columns {
                    let is_fk = schema
                        .relationships
                        .iter()
                        .any(|rel| rel.from_table == table.name && rel.from_column == column_info.name);
                    let mut label = format!("    {}: {}", column_info.name, column_info.data_type);
                    if column_info.primary_key {
                        label.push_str(" [PK]");
                    }
                    if is_fk {
                        label.push_str(" [FK]");
                    }
                    col = col.push(text(label).size(12));
                }
            }
        }

        col = col.push(text("Functions").size(14));
        if let Some(err) = &self.function_error {
            col = col.push(text(err.clone()).color(iced::Color::from_rgb8(0xe0, 0x5a, 0x5a)).size(12));
        } else {
            match &self.functions {
                None => col = col.push(text("Loading...").size(12)),
                Some(functions) if functions.is_empty() => {
                    // Empty is expected/normal for SQLite (Postgres-only
                    // feature, see `fetch_function_list`), not an error.
                    col = col.push(text("No functions").size(12));
                }
                Some(functions) => {
                    for function in functions {
                        col = col.push(
                            button(text(function.signature.clone()).size(12))
                                .on_press(SqlMessage::LoadFunctionDefinition(function.signature.clone())),
                        );
                    }
                }
            }
        }

        col.into()
    }

    fn view_saved_queries_tree(&self) -> Element<'_, SqlMessage> {
        let filter = self.query_filter.to_lowercase();
        let matches = |name: &str| filter.is_empty() || name.to_lowercase().contains(&filter);

        let mut col = column![
            row![
                text("Saved queries").size(16),
                button(text("Import")).on_press(SqlMessage::ImportQueriesPressed),
                button(text("Export")).on_press(SqlMessage::ExportQueriesPressed),
            ]
            .spacing(8),
            text("Drop a .sql or .json file here to import").size(11),
            text_input("filter queries", &self.query_filter).on_input(SqlMessage::QueryFilterChanged),
            row![
                text_input("new folder name", &self.new_folder_name)
                    .on_input(SqlMessage::NewFolderNameChanged),
                button(text("+ Folder")).on_press(SqlMessage::CreateFolderPressed),
            ]
            .spacing(6),
        ]
        .spacing(4);

        if let Some(status) = &self.import_export_status {
            let (message, color) = match status {
                Ok(msg) => (msg.clone(), iced::Color::from_rgb8(0x5a, 0xc0, 0x7a)),
                Err(msg) => (msg.clone(), iced::Color::from_rgb8(0xe0, 0x5a, 0x5a)),
            };
            col = col.push(text(message).size(11).color(color));
        }

        let query_row = |q: &SavedQuery| -> Element<'_, SqlMessage> {
            if let Some((original, editing_name)) = &self.renaming_query {
                if original == &q.name {
                    return row![
                        text_input("new name", editing_name)
                            .on_input(SqlMessage::RenameQueryFieldChanged),
                        button(text("OK"))
                            .on_press(SqlMessage::RenameQueryConfirm(original.clone())),
                        button(text("x")).on_press(SqlMessage::RenameQueryCancel),
                    ]
                    .spacing(4)
                    .into();
                }
            }
            let move_value = self.move_target.get(&q.name).cloned().unwrap_or_default();
            let name_for_move = q.name.clone();
            let name_for_move2 = q.name.clone();
            row![
                button(text(q.name.clone())).on_press(SqlMessage::LoadSavedQuery(q.sql.clone())),
                button(text("rename")).on_press(SqlMessage::RenameQueryStart(q.name.clone())),
                button(text("delete")).on_press(SqlMessage::DeleteQueryPressed(q.name.clone())),
                text_input("move to folder", &move_value)
                    .on_input(move |v| SqlMessage::MoveQueryFieldChanged(name_for_move.clone(), v))
                    .width(Length::Fixed(110.0)),
                button(text("Move")).on_press(SqlMessage::MoveQueryPressed(name_for_move2)),
            ]
            .spacing(4)
            .into()
        };

        // Root-level (no folder) queries matching the filter.
        for q in self.saved_queries.iter().filter(|q| q.folder.is_none() && matches(&q.name)) {
            col = col.push(query_row(q));
        }

        for folder in &self.saved_folders {
            let folder_queries: Vec<&SavedQuery> = self
                .saved_queries
                .iter()
                .filter(|q| q.folder.as_deref() == Some(folder.as_str()))
                .collect();
            let has_match = filter.is_empty() || folder_queries.iter().any(|q| matches(&q.name));
            if !has_match {
                continue;
            }
            let expanded = self.expanded_folders.contains(folder);
            let arrow = if expanded { "v" } else { ">" };
            col = col.push(
                row![
                    button(text(format!("{arrow} {folder}")))
                        .on_press(SqlMessage::ToggleFolderExpanded(folder.clone())),
                    button(text("delete folder")).on_press(SqlMessage::DeleteFolderPressed(folder.clone())),
                ]
                .spacing(6),
            );
            if expanded {
                for q in folder_queries.iter().filter(|q| matches(&q.name)) {
                    col = col.push(row![text("  "), query_row(q)].spacing(0));
                }
            }
        }

        col.into()
    }

    fn view_history_panel(&self) -> Element<'_, SqlMessage> {
        let filter = self.history_filter.to_lowercase();
        let mut col = column![
            row![
                text("Run history").size(16),
                button(text("Clear")).on_press(SqlMessage::ClearHistoryPressed),
            ]
            .spacing(8),
            text_input("filter history", &self.history_filter)
                .on_input(SqlMessage::HistoryFilterChanged),
        ]
        .spacing(4);

        for entry in &self.history {
            if !filter.is_empty()
                && !entry.sql.to_lowercase().contains(&filter)
                && !entry.query_name.to_lowercase().contains(&filter)
            {
                continue;
            }
            let status_color = if entry.status == "error" {
                iced::Color::from_rgb8(0xe0, 0x5a, 0x5a)
            } else {
                iced::Color::from_rgb8(0x5a, 0xc0, 0x7a)
            };
            let preview: String = entry.sql.chars().take(60).collect();
            col = col.push(
                row![
                    button(text(format!("[{}] {}", entry.status, preview)).color(status_color))
                        .on_press(SqlMessage::LoadHistoryEntry(entry.id.clone())),
                    text(entry.created_at.clone()).size(11),
                    button(text("delete")).on_press(SqlMessage::DeleteHistoryEntry(entry.id.clone())),
                ]
                .spacing(6),
            );
        }

        col.into()
    }

    fn view_browse_toolbar(&self) -> Element<'_, SqlMessage> {
        let Some(browse) = &self.browse else {
            return row![].into();
        };
        row![
            text(format!("Browsing: {} (page {})", browse.table, browse.page)),
            button(text("Prev")).on_press(SqlMessage::PrevPage),
            button(text("Next")).on_press(SqlMessage::NextPage),
            button(text("+ Add row")).on_press(SqlMessage::StartAddRow),
        ]
        .spacing(8)
        .into()
    }

    fn view_timezone_pill(&self) -> Element<'_, SqlMessage> {
        match &self.timezone {
            None => text("").size(11).into(),
            Some(Ok(tz)) => text(format!("TZ: {} ({})", tz.timezone, tz.utc_offset))
                .size(11)
                .color(iced::Color::from_rgb8(0x5a, 0xc0, 0x7a))
                .into(),
            Some(Err(err)) => text(format!("TZ error: {err}"))
                .size(11)
                .color(iced::Color::from_rgb8(0xe0, 0x5a, 0x5a))
                .into(),
        }
    }

    fn view_csv_export_bar(&self) -> Element<'_, SqlMessage> {
        let status: Element<'_, SqlMessage> = match &self.csv_status {
            None => text("").size(11).into(),
            Some(Ok(path)) => text(format!("Saved to {path}"))
                .size(11)
                .color(iced::Color::from_rgb8(0x5a, 0xc0, 0x7a))
                .into(),
            Some(Err(err)) => text(err.clone())
                .size(11)
                .color(iced::Color::from_rgb8(0xe0, 0x5a, 0x5a))
                .into(),
        };
        row![
            button(text("Export CSV")).on_press(SqlMessage::ExportCsvPressed),
            status,
        ]
        .spacing(8)
        .into()
    }

    /// Unified output-area toolbar: filter/columns/widen (delegated to the
    /// grid), CSV export, revert (discard in-progress inline edits), and a
    /// running-queries indicator. "Revert" only ever has something to act
    /// on today when a table-browse row edit is in progress
    /// (`editing_row`) — inline editing of arbitrary query results doesn't
    /// exist yet (`StartEditRow` is gated on `self.browse.is_some()`), so
    /// this button naturally does nothing when there's nothing to revert.
    /// "Running queries" is an honest placeholder: `RunPressed` always
    /// calls the synchronous `execute_sql` path, never `run_background`,
    /// so there is no real concurrent-jobs list to show yet — this reflects
    /// the single in-flight run via `self.running` rather than a job list.
    fn view_output_toolbar(&self) -> Element<'_, SqlMessage> {
        let grid_toolbar = self.grid.view_toolbar().map(SqlMessage::Grid);
        let csv_bar = self.view_csv_export_bar();

        let revert_button: Element<'_, SqlMessage> = if self.editing_row.is_some() {
            button(text("Revert")).on_press(SqlMessage::CancelRowEdit).into()
        } else {
            column![].into()
        };

        let running_label = if self.running {
            "Running queries (1)"
        } else {
            "Running queries"
        };
        let mut running_button = row![button(text(running_label)).on_press(SqlMessage::ToggleRunningQueries)];
        if self.running_queries_open {
            let status = if self.running {
                "A query is currently running."
            } else {
                "Nothing running."
            };
            running_button = running_button.push(text(status).size(11));
        }

        column![
            row![grid_toolbar, csv_bar, revert_button].spacing(12),
            running_button.spacing(8),
        ]
        .spacing(6)
        .into()
    }

    fn view_cron_panel(&self) -> Element<'_, SqlMessage> {
        let mut col = column![text("Scheduled queries").size(16)].spacing(4);

        for task in &self.cron_tasks {
            let enabled_label = if task.enabled { "enabled" } else { "disabled" };
            let interval_label = if task.interval_ms % 3_600_000 == 0 {
                format!("{}h", task.interval_ms / 3_600_000)
            } else {
                format!("{}m", task.interval_ms / 60_000)
            };
            col = col.push(
                row![
                    text(format!("{} ({interval_label}, {enabled_label})", task.name)).size(12),
                    button(text("Run now")).on_press(SqlMessage::RunCronNow(task.id.clone())),
                    button(text(enabled_label)).on_press(SqlMessage::ToggleCronEnabled(task.id.clone())),
                    button(text("delete")).on_press(SqlMessage::DeleteCronTask(task.id.clone())),
                ]
                .spacing(6),
            );
        }

        col = col.push(text("New scheduled query").size(13));
        col = col.push(
            text_input("name", &self.cron_name).on_input(SqlMessage::CronNameChanged),
        );
        col = col.push(
            text_editor(&self.cron_sql)
                .on_action(SqlMessage::CronSqlAction)
                .height(Length::Fixed(60.0)),
        );
        col = col.push(
            row![
                text_input("interval", &self.cron_interval_value)
                    .on_input(SqlMessage::CronIntervalValueChanged)
                    .width(Length::Fixed(60.0)),
                button(text("minutes")).on_press(SqlMessage::CronIntervalUnitChanged("minutes".to_string())),
                button(text("hours")).on_press(SqlMessage::CronIntervalUnitChanged("hours".to_string())),
                text(self.cron_interval_unit.clone()).size(11),
            ]
            .spacing(6),
        );
        col = col.push(
            row![
                text("Alert if row count").size(11),
                button(text(self.cron_alert_comparator.clone())).on_press(
                    SqlMessage::CronAlertComparatorChanged(next_comparator(&self.cron_alert_comparator))
                ),
                text_input("value", &self.cron_alert_value)
                    .on_input(SqlMessage::CronAlertValueChanged)
                    .width(Length::Fixed(60.0)),
            ]
            .spacing(6),
        );
        col = col.push(button(text("+ Create scheduled query")).on_press(SqlMessage::CreateCronTaskPressed));

        if !self.cron_activity.is_empty() {
            col = col.push(text("Recent activity").size(12));
            for line in self.cron_activity.iter().take(10) {
                col = col.push(text(line.clone()).size(11));
            }
        }

        col.into()
    }

    fn view_row_edit_form(&self) -> Element<'_, SqlMessage> {
        let Some((_, values)) = &self.editing_row else {
            return column![].into();
        };
        let mut form = column![text("Edit row").size(14)].spacing(4);
        for col in &self.grid.columns {
            let value = values.get(&col.title).cloned().unwrap_or_default();
            let title = col.title.clone();
            form = form.push(
                row![
                    text(col.title.clone()).width(Length::Fixed(140.0)),
                    text_input("", &value)
                        .on_input(move |v| SqlMessage::EditFieldChanged(title.clone(), v)),
                ]
                .spacing(8),
            );
        }
        form = form.push(
            row![
                button(text("Save")).on_press(SqlMessage::SaveRowEdit),
                button(text("Cancel")).on_press(SqlMessage::CancelRowEdit),
            ]
            .spacing(8),
        );
        form.into()
    }

    fn view_new_row_form(&self) -> Element<'_, SqlMessage> {
        let Some(values) = &self.new_row else {
            return column![].into();
        };
        let mut form = column![text("Add row").size(14)].spacing(4);
        for col in &self.grid.columns {
            let value = values.get(&col.title).cloned().unwrap_or_default();
            let title = col.title.clone();
            form = form.push(
                row![
                    text(col.title.clone()).width(Length::Fixed(140.0)),
                    text_input("", &value)
                        .on_input(move |v| SqlMessage::NewRowFieldChanged(title.clone(), v)),
                ]
                .spacing(8),
            );
        }
        form = form.push(
            row![
                button(text("Insert")).on_press(SqlMessage::SaveNewRow),
                button(text("Cancel")).on_press(SqlMessage::CancelAddRow),
            ]
            .spacing(8),
        );
        form.into()
    }

    fn view_erd(&self) -> Element<'_, SqlMessage> {
        let Some(schema) = &self.schema else {
            return container(text("Select a connection to load schema first"))
                .padding(16)
                .into();
        };

        let visible_tables: Vec<&og_testdesk_core::sql::models::SqlTableInfo> = schema
            .tables
            .iter()
            .filter(|t| sql_erd::table_matches_filter(t, &self.erd_filter))
            .collect();

        let mut layout: Vec<CardLayout> = sql_erd::layout_cards(&visible_tables);
        sql_erd::mark_fk_columns(&mut layout, &schema.relationships);

        let canvas_width = layout
            .iter()
            .map(|c| c.bounds.x + c.bounds.width)
            .fold(600.0_f32, f32::max);
        let canvas_height = layout
            .iter()
            .map(|c| c.bounds.y + c.bounds.height)
            .fold(400.0_f32, f32::max);

        let diagram: Element<'_, ErdMessage> = iced::widget::Canvas::new(ErdProgram {
            schema,
            layout,
            highlighted: &self.erd_highlighted,
        })
        .width(Length::Fixed(canvas_width))
        .height(Length::Fixed(canvas_height))
        .into();

        column![
            row![
                text("Relationship diagram").size(16),
                text_input("Filter tables/columns", &self.erd_filter)
                    .on_input(SqlMessage::ErdFilterChanged)
                    .width(Length::Fixed(240.0)),
            ]
            .spacing(12),
            scrollable(diagram.map(SqlMessage::Erd))
                .direction(scrollable::Direction::Both {
                    vertical: scrollable::Scrollbar::default(),
                    horizontal: scrollable::Scrollbar::default(),
                })
                .width(Length::Fill)
                .height(Length::Fill),
        ]
        .spacing(8)
        .padding(16)
        .into()
    }

    fn handle_cell_clicked(&mut self, row_index: usize, col_index: usize) -> Option<Task<SqlMessage>> {
        let browse = self.browse.as_ref()?;
        let schema = self.schema.as_ref()?;
        let column = self.grid.columns.get(col_index)?;
        let fk = schema
            .relationships
            .iter()
            .find(|rel| rel.from_table == browse.table && rel.from_column == column.title)?;
        let value = self.grid.rows.get(row_index)?.get(col_index)?;
        if value.is_empty() {
            return None;
        }
        Some(Task::done(SqlMessage::BrowseFiltered(
            fk.to_table.clone(),
            fk.to_column.clone(),
            value.clone(),
        )))
    }

    fn current_table_fk_columns(&self) -> HashSet<String> {
        let mut set = HashSet::new();
        if let (Some(browse), Some(schema)) = (&self.browse, &self.schema) {
            for rel in &schema.relationships {
                if rel.from_table == browse.table {
                    set.insert(rel.from_column.clone());
                }
            }
        }
        set
    }

    pub fn view(&self) -> Element<'_, SqlMessage> {
        if self.view_mode == SqlViewMode::ConnectionLanding {
            return self.view_connection_landing();
        }

        let schema_tree = self.view_schema_tree();
        let saved_queries_tree = self.view_saved_queries_tree();
        let history_panel = self.view_history_panel();

        // Three fixed sections top-to-bottom per the app shell spec:
        // Tables & Functions, Saved queries, History. Cron/alerts moved out
        // of the sidebar to a Schedule-toggled panel near the editor (see
        // `schedule_panel_open`) so the sidebar stays strictly three-part.
        let sidebar = scrollable(
            column![
                row![
                    text(format!(
                        "Connection: {}",
                        self.selected_connection.as_deref().unwrap_or("none")
                    ))
                    .size(13),
                    button(text("Manage connections")).on_press(SqlMessage::GoToLanding),
                ]
                .spacing(8),
                schema_tree,
                iced::widget::horizontal_rule(1),
                saved_queries_tree,
                iced::widget::horizontal_rule(1),
                history_panel,
            ]
            .spacing(16),
        )
        .width(Length::Fixed(280.0));

        let run_label = if self.running { "Running..." } else { "Run" };
        let highlighter_settings = SqlHighlighterSettings {
            search_term: if self.find_open { self.find_term.clone() } else { String::new() },
            current_match: self.find_matches.get(self.find_current).copied(),
        };
        let editor_widget = text_editor(&self.editor)
            .on_action(SqlMessage::EditorAction)
            .highlight_with::<SqlHighlighter>(highlighter_settings, format_for)
            .height(Length::Fill);

        let find_bar: Element<'_, SqlMessage> = if self.find_open {
            let count_label = if self.find_matches.is_empty() {
                "0 of 0".to_string()
            } else {
                format!("{} of {}", self.find_current + 1, self.find_matches.len())
            };
            row![
                text_input("Find in editor", &self.find_term).on_input(SqlMessage::FindTermChanged),
                text(count_label).size(12),
                button(text("Prev")).on_press(SqlMessage::FindPrev),
                button(text("Next")).on_press(SqlMessage::FindNext),
                button(text("x")).on_press(SqlMessage::CloseFind),
            ]
            .spacing(6)
            .into()
        } else {
            row![button(text("Find (Ctrl/Cmd+F)")).on_press(SqlMessage::ToggleFind)].into()
        };

        let autocomplete_popup: Element<'_, SqlMessage> = if self.autocomplete.is_empty() {
            column![].into()
        } else {
            self.autocomplete.iter().fold(
                column![text("Suggestions:").size(11)].spacing(2),
                |col, name| {
                    col.push(
                        button(text(name.clone()).size(12))
                            .on_press(SqlMessage::AutocompleteSelected(name.clone())),
                    )
                },
            )
            .into()
        };

        let variables_bar: Element<'_, SqlMessage> = if self.variable_order.is_empty() {
            column![].into()
        } else {
            self.variable_order.iter().fold(
                column![text("Variables").size(13)].spacing(4),
                |col, name| {
                    let (raw, mode) = self
                        .variables
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| (String::new(), VariableFormat::Raw));
                    let name_for_input = name.clone();
                    let name_for_format = name.clone();
                    let format_buttons = VariableFormat::ALL.iter().fold(row![].spacing(4), |r, f| {
                        let selected = *f == mode;
                        let label = if selected {
                            format!("[{}]", f.label())
                        } else {
                            f.label().to_string()
                        };
                        let name_for_format = name_for_format.clone();
                        r.push(
                            button(text(label).size(11))
                                .on_press(SqlMessage::VariableFormatChanged(name_for_format, *f)),
                        )
                    });
                    col.push(
                        row![
                            text(format!("{{{{{name}}}}}")).size(12).width(Length::Fixed(120.0)),
                            text_input("value", &raw)
                                .on_input(move |v| {
                                    SqlMessage::VariableValueChanged(name_for_input.clone(), v)
                                })
                                .width(Length::Fixed(160.0)),
                            format_buttons,
                        ]
                        .spacing(6),
                    )
                },
            )
            .into()
        };

        let save_to_file_status: Element<'_, SqlMessage> = match &self.save_to_file_status {
            None => text("").size(11).into(),
            Some(Ok(path)) => text(format!("Saved to {path}"))
                .size(11)
                .color(iced::Color::from_rgb8(0x5a, 0xc0, 0x7a))
                .into(),
            Some(Err(err)) => text(err.clone())
                .size(11)
                .color(iced::Color::from_rgb8(0xe0, 0x5a, 0x5a))
                .into(),
        };

        let schedule_panel: Element<'_, SqlMessage> = if self.schedule_panel_open {
            container(self.view_cron_panel()).padding(8).into()
        } else {
            column![].into()
        };

        let editor_pane = column![
            text(format!(
                "Editor (connection: {})",
                self.selected_connection.as_deref().unwrap_or("none selected")
            )),
            find_bar,
            editor_widget,
            autocomplete_popup,
            variables_bar,
            // Below the editor, per the app-shell spec: timezone, then the
            // Schedule entry point, then Run/Save/Save-to-file.
            row![
                self.view_timezone_pill(),
                button(text("Schedule")).on_press(SqlMessage::ToggleSchedulePanel),
            ]
            .spacing(12),
            schedule_panel,
            row![
                button(text(run_label)).on_press(SqlMessage::RunPressed),
                text_input("query name", &self.save_query_name)
                    .on_input(SqlMessage::SaveQueryNameChanged),
                text_input("folder (optional)", &self.save_query_folder)
                    .on_input(SqlMessage::SaveQueryFolderChanged),
                button(text("Save query")).on_press(SqlMessage::SaveQueryPressed),
                button(text("Save to file")).on_press(SqlMessage::SaveToFilePressed),
            ]
            .spacing(8),
            save_to_file_status,
        ]
        .spacing(8);

        let divider = mouse_area(
            container(text(""))
                .width(Length::Fill)
                .height(Length::Fixed(6.0))
                .style(|theme: &iced::Theme| container::Style {
                    background: Some(theme.extended_palette().background.strong.color.into()),
                    ..Default::default()
                }),
        )
        .on_press(SqlMessage::SplitDragStart);

        let results_pane: Element<'_, SqlMessage> = if let Some(err) = &self.last_error {
            container(text(err.clone()).color(iced::Color::from_rgb8(0xe0, 0x5a, 0x5a)))
                .padding(8)
                .into()
        } else {
            let grid_view = self.grid.view().map(SqlMessage::Grid);
            let output_toolbar = self.view_output_toolbar();
            if self.browse.is_some() {
                column![
                    self.view_browse_toolbar(),
                    output_toolbar,
                    grid_view,
                    self.view_row_edit_form(),
                    self.view_new_row_form(),
                ]
                .spacing(8)
                .into()
            } else {
                column![output_toolbar, grid_view].spacing(8).into()
            }
        };

        let editor_portion = (self.split_ratio * 1000.0) as u16;
        let results_portion = ((1.0 - self.split_ratio) * 1000.0) as u16;

        let main_content: Element<'_, SqlMessage> = match self.view_mode {
            SqlViewMode::QueryEditor => {
                let main_area = column![
                    container(editor_pane)
                        .height(Length::FillPortion(editor_portion.max(1))),
                    divider,
                    container(results_pane)
                        .height(Length::FillPortion(results_portion.max(1))),
                ];
                main_area.width(Length::Fill).padding(16).into()
            }
            SqlViewMode::RelationshipDiagram => self.view_erd(),
            // Unreachable: `view()` returns early for this case above.
            SqlViewMode::ConnectionLanding => column![].into(),
        };

        container(row![sidebar, main_content].spacing(16))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn reload_connections(engine: Arc<SqlEngineState>) -> Task<SqlMessage> {
    Task::perform(
        async move { engine::list_connections(&engine) },
        SqlMessage::ConnectionsReloaded,
    )
}

fn reload_saved_queries(connection: String) -> Task<SqlMessage> {
    Task::perform(
        async move { engine::list_saved_queries(&connection).await },
        SqlMessage::SavedQueriesReloaded,
    )
}

fn reload_schema(engine: Arc<SqlEngineState>, connection: String) -> Task<SqlMessage> {
    Task::perform(
        async move {
            let conns = engine.connections();
            let Some(conn) = find_connection(&connection, &conns).cloned() else {
                return Err("Connection not found".to_string());
            };
            engine::fetch_relationship_schema(&conn, &engine).await
        },
        SqlMessage::SchemaLoaded,
    )
}

fn reload_functions(engine: Arc<SqlEngineState>, connection: String) -> Task<SqlMessage> {
    Task::perform(
        async move {
            let conns = engine.connections();
            let Some(conn) = find_connection(&connection, &conns).cloned() else {
                return Err("Connection not found".to_string());
            };
            engine::fetch_function_list(&conn, &engine).await
        },
        SqlMessage::FunctionsLoaded,
    )
}

/// Opens a native file picker and imports the chosen file's contents as
/// saved queries via `engine::import_queries` (accepts either a
/// `SavedQueryExport` bundle or a plain `Vec<SavedQuery>` JSON array).
async fn pick_and_import_queries(connection: String) -> Result<String, String> {
    let path = tokio::task::spawn_blocking(move || {
        rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .add_filter("SQL", &["sql"])
            .pick_file()
    })
    .await
    .map_err(|err| format!("File dialog task failed: {err}"))?;
    let Some(path) = path else {
        return Err("Import cancelled.".to_string());
    };
    import_queries_from_path(connection, path).await
}

/// Shared import logic for both the "Import" button and drag-and-drop
/// (`iced::window::Event::FileDropped`, real OS-level drop support
/// confirmed present in iced 0.13's window event set). A dropped `.sql`
/// file (not a JSON bundle) is wrapped as a single saved query named
/// after the file stem, since `import_queries` only understands JSON.
async fn import_queries_from_path(connection: String, path: std::path::PathBuf) -> Result<String, String> {
    let contents = tokio::task::spawn_blocking(move || std::fs::read_to_string(&path))
        .await
        .map_err(|err| format!("File read task failed: {err}"))?
        .map_err(|err| format!("Failed to read file: {err}"))?;

    let trimmed = contents.trim_start();
    let payload = if trimmed.starts_with('{') || trimmed.starts_with('[') {
        contents
    } else {
        // Plain .sql file: wrap as a single unnamed saved query.
        serde_json::to_string(&serde_json::json!([{
            "name": "Imported query",
            "sql": contents,
            "folder": null,
        }]))
        .map_err(|err| format!("Failed to wrap SQL file: {err}"))?
    };

    engine::import_queries(&connection, &payload, "rename")
        .await
        .map(|_| "Imported saved queries.".to_string())
}

fn browse_task(
    engine: Arc<SqlEngineState>,
    connection: String,
    table: String,
    page: u32,
    filter: Option<TableBrowseFilter>,
) -> Task<SqlMessage> {
    Task::perform(
        async move {
            let filters = filter.map(|f| vec![f]).unwrap_or_default();
            engine::browse_table(&engine, &connection, None, &table, Some(page), None, &filters).await
        },
        SqlMessage::BrowseResultLoaded,
    )
}

fn reload_saved_folders(connection: String) -> Task<SqlMessage> {
    Task::perform(
        async move { engine::list_saved_query_folders(&connection).await },
        SqlMessage::SavedFoldersReloaded,
    )
}

fn reload_history(connection: String) -> Task<SqlMessage> {
    Task::perform(
        async move { app_db::get_sql_run_history_summaries(&connection, None, HISTORY_LIMIT).await },
        SqlMessage::HistoryReloaded,
    )
}

/// Runs `action`, then reloads the saved-query list for `connection`.
fn reload_saved_after(
    connection: String,
    action: impl std::future::Future<Output = ()> + Send + 'static,
) -> Task<SqlMessage> {
    Task::perform(
        async move {
            action.await;
            engine::list_saved_queries(&connection).await
        },
        SqlMessage::SavedQueriesReloaded,
    )
}

/// Runs `action`, then reloads the saved-query-folder list for `connection`.
fn reload_folders_after(
    connection: String,
    action: impl std::future::Future<Output = ()> + Send + 'static,
) -> Task<SqlMessage> {
    Task::perform(
        async move {
            action.await;
            engine::list_saved_query_folders(&connection).await
        },
        SqlMessage::SavedFoldersReloaded,
    )
}

fn reload_timezone(engine: Arc<SqlEngineState>, connection: String) -> Task<SqlMessage> {
    Task::perform(
        async move { engine::fetch_timezone(&engine, &connection).await },
        SqlMessage::TimezoneLoaded,
    )
}

fn load_cron_tasks() -> Task<SqlMessage> {
    Task::perform(async move { engine::get_cron_tasks().await }, SqlMessage::CronTasksLoaded)
}

fn save_cron_tasks(tasks: Vec<CronTask>) -> Task<SqlMessage> {
    Task::perform(
        async move {
            let values: Vec<serde_json::Value> = tasks.iter().map(CronTask::to_value).collect();
            let _ = engine::save_cron_tasks(&values).await;
            values
        },
        SqlMessage::CronTasksLoaded,
    )
}

/// Runs one cron task's SQL, records it in run history with
/// `run_source: "cron"`, evaluates its alert rule against the row count,
/// and reports the outcome for the cron-activity log.
fn run_cron_task(engine: Arc<SqlEngineState>, task: CronTask) -> Task<SqlMessage> {
    let task_id = task.id.clone();
    Task::perform(
        async move {
            let form = SqlForm {
                sql: task.sql.clone(),
                connection: task.connection.clone(),
                variables: None,
                tab_id: None,
                query_name: Some(task.name.clone()),
                query_folder: None,
                run_source: Some("cron".to_string()),
                cron_task_id: Some(task.id.clone()),
                cron_task_name: Some(task.name.clone()),
                alert: task.alert.clone(),
            };
            let execution = engine::execute_sql(form, &engine).await;
            let row_count_text = format!("{} rows", execution.rows.len());
            let alert_message = engine::alert_trigger_message(task.alert.as_ref(), &row_count_text, &task.name);
            let created_at = engine::now_isoish();
            let record = SqlRunHistoryRecord {
                id: format!("cron-run-{}-{}", now_ms(), task.id),
                connection: task.connection.clone(),
                tab_id: String::new(),
                sql: task.sql.clone(),
                query_name: task.name.clone(),
                query_folder: String::new(),
                run_source: "cron".to_string(),
                cron_task_id: task.id.clone(),
                cron_task_name: task.name.clone(),
                status: if execution.error.is_some() { "error".to_string() } else { "completed".to_string() },
                created_at: created_at.clone(),
                completed_at: Some(created_at),
                row_count_text: Some(row_count_text),
                result_json: None,
                error: execution.error.clone(),
                alert_triggered: alert_message.is_some(),
                alert_message,
            };
            if let Err(err) = app_db::upsert_sql_run_history(&record).await {
                eprintln!("Failed to persist cron run history: {err}");
            }
            execution
        },
        move |execution| SqlMessage::CronRunFinished(task_id.clone(), Arc::new(execution)),
    )
}

fn history_record_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    format!("manual-{millis}-{}", std::process::id())
}

#[cfg(test)]
mod phase6_tests {
    use super::*;

    #[test]
    fn csv_field_passes_through_plain_values() {
        assert_eq!(csv_field("hello"), "hello");
        assert_eq!(csv_field(""), "");
    }

    #[test]
    fn csv_field_quotes_values_needing_it() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn build_csv_produces_header_and_rows_with_crlf() {
        let headers = vec!["id".to_string(), "name".to_string()];
        let rows = vec![
            vec!["1".to_string(), "Alice".to_string()],
            vec!["2".to_string(), "Bob, Jr.".to_string()],
        ];
        let csv = build_csv(&headers, &rows);
        assert_eq!(csv, "id,name\r\n1,Alice\r\n2,\"Bob, Jr.\"\r\n");
    }

    #[test]
    fn build_csv_with_no_rows_is_just_the_header() {
        let headers = vec!["a".to_string()];
        assert_eq!(build_csv(&headers, &[]), "a\r\n");
    }

    fn sample_task(interval_ms: i64, last_run_ms: i64, enabled: bool) -> CronTask {
        CronTask {
            id: "t1".to_string(),
            name: "Nightly check".to_string(),
            sql: "SELECT 1".to_string(),
            connection: "conn".to_string(),
            interval_ms,
            alert: Some(SqlAlertRule { comparator: ">".to_string(), value: 5 }),
            enabled,
            last_run_ms,
        }
    }

    #[test]
    fn cron_task_round_trips_through_value() {
        let task = sample_task(60_000, 12345, true);
        let value = task.to_value();
        let parsed = CronTask::from_value(&value).expect("should parse back");
        assert_eq!(parsed.id, task.id);
        assert_eq!(parsed.name, task.name);
        assert_eq!(parsed.sql, task.sql);
        assert_eq!(parsed.connection, task.connection);
        assert_eq!(parsed.interval_ms, task.interval_ms);
        assert_eq!(parsed.enabled, task.enabled);
        assert_eq!(parsed.last_run_ms, task.last_run_ms);
        let alert = parsed.alert.expect("alert should round-trip");
        assert_eq!(alert.comparator, ">");
        assert_eq!(alert.value, 5);
    }

    #[test]
    fn cron_task_round_trips_with_no_alert() {
        let mut task = sample_task(60_000, 0, false);
        task.alert = None;
        let value = task.to_value();
        let parsed = CronTask::from_value(&value).expect("should parse back");
        assert!(parsed.alert.is_none());
        assert!(!parsed.enabled);
    }

    #[test]
    fn cron_task_from_value_rejects_missing_required_fields() {
        let value = serde_json::json!({ "id": "t1" });
        assert!(CronTask::from_value(&value).is_none());
    }

    #[test]
    fn is_due_true_when_interval_elapsed_and_enabled() {
        let task = sample_task(60_000, 1_000, true);
        assert!(task.is_due(61_000));
        assert!(task.is_due(100_000));
    }

    #[test]
    fn is_due_false_when_interval_not_elapsed() {
        let task = sample_task(60_000, 1_000, true);
        assert!(!task.is_due(30_000));
    }

    #[test]
    fn is_due_false_when_disabled() {
        let task = sample_task(60_000, 1_000, false);
        assert!(!task.is_due(1_000_000));
    }

    #[test]
    fn is_due_false_for_zero_interval() {
        let task = sample_task(0, 0, true);
        assert!(!task.is_due(1_000_000));
    }

    #[test]
    fn next_comparator_cycles_through_all_and_wraps() {
        let mut c = "=".to_string();
        let mut seen = vec![c.clone()];
        for _ in 0..ALERT_COMPARATORS.len() - 1 {
            c = next_comparator(&c);
            seen.push(c.clone());
        }
        assert_eq!(seen, ALERT_COMPARATORS.to_vec());
        assert_eq!(next_comparator(&c), "=");
    }

    #[test]
    fn next_comparator_falls_back_to_first_for_unknown_input() {
        assert_eq!(next_comparator("bogus"), "!=");
    }
}
