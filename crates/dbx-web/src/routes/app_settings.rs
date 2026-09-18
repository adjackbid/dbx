use std::sync::Arc;

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use axum::extract::State;
use axum::Json;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use dbx_core::storage::{McpUserPolicy, McpUserPolicyState};
use pbkdf2::pbkdf2_hmac;
use serde::Deserialize;
use sha2::Sha256;

use crate::error::AppError;
use crate::state::WebState;

const CONFIG_PBKDF2_ITERATIONS: u32 = 100_000;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePinnedTreeNodeIdsRequest {
    pub ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedConfigPayload {
    pub format: String,
    pub version: u8,
    pub salt: String,
    pub iv: String,
    pub data: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecryptConfigRequest {
    pub payload: EncryptedConfigPayload,
    pub passphrase: String,
}

pub async fn load_pinned_tree_node_ids(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
) -> Result<Json<Vec<String>>, AppError> {
    let ids = state.app.storage.load_pinned_tree_node_ids_for_user(&session.user_id).await.map_err(AppError::from)?;
    Ok(Json(ids))
}

pub async fn save_pinned_tree_node_ids(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Json(body): Json<SavePinnedTreeNodeIdsRequest>,
) -> Result<Json<()>, AppError> {
    state.app.storage.save_pinned_tree_node_ids_for_user(&session.user_id, &body.ids).await.map_err(AppError::from)?;
    Ok(Json(()))
}

/// MCP scope and permission of the signed-in account. Each account configures
/// its own scope, so MCP clients only ever see that account's connections.
pub async fn load_mcp_user_policy(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
) -> Result<Json<McpUserPolicyState>, AppError> {
    state.app.storage.load_mcp_user_policy(&session.user_id).await.map(Json).map_err(AppError::from)
}

pub async fn save_mcp_user_policy(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Json(policy): Json<McpUserPolicy>,
) -> Result<Json<()>, AppError> {
    state.app.storage.save_mcp_user_policy(&session.user_id, &policy).await.map_err(AppError::from)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveMaxAgentTurnsRequest {
    pub max_agent_turns: u32,
}

pub async fn load_max_agent_turns(State(state): State<Arc<WebState>>) -> Result<Json<u32>, AppError> {
    state.app.storage.load_max_agent_turns().await.map(Json).map_err(AppError::from)
}

/// Instance-wide AI limits. Every account may read them so the UI can show the
/// effective value, but only an admin may change them for the whole instance.
pub async fn save_max_agent_turns(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Json(body): Json<SaveMaxAgentTurnsRequest>,
) -> Result<Json<()>, AppError> {
    require_admin(&session)?;
    state.app.storage.save_max_agent_turns(body.max_agent_turns).await.map_err(AppError::from)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveMaxRetriesRequest {
    pub max_retries: u32,
}

pub async fn load_max_retries(State(state): State<Arc<WebState>>) -> Result<Json<u32>, AppError> {
    state.app.storage.load_max_retries().await.map(Json).map_err(AppError::from)
}

pub async fn save_max_retries(
    State(state): State<Arc<WebState>>,
    axum::extract::Extension(session): axum::extract::Extension<crate::state::UserSession>,
    Json(body): Json<SaveMaxRetriesRequest>,
) -> Result<Json<()>, AppError> {
    require_admin(&session)?;
    state.app.storage.save_max_retries(body.max_retries).await.map_err(AppError::from)?;
    Ok(Json(()))
}

/// Instance-wide settings may only be changed by an admin of the Web instance.
fn require_admin(session: &crate::state::UserSession) -> Result<(), AppError> {
    if session.is_admin {
        Ok(())
    } else {
        Err(AppError::forbidden("This setting can only be changed by an administrator"))
    }
}

pub async fn decrypt_config(Json(body): Json<DecryptConfigRequest>) -> Result<Json<String>, AppError> {
    decrypt_config_payload(&body.payload, &body.passphrase).map(Json).map_err(AppError::from)
}

fn decrypt_config_payload(payload: &EncryptedConfigPayload, passphrase: &str) -> Result<String, String> {
    if payload.format != "dbx-encrypted" || payload.version != 1 {
        return Err("Unsupported encrypted config format".to_string());
    }
    let salt = BASE64.decode(&payload.salt).map_err(|_| "wrong_passphrase".to_string())?;
    let iv = BASE64.decode(&payload.iv).map_err(|_| "wrong_passphrase".to_string())?;
    let ciphertext = BASE64.decode(&payload.data).map_err(|_| "wrong_passphrase".to_string())?;
    if iv.len() != 12 {
        return Err("wrong_passphrase".to_string());
    }

    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), &salt, CONFIG_PBKDF2_ITERATIONS, &mut key);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "wrong_passphrase".to_string())?;
    let plaintext =
        cipher.decrypt(Nonce::from_slice(&iv), ciphertext.as_ref()).map_err(|_| "wrong_passphrase".to_string())?;
    String::from_utf8(plaintext).map_err(|_| "wrong_passphrase".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        decrypt_config_payload, load_max_agent_turns, save_max_agent_turns, save_max_retries, EncryptedConfigPayload,
        SaveMaxAgentTurnsRequest, SaveMaxRetriesRequest,
    };
    use crate::state::{UserSession, WebState};
    use axum::extract::State;
    use axum::Json;
    use dbx_core::connection::AppState;
    use dbx_core::storage::Storage;
    use std::sync::Arc;

    fn session(is_admin: bool) -> UserSession {
        UserSession {
            user_id: if is_admin { "admin-1" } else { "user-1" }.to_string(),
            username: if is_admin { "admin" } else { "user" }.to_string(),
            display_name: String::new(),
            is_admin,
            auth_source: dbx_core::user::AuthSource::Local,
            created_at: 0,
            last_accessed_at: 0,
        }
    }

    async fn test_web_state() -> (Arc<WebState>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dbx-web-app-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let storage = Storage::open(&dir.join("storage.db")).await.unwrap();
        let app = Arc::new(AppState::new_with_plugin_dir(storage, dir.join("plugins")));
        (Arc::new(WebState::for_tests(app, dir.clone())), dir)
    }

    #[tokio::test]
    async fn instance_ai_limits_can_only_be_changed_by_an_admin() {
        let (state, dir) = test_web_state().await;

        let denied = save_max_agent_turns(
            State(state.clone()),
            axum::extract::Extension(session(false)),
            Json(SaveMaxAgentTurnsRequest { max_agent_turns: 10 }),
        )
        .await
        .unwrap_err();
        assert_eq!(denied.status, axum::http::StatusCode::FORBIDDEN);
        let denied = save_max_retries(
            State(state.clone()),
            axum::extract::Extension(session(false)),
            Json(SaveMaxRetriesRequest { max_retries: 3 }),
        )
        .await
        .unwrap_err();
        assert_eq!(denied.status, axum::http::StatusCode::FORBIDDEN);

        let _ = save_max_agent_turns(
            State(state.clone()),
            axum::extract::Extension(session(true)),
            Json(SaveMaxAgentTurnsRequest { max_agent_turns: 12 }),
        )
        .await
        .unwrap();

        // Every account may still read the effective instance limits.
        let Json(turns) = load_max_agent_turns(State(state.clone())).await.unwrap();
        assert_eq!(turns, 12);
        assert_eq!(state.app.storage.load_max_retries().await.unwrap(), dbx_core::ai::DEFAULT_MAX_RETRIES);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn prompt_templates_and_instructions_follow_the_session_account() {
        let (state, dir) = test_web_state().await;

        let _ = super::super::prompt_template::save_prompt_template(
            State(state.clone()),
            axum::extract::Extension(session(false)),
            Json(super::super::prompt_template::SavePromptTemplateRequest {
                id: "t1".to_string(),
                name: "User rules".to_string(),
                content: "content".to_string(),
            }),
        )
        .await
        .unwrap();
        let _ = super::super::prompt_template::set_global_instructions(
            State(state.clone()),
            axum::extract::Extension(session(false)),
            Json(super::super::prompt_template::SetGlobalInstructionsRequest { content: "only mine".to_string() }),
        )
        .await
        .unwrap();

        let Json(user_templates) = super::super::prompt_template::load_prompt_templates(
            State(state.clone()),
            axum::extract::Extension(session(false)),
        )
        .await
        .unwrap();
        assert_eq!(user_templates.len(), 1);

        // Another account must not see the template or the instructions.
        let Json(other_templates) = super::super::prompt_template::load_prompt_templates(
            State(state.clone()),
            axum::extract::Extension(session(true)),
        )
        .await
        .unwrap();
        assert!(other_templates.is_empty());
        let Json(other_instructions) = super::super::prompt_template::get_global_instructions(
            State(state.clone()),
            axum::extract::Extension(session(true)),
        )
        .await
        .unwrap();
        assert_eq!(other_instructions.content, "");

        let _ = std::fs::remove_dir_all(dir);
    }

    fn exported_browser_payload() -> EncryptedConfigPayload {
        EncryptedConfigPayload {
            format: "dbx-encrypted".to_string(),
            version: 1,
            salt: "AAECAwQFBgcICQoLDA0ODw==".to_string(),
            iv: "EBESExQVFhcYGRob".to_string(),
            data: "sCyBTex9XqcCCH5mOyJcF/UN9kpnMp+t0VeEtGrJBMt+QyR85kYhUWezuC9yEhM5jF0=".to_string(),
        }
    }

    #[test]
    fn decrypts_browser_exported_config_payload() {
        let plaintext = decrypt_config_payload(&exported_browser_payload(), "passphrase").unwrap();

        assert_eq!(plaintext, r#"{"connections":[{"name":"local"}]}"#);
    }

    #[test]
    fn rejects_wrong_config_passphrase() {
        let error = decrypt_config_payload(&exported_browser_payload(), "wrong").unwrap_err();

        assert_eq!(error, "wrong_passphrase");
    }
}
