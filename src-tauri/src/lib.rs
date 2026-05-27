mod db;
mod grpc;

use std::error::Error as StdError;
use std::path::PathBuf;
use std::time::Instant;

use reqwest::{multipart, Method};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::{Manager, State};

fn format_error_chain(err: &(dyn StdError + 'static)) -> String {
    let mut out = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let msg = cause.to_string();
        if !out.contains(&msg) {
            out.push_str(" -> ");
            out.push_str(&msg);
        }
        source = cause.source();
    }
    out
}

use grpc::{GrpcRequestPayload, GrpcResponsePayload, ParseProtoPayload, ProtoServiceDescriptor};

#[derive(Debug, Deserialize)]
struct HttpRequestPayload {
    method: String,
    url: String,
    headers: Vec<HttpHeader>,
    #[serde(default)]
    body_type: String,
    body: Option<String>,
    #[serde(default)]
    form_data: Vec<HttpFormField>,
    #[serde(default)]
    binary_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HttpFormField {
    #[serde(default)]
    enabled: bool,
    key: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    field_type: String,
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    file_name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HttpHeader {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
struct HttpResponsePayload {
    status: u16,
    headers: Vec<HttpHeader>,
    body: String,
    elapsed_ms: u128,
    size_bytes: usize,
}

#[tauri::command]
async fn send_http_request(payload: HttpRequestPayload) -> Result<HttpResponsePayload, String> {
    let method = Method::from_bytes(payload.method.as_bytes())
        .map_err(|err| format!("invalid HTTP method: {err}"))?;

    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .build()
        .map_err(|err| format!("failed to build http client: {}", format_error_chain(&err)))?;

    let mut request = client.request(method, &payload.url);

    for header in &payload.headers {
        let key = header.key.trim();
        if key.is_empty() {
            continue;
        }
        let name = match reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
            Ok(name) => name,
            Err(err) => return Err(format!("invalid header name '{key}': {err}")),
        };
        let value = match reqwest::header::HeaderValue::from_str(header.value.trim()) {
            Ok(value) => value,
            Err(err) => return Err(format!("invalid header value for '{key}': {err}")),
        };
        request = request.header(name, value);
    }

    match payload.body_type.as_str() {
        "form-data" => {
            let mut form = multipart::Form::new();
            for field in payload.form_data {
                if !field.enabled || field.key.trim().is_empty() {
                    continue;
                }
                let key = field.key.trim().to_string();
                if field.field_type == "file" {
                    let path = field
                        .file_path
                        .as_deref()
                        .filter(|path| !path.trim().is_empty())
                        .ok_or_else(|| format!("form-data field '{key}' has no file selected"))?;
                    let bytes = tokio::fs::read(path)
                        .await
                        .map_err(|err| format!("failed to read form-data file '{path}': {err}"))?;
                    let file_name = field
                        .file_name
                        .filter(|name| !name.trim().is_empty())
                        .or_else(|| {
                            PathBuf::from(path)
                                .file_name()
                                .map(|name| name.to_string_lossy().to_string())
                        })
                        .unwrap_or_else(|| "file".to_string());
                    let part = multipart::Part::bytes(bytes).file_name(file_name);
                    form = form.part(key, part);
                } else {
                    form = form.text(key, field.value);
                }
            }
            request = request.multipart(form);
        }
        "binary" => {
            let path = payload
                .binary_path
                .as_deref()
                .filter(|path| !path.trim().is_empty())
                .ok_or_else(|| "binary body requires a selected file".to_string())?;
            let bytes = tokio::fs::read(path)
                .await
                .map_err(|err| format!("failed to read binary body file '{path}': {err}"))?;
            request = request.body(bytes);
        }
        _ => {
            if let Some(body) = payload.body {
                if !body.is_empty() {
                    request = request.body(body);
                }
            }
        }
    }

    let started = Instant::now();
    let response = request
        .send()
        .await
        .map_err(|err| format!("request failed: {}", format_error_chain(&err)))?;
    let elapsed_ms = started.elapsed().as_millis();

    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| HttpHeader {
            key: name.to_string(),
            value: value.to_str().unwrap_or("").to_string(),
        })
        .collect::<Vec<_>>();
    let body = response
        .text()
        .await
        .map_err(|err| format!("failed to read response body: {}", format_error_chain(&err)))?;
    let size_bytes = body.as_bytes().len();

    Ok(HttpResponsePayload {
        status,
        headers,
        body,
        elapsed_ms,
        size_bytes,
    })
}

#[tauri::command]
async fn send_grpc_request(
    payload: GrpcRequestPayload,
) -> Result<GrpcResponsePayload, String> {
    grpc::send_grpc_request(payload).await
}

#[tauri::command]
async fn parse_proto_services(
    payload: ParseProtoPayload,
) -> Result<Vec<ProtoServiceDescriptor>, String> {
    grpc::parse_proto_services(payload).await
}

#[tauri::command]
async fn load_state(pool: State<'_, SqlitePool>) -> Result<String, String> {
    let value = db::load_state_json(pool.inner())
        .await
        .map_err(|err| format!("failed to load state: {err:#}"))?;
    serde_json::to_string(&value).map_err(|err| format!("failed to encode state: {err}"))
}

#[tauri::command]
async fn save_state(pool: State<'_, SqlitePool>, state: String) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(&state)
        .map_err(|err| format!("invalid state json: {err}"))?;
    db::save_state_json(pool.inner(), &value)
        .await
        .map_err(|err| format!("failed to save state: {err:#}"))
}

#[tauri::command]
async fn record_history(
    pool: State<'_, SqlitePool>,
    entry: db::HistoryEntry,
) -> Result<i64, String> {
    db::insert_history(pool.inner(), &entry)
        .await
        .map_err(|err| format!("failed to record history: {err:#}"))
}

#[tauri::command]
async fn list_history(
    pool: State<'_, SqlitePool>,
    limit: Option<i64>,
    offset: Option<i64>,
    search: Option<String>,
) -> Result<serde_json::Value, String> {
    let rows = db::list_history(
        pool.inner(),
        limit.unwrap_or(db::HISTORY_DEFAULT_LIMIT),
        offset.unwrap_or(0),
        search,
    )
    .await
    .map_err(|err| format!("failed to list history: {err:#}"))?;
    Ok(serde_json::Value::Array(rows))
}

#[tauri::command]
async fn clear_history(pool: State<'_, SqlitePool>) -> Result<u64, String> {
    db::clear_history(pool.inner())
        .await
        .map_err(|err| format!("failed to clear history: {err:#}"))
}

#[tauri::command]
fn storage_path() -> String {
    let path: PathBuf = db::db_path();
    let display = path.to_string_lossy().to_string();
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy().to_string();
        if let Some(rest) = display.strip_prefix(&home_str) {
            return format!("~{}", rest);
        }
    }
    display
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let pool = tauri::async_runtime::block_on(db::init_pool())
                .map_err(|err| format!("failed to init sqlite: {err:#}"))?;
            app.manage(pool);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_http_request,
            send_grpc_request,
            parse_proto_services,
            load_state,
            save_state,
            record_history,
            list_history,
            clear_history,
            storage_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
