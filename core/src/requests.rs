use crate::app_db;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{self, Read},
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const MAX_PROXY_RESPONSE_BODY_BYTES: u64 = 10 * 1024 * 1024;
static RUNNING_REQUESTS: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();

fn running_requests() -> &'static Mutex<HashMap<String, u32>> {
    RUNNING_REQUESTS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SavedRequest {
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: String,
    pub body: String,
    pub auth_type: Option<String>,
    pub oauth_token_url: Option<String>,
    pub oauth_client_id: Option<String>,
    pub oauth_client_secret: Option<String>,
    pub oauth_scope: Option<String>,
    #[serde(default)]
    pub folder: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct RequestVariableSet {
    pub name: String,
    #[serde(default)]
    pub values: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct RequestVariables {
    #[serde(default)]
    pub active_set: String,
    #[serde(default)]
    pub sets: Vec<RequestVariableSet>,
    #[serde(default)]
    pub global: HashMap<String, String>,
}

pub struct PostmanImportResult {
    pub imported: usize,
    pub folders: usize,
    pub variables: usize,
    pub warnings: Vec<String>,
}

#[derive(Clone)]
struct ParsedPostmanRequest {
    request: SavedRequest,
    folder: Option<String>,
}

pub struct ProxyRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub body_mode: Option<String>,
    pub form_data: Vec<(String, String)>,
    pub graphql: Option<GraphqlPayload>,
    pub request_id: Option<String>,
}

pub struct GraphqlPayload {
    pub query: String,
    pub variables: Option<serde_json::Value>,
    pub operation_name: Option<String>,
}

pub struct ProxyResponse {
    pub status: u16,
    pub headers: String,
    pub body: String,
    pub body_truncated: bool,
    pub body_bytes: u64,
    pub body_limit_bytes: u64,
    pub stderr: String,
    pub curl_exit: i32,
    pub duration_ms: u128,
}

fn read_proxy_response_body(path: &std::path::Path) -> io::Result<(String, bool, u64)> {
    let body_bytes = fs::metadata(path).map(|metadata| metadata.len()).unwrap_or(0);
    let mut file = fs::File::open(path)?;
    let mut limited = file.by_ref().take(MAX_PROXY_RESPONSE_BODY_BYTES);
    let mut body = Vec::new();
    limited.read_to_end(&mut body)?;
    Ok((
        String::from_utf8_lossy(&body).to_string(),
        body_bytes > MAX_PROXY_RESPONSE_BODY_BYTES,
        body_bytes,
    ))
}

async fn load_requests() -> Vec<SavedRequest> {
    app_db::get_json("requests", "saved").await.unwrap_or_default()
}

async fn save_requests(requests: &[SavedRequest]) -> Result<(), sqlx::Error> {
    app_db::put_json("requests", "saved", &requests).await
}

async fn load_request_folders() -> Vec<String> {
    app_db::get_json("requests", "folders").await.unwrap_or_default()
}

async fn save_request_folders(folders: &[String]) -> Result<(), sqlx::Error> {
    app_db::put_json("requests", "folders", &folders).await
}

pub fn normalize_folder(folder: Option<&str>) -> String {
    normalize_folder_path(folder.unwrap_or(""))
}

pub fn normalize_folder_path(folder: &str) -> String {
    folder
        .replace(" / ", "/")
        .split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn folder_basename(folder: &str) -> String {
    normalize_folder_path(folder)
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

pub fn add_folder_path(folders: &mut Vec<String>, folder: &str) {
    let folder = normalize_folder_path(folder);
    if folder.is_empty() {
        return;
    }
    let parts = folder.split('/').collect::<Vec<_>>();
    for index in 1..=parts.len() {
        let path = parts[..index].join("/");
        if !folders
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&path))
        {
            folders.push(path);
        }
    }
}

fn is_same_or_child_folder(folder: &str, parent: &str) -> bool {
    folder == parent
        || folder
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn move_folder_path(value: &str, old_folder: &str, new_folder: &str) -> String {
    if value == old_folder {
        return new_folder.to_string();
    }
    if let Some(rest) = value.strip_prefix(old_folder) {
        if rest.starts_with('/') {
            return format!("{new_folder}{rest}");
        }
    }
    value.to_string()
}

fn request_identity_matches(request: &SavedRequest, name: &str, folder: Option<&str>) -> bool {
    request.name == name && normalize_folder(request.folder.as_deref()) == normalize_folder(folder)
}

async fn load_request_variables() -> RequestVariables {
    let variables = app_db::get_json("requests", "variables").await.unwrap_or_default();
    normalize_request_variables(variables)
}

async fn save_request_variables(variables: &RequestVariables) -> Result<(), sqlx::Error> {
    let normalized = normalize_request_variables(variables.clone());
    app_db::put_json("requests", "variables", &normalized).await
}

fn normalize_request_variables(mut variables: RequestVariables) -> RequestVariables {
    if variables.sets.is_empty() && !variables.global.is_empty() {
        variables.sets.push(RequestVariableSet {
            name: "Default".to_string(),
            values: variables.global.clone(),
        });
    }

    let mut normalized_sets = Vec::new();
    for mut set in variables.sets {
        let name = set.name.trim();
        if name.is_empty() {
            continue;
        }
        let values = set
            .values
            .drain()
            .filter_map(|(key, value)| {
                let key = key.trim();
                if key.is_empty() {
                    None
                } else {
                    Some((key.to_string(), value))
                }
            })
            .collect::<HashMap<_, _>>();
        if let Some(existing_idx) = normalized_sets
            .iter()
            .position(|existing: &RequestVariableSet| existing.name.eq_ignore_ascii_case(name))
        {
            normalized_sets[existing_idx].values.extend(values);
        } else {
            normalized_sets.push(RequestVariableSet {
                name: name.to_string(),
                values,
            });
        }
    }

    let active_set = variables.active_set.trim();
    let active_set = if normalized_sets.iter().any(|set| set.name == active_set) {
        active_set.to_string()
    } else {
        normalized_sets
            .first()
            .map(|set| set.name.clone())
            .unwrap_or_default()
    };

    RequestVariables {
        active_set,
        sets: normalized_sets,
        global: HashMap::new(),
    }
}

fn upsert_variable_set(variables: &mut RequestVariables, name: &str, values: HashMap<String, String>) {
    let normalized_name = name.trim();
    if normalized_name.is_empty() {
        return;
    }
    if let Some(existing) = variables
        .sets
        .iter_mut()
        .find(|set| set.name.eq_ignore_ascii_case(normalized_name))
    {
        existing.values.extend(values);
        variables.active_set = existing.name.clone();
    } else {
        variables.sets.push(RequestVariableSet {
            name: normalized_name.to_string(),
            values,
        });
        variables.active_set = normalized_name.to_string();
    }
}

fn normalize_saved_folder(folder: Option<&str>) -> Option<String> {
    folder
        .map(normalize_folder_path)
        .filter(|value| !value.is_empty())
}

// --- Saved request CRUD ---

pub async fn get_saved_request(name: &str, folder: Option<&str>) -> Option<SavedRequest> {
    load_requests()
        .await
        .into_iter()
        .find(|request| request_identity_matches(request, name, folder))
}

pub async fn list_saved_requests() -> Vec<SavedRequest> {
    load_requests().await
}

pub async fn list_request_folders() -> Vec<String> {
    load_request_folders().await
}

#[allow(clippy::too_many_arguments)]
pub async fn save_request(
    name: &str,
    method: &str,
    url: &str,
    headers: &str,
    body: &str,
    auth_type: Option<String>,
    oauth_token_url: Option<String>,
    oauth_client_id: Option<String>,
    oauth_client_secret: Option<String>,
    oauth_scope: Option<String>,
    folder: Option<&str>,
) -> Result<(), sqlx::Error> {
    let mut requests = load_requests().await;
    let folder = normalize_saved_folder(folder);
    let new_req = SavedRequest {
        name: name.to_string(),
        method: method.to_string(),
        url: url.to_string(),
        headers: headers.to_string(),
        body: body.to_string(),
        auth_type,
        oauth_token_url,
        oauth_client_id,
        oauth_client_secret,
        oauth_scope,
        folder: folder.clone(),
    };
    if let Some(idx) = requests
        .iter()
        .position(|r| request_identity_matches(r, name, folder.as_deref()))
    {
        requests[idx] = new_req;
    } else {
        requests.push(new_req);
    }
    save_requests(&requests).await
}

pub async fn create_request_folder(folder_name: &str) -> Result<(), sqlx::Error> {
    let folder_name = normalize_folder_path(folder_name);
    if folder_name.is_empty() {
        return Ok(());
    }
    let mut folders = load_request_folders().await;
    if !folders.iter().any(|folder| folder.eq_ignore_ascii_case(&folder_name)) {
        folders.push(folder_name);
        folders.sort_by_key(|folder| folder.to_lowercase());
        save_request_folders(&folders).await?;
    }
    Ok(())
}

pub async fn delete_request_folder(folder_name: &str) -> Result<(), sqlx::Error> {
    let folder_name = normalize_folder_path(folder_name);
    if folder_name.is_empty() {
        return Ok(());
    }
    let mut folders = load_request_folders().await;
    folders.retain(|folder| !is_same_or_child_folder(&normalize_folder_path(folder), &folder_name));
    save_request_folders(&folders).await?;

    let mut requests = load_requests().await;
    requests.retain(|request| {
        let folder = request
            .folder
            .as_deref()
            .map(normalize_folder_path)
            .unwrap_or_default();
        !is_same_or_child_folder(&folder, &folder_name)
    });
    save_requests(&requests).await
}

pub async fn move_request(
    name: &str,
    old_folder: Option<&str>,
    new_folder: Option<&str>,
) -> Result<(), sqlx::Error> {
    let new_folder = new_folder
        .map(normalize_folder_path)
        .filter(|folder| !folder.is_empty());

    let mut requests = load_requests().await;
    if let Some(request) = requests
        .iter_mut()
        .find(|request| request_identity_matches(request, name, old_folder))
    {
        request.folder = new_folder.clone();
        save_requests(&requests).await?;
    }

    if let Some(folder) = new_folder {
        let mut folders = load_request_folders().await;
        if !folders.iter().any(|existing| existing.eq_ignore_ascii_case(&folder)) {
            folders.push(folder);
            folders.sort_by_key(|folder| folder.to_lowercase());
            save_request_folders(&folders).await?;
        }
    }
    Ok(())
}

pub async fn move_request_folder(folder_name: &str, new_parent: Option<&str>) -> Result<(), String> {
    let old_folder = normalize_folder_path(folder_name);
    let new_parent = new_parent.map(normalize_folder_path).unwrap_or_default();

    if old_folder.is_empty() || is_same_or_child_folder(&new_parent, &old_folder) {
        return Err("Invalid folder move".to_string());
    }
    let basename = folder_basename(&old_folder);
    if basename.is_empty() {
        return Err("Invalid folder name".to_string());
    }
    let new_folder = if new_parent.is_empty() {
        basename
    } else {
        format!("{new_parent}/{basename}")
    };
    if old_folder == new_folder {
        return Ok(());
    }

    let mut folders = load_request_folders().await;
    let mut folders_changed = false;
    for folder in folders.iter_mut() {
        let normalized = normalize_folder_path(folder);
        if is_same_or_child_folder(&normalized, &old_folder) {
            *folder = move_folder_path(&normalized, &old_folder, &new_folder);
            folders_changed = true;
        }
    }
    if !folders.iter().any(|folder| folder.eq_ignore_ascii_case(&new_folder)) {
        folders.push(new_folder.clone());
        folders_changed = true;
    }
    if folders_changed {
        folders.sort_by_key(|folder| folder.to_lowercase());
        save_request_folders(&folders).await.map_err(|err| err.to_string())?;
    }

    let mut requests = load_requests().await;
    let mut requests_changed = false;
    for request in requests.iter_mut() {
        let Some(folder) = request.folder.as_ref() else {
            continue;
        };
        let folder = normalize_folder_path(folder);
        if is_same_or_child_folder(&folder, &old_folder) {
            request.folder = Some(move_folder_path(&folder, &old_folder, &new_folder));
            requests_changed = true;
        }
    }
    if requests_changed {
        save_requests(&requests).await.map_err(|err| err.to_string())?;
    }
    Ok(())
}

pub async fn delete_request(name: &str, folder: Option<&str>) -> Result<(), sqlx::Error> {
    let mut requests = load_requests().await;
    if let Some(idx) = requests
        .iter()
        .position(|r| request_identity_matches(r, name, folder))
    {
        requests.remove(idx);
        save_requests(&requests).await?;
    }
    Ok(())
}

pub async fn rename_request(name: &str, folder: Option<&str>, new_name: &str) -> Result<(), sqlx::Error> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Ok(());
    }
    let mut requests = load_requests().await;
    let duplicate_exists = requests
        .iter()
        .any(|request| request_identity_matches(request, new_name, folder));
    if duplicate_exists {
        return Ok(());
    }
    if let Some(request) = requests
        .iter_mut()
        .find(|request| request_identity_matches(request, name, folder))
    {
        request.name = new_name.to_string();
        save_requests(&requests).await?;
    }
    Ok(())
}

