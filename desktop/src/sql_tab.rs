use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use iced::widget::{button, column, container, mouse_area, row, scrollable, text, text_editor, text_input};
use iced::{Element, Length, Task};

use og_testdesk_core::sql::engine::{
    self, SqlEngineState, TableBrowseResult, TableWriteResult,
};
use og_testdesk_core::sql::helpers::find_connection;
use og_testdesk_core::sql::models::{
    AddConnForm, DbConnection, SavedQuery, SqlExecution, SqlForm, SqlRelationshipSchema,
    TableBrowseFilter, TableUpdateChange,
};

use crate::sql_grid::{GridMessage, ResultsGrid};
use crate::sql_highlighter::{SqlHighlighter, format_for};

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
    SaveQueryPressed,
    SavedQueriesReloaded(Vec<SavedQuery>),
    LoadSavedQuery(String),

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
    save_query_name: String,

    split_ratio: f32,
    resizing: bool,
    drag_last_y: Option<f32>,

    schema: Option<SqlRelationshipSchema>,
    schema_error: Option<String>,
    expanded_tables: HashSet<String>,
    browse: Option<BrowseState>,

    editing_row: Option<(usize, HashMap<String, String>)>,
    new_row: Option<HashMap<String, String>>,
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
            save_query_name: String::new(),

            split_ratio: 0.4,
            resizing: false,
            drag_last_y: None,

            schema: None,
            schema_error: None,
            expanded_tables: HashSet::new(),
            browse: None,

            editing_row: None,
            new_row: None,
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
                    reload_schema(self.engine.clone(), nickname),
                ]);
            }
            SqlMessage::EditorAction(action) => self.editor.perform(action),
            SqlMessage::RunPressed => {
                if let Some(connection) = self.selected_connection.clone() {
                    self.running = true;
                    self.last_error = None;
                    self.browse = None;
                    self.editing_row = None;
                    self.new_row = None;
                    let engine = self.engine.clone();
                    let sql = self.editor.text();
                    return Task::perform(
                        async move {
                            let form = SqlForm {
                                sql,
                                connection,
                                variables: None,
                                tab_id: None,
                                query_name: None,
                                query_folder: None,
                                run_source: None,
                                cron_task_id: None,
                                cron_task_name: None,
                                alert: None,
                            };
                            engine::execute_sql(form, &engine).await
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
            SqlMessage::SaveQueryPressed => {
                if let Some(connection) = self.selected_connection.clone() {
                    if !self.save_query_name.trim().is_empty() {
                        let name = self.save_query_name.clone();
                        let sql = self.editor.text();
                        self.save_query_name.clear();
                        return Task::perform(
                            async move {
                                let _ = engine::save_query(&connection, &name, &sql, None).await;
                                engine::list_saved_queries(&connection).await
                            },
                            SqlMessage::SavedQueriesReloaded,
                        );
                    }
                }
            }
            SqlMessage::SavedQueriesReloaded(queries) => self.saved_queries = queries,
            SqlMessage::LoadSavedQuery(sql) => self.editor = text_editor::Content::with_text(&sql),

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
        }
        Task::none()
    }

    /// Returns `Some(task)` if the click was on an FK cell and should
    /// navigate to the referenced table instead of the default grid
    /// handling (sort etc). Returns `None` for a plain-cell click.
    fn view_schema_tree(&self) -> Element<'_, SqlMessage> {
        let mut col = column![row![
            text("Schema").size(16),
            button(text("Refresh")).on_press(SqlMessage::RefreshSchema),
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

        let saved_queries_list = self.saved_queries.iter().fold(
            column![text("Saved queries").size(16)].spacing(4),
            |col, q| {
                col.push(
                    button(text(q.name.clone())).on_press(SqlMessage::LoadSavedQuery(q.sql.clone())),
                )
            },
        );

        let schema_tree = self.view_schema_tree();

        let sidebar = scrollable(
            column![connections_list, add_connection_form, schema_tree, saved_queries_list]
                .spacing(16),
        )
        .width(Length::Fixed(280.0));

        let run_label = if self.running { "Running..." } else { "Run" };
        let editor_widget = text_editor(&self.editor)
            .on_action(SqlMessage::EditorAction)
            .highlight_with::<SqlHighlighter>((), format_for)
            .height(Length::Fill);

        let editor_pane = column![
            text(format!(
                "Editor (connection: {})",
                self.selected_connection.as_deref().unwrap_or("none selected")
            )),
            editor_widget,
            row![
                button(text(run_label)).on_press(SqlMessage::RunPressed),
                text_input("query name", &self.save_query_name)
                    .on_input(SqlMessage::SaveQueryNameChanged),
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

        let main_area = column![
            container(editor_pane)
                .height(Length::FillPortion(editor_portion.max(1))),
            divider,
            container(results_pane)
                .height(Length::FillPortion(results_portion.max(1))),
        ];

        container(row![sidebar, main_area.width(Length::Fill).padding(16)].spacing(16))
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
