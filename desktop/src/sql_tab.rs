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
    AddConnForm, DbConnection, SavedQuery, SqlExecution, SqlForm, SqlRelationshipSchema,
    TableBrowseFilter, TableUpdateChange,
};

const HISTORY_LIMIT: i64 = 200;

use crate::sql_erd::{self, CardLayout, ErdMessage, ErdProgram};
use crate::sql_grid::{GridMessage, ResultsGrid};
use crate::sql_highlighter::{
    self, SqlHighlighter, SqlHighlighterSettings, VariableFormat, format_for,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SqlViewMode {
    QueryEditor,
    RelationshipDiagram,
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
}

impl SqlTab {
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

            editing_row: None,
            new_row: None,

            variables: HashMap::new(),
            variable_order: Vec::new(),

            autocomplete: Vec::new(),

            find_open: false,
            find_term: String::new(),
            find_matches: Vec::new(),
            find_current: 0,

            view_mode: SqlViewMode::QueryEditor,
            erd_filter: String::new(),
            erd_highlighted: None,
        };
        let task = reload_connections(engine);
        (tab, task)
    }

    pub fn subscription(&self) -> iced::Subscription<SqlMessage> {
        iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                Some(SqlMessage::SplitCursorMoved(position.y))
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(SqlMessage::SplitDragEnd)
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
                self.schema = None;
                self.schema_error = None;
                self.browse = None;
                self.editing_row = None;
                self.new_row = None;
                return Task::batch([
                    reload_saved_queries(nickname.clone()),
                    reload_saved_folders(nickname.clone()),
                    reload_schema(self.engine.clone(), nickname.clone()),
                    reload_history(nickname),
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
                if let GridMessage::CellClicked(row_index, col_index) = grid_message {
                    if let Some(task) = self.handle_cell_clicked(row_index, col_index) {
                        return task;
                    }
                    if self.browse.is_some() {
                        return self.update(SqlMessage::StartEditRow(row_index));
                    }
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
                    return reload_schema(self.engine.clone(), connection);
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
                    SqlViewMode::RelationshipDiagram => SqlViewMode::QueryEditor,
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

    /// Returns `Some(task)` if the click was on an FK cell and should
    /// navigate to the referenced table instead of the default grid
    /// handling (sort etc). Returns `None` for a plain-cell click.
    fn view_schema_tree(&self) -> Element<'_, SqlMessage> {
        let diagram_label = match self.view_mode {
            SqlViewMode::QueryEditor => "View diagram",
            SqlViewMode::RelationshipDiagram => "Back to editor",
        };
        let mut col = column![row![
            text("Schema").size(16),
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

        col.into()
    }

    fn view_saved_queries_tree(&self) -> Element<'_, SqlMessage> {
        let filter = self.query_filter.to_lowercase();
        let matches = |name: &str| filter.is_empty() || name.to_lowercase().contains(&filter);

        let mut col = column![
            text("Saved queries").size(16),
            text_input("filter queries", &self.query_filter).on_input(SqlMessage::QueryFilterChanged),
            row![
                text_input("new folder name", &self.new_folder_name)
                    .on_input(SqlMessage::NewFolderNameChanged),
                button(text("+ Folder")).on_press(SqlMessage::CreateFolderPressed),
            ]
            .spacing(6),
        ]
        .spacing(4);

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
        let connections_list = self.connections.iter().fold(
            column![text("Connections").size(16)].spacing(4),
            |col, conn| {
                let selected = self.selected_connection.as_deref() == Some(conn.nickname.as_str());
                let label = if selected {
                    format!("> {} ({})", conn.nickname, conn.db_type)
                } else {
                    format!("{} ({})", conn.nickname, conn.db_type)
                };
                col.push(
                    row![
                        button(text(label))
                            .on_press(SqlMessage::ConnectionSelected(conn.nickname.clone())),
                        button(text("delete"))
                            .on_press(SqlMessage::DeleteConnection(conn.nickname.clone())),
                    ]
                    .spacing(6),
                )
            },
        );

        let add_connection_form = column![
            text("Add connection").size(16),
            text_input("nickname", &self.new_conn_nickname)
                .on_input(SqlMessage::NewConnNickname),
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

        let schema_tree = self.view_schema_tree();
        let saved_queries_tree = self.view_saved_queries_tree();
        let history_panel = self.view_history_panel();

        let sidebar = scrollable(
            column![
                connections_list,
                add_connection_form,
                schema_tree,
                saved_queries_tree,
                history_panel
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

        let editor_pane = column![
            text(format!(
                "Editor (connection: {})",
                self.selected_connection.as_deref().unwrap_or("none selected")
            )),
            find_bar,
            editor_widget,
            autocomplete_popup,
            variables_bar,
            row![
                button(text(run_label)).on_press(SqlMessage::RunPressed),
                text_input("query name", &self.save_query_name)
                    .on_input(SqlMessage::SaveQueryNameChanged),
                text_input("folder (optional)", &self.save_query_folder)
                    .on_input(SqlMessage::SaveQueryFolderChanged),
                button(text("Save query")).on_press(SqlMessage::SaveQueryPressed),
            ]
            .spacing(8),
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
            if self.browse.is_some() {
                column![
                    self.view_browse_toolbar(),
                    grid_view,
                    self.view_row_edit_form(),
                    self.view_new_row_form(),
                ]
                .spacing(8)
                .into()
            } else {
                grid_view
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

fn history_record_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    format!("manual-{millis}-{}", std::process::id())
}
