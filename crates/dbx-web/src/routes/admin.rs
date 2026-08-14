use std::sync::Arc;

use axum::extract::{Multipart, State};
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use dbx_core::backup::{export_backup, import_backup, ExportRequest, ImportSummary};
use dbx_core::ldap::{test_ldap_bind, test_ldap_connection, test_ldap_search, LdapConfig, LdapTestResult};

use crate::error::AppError;
use crate::state::{UserSession, WebState};

// ===== LDAP Settings (Admin only) =====

pub async fn get_ldap_config(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<UserSession>,
) -> Result<Response, StatusCode> {
    if !session.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    let config = state.app.storage.load_ldap_config().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(config.masked()).into_response())
}

pub async fn update_ldap_config(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<UserSession>,
    Json(body): Json<LdapConfig>,
) -> Result<Response, StatusCode> {
    if !session.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    let existing = state.app.storage.load_ldap_config().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let merged = existing.merge_password(&body);
    state.app.storage.save_ldap_config(&merged).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response())
}

pub async fn test_ldap_connection_api(
    axum::extract::Extension(session): axum::extract::Extension<UserSession>,
    Json(body): Json<LdapTestRequest>,
) -> Result<Response, StatusCode> {
    if !session.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    let config = body.config;
    // LDAP operations are synchronous/blocking — run in spawn_blocking
    // to avoid stalling the async web server (which causes ERR_EMPTY_RESPONSE
    // and makes all other endpoints time out).
    let result = tokio::task::spawn_blocking(move || test_ldap_connection(&config)).await.map_err(|e| {
        log::error!("LDAP connection test panicked: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(result).into_response())
}

pub async fn test_ldap_search_api(
    axum::extract::Extension(session): axum::extract::Extension<UserSession>,
    Json(body): Json<LdapTestRequest>,
) -> Result<Response, StatusCode> {
    if !session.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    let config = body.config;
    let test_username = body.test_username.unwrap_or_default();
    let result = tokio::task::spawn_blocking(move || test_ldap_search(&config, &test_username)).await.map_err(|e| {
        log::error!("LDAP search test panicked: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(result).into_response())
}

pub async fn test_ldap_bind_api(
    axum::extract::Extension(session): axum::extract::Extension<UserSession>,
    Json(body): Json<LdapTestRequest>,
) -> Result<Response, StatusCode> {
    if !session.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    let config = body.config;
    let test_username = body.test_username.unwrap_or_default();
    let test_password = body.test_password.unwrap_or_default();
    let result = tokio::task::spawn_blocking(move || test_ldap_bind(&config, &test_username, &test_password))
        .await
        .map_err(|e| {
            log::error!("LDAP bind test panicked: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(result).into_response())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LdapTestRequest {
    pub config: LdapConfig,
    pub test_username: Option<String>,
    pub test_password: Option<String>,
}

// ===== Audit Log & SQL History (Admin only) =====

pub async fn get_audit_logs(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<UserSession>,
    axum::extract::Query(params): axum::extract::Query<AuditLogQuery>,
) -> Result<Response, StatusCode> {
    if !session.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    let logs = state
        .app
        .storage
        .load_audit_logs(
            params.limit.unwrap_or(100),
            params.offset.unwrap_or(0),
            params.user_id.as_deref(),
            params.action.as_deref(),
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(logs).into_response())
}

pub async fn get_sql_history_all(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<UserSession>,
    axum::extract::Query(params): axum::extract::Query<AuditLogQuery>,
) -> Result<Response, StatusCode> {
    if !session.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    let logs = state
        .app
        .storage
        .load_all_history_with_users(params.limit.unwrap_or(100), params.offset.unwrap_or(0), params.user_id.as_deref())
        .await
        .map_err(|e| {
            log::error!("Failed to load SQL history: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(logs).into_response())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub user_id: Option<String>,
    pub action: Option<String>,
}

// ===== Backup Export/Import =====

pub async fn export_backup_api(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<UserSession>,
    Json(body): Json<ExportRequest>,
) -> Result<Response, AppError> {
    let user = state
        .app
        .storage
        .get_user_by_id(&session.user_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::from("User not found"))?;

    let zip_data = export_backup(
        &state.app.storage,
        &session.user_id,
        &user.username,
        &user.display_name,
        env!("CARGO_PKG_VERSION"),
        &body,
    )
    .await
    .map_err(AppError::from)?;

    let filename = format!("dbx-backup-{}.zip", chrono::Utc::now().format("%Y%m%d%H%M%S"));

    Ok((
        StatusCode::OK,
        [("content-type", "application/zip"), ("content-disposition", &format!("attachment; filename=\"{filename}\""))],
        zip_data,
    )
        .into_response())
}

pub async fn import_backup_api(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<UserSession>,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let mut file_data: Option<Vec<u8>> = None;
    let mut passphrase = String::new();
    let mut mode = "merge".to_string();

    while let Ok(Some(mut field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                let data = field.bytes().await.map_err(|e| AppError::from(format!("Failed to read file: {e}")))?;
                file_data = Some(data.to_vec());
            }
            "passphrase" => {
                passphrase =
                    field.text().await.map_err(|e| AppError::from(format!("Failed to read passphrase: {e}")))?;
            }
            "mode" => {
                mode = field.text().await.map_err(|e| AppError::from(format!("Failed to read mode: {e}")))?;
            }
            _ => {}
        }
    }

    let file_data = file_data.ok_or_else(|| AppError::from("No backup file provided"))?;

    let request = dbx_core::backup::ImportRequest { passphrase, mode };

    let summary =
        import_backup(&state.app.storage, &session.user_id, &file_data, &request).await.map_err(AppError::from)?;

    Ok(Json(summary).into_response())
}
