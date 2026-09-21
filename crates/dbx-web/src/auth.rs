use std::sync::Arc;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use dbx_core::user::{CreateUserRequest, UpdateUserRequest, UserInfo};

use crate::state::{LoginRateLimit, WebState};

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: Option<String>,
    pub password: String,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

#[derive(Serialize)]
pub struct AuthCheckResponse {
    pub authenticated: bool,
    pub required: bool,
    pub setup_required: bool,
    pub user: Option<AuthUserInfo>,
    /// Running build identity, so the sign-in screen can show which version is
    /// being served without requiring a session.
    pub version: String,
    pub commit: String,
    /// Keeps the wire name identical to `/api/version`'s `buildTimeMs`.
    #[serde(rename = "buildTimeMs")]
    pub build_time_ms: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AuthUserInfo {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub is_admin: bool,
    pub auth_source: String,
}

const MAX_ATTEMPTS: u32 = 5;
const LOCKOUT_SECS: u64 = 60;

fn session_cookie_path(state: &WebState) -> &str {
    state.public_base_path.as_str()
}

fn api_path_suffix<'a>(path: &'a str, public_base_path: &str) -> Option<&'a str> {
    if let Some(suffix) = path.strip_prefix("/api/") {
        return Some(suffix);
    }
    let base = public_base_path.trim_end_matches('/');
    if base.is_empty() || base == "/" {
        return None;
    }
    path.strip_prefix(base)?.strip_prefix("/api/")
}

fn middleware_api_path_suffix<'a>(path: &'a str, public_base_path: &str) -> Option<&'a str> {
    if let Some(suffix) = api_path_suffix(path, public_base_path) {
        return Some(suffix);
    }

    let base = public_base_path.trim_end_matches('/');
    if !base.is_empty() && base != "/" && path.strip_prefix(base).is_some() {
        return None;
    }

    path.strip_prefix('/').filter(|suffix| !suffix.is_empty())
}

