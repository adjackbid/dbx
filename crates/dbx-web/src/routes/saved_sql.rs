use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use dbx_core::saved_sql::{SavedSqlFile, SavedSqlFolder, SavedSqlLibrary};

use crate::error::AppError;
use crate::state::WebState;

pub async fn load_saved_sql_library(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
) -> Result<Json<SavedSqlLibrary>, AppError> {
    let library = state.app.storage.load_saved_sql_library_summary(&session.user_id).await.map_err(AppError::from)?;
    Ok(Json(library))
}

pub async fn load_saved_sql_file(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Path(id): Path<String>,
) -> Result<Json<Option<SavedSqlFile>>, AppError> {
    let file = state.app.storage.load_saved_sql_file(&id, &session.user_id).await.map_err(AppError::from)?;
    Ok(Json(file))
}

pub async fn save_saved_sql_folder(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Json(folder): Json<SavedSqlFolder>,
) -> Result<Json<SavedSqlFolder>, AppError> {
    state.app.storage.save_saved_sql_folder(&folder, &session.user_id).await.map_err(AppError::from)?;
    Ok(Json(folder))
}

pub async fn delete_saved_sql_folder(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Path(id): Path<String>,
) -> Result<Json<()>, AppError> {
    state.app.storage.delete_saved_sql_folder(&id, &session.user_id).await.map_err(AppError::from)?;
    Ok(Json(()))
}

pub async fn save_saved_sql_file(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Json(file): Json<SavedSqlFile>,
) -> Result<Json<SavedSqlFile>, AppError> {
    state.app.storage.save_saved_sql_file(&file, &session.user_id).await.map_err(AppError::from)?;
    Ok(Json(file))
}

pub async fn delete_saved_sql_file(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Path(id): Path<String>,
) -> Result<Json<()>, AppError> {
    state.app.storage.delete_saved_sql_file(&id, &session.user_id).await.map_err(AppError::from)?;
    Ok(Json(()))
}
