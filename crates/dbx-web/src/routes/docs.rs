use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use axum::extract::{Extension, State};
use axum::Json;
use dbx_core::docs::{CollectOptions, SchemaSnapshot};
use dbx_core::models::connection::ConnectionConfig;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::{UserSession, WebState};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsSnapshotRequest {
    pub connection_id: String,
    pub database: String,
    #[serde(default)]
    pub schemas: Vec<String>,
    #[serde(default)]
    pub tables: Vec<String>,
    #[serde(default)]
    pub project_name: Option<String>,
}

/// Connections are scoped to the signed-in account: a connection id belonging to
/// another account must resolve to "not found" instead of leaking its schema.
async fn load_connection(
    state: &Arc<WebState>,
    user_id: &str,
    connection_id: &str,
) -> Result<ConnectionConfig, AppError> {
    state
        .app
        .storage
        .load_connections(user_id)
        .await
        .map_err(AppError::from)?
        .into_iter()
        .find(|config| config.id == connection_id)
        .ok_or_else(|| AppError::from(format!("Connection with id '{connection_id}' not found")))
}

pub async fn collect_snapshot(
    State(state): State<Arc<WebState>>,
    Extension(session): Extension<UserSession>,
    Json(request): Json<DocsSnapshotRequest>,
) -> Result<Json<SchemaSnapshot>, AppError> {
    let connection = load_connection(&state, &session.user_id, &request.connection_id).await?;

    let options = CollectOptions {
        database: request.database.clone(),
        schemas: request.schemas.clone(),
        tables: request.tables.clone(),
        project_name: request.project_name.clone().unwrap_or_else(|| connection.name.clone()),
    };

    let snapshot =
        dbx_core::docs::collect_snapshot(&state.app, &connection, &options, &|_progress| {}, &AtomicBool::new(false))
            .await
            .map_err(AppError::from)?;

    Ok(Json(snapshot))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsAnnotationsRequest {
    pub connection_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsApplyRequest {
    pub connection_id: String,
    pub snapshot: SchemaSnapshot,
    pub annotations: dbx_core::docs::annotations::AnnotationFile,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsSaveRequest {
    pub connection_id: String,
    pub annotations: dbx_core::docs::annotations::AnnotationFile,
}

/// `WebState` already carries the data directory (`pub data_dir: PathBuf`), so
/// this needs no lookup and no new dependency — dbx-web does NOT depend on
/// dbx-mcp.
fn notes_path_for(state: &Arc<WebState>, config: &ConnectionConfig) -> std::path::PathBuf {
    dbx_core::docs::annotations::resolve_notes_path(&config.id, config.docs_notes_path.as_deref(), &state.data_dir)
}

pub async fn load_annotations(
    State(state): State<Arc<WebState>>,
    Extension(session): Extension<UserSession>,
    Json(request): Json<DocsAnnotationsRequest>,
) -> Result<Json<Option<dbx_core::docs::annotations::AnnotationFile>>, AppError> {
    let config = load_connection(&state, &session.user_id, &request.connection_id).await?;
    let path = notes_path_for(&state, &config);
    Ok(Json(dbx_core::docs::annotations::load_annotations(&path).map_err(AppError::from)?))
}

pub async fn apply_annotations(
    State(state): State<Arc<WebState>>,
    Extension(session): Extension<UserSession>,
    Json(request): Json<DocsApplyRequest>,
) -> Result<Json<SchemaSnapshot>, AppError> {
    let config = load_connection(&state, &session.user_id, &request.connection_id).await?;
    let mut applied = request.snapshot;
    dbx_core::docs::annotations::apply_annotations(&mut applied, &request.annotations, config.db_type);
    Ok(Json(applied))
}

pub async fn save_annotations(
    State(state): State<Arc<WebState>>,
    Extension(session): Extension<UserSession>,
    Json(request): Json<DocsSaveRequest>,
) -> Result<Json<()>, AppError> {
    let config = load_connection(&state, &session.user_id, &request.connection_id).await?;
    let path = notes_path_for(&state, &config);
    dbx_core::docs::annotations::save_annotations(&path, &request.annotations).map_err(AppError::from)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsExportRequest {
    pub snapshot: SchemaSnapshot,
    pub annotations: dbx_core::docs::annotations::AnnotationFile,
    pub lang: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsExportResponse {
    pub content: String,
}

/// Returns the rendered HTML as a string rather than writing a file: the
/// browser has no filesystem to write to, so `http.ts` downloads this content
/// as a blob instead of the Tauri command's `std::fs::write`.
pub async fn export_html(Json(request): Json<DocsExportRequest>) -> Result<Json<DocsExportResponse>, AppError> {
    let content = dbx_core::docs::to_standalone_html(&request.snapshot, &request.annotations, &request.lang)
        .map_err(AppError::from)?;
    Ok(Json(DocsExportResponse { content }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbx_core::connection::AppState;
    use dbx_core::storage::Storage;
    use serde_json::json;

    async fn state_with_connection(user_id: &str, connection_id: &str) -> (Arc<WebState>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dbx-docs-scope-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let storage = Storage::open(&dir.join("storage.db")).await.unwrap();
        let config: ConnectionConfig = serde_json::from_value(json!({
            "id": connection_id,
            "name": "docs scope",
            "db_type": "sqlite",
            "host": dir.join("query.db").to_str().unwrap(),
            "port": 0,
            "username": "",
            "password": "",
            "database": "main",
            "save_password": false
        }))
        .unwrap();
        storage.save_connections(std::slice::from_ref(&config), user_id).await.unwrap();
        let app = Arc::new(AppState::new_with_plugin_dir(storage, dir.join("plugins")));
        (Arc::new(WebState::for_tests(app, dir.clone())), dir)
    }

    #[tokio::test]
    async fn connection_lookup_is_scoped_to_the_signed_in_account() {
        let (state, dir) = state_with_connection("user-a", "scoped-connection").await;

        let owned = load_connection(&state, "user-a", "scoped-connection").await.unwrap();
        assert_eq!(owned.id, "scoped-connection");

        let other = load_connection(&state, "user-b", "scoped-connection").await;
        assert!(other.is_err(), "another account must not resolve this connection id");

        std::fs::remove_dir_all(&dir).ok();
    }
}