pub async fn login(State(state): State<Arc<WebState>>, Json(body): Json<LoginRequest>) -> Result<Response, StatusCode> {
    let storage = &state.app.storage;

    // Check if any users exist — if not, we're in setup mode
    let user_count = storage.count_users().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if user_count == 0 {
        // Legacy compatibility: no users table content, allow first-time setup
        return Ok((StatusCode::OK, Json(serde_json::json!({"ok": true, "setup_required": true}))).into_response());
    }

    let username = match body.username {
        Some(ref u) if !u.trim().is_empty() => u.trim().to_string(),
        _ => {
            return Ok(
                (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Username is required"}))).into_response()
            );
        }
    };

    // Check rate limit (per-user)
    {
        let mut rl_map = state.login_rate_limit.lock().await;
        let rl = rl_map.entry(username.clone()).or_insert(LoginRateLimit { fail_count: 0, locked_until: None });
        if let Some(locked_until) = rl.locked_until {
            if locked_until > std::time::Instant::now() {
                let remaining = (locked_until - std::time::Instant::now()).as_secs();
                return Ok((
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(serde_json::json!({"error": format!("Please try again in {remaining}s")})),
                )
                    .into_response());
            }
        }
    }

    let user =
        storage.verify_user_password(&username, &body.password).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // If local verification failed, try LDAP
    let user = match user {
        Some(u) => Some(u),
        None => {
            // Check if the user exists with LDAP auth_source
            let existing_user =
                storage.get_user_by_username(&username).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            match existing_user {
                Some(u) if u.auth_source == dbx_core::user::AuthSource::Ldap && u.is_active => {
                    // Verify via LDAP — MUST run in spawn_blocking (ldap3 is sync/blocking)
                    let ldap_config =
                        storage.load_ldap_config().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    if ldap_config.enabled {
                        let ldap_config_clone = ldap_config.clone();
                        let username_clone = username.clone();
                        let password_clone = body.password.clone();
                        let result = tokio::task::spawn_blocking(move || {
                            dbx_core::ldap::authenticate_ldap(&ldap_config_clone, &username_clone, &password_clone)
                        })
                        .await
                        .map_err(|e| {
                            log::error!("LDAP auth panicked: {e}");
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;
                        match result {
                            Ok(details) => {
                                if !details.display_name.is_empty() && details.display_name != u.display_name {
                                    let _ = storage
                                        .update_user(
                                            &u.id,
                                            &dbx_core::user::UpdateUserRequest {
                                                display_name: Some(details.display_name),
                                                is_admin: None,
                                                is_active: None,
                                            },
                                        )
                                        .await;
                                }
                                Some(u)
                            }
                            Err(_) => None,
                        }
                    } else {
                        None
                    }
                }
                Some(_) => None,
                None => {
                    // User doesn't exist locally — try LDAP auto-create
                    let ldap_config =
                        storage.load_ldap_config().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    if ldap_config.enabled && ldap_config.auto_create_user {
                        let ldap_config_clone = ldap_config.clone();
                        let username_clone = username.clone();
                        let password_clone = body.password.clone();
                        let result = tokio::task::spawn_blocking(move || {
                            dbx_core::ldap::authenticate_ldap(&ldap_config_clone, &username_clone, &password_clone)
                        })
                        .await
                        .map_err(|e| {
                            log::error!("LDAP auth (auto-create) panicked: {e}");
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;
                        match result {
                            Ok(details) => {
                                let is_admin = details.is_admin_by_filter;
                                match storage
                                    .create_ldap_user(
                                        &details.username,
                                        &details.display_name,
                                        &details.user_dn,
                                        is_admin,
                                    )
                                    .await
                                {
                                    Ok(u) => Some(u),
                                    Err(e) => {
                                        log::warn!("Failed to auto-create LDAP user: {e}");
                                        None
                                    }
                                }
                            }
                            Err(_) => None,
                        }
                    } else {
                        None
                    }
                }
            }
        }
    };

    if user.is_none() {
        let failed_username = username.clone();
        let mut rl_map = state.login_rate_limit.lock().await;
        let rl = rl_map.entry(username).or_insert(LoginRateLimit { fail_count: 0, locked_until: None });
        rl.fail_count += 1;
        if rl.fail_count >= MAX_ATTEMPTS {
            rl.locked_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(LOCKOUT_SECS));
            rl.fail_count = 0;
        }
        drop(rl_map);
        // Audit log failed login
        let _ = state.app.storage.add_audit_log("", &failed_username, "login_failed", None, None, false).await;
        return Err(StatusCode::UNAUTHORIZED);
    }

    let user = user.unwrap();

    // Success — reset rate limit
    {
        let mut rl_map = state.login_rate_limit.lock().await;
        rl_map.remove(&username);
    }

    // Audit log
    let _ = state.app.storage.add_audit_log(&user.id, &user.username, "login", None, None, true).await;

    let token = state.create_session(&user).await;

    let cookie = format!("dbx_session={token}; Path={}; HttpOnly; SameSite=Lax", session_cookie_path(&state));
    Ok((
        StatusCode::OK,
        [("set-cookie", cookie.as_str())],
        Json(serde_json::json!({
            "ok": true,
            "user": AuthUserInfo {
                id: user.id,
                username: user.username,
                display_name: user.display_name,
                is_admin: user.is_admin,
                auth_source: user.auth_source.to_string(),
            }
        })),
    )
        .into_response())
}

pub async fn setup(State(state): State<Arc<WebState>>, Json(body): Json<LoginRequest>) -> Result<Response, StatusCode> {
    let storage = &state.app.storage;

    // Only allow setup when no users exist
    let user_count = storage.count_users().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if user_count > 0 {
        return Err(StatusCode::FORBIDDEN);
    }

    let username = match body.username {
        Some(ref u) if !u.trim().is_empty() => u.trim().to_string(),
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    if body.password.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let user = storage
        .create_user(&CreateUserRequest {
            username: username.clone(),
            password: body.password,
            display_name: Some("Administrator".to_string()),
            is_admin: true,
        })
        .await
        .map_err(|e| {
            log::error!("Setup failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Clear legacy password hash since we now have a proper user
    let _ = storage.save_password_hash("").await;

    let token = state.create_session(&user).await;

    let cookie = format!("dbx_session={token}; Path={}; HttpOnly; SameSite=Lax", session_cookie_path(&state));
    Ok((
        StatusCode::OK,
        [("set-cookie", cookie.as_str())],
        Json(serde_json::json!({
            "ok": true,
            "user": AuthUserInfo {
                id: user.id,
                username: user.username,
                display_name: user.display_name,
                is_admin: user.is_admin,
                auth_source: user.auth_source.to_string(),
            }
        })),
    )
        .into_response())
}

pub async fn check(State(state): State<Arc<WebState>>, req: Request<axum::body::Body>) -> Json<AuthCheckResponse> {
    if state.password_disabled {
        return Json(AuthCheckResponse {
            authenticated: true,
            required: false,
            setup_required: false,
            user: None,
            ..build_identity()
        });
    }

    let user_count = state.app.storage.count_users().await.unwrap_or(0);
    if user_count == 0 {
        return Json(AuthCheckResponse {
            authenticated: false,
            required: false,
            setup_required: true,
            user: None,
            ..build_identity()
        });
    }

    let authenticated = match extract_session_token(&req) {
        Some(token) => {
            if let Some(session) = state.get_session(&token).await {
                state.touch_session(&token).await;
                return Json(AuthCheckResponse {
                    authenticated: true,
                    required: true,
                    setup_required: false,
                    user: Some(AuthUserInfo {
                        id: session.user_id,
                        username: session.username,
                        display_name: session.display_name,
                        is_admin: session.is_admin,
                        auth_source: session.auth_source.to_string(),
                    }),
                    ..build_identity()
                });
            }
            false
        }
        None => false,
    };

    Json(AuthCheckResponse { authenticated, required: true, setup_required: false, user: None, ..build_identity() })
}

/// Build identity used by both `/api/version` and `/api/auth/check`.
fn build_identity() -> AuthCheckResponse {
    let info = crate::routes::update::build_info();
    let field = |name: &str| info.get(name).and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    AuthCheckResponse {
        authenticated: false,
        required: false,
        setup_required: false,
        user: None,
        version: field("version"),
        commit: field("commit"),
        build_time_ms: field("buildTimeMs"),
    }
}

pub async fn change_password(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<Response, StatusCode> {
    state.app.storage.change_own_password(&session.user_id, &body.old_password, &body.new_password).await.map_err(
        |e| {
            log::warn!("Change password failed: {e}");
            StatusCode::BAD_REQUEST
        },
    )?;

    Ok((StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response())
}

pub async fn logout(State(state): State<Arc<WebState>>, req: Request<axum::body::Body>) -> Response {
    if let Some(token) = extract_session_token(&req) {
        if let Some(session) = state.get_session(&token).await {
            let _ =
                state.app.storage.add_audit_log(&session.user_id, &session.username, "logout", None, None, true).await;
        }
        state.remove_session(&token).await;
    }
    let cookie = format!("dbx_session=; Path={}; HttpOnly; Max-Age=0", session_cookie_path(&state));
    (StatusCode::OK, [("set-cookie", cookie.as_str())], Json(serde_json::json!({"ok": true}))).into_response()
}

pub fn session_token_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix("dbx_session=") {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn extract_session_token<B>(req: &Request<B>) -> Option<String> {
    session_token_from_headers(req.headers())
}

pub async fn auth_middleware(
    State(state): State<Arc<WebState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    // Auth endpoints are always accessible.
    let api_suffix = middleware_api_path_suffix(req.uri().path(), &state.public_base_path);
    if api_suffix.is_some_and(|suffix| suffix.starts_with("auth/")) {
        return next.run(req).await;
    }

    // Non-API requests (static files) are always accessible.
    if api_suffix.is_none() {
        return next.run(req).await;
    }

    if state.password_disabled {
        return next.run(req).await;
    }

    // Check if any users exist
    let user_count = state.app.storage.count_users().await.unwrap_or(1);
    if user_count == 0 {
        // No users configured — allow access (setup mode)
        return next.run(req).await;
    }

    // Check session token
    if let Some(token) = extract_session_token(&req) {
        if let Some(session) = state.get_session(&token).await {
            state.touch_session(&token).await;
            // Inject user info into request extensions
            let mut req = req;
            req.extensions_mut().insert(session);
            return next.run(req).await;
        }
    }

    StatusCode::UNAUTHORIZED.into_response()
}

/// Extract the current session from request extensions (set by middleware).
/// Returns 401 if not authenticated, 403 if not admin.
pub fn require_session(req: &Request<axum::body::Body>) -> Result<crate::state::UserSession, StatusCode> {
    req.extensions().get::<crate::state::UserSession>().cloned().ok_or(StatusCode::UNAUTHORIZED)
}

pub fn require_admin(req: &Request<axum::body::Body>) -> Result<crate::state::UserSession, StatusCode> {
    let session = require_session(req)?;
    if !session.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(session)
}

// ===== User Management API (Admin only) =====

pub async fn list_users(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
) -> Result<Response, StatusCode> {
    if !session.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    let users = state.app.storage.list_users().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let infos: Vec<UserInfo> = users.into_iter().map(|u| u.into()).collect();
    Ok(Json(infos).into_response())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserPayload {
    pub username: String,
    pub password: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub is_ldap: bool,
}

pub async fn create_user_api(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Json(body): Json<CreateUserPayload>,
) -> Result<Response, StatusCode> {
    if !session.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    if body.is_ldap {
        // Create LDAP user — no password needed
        let display_name = body.display_name.clone().unwrap_or_else(|| body.username.clone());
        let user = state.app.storage.create_ldap_user(&body.username, &display_name, "", body.is_admin).await.map_err(
            |e| {
                log::warn!("Create LDAP user failed: {e}");
                StatusCode::BAD_REQUEST
            },
        )?;
        let info: UserInfo = user.into();
        Ok((StatusCode::CREATED, Json(info)).into_response())
    } else {
        let user = state
            .app
            .storage
            .create_user(&dbx_core::user::CreateUserRequest {
                username: body.username,
                password: body.password,
                display_name: body.display_name,
                is_admin: body.is_admin,
            })
            .await
            .map_err(|e| {
                log::warn!("Create user failed: {e}");
                StatusCode::BAD_REQUEST
            })?;
        let info: UserInfo = user.into();
        Ok((StatusCode::CREATED, Json(info)).into_response())
    }
}

pub async fn update_user_api(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<Response, StatusCode> {
    if !session.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }

    // Prevent self-demotion if last admin
    if let Some(is_admin) = body.is_admin {
        if !is_admin && session.user_id == user_id {
            let admin_count =
                state.app.storage.count_active_admins().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            if admin_count <= 1 {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Cannot demote the last active admin"})),
                )
                    .into_response());
            }
        }
    }

    state.app.storage.update_user(&user_id, &body).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response())
}

pub async fn delete_user_api(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> Result<Response, StatusCode> {
    if !session.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    state.app.storage.delete_user(&user_id).await.map_err(|e| {
        log::warn!("Delete user failed: {e}");
        StatusCode::BAD_REQUEST
    })?;
    Ok((StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response())
}

pub async fn reset_user_password_api(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
    Json(body): Json<ResetPasswordRequest>,
) -> Result<Response, StatusCode> {
    if !session.is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    state.app.storage.reset_user_password(&user_id, &body.new_password).await.map_err(|e| {
        log::warn!("Reset password failed: {e}");
        StatusCode::BAD_REQUEST
    })?;
    Ok((StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response())
}

#[derive(Deserialize)]
pub struct ResetPasswordRequest {
    pub new_password: String,
}

#[cfg(test)]
mod tests {
    use super::{api_path_suffix, build_identity, middleware_api_path_suffix};

    #[test]
    fn api_path_suffix_handles_root_api_paths() {
        assert_eq!(api_path_suffix("/api/auth/check", "/"), Some("auth/check"));
        assert_eq!(api_path_suffix("/api/query/execute", "/"), Some("query/execute"));
        assert_eq!(api_path_suffix("/dbx/api/auth/check", "/"), None);
    }

    #[test]
    fn api_path_suffix_handles_mounted_api_paths() {
        assert_eq!(api_path_suffix("/dbx/api/auth/check", "/dbx"), Some("auth/check"));
        assert_eq!(api_path_suffix("/tools/dbx/api/query/execute", "/tools/dbx"), Some("query/execute"));
        assert_eq!(api_path_suffix("/dbx/login", "/dbx"), None);
    }

    #[test]
    fn middleware_api_path_suffix_handles_nested_router_paths() {
        assert_eq!(middleware_api_path_suffix("/auth/check", "/"), Some("auth/check"));
        assert_eq!(middleware_api_path_suffix("/connection/list", "/"), Some("connection/list"));
        assert_eq!(middleware_api_path_suffix("/api/connection/list", "/"), Some("connection/list"));
        assert_eq!(middleware_api_path_suffix("/dbx/api/connection/list", "/dbx"), Some("connection/list"));
        assert_eq!(middleware_api_path_suffix("/dbx/login", "/dbx"), None);
    }

    #[test]
    fn auth_check_serializes_the_build_identity_the_login_page_reads() {
        let value = serde_json::to_value(build_identity()).expect("checks are serializable");
        for key in ["version", "commit", "buildTimeMs"] {
            let field = value.get(key).and_then(serde_json::Value::as_str).unwrap_or_default();
            assert!(!field.is_empty(), "`{key}` must be a non-empty string for the sign-in screen");
        }
    }

    #[test]
    fn auth_check_reports_the_same_identity_as_the_version_route() {
        let check = serde_json::to_value(build_identity()).expect("checks are serializable");
        let version = crate::routes::update::build_info();
        for key in ["version", "commit", "buildTimeMs"] {
            assert_eq!(check.get(key), version.get(key), "`{key}` differs between the two routes");
        }
    }
}
