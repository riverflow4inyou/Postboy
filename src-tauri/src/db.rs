use std::path::PathBuf;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS folder (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    expanded    INTEGER NOT NULL DEFAULT 1,
    sort_order  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS request (
    id            TEXT PRIMARY KEY,
    folder_id     TEXT NOT NULL,
    name          TEXT NOT NULL,
    kind          TEXT NOT NULL DEFAULT 'http',
    method        TEXT NOT NULL,
    url           TEXT NOT NULL,
    params_json   TEXT NOT NULL DEFAULT '[]',
    headers_json  TEXT NOT NULL DEFAULT '[]',
    body_type     TEXT NOT NULL DEFAULT 'none',
    raw_lang      TEXT NOT NULL DEFAULT 'JSON',
    body          TEXT NOT NULL DEFAULT '',
    body_rows_json TEXT NOT NULL DEFAULT '[]',
    binary_path   TEXT NOT NULL DEFAULT '',
    grpc_json     TEXT NOT NULL DEFAULT '{}',
    sort_order    INTEGER NOT NULL DEFAULT 0,
    updated_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_request_folder ON request(folder_id);

CREATE TABLE IF NOT EXISTS environment (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    kind            TEXT NOT NULL,
    variables_json  TEXT NOT NULL DEFAULT '[]',
    headers_json    TEXT NOT NULL DEFAULT '[]',
    sort_order      INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS history (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id    TEXT,
    method        TEXT NOT NULL,
    url           TEXT NOT NULL,
    status        INTEGER,
    elapsed_ms    INTEGER,
    size_bytes    INTEGER,
    error         TEXT,
    sent_at       INTEGER NOT NULL,
    snapshot_json TEXT NOT NULL DEFAULT '{}',
    response_json TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_history_sent ON history(sent_at DESC);

CREATE TABLE IF NOT EXISTS kv_setting (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);
"#;

pub const HISTORY_DEFAULT_LIMIT: i64 = 100;
const HISTORY_MAX_STORED: i64 = 100;

pub fn data_dir() -> PathBuf {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(".postboy")
}

pub fn db_path() -> PathBuf {
    data_dir().join("postboy.db")
}

pub fn legacy_json_path() -> PathBuf {
    data_dir().join("data.json")
}

pub async fn init_pool() -> Result<SqlitePool> {
    let dir = data_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("failed to create data dir {}", dir.display()))?;

    let path = db_path();
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .context("failed to open sqlite pool")?;

    sqlx::query(SCHEMA_SQL)
        .execute(&pool)
        .await
        .context("failed to apply schema")?;

    ensure_request_columns(&pool).await?;
    ensure_environment_columns(&pool).await?;
    ensure_history_columns(&pool).await?;

    migrate_legacy_json_if_present(&pool).await.ok();

    Ok(pool)
}

/// Idempotent migration for older DBs that pre-date the `kind` / `grpc_json`
/// columns on the `request` table. SQLite has no `ADD COLUMN IF NOT EXISTS`,
/// so we check first and only ALTER when missing.
async fn ensure_request_columns(pool: &SqlitePool) -> Result<()> {
    let rows = sqlx::query("PRAGMA table_info('request')")
        .fetch_all(pool)
        .await
        .context("failed to inspect request schema")?;
    let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for row in rows {
        names.insert(row.get::<String, _>("name"));
    }
    if !names.contains("kind") {
        sqlx::query("ALTER TABLE request ADD COLUMN kind TEXT NOT NULL DEFAULT 'http'")
            .execute(pool)
            .await
            .context("failed to add request.kind column")?;
    }
    if !names.contains("grpc_json") {
        sqlx::query("ALTER TABLE request ADD COLUMN grpc_json TEXT NOT NULL DEFAULT '{}'")
            .execute(pool)
            .await
            .context("failed to add request.grpc_json column")?;
    }
    if !names.contains("body_rows_json") {
        sqlx::query("ALTER TABLE request ADD COLUMN body_rows_json TEXT NOT NULL DEFAULT '[]'")
            .execute(pool)
            .await
            .context("failed to add request.body_rows_json column")?;
    }
    if !names.contains("binary_path") {
        sqlx::query("ALTER TABLE request ADD COLUMN binary_path TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await
            .context("failed to add request.binary_path column")?;
    }
    Ok(())
}

async fn ensure_environment_columns(pool: &SqlitePool) -> Result<()> {
    let rows = sqlx::query("PRAGMA table_info('environment')")
        .fetch_all(pool)
        .await
        .context("failed to inspect environment schema")?;
    let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for row in rows {
        names.insert(row.get::<String, _>("name"));
    }
    if !names.contains("headers_json") {
        sqlx::query("ALTER TABLE environment ADD COLUMN headers_json TEXT NOT NULL DEFAULT '[]'")
            .execute(pool)
            .await
            .context("failed to add environment.headers_json column")?;
    }
    Ok(())
}

/// Idempotent migration that backfills the `snapshot_json` column on the
/// `history` table for users upgrading from earlier builds that only stored
/// method/url metadata.
async fn ensure_history_columns(pool: &SqlitePool) -> Result<()> {
    let rows = sqlx::query("PRAGMA table_info('history')")
        .fetch_all(pool)
        .await
        .context("failed to inspect history schema")?;
    let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for row in rows {
        names.insert(row.get::<String, _>("name"));
    }
    if !names.contains("snapshot_json") {
        sqlx::query("ALTER TABLE history ADD COLUMN snapshot_json TEXT NOT NULL DEFAULT '{}'")
            .execute(pool)
            .await
            .context("failed to add history.snapshot_json column")?;
    }
    if !names.contains("response_json") {
        sqlx::query("ALTER TABLE history ADD COLUMN response_json TEXT NOT NULL DEFAULT '{}'")
            .execute(pool)
            .await
            .context("failed to add history.response_json column")?;
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryEntry {
    #[serde(default)]
    pub request_id: Option<String>,
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub status: Option<i64>,
    #[serde(default)]
    pub elapsed_ms: Option<i64>,
    #[serde(default)]
    pub size_bytes: Option<i64>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub sent_at: Option<i64>,
    /// Full snapshot of the request that produced this entry, so the UI can
    /// fully restore headers/body/params when the user clicks a history row.
    #[serde(default)]
    pub snapshot: Option<Value>,
    /// Full last-response payload (status/headers/body/elapsed/size/lang/error)
    /// so the UI can restore the response panel when reopening a history row.
    #[serde(default)]
    pub response: Option<Value>,
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Load full app state as a JSON object (compatible with the frontend `PersistedState`).
pub async fn load_state_json(pool: &SqlitePool) -> Result<Value> {
    let folders: Vec<Value> = sqlx::query(
        "SELECT id, name, expanded FROM folder ORDER BY sort_order ASC, name ASC",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        json!({
            "id": row.get::<String, _>("id"),
            "name": row.get::<String, _>("name"),
            "expanded": row.get::<i64, _>("expanded") != 0,
        })
    })
    .collect();

    let requests: Vec<Value> = sqlx::query(
        "SELECT id, folder_id, name, kind, method, url, params_json, headers_json,
                body_type, raw_lang, body, body_rows_json, binary_path, grpc_json
         FROM request ORDER BY folder_id ASC, sort_order ASC, updated_at ASC",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        let params: Value = serde_json::from_str(&row.get::<String, _>("params_json"))
            .unwrap_or_else(|_| json!([]));
        let headers: Value = serde_json::from_str(&row.get::<String, _>("headers_json"))
            .unwrap_or_else(|_| json!([]));
        let body_rows: Value = serde_json::from_str(&row.get::<String, _>("body_rows_json"))
            .unwrap_or_else(|_| json!([]));
        let grpc: Value = serde_json::from_str(&row.get::<String, _>("grpc_json"))
            .unwrap_or_else(|_| json!({}));
        let kind = row.get::<String, _>("kind");
        let kind = if kind.is_empty() { "http".to_string() } else { kind };
        json!({
            "id": row.get::<String, _>("id"),
            "folderId": row.get::<String, _>("folder_id"),
            "name": row.get::<String, _>("name"),
            "kind": kind,
            "method": row.get::<String, _>("method"),
            "url": row.get::<String, _>("url"),
            "params": params,
            "headers": headers,
            "bodyType": row.get::<String, _>("body_type"),
            "rawLang": row.get::<String, _>("raw_lang"),
            "body": row.get::<String, _>("body"),
            "bodyRows": body_rows,
            "binaryPath": row.get::<String, _>("binary_path"),
            "grpc": grpc,
        })
    })
    .collect();

    let environments: Vec<Value> = sqlx::query(
        "SELECT id, name, kind, variables_json, headers_json FROM environment ORDER BY sort_order ASC",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        let variables: Value = serde_json::from_str(&row.get::<String, _>("variables_json"))
            .unwrap_or_else(|_| json!([]));
        let headers: Value = serde_json::from_str(&row.get::<String, _>("headers_json"))
            .unwrap_or_else(|_| json!([]));
        json!({
            "id": row.get::<String, _>("id"),
            "name": row.get::<String, _>("name"),
            "kind": row.get::<String, _>("kind"),
            "variables": variables,
            "headers": headers,
        })
    })
    .collect();

    let history = list_history(pool, HISTORY_DEFAULT_LIMIT, 0, None).await?;

    let settings = load_settings(pool).await?;
    let s = |k: &str| -> Option<&Value> { settings.get(k) };

    let state = json!({
        "folders": folders,
        "requests": requests,
        "environments": environments,
        "activeEnvId": s("active_env_id").cloned().unwrap_or(Value::String(String::new())),
        "selectedEnvId": s("selected_env_id").cloned().unwrap_or(Value::String(String::new())),
        "openTabIds": s("open_tab_ids").cloned().unwrap_or_else(|| json!([])),
        "activeTabId": s("active_tab_id").cloned().unwrap_or(Value::String(String::new())),
        "history": history,
        "sidebarWidth": s("sidebar_width").cloned().unwrap_or(json!(280)),
        "requestPaneHeight": s("request_pane_height").cloned().unwrap_or(json!(360)),
        "theme": s("theme").cloned().unwrap_or(Value::String("dark".into())),
    });

    Ok(state)
}

/// Persist full app state atomically. History is intentionally ignored here —
/// it is appended via [`insert_history`] when each request finishes, so we
/// never want a debounced save to clobber rows we just wrote.
pub async fn save_state_json(pool: &SqlitePool, state: &Value) -> Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM folder").execute(&mut *tx).await?;
    if let Some(arr) = state.get("folders").and_then(|v| v.as_array()) {
        for (idx, folder) in arr.iter().enumerate() {
            sqlx::query(
                "INSERT INTO folder (id, name, expanded, sort_order) VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(string_field(folder, "id"))
            .bind(string_field(folder, "name"))
            .bind(if folder.get("expanded").and_then(|v| v.as_bool()).unwrap_or(true) {
                1_i64
            } else {
                0_i64
            })
            .bind(idx as i64)
            .execute(&mut *tx)
            .await?;
        }
    }

    sqlx::query("DELETE FROM request").execute(&mut *tx).await?;
    if let Some(arr) = state.get("requests").and_then(|v| v.as_array()) {
        let now = now_ms();
        for (idx, req) in arr.iter().enumerate() {
            let params = req.get("params").cloned().unwrap_or_else(|| json!([]));
            let headers = req.get("headers").cloned().unwrap_or_else(|| json!([]));
            let body_rows = req.get("bodyRows").cloned().unwrap_or_else(|| json!([]));
            let grpc = req.get("grpc").cloned().unwrap_or_else(|| json!({}));
            sqlx::query(
                "INSERT INTO request (id, folder_id, name, kind, method, url, params_json,
                                      headers_json, body_type, raw_lang, body, body_rows_json,
                                      binary_path, grpc_json,
                                      sort_order, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            )
            .bind(string_field(req, "id"))
            .bind(string_field(req, "folderId"))
            .bind(string_field(req, "name"))
            .bind(string_field_or(req, "kind", "http"))
            .bind(string_field(req, "method"))
            .bind(string_field(req, "url"))
            .bind(params.to_string())
            .bind(headers.to_string())
            .bind(string_field_or(req, "bodyType", "none"))
            .bind(string_field_or(req, "rawLang", "JSON"))
            .bind(string_field(req, "body"))
            .bind(body_rows.to_string())
            .bind(string_field(req, "binaryPath"))
            .bind(grpc.to_string())
            .bind(idx as i64)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
    }

    sqlx::query("DELETE FROM environment").execute(&mut *tx).await?;
    if let Some(arr) = state.get("environments").and_then(|v| v.as_array()) {
        for (idx, env) in arr.iter().enumerate() {
            let variables = env.get("variables").cloned().unwrap_or_else(|| json!([]));
            let headers = env.get("headers").cloned().unwrap_or_else(|| json!([]));
            sqlx::query(
                "INSERT INTO environment (id, name, kind, variables_json, headers_json, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .bind(string_field(env, "id"))
            .bind(string_field(env, "name"))
            .bind(string_field_or(env, "kind", "custom"))
            .bind(variables.to_string())
            .bind(headers.to_string())
            .bind(idx as i64)
            .execute(&mut *tx)
            .await?;
        }
    }

    upsert_setting_tx(&mut tx, "active_env_id", state.get("activeEnvId")).await?;
    upsert_setting_tx(&mut tx, "selected_env_id", state.get("selectedEnvId")).await?;
    upsert_setting_tx(&mut tx, "open_tab_ids", state.get("openTabIds")).await?;
    upsert_setting_tx(&mut tx, "active_tab_id", state.get("activeTabId")).await?;
    upsert_setting_tx(&mut tx, "sidebar_width", state.get("sidebarWidth")).await?;
    upsert_setting_tx(
        &mut tx,
        "request_pane_height",
        state.get("requestPaneHeight"),
    )
    .await?;
    upsert_setting_tx(&mut tx, "theme", state.get("theme")).await?;

    tx.commit().await?;
    Ok(())
}

pub async fn insert_history(pool: &SqlitePool, entry: &HistoryEntry) -> Result<i64> {
    let sent_at = entry.sent_at.unwrap_or_else(now_ms);
    let snapshot = entry
        .snapshot
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "{}".to_string());
    let response = entry
        .response
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "{}".to_string());
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        "INSERT INTO history (request_id, method, url, status, elapsed_ms, size_bytes, error,
                              sent_at, snapshot_json, response_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         RETURNING id",
    )
    .bind(&entry.request_id)
    .bind(&entry.method)
    .bind(&entry.url)
    .bind(entry.status)
    .bind(entry.elapsed_ms)
    .bind(entry.size_bytes)
    .bind(&entry.error)
    .bind(sent_at)
    .bind(snapshot)
    .bind(response)
    .fetch_one(&mut *tx)
    .await?;
    let id = row.get::<i64, _>("id");

    sqlx::query(
        "DELETE FROM history
         WHERE id NOT IN (
             SELECT id FROM history ORDER BY sent_at DESC, id DESC LIMIT ?1
         )",
    )
    .bind(HISTORY_MAX_STORED)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(id)
}

pub async fn list_history(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
    search: Option<String>,
) -> Result<Vec<Value>> {
    let limit = limit.clamp(1, HISTORY_MAX_STORED);
    let offset = offset.max(0);
    let search = search.unwrap_or_default();
    let like = format!("%{}%", search);

    let rows = if search.is_empty() {
        sqlx::query(
            "SELECT id, request_id, method, url, status, elapsed_ms, size_bytes, error,
                    sent_at, snapshot_json, response_json
             FROM history ORDER BY sent_at DESC LIMIT ?1 OFFSET ?2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT id, request_id, method, url, status, elapsed_ms, size_bytes, error,
                    sent_at, snapshot_json, response_json
             FROM history WHERE url LIKE ?1 OR method LIKE ?1
             ORDER BY sent_at DESC LIMIT ?2 OFFSET ?3",
        )
        .bind(like)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(|row| {
            let id: i64 = row.get("id");
            let status: Option<i64> = row.try_get("status").ok();
            // Old rows have `{}` after the ALTER TABLE migration; expose them as
            // null so the UI can fall back to the basic method/url restore.
            let parse_blob = |col: &str| -> Value {
                let raw = row
                    .try_get::<String, _>(col)
                    .unwrap_or_else(|_| String::from("{}"));
                let parsed: Value =
                    serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
                match &parsed {
                    Value::Object(map) if map.is_empty() => Value::Null,
                    _ => parsed,
                }
            };
            json!({
                "id": id.to_string(),
                "requestId": row.try_get::<Option<String>, _>("request_id").unwrap_or(None),
                "method": row.get::<String, _>("method"),
                "url": row.get::<String, _>("url"),
                "status": status,
                "elapsedMs": row.try_get::<Option<i64>, _>("elapsed_ms").unwrap_or(None),
                "sizeBytes": row.try_get::<Option<i64>, _>("size_bytes").unwrap_or(None),
                "error": row.try_get::<Option<String>, _>("error").unwrap_or(None),
                "timestamp": row.get::<i64, _>("sent_at"),
                "snapshot": parse_blob("snapshot_json"),
                "response": parse_blob("response_json"),
            })
        })
        .collect())
}

pub async fn clear_history(pool: &SqlitePool) -> Result<u64> {
    let res = sqlx::query("DELETE FROM history").execute(pool).await?;
    Ok(res.rows_affected())
}

async fn load_settings(pool: &SqlitePool) -> Result<serde_json::Map<String, Value>> {
    let rows = sqlx::query("SELECT key, value FROM kv_setting")
        .fetch_all(pool)
        .await?;
    let mut map = serde_json::Map::new();
    for row in rows {
        let key: String = row.get("key");
        let raw: String = row.get("value");
        let val: Value = serde_json::from_str(&raw).unwrap_or(Value::String(raw));
        map.insert(key, val);
    }
    Ok(map)
}

async fn upsert_setting_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &str,
    value: Option<&Value>,
) -> Result<()> {
    let serialized = match value {
        None | Some(Value::Null) => return Ok(()),
        Some(v) => v.to_string(),
    };
    sqlx::query(
        "INSERT INTO kv_setting (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(serialized)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn string_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

fn string_field_or(v: &Value, key: &str, default: &str) -> String {
    let s = v.get(key).and_then(|x| x.as_str()).unwrap_or(default);
    if s.is_empty() {
        default.to_string()
    } else {
        s.to_string()
    }
}

async fn migrate_legacy_json_if_present(pool: &SqlitePool) -> Result<()> {
    let path = legacy_json_path();
    if !path.exists() {
        return Ok(());
    }
    let count: i64 = sqlx::query("SELECT COUNT(*) AS c FROM request")
        .fetch_one(pool)
        .await?
        .get("c");
    if count > 0 {
        return Ok(());
    }

    let raw = tokio::fs::read_to_string(&path).await?;
    if raw.trim().is_empty() {
        return Ok(());
    }
    let state: Value = serde_json::from_str(&raw).context("legacy data.json is not valid JSON")?;

    save_state_json(pool, &state).await?;

    if let Some(history) = state.get("history").and_then(|v| v.as_array()) {
        for entry in history {
            let method = entry.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
            let url = entry.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let status = entry.get("status").and_then(|v| v.as_i64());
            let ts = entry
                .get("timestamp")
                .and_then(|v| v.as_i64())
                .unwrap_or_else(now_ms);
            sqlx::query(
                "INSERT INTO history (method, url, status, sent_at) VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(method)
            .bind(url)
            .bind(status)
            .bind(ts)
            .execute(pool)
            .await
            .ok();
        }
    }

    let archive = path.with_extension("json.migrated");
    let _ = tokio::fs::rename(&path, &archive).await;
    Ok(())
}