// --- Outbound HTTP proxy (via curl, matching the original subprocess-based runner) ---

pub fn run_proxy_request(payload: ProxyRequest) -> io::Result<ProxyResponse> {
    let started = Instant::now();
    let mut cmd = Command::new("curl");
    let request_id = payload.request_id.clone().unwrap_or_else(|| {
        format!(
            "req_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    });

    let run_id = format!(
        "{}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        payload.method
    );
    let mut header_path = std::env::temp_dir();
    header_path.push(format!("og_testdesk_headers_{run_id}.txt"));
    let mut body_path = std::env::temp_dir();
    body_path.push(format!("og_testdesk_body_{run_id}.txt"));

    cmd.arg("-sS")
        .arg("--connect-timeout")
        .arg("15")
        .arg("--max-time")
        .arg("60")
        .arg("-X")
        .arg(&payload.method);

    for (key, value) in &payload.headers {
        if !key.trim().is_empty() {
            cmd.arg("-H").arg(format!("{key}: {value}"));
        }
    }

    let body_mode = payload.body_mode.as_deref().unwrap_or("raw");
    if payload.method != "GET" && payload.method != "HEAD" {
        match body_mode {
            "form-data" => {
                for (key, value) in &payload.form_data {
                    if !key.trim().is_empty() {
                        cmd.arg("-F").arg(format!("{key}={value}"));
                    }
                }
            }
            "binary" => {
                if !payload.body.is_empty() {
                    cmd.arg("--data-binary").arg(&payload.body);
                }
            }
            "graphql" => {
                if let Some(graphql) = &payload.graphql {
                    let mut graphql_body = serde_json::Map::new();
                    graphql_body.insert(
                        "query".to_string(),
                        serde_json::Value::String(graphql.query.clone()),
                    );
                    graphql_body.insert(
                        "variables".to_string(),
                        graphql
                            .variables
                            .clone()
                            .unwrap_or(serde_json::Value::Object(Default::default())),
                    );
                    if let Some(operation_name) = graphql
                        .operation_name
                        .as_ref()
                        .filter(|value| !value.trim().is_empty())
                    {
                        graphql_body.insert(
                            "operationName".to_string(),
                            serde_json::Value::String(operation_name.clone()),
                        );
                    }
                    let graphql_json = serde_json::Value::Object(graphql_body).to_string();
                    cmd.arg("--data-raw").arg(graphql_json);
                }
            }
            _ => {
                if !payload.body.is_empty() {
                    cmd.arg("-d").arg(&payload.body);
                }
            }
        }
    }

    cmd.arg("-D")
        .arg(&header_path)
        .arg("-o")
        .arg(&body_path)
        .arg("-w")
        .arg("%{http_code}")
        .arg(&payload.url);

    let child = cmd.spawn()?;
    if let Ok(mut map) = running_requests().lock() {
        map.insert(request_id.clone(), child.id());
    }
    let output = child.wait_with_output()?;
    if let Ok(mut map) = running_requests().lock() {
        map.remove(&request_id);
    }

    let headers = fs::read_to_string(&header_path).unwrap_or_default();
    let (body, body_truncated, body_bytes) =
        read_proxy_response_body(&body_path).unwrap_or_else(|_| (String::new(), false, 0));
    let _ = fs::remove_file(&header_path);
    let _ = fs::remove_file(&body_path);

    let status = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u16>()
        .unwrap_or(0);

    Ok(ProxyResponse {
        status,
        headers,
        body,
        body_truncated,
        body_bytes,
        body_limit_bytes: MAX_PROXY_RESPONSE_BODY_BYTES,
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        curl_exit: output.status.code().unwrap_or(-1),
        duration_ms: started.elapsed().as_millis(),
    })
}

pub fn cancel_proxy_request(request_id: &str) -> Result<(), String> {
    let pid = {
        let mut map = running_requests().lock().unwrap();
        map.remove(request_id)
    };
    let Some(pid) = pid else {
        return Err("Request not found".to_string());
    };
    let result = Command::new("kill").arg("-TERM").arg(pid.to_string()).status();
    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err("Failed to cancel request".to_string()),
        Err(err) => Err(format!("Cancel error: {err}")),
    }
}

// --- Variables / history / scratchpads ---

pub async fn get_request_variables() -> RequestVariables {
    load_request_variables().await
}

pub async fn save_request_variables_value(variables: RequestVariables) -> Result<RequestVariables, sqlx::Error> {
    let normalized = normalize_request_variables(variables);
    save_request_variables(&normalized).await?;
    Ok(normalized)
}

pub async fn get_request_history() -> serde_json::Value {
    app_db::get_json("requests", "history")
        .await
        .unwrap_or_else(|| serde_json::json!([]))
}

pub async fn save_request_history(history: &serde_json::Value) -> Result<(), sqlx::Error> {
    app_db::put_json("requests", "history", history).await
}

pub async fn get_scratchpads() -> serde_json::Value {
    app_db::get_json("scratchpads", "pads")
        .await
        .unwrap_or_else(|| serde_json::json!([]))
}

pub async fn save_scratchpads(pads: &serde_json::Value) -> Result<(), sqlx::Error> {
    app_db::put_json("scratchpads", "pads", pads).await
}

// --- Postman collection import ---

pub async fn import_postman_collection(
    collection: &serde_json::Value,
    duplicate_mode: &str,
) -> Result<PostmanImportResult, String> {
    let mut warnings = Vec::new();
    let collection_name = collection
        .get("info")
        .and_then(|info| info.get("name"))
        .and_then(|name| name.as_str())
        .unwrap_or("Postman Import")
        .trim()
        .to_string();

    let mut parsed = Vec::new();
    let collection_auth = collection.get("auth");
    let Some(items) = collection.get("item").and_then(|items| items.as_array()) else {
        return Err("Postman collection has no item array".to_string());
    };
    parse_postman_items(items, &mut Vec::new(), collection_auth, &mut parsed, &mut warnings);

    let mut imported_folders = load_request_folders().await;
    for parsed_request in &parsed {
        if let Some(folder) = &parsed_request.folder {
            if !imported_folders
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(folder))
            {
                imported_folders.push(folder.clone());
            }
        }
    }
    imported_folders.sort_by_key(|folder| folder.to_lowercase());

    let mut requests = load_requests().await;
    let mut imported_count = 0;
    for parsed_request in parsed {
        let mut request = parsed_request.request;
        let folder = request.folder.clone();
        if let Some(existing_idx) = requests
            .iter()
            .position(|existing| request_identity_matches(existing, &request.name, folder.as_deref()))
        {
            match duplicate_mode {
                "skip" => continue,
                "overwrite" => requests[existing_idx] = request,
                _ => {
                    request.name = unique_request_name(&requests, &request.name, folder.as_deref());
                    requests.push(request);
                }
            }
        } else {
            requests.push(request);
        }
        imported_count += 1;
    }

    let mut request_variables = load_request_variables().await;
    let imported_variables = extract_postman_variables(collection, &collection_name);
    let variable_count = imported_variables.len();
    upsert_variable_set(&mut request_variables, &collection_name, imported_variables);

    save_requests(&requests).await.map_err(|err| err.to_string())?;
    save_request_folders(&imported_folders).await.map_err(|err| err.to_string())?;
    save_request_variables(&request_variables).await.map_err(|err| err.to_string())?;

    Ok(PostmanImportResult {
        imported: imported_count,
        folders: imported_folders.len(),
        variables: variable_count,
        warnings,
    })
}

fn parse_postman_items(
    items: &[serde_json::Value],
    folder_path: &mut Vec<String>,
    inherited_auth: Option<&serde_json::Value>,
    parsed: &mut Vec<ParsedPostmanRequest>,
    warnings: &mut Vec<String>,
) {
    for item in items {
        let item_name = item
            .get("name")
            .and_then(|name| name.as_str())
            .unwrap_or("Untitled")
            .trim();
        if item_name.is_empty() {
            continue;
        }

        let item_auth = item.get("auth").or(inherited_auth);
        if let Some(children) = item.get("item").and_then(|children| children.as_array()) {
            folder_path.push(item_name.to_string());
            parse_postman_items(children, folder_path, item_auth, parsed, warnings);
            folder_path.pop();
            continue;
        }

        let Some(request_value) = item.get("request") else {
            continue;
        };
        let folder = if folder_path.is_empty() {
            None
        } else {
            Some(folder_path.join(" / "))
        };
        let mut headers = postman_headers_to_lines(request_value.get("header"));
        apply_postman_auth(item_auth, request_value.get("url"), &mut headers, warnings, item_name);
        let (body, body_warnings) = postman_body_to_string(request_value.get("body"), item_name);
        warnings.extend(body_warnings);

        let request = SavedRequest {
            name: item_name.to_string(),
            method: request_value
                .get("method")
                .and_then(|method| method.as_str())
                .unwrap_or("GET")
                .to_uppercase(),
            url: postman_url_to_string(request_value.get("url")),
            headers: headers.join("\n"),
            body,
            auth_type: postman_auth_type(item_auth).map(|value| value.to_string()),
            oauth_token_url: postman_oauth_value(item_auth, "accessTokenUrl"),
            oauth_client_id: postman_oauth_value(item_auth, "clientId"),
            oauth_client_secret: postman_oauth_value(item_auth, "clientSecret"),
            oauth_scope: postman_oauth_value(item_auth, "scope"),
            folder: folder.clone(),
        };
        parsed.push(ParsedPostmanRequest { request, folder });
    }
}

fn postman_headers_to_lines(headers: Option<&serde_json::Value>) -> Vec<String> {
    headers
        .and_then(|headers| headers.as_array())
        .map(|headers| {
            headers
                .iter()
                .filter(|header| {
                    !header
                        .get("disabled")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false)
                })
                .filter_map(|header| {
                    let key = header.get("key").and_then(|key| key.as_str())?.trim();
                    if key.is_empty() {
                        return None;
                    }
                    let value = header.get("value").and_then(|value| value.as_str()).unwrap_or("");
                    Some(format!("{key}: {value}"))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn postman_url_to_string(url: Option<&serde_json::Value>) -> String {
    let Some(url) = url else {
        return String::new();
    };
    if let Some(raw) = url.as_str() {
        return raw.to_string();
    }
    if let Some(raw) = url.get("raw").and_then(|raw| raw.as_str()) {
        return raw.to_string();
    }

    let protocol = url.get("protocol").and_then(|protocol| protocol.as_str()).unwrap_or("https");
    let host = postman_string_or_array(url.get("host"), ".");
    let path = postman_string_or_array(url.get("path"), "/");
    let mut rendered = if host.is_empty() {
        path
    } else if path.is_empty() {
        format!("{protocol}://{host}")
    } else {
        format!("{protocol}://{host}/{path}")
    };

    let query = url
        .get("query")
        .and_then(|query| query.as_array())
        .map(|items| {
            items
                .iter()
                .filter(|item| {
                    !item
                        .get("disabled")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false)
                })
                .filter_map(|item| {
                    let key = item.get("key").and_then(|key| key.as_str())?;
                    let value = item.get("value").and_then(|value| value.as_str()).unwrap_or("");
                    Some(format!("{}={}", percent_encode(key), percent_encode(value)))
                })
                .collect::<Vec<_>>()
                .join("&")
        })
        .unwrap_or_default();
    if !query.is_empty() {
        rendered.push('?');
        rendered.push_str(&query);
    }
    rendered
}

fn postman_string_or_array(value: Option<&serde_json::Value>, separator: &str) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.to_string(),
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.as_str())
            .collect::<Vec<_>>()
            .join(separator),
        _ => String::new(),
    }
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            b' ' => vec!['+'],
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn postman_body_to_string(body: Option<&serde_json::Value>, request_name: &str) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    let Some(body) = body else {
        return (String::new(), warnings);
    };
    match body.get("mode").and_then(|mode| mode.as_str()).unwrap_or("") {
        "raw" => (
            body.get("raw").and_then(|raw| raw.as_str()).unwrap_or("").to_string(),
            warnings,
        ),
        "urlencoded" => {
            let encoded = body
                .get("urlencoded")
                .and_then(|items| items.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter(|item| {
                            !item
                                .get("disabled")
                                .and_then(|value| value.as_bool())
                                .unwrap_or(false)
                        })
                        .filter_map(|item| {
                            let key = item.get("key").and_then(|key| key.as_str())?;
                            let value = item.get("value").and_then(|value| value.as_str()).unwrap_or("");
                            Some(format!("{}={}", percent_encode(key), percent_encode(value)))
                        })
                        .collect::<Vec<_>>()
                        .join("&")
                })
                .unwrap_or_default();
            (encoded, warnings)
        }
        "formdata" => {
            warnings.push(format!(
                "{request_name}: imported form-data body as text; file fields are skipped"
            ));
            let text = body
                .get("formdata")
                .and_then(|items| items.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter(|item| {
                            !item
                                .get("disabled")
                                .and_then(|value| value.as_bool())
                                .unwrap_or(false)
                        })
                        .filter_map(|item| {
                            if item.get("type").and_then(|value| value.as_str()) == Some("file") {
                                return None;
                            }
                            let key = item.get("key").and_then(|key| key.as_str())?;
                            let value = item.get("value").and_then(|value| value.as_str()).unwrap_or("");
                            Some(format!("{key}={value}"))
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            (text, warnings)
        }
        "graphql" => {
            let graphql = body.get("graphql").cloned().unwrap_or_default();
            let text = serde_json::to_string_pretty(&graphql).unwrap_or_default();
            (text, warnings)
        }
        "file" => {
            warnings.push(format!("{request_name}: file body import is not supported"));
            (String::new(), warnings)
        }
        _ => (String::new(), warnings),
    }
}

fn postman_auth_type(auth: Option<&serde_json::Value>) -> Option<&str> {
    match auth.and_then(|auth| auth.get("type")).and_then(|value| value.as_str()) {
        Some("bearer") => Some("bearer"),
        Some("basic") => Some("basic"),
        Some("apikey") => Some("apikey"),
        Some("oauth2") => Some("oauth2"),
        _ => None,
    }
}

fn postman_auth_array_value(
    auth: Option<&serde_json::Value>,
    auth_type: &str,
    key: &str,
) -> Option<String> {
    auth.and_then(|auth| auth.get(auth_type))
        .and_then(|items| items.as_array())
        .and_then(|items| {
            items.iter().find_map(|item| {
                if item.get("key").and_then(|item_key| item_key.as_str()) == Some(key) {
                    item.get("value").and_then(|value| match value {
                        serde_json::Value::String(text) => Some(text.to_string()),
                        _ => Some(value.to_string()),
                    })
                } else {
                    None
                }
            })
        })
}

fn postman_oauth_value(auth: Option<&serde_json::Value>, key: &str) -> Option<String> {
    postman_auth_array_value(auth, "oauth2", key)
}

fn apply_postman_auth(
    auth: Option<&serde_json::Value>,
    request_url: Option<&serde_json::Value>,
    headers: &mut Vec<String>,
    warnings: &mut Vec<String>,
    request_name: &str,
) {
    let Some(auth_type) = auth.and_then(|auth| auth.get("type")).and_then(|value| value.as_str())
    else {
        return;
    };
    match auth_type {
        "bearer" => {
            if let Some(token) = postman_auth_array_value(auth, "bearer", "token") {
                headers.push(format!("Authorization: Bearer {token}"));
            }
        }
        "basic" => {
            let username = postman_auth_array_value(auth, "basic", "username").unwrap_or_default();
            let password = postman_auth_array_value(auth, "basic", "password").unwrap_or_default();
            if !username.is_empty() || !password.is_empty() {
                headers.push(format!("Authorization: Basic {{basic_auth:{username}:{password}}}"));
                warnings.push(format!("{request_name}: basic auth was imported as a placeholder header"));
            }
        }
        "apikey" => {
            let key = postman_auth_array_value(auth, "apikey", "key").unwrap_or_default();
            let value = postman_auth_array_value(auth, "apikey", "value").unwrap_or_default();
            let location = postman_auth_array_value(auth, "apikey", "in").unwrap_or_else(|| "header".to_string());
            if key.is_empty() {
                return;
            }
            if location == "query" {
                let raw_url = postman_url_to_string(request_url);
                let separator = if raw_url.contains('?') { "&" } else { "?" };
                headers.push(format!(
                    "X-Postman-Imported-Query-Auth: {raw_url}{separator}{}={}",
                    percent_encode(&key),
                    percent_encode(&value)
                ));
                warnings.push(format!(
                    "{request_name}: query API key was noted in headers; add it to the URL if needed"
                ));
            } else {
                headers.push(format!("{key}: {value}"));
            }
        }
        "oauth2" => {
            if let Some(token) = postman_auth_array_value(auth, "oauth2", "accessToken") {
                headers.push(format!("Authorization: Bearer {token}"));
            }
        }
        other => warnings.push(format!("{request_name}: auth type '{other}' is not fully supported")),
    }
}

fn extract_postman_variables(collection: &serde_json::Value, collection_name: &str) -> HashMap<String, String> {
    let mut variables = HashMap::new();
    if let Some(items) = collection.get("variable").and_then(|items| items.as_array()) {
        for item in items {
            let Some(key) = item.get("key").and_then(|key| key.as_str()).map(str::trim) else {
                continue;
            };
            if key.is_empty() {
                continue;
            }
            let value = item
                .get("value")
                .or_else(|| item.get("initialValue"))
                .and_then(|value| match value {
                    serde_json::Value::String(text) => Some(text.to_string()),
                    serde_json::Value::Null => Some(String::new()),
                    _ => Some(value.to_string()),
                })
                .unwrap_or_default();
            variables.insert(key.to_string(), value);
        }
    }
    if !variables.contains_key("collection_name") {
        variables.insert("collection_name".to_string(), collection_name.to_string());
    }
    variables
}

fn unique_request_name(requests: &[SavedRequest], base_name: &str, folder: Option<&str>) -> String {
    let mut count = 2;
    loop {
        let candidate = format!("{base_name} ({count})");
        if !requests
            .iter()
            .any(|request| request_identity_matches(request, &candidate, folder))
        {
            return candidate;
        }
        count += 1;
    }
}
