use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use dbx_core::models::connection::TransportLayerConfig;
use serde::Deserialize;

use crate::error::AppError;
use crate::state::WebState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTunnelProfilesRequest {
    pub profiles: Vec<TransportLayerConfig>,
}

pub async fn load_tunnel_profiles(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
) -> Result<Json<Vec<TransportLayerConfig>>, AppError> {
    state.app.storage.load_tunnel_profiles(&session.user_id).await.map(Json).map_err(AppError::from)
}

pub async fn save_tunnel_profiles(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Json(body): Json<SaveTunnelProfilesRequest>,
) -> Result<Json<()>, AppError> {
    state.app.storage.save_tunnel_profiles(&body.profiles, &session.user_id).await.map(Json).map_err(AppError::from)
}

pub async fn test_tunnel_profile(
    State(state): State<Arc<WebState>>,
    Json(profile): Json<TransportLayerConfig>,
) -> Result<Json<String>, AppError> {
    state.app.test_tunnel_profile(&profile).await.map(Json).map_err(AppError::from)
}
