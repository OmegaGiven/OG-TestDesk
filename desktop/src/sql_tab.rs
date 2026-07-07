use std::sync::Arc;

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length, Task};

use og_testdesk_core::sql::engine::{self, SqlEngineState};
use og_testdesk_core::sql::models::{AddConnForm, DbConnection, SavedQuery, SqlExecution, SqlForm};

#[derive(Debug, Clone)]
pub enum SqlMessage {
    ConnectionSelected(String),
    EditorChanged(String),
    RunPressed,
    QueryFinished(Arc<SqlExecution>),

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
}

pub struct SqlTab {
    engine: Arc<SqlEngineState>,
    connections: Vec<DbConnection>,
    selected_connection: Option<String>,
    editor_text: String,
    last_execution: Option<SqlExecution>,
    running: bool,

    new_conn_nickname: String,
    new_conn_db_type: String,
    new_conn_host: String,
    new_conn_db_name: String,
    new_conn_user: String,
    new_conn_password: String,

    saved_queries: Vec<SavedQuery>,
    save_query_name: String,
}

impl SqlTab {
    pub fn new(engine: Arc<SqlEngineState>) -> (Self, Task<SqlMessage>) {
        let tab = Self {
            engine: engine.clone(),
            connections: Vec::new(),
            selected_connection: None,
            editor_text: String::new(),
            last_execution: None,
            running: false,

            new_conn_nickname: String::new(),
            new_conn_db_type: "postgres".to_string(),
            new_conn_host: String::new(),
            new_conn_db_name: String::new(),
            new_conn_user: String::new(),
            new_conn_password: String::new(),

            saved_queries: Vec::new(),
            save_query_name: String::new(),
        };
        let task = reload_connections(engine);
        (tab, task)
    }

    pub fn update(&mut self, message: SqlMessage) -> Task<SqlMessage> {
        match message {
            SqlMessage::ConnectionSelected(nickname) => {
                self.selected_connection = Some(nickname.clone());
                return reload_saved_queries(nickname);
            }
            SqlMessage::EditorChanged(text) => self.editor_text = text,
            SqlMessage::RunPressed => {
                if let Some(connection) = self.selected_connection.clone() {
                    self.running = true;
                    let engine = self.engine.clone();
                    let sql = self.editor_text.clone();
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
                self.last_execution = Some((*execution).clone());
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
                        let sql = self.editor_text.clone();
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
            SqlMessage::LoadSavedQuery(sql) => self.editor_text = sql,
        }
        Task::none()
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

        let sidebar = scrollable(
            column![connections_list, add_connection_form, saved_queries_list].spacing(16),
        )
        .width(Length::Fixed(280.0));

        let run_label = if self.running { "Running..." } else { "Run" };
        let editor = column![
            text(format!(
                "Editor (connection: {})",
                self.selected_connection.as_deref().unwrap_or("none selected")
            )),
            text_input("SELECT 1", &self.editor_text).on_input(SqlMessage::EditorChanged),
            row![
                button(text(run_label)).on_press(SqlMessage::RunPressed),
                text_input("query name", &self.save_query_name)
                    .on_input(SqlMessage::SaveQueryNameChanged),
                button(text("Save query")).on_press(SqlMessage::SaveQueryPressed),
            ]
            .spacing(8),
        ]
        .spacing(8);

        let results: Element<'_, SqlMessage> = match &self.last_execution {
            Some(SqlExecution { error: Some(err), .. }) => text(err.clone()).into(),
            Some(execution) => {
                let header = row(execution
                    .headers
                    .iter()
                    .map(|h| text(h.clone()).width(Length::Fixed(140.0)).into())
                    .collect::<Vec<Element<'_, SqlMessage>>>())
                .spacing(4);

                let body = execution.rows.iter().fold(
                    column![header].spacing(4),
                    |col, row_values| {
                        col.push(
                            row(row_values
                                .iter()
                                .map(|v| text(v.clone()).width(Length::Fixed(140.0)).into())
                                .collect::<Vec<Element<'_, SqlMessage>>>())
                            .spacing(4),
                        )
                    },
                );
                scrollable(body).into()
            }
            None => text("Run a query to see results").into(),
        };

        container(
            row![sidebar, column![editor, results].spacing(16).padding(16)]
                .spacing(16),
        )
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
