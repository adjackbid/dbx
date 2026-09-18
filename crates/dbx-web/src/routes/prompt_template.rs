use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use dbx_core::prompt_template::PromptTemplate;

use crate::error::AppError;
use crate::state::WebState;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePromptTemplateRequest {
    pub id: String,
    pub name: String,
    pub content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetGlobalInstructionsRequest {
    pub content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetGlobalInstructionsResponse {
    pub content: String,
}

// ---------------------------------------------------------------------------
// Prompt Templates CRUD
// ---------------------------------------------------------------------------

pub async fn load_prompt_templates(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
) -> Result<Json<Vec<PromptTemplate>>, AppError> {
    let templates = state.app.storage.load_prompt_templates(&session.user_id).await.map_err(AppError::from)?;
    Ok(Json(templates))
}

pub async fn save_prompt_template(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Json(body): Json<SavePromptTemplateRequest>,
) -> Result<Json<PromptTemplate>, AppError> {
    let result =
        state.app.storage.save_prompt_template(&body.id, &body.name, &body.content, &session.user_id).await.map_err(
            |e| {
                // Map validation errors to 400
                if e.contains("too long") || e.contains("cannot be empty") || e.contains("duplicate") {
                    AppError::bad_request(e)
                } else {
                    AppError::from(e)
                }
            },
        )?;
    Ok(Json(result))
}

pub async fn delete_prompt_template(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Path(id): Path<String>,
) -> Result<Json<()>, AppError> {
    state.app.storage.delete_prompt_template(&id, &session.user_id).await.map_err(|e| {
        if e.contains("not found") {
            AppError::not_found(e)
        } else {
            AppError::from(e)
        }
    })?;
    Ok(Json(()))
}

// ---------------------------------------------------------------------------
// Global Custom Instructions
// ---------------------------------------------------------------------------

pub async fn get_global_instructions(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
) -> Result<Json<GetGlobalInstructionsResponse>, AppError> {
    let content =
        state.app.storage.load_ai_global_custom_instructions(&session.user_id).await.map_err(AppError::from)?;
    Ok(Json(GetGlobalInstructionsResponse { content }))
}

pub async fn set_global_instructions(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Json(body): Json<SetGlobalInstructionsRequest>,
) -> Result<Json<()>, AppError> {
    state.app.storage.save_ai_global_custom_instructions(&body.content, &session.user_id).await.map_err(|e| {
        if e.contains("too long") {
            AppError::bad_request(e)
        } else {
            AppError::from(e)
        }
    })?;
    Ok(Json(()))
}
