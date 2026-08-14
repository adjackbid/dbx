use dbx_core::connection::AppState;
use dbx_core::user::{AuthSource, User};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, watch, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::sse::TransferProgressChannel;

pub struct LoginRateLimit {
    pub fail_count: u32,
    pub locked_until: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NacosImportContext {
    pub owner_session: Option<String>,
    pub connection_id: String,
    pub target_namespace: String,
    pub plan_hash: String,
}

#[derive(Debug, Clone)]
pub struct UserSession {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub is_admin: bool,
    pub auth_source: AuthSource,
    pub created_at: u64,
    pub last_accessed_at: u64,
}

const SESSION_TIMEOUT_SECS: u64 = 8 * 3600;

pub struct WebState {
    pub app: Arc<AppState>,
    pub data_dir: PathBuf,
    pub public_base_path: String,
    pub password_disabled: bool,
    pub password_hash: RwLock<Option<String>>,
    pub sessions: RwLock<HashMap<String, UserSession>>,
    pub sse_channels: RwLock<HashMap<String, broadcast::Sender<String>>>,
    pub transfer_progress_channels: RwLock<HashMap<String, Arc<TransferProgressChannel>>>,
    pub table_import_channels: RwLock<HashMap<String, watch::Sender<String>>>,
    pub sql_file_executions: RwLock<HashMap<String, CancellationToken>>,
    pub nacos_imports: RwLock<HashMap<String, NacosImportContext>>,
    pub login_rate_limit: Mutex<HashMap<String, LoginRateLimit>>,
    /// Table export temp files: export_id -> (file_path, format)
    pub export_files: RwLock<HashMap<String, (String, String)>>,
    pub ssh_prompts: Arc<crate::ssh_prompt::SshPromptHub>,
}

impl WebState {
    pub async fn remove_sse_channel(&self, id: &str) {
        self.sse_channels.write().await.remove(id);
    }

    pub async fn get_session(&self, token: &str) -> Option<UserSession> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(token)?;
        if session_expired(session) {
            return None;
        }
        Some(session.clone())
    }

    pub async fn touch_session(&self, token: &str) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(token) {
            session.last_accessed_at = now_secs();
        }
    }

    pub async fn create_session(&self, user: &User) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        let now = now_secs();
        self.sessions.write().await.insert(
            token.clone(),
            UserSession {
                user_id: user.id.clone(),
                username: user.username.clone(),
                display_name: user.display_name.clone(),
                is_admin: user.is_admin,
                auth_source: user.auth_source,
                created_at: now,
                last_accessed_at: now,
            },
        );
        token
    }

    pub async fn remove_session(&self, token: &str) {
        self.sessions.write().await.remove(token);
    }

    /// Test helper: full field set so new WebState fields don't break scattered test fixtures.
    #[cfg(test)]
    pub fn for_tests(app: Arc<AppState>, data_dir: PathBuf) -> Self {
        Self {
            app,
            data_dir,
            public_base_path: "/".to_string(),
            password_disabled: false,
            password_hash: RwLock::new(None),
            sessions: RwLock::new(HashMap::new()),
            sse_channels: RwLock::new(HashMap::new()),
            transfer_progress_channels: RwLock::new(HashMap::new()),
            table_import_channels: RwLock::new(HashMap::new()),
            sql_file_executions: RwLock::new(HashMap::new()),
            nacos_imports: RwLock::new(HashMap::new()),
            login_rate_limit: Mutex::new(HashMap::new()),
            export_files: RwLock::new(HashMap::new()),
            ssh_prompts: Arc::new(crate::ssh_prompt::SshPromptHub::new()),
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn session_expired(session: &UserSession) -> bool {
    let now = now_secs();
    now.saturating_sub(session.last_accessed_at) > SESSION_TIMEOUT_SECS
}
