use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::error::AppError;
use crate::state::WebState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLayoutRequest {
    pub layout: serde_json::Value,
}

pub async fn save_sidebar_layout(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Json(body): Json<SaveLayoutRequest>,
) -> Result<Json<()>, AppError> {
    state.app.storage.save_sidebar_layout_for_user(&session.user_id, &body.layout).await.map_err(AppError::from)?;
    Ok(Json(()))
}

pub async fn load_sidebar_layout(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
) -> Result<Json<serde_json::Value>, AppError> {
    let layout = state.app.storage.load_sidebar_layout_for_user(&session.user_id).await.map_err(AppError::from)?;
    Ok(Json(layout.unwrap_or(serde_json::json!(null))))
}
