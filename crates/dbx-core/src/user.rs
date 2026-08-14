use std::time::{SystemTime, UNIX_EPOCH};

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rusqlite::{params, params_from_iter, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::history::HistoryEntry;
use crate::storage::Storage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthSource {
    Local,
    Ldap,
}

impl std::fmt::Display for AuthSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthSource::Local => write!(f, "local"),
            AuthSource::Ldap => write!(f, "ldap"),
        }
    }
}

impl AuthSource {
    pub fn from_str(s: &str) -> Self {
        match s {
            "ldap" => AuthSource::Ldap,
            _ => AuthSource::Local,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub auth_source: AuthSource,
    pub ldap_dn: Option<String>,
    pub is_admin: bool,
    pub is_active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    pub display_name: Option<String>,
    pub is_admin: Option<bool>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub auth_source: AuthSource,
    pub is_admin: bool,
    pub is_active: bool,
}

impl From<User> for UserInfo {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            display_name: u.display_name,
            auth_source: u.auth_source,
            is_admin: u.is_admin,
            is_active: u.is_active,
        }
    }
}

fn unix_timestamp_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("Failed to hash password: {e}"))
}

fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .and_then(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).map(|_| ()))
        .is_ok()
}

impl Storage {
    pub async fn list_users(&self) -> Result<Vec<User>, String> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, username, display_name, auth_source, ldap_dn, is_admin, is_active, created_at, updated_at
                     FROM users ORDER BY created_at ASC",
                )
                .map_err(|e| e.to_string())?;
            let users = stmt
                .query_map([], |row| {
                    Ok(User {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        display_name: row.get(2)?,
                        auth_source: AuthSource::from_str(&row.get::<_, String>(3)?),
                        ldap_dn: row.get(4)?,
                        is_admin: row.get::<_, i64>(5)? != 0,
                        is_active: row.get::<_, i64>(6)? != 0,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                    })
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            Ok(users)
        })
        .await
    }

    pub async fn get_user_by_id(&self, user_id: &str) -> Result<Option<User>, String> {
        let user_id = user_id.to_string();
        self.with_conn(move |conn| {
            let user = conn
                .query_row(
                    "SELECT id, username, display_name, auth_source, ldap_dn, is_admin, is_active, created_at, updated_at
                     FROM users WHERE id = ?1",
                    params![user_id],
                    |row| {
                        Ok(User {
                            id: row.get(0)?,
                            username: row.get(1)?,
                            display_name: row.get(2)?,
                            auth_source: AuthSource::from_str(&row.get::<_, String>(3)?),
                            ldap_dn: row.get(4)?,
                            is_admin: row.get::<_, i64>(5)? != 0,
                            is_active: row.get::<_, i64>(6)? != 0,
                            created_at: row.get(7)?,
                            updated_at: row.get(8)?,
                        })
                    },
                )
                .optional()
                .map_err(|e| e.to_string())?;
            Ok(user)
        })
        .await
    }

    pub async fn get_user_by_username(&self, username: &str) -> Result<Option<User>, String> {
        let username = username.to_string();
        self.with_conn(move |conn| {
            let user = conn
                .query_row(
                    "SELECT id, username, display_name, auth_source, ldap_dn, is_admin, is_active, created_at, updated_at
                     FROM users WHERE username = ?1",
                    params![username],
                    |row| {
                        Ok(User {
                            id: row.get(0)?,
                            username: row.get(1)?,
                            display_name: row.get(2)?,
                            auth_source: AuthSource::from_str(&row.get::<_, String>(3)?),
                            ldap_dn: row.get(4)?,
                            is_admin: row.get::<_, i64>(5)? != 0,
                            is_active: row.get::<_, i64>(6)? != 0,
                            created_at: row.get(7)?,
                            updated_at: row.get(8)?,
                        })
                    },
                )
                .optional()
                .map_err(|e| e.to_string())?;
            Ok(user)
        })
        .await
    }

    pub async fn create_user(&self, req: &CreateUserRequest) -> Result<User, String> {
        let username = req.username.trim().to_string();
        if username.is_empty() {
            return Err("Username cannot be empty".to_string());
        }
        if req.password.is_empty() {
            return Err("Password cannot be empty".to_string());
        }
        if req.password.len() < 8 {
            return Err("Password must be at least 8 characters".to_string());
        }

        let existing = self.get_user_by_username(&username).await?;
        if existing.is_some() {
            return Err(format!("Username '{username}' already exists"));
        }

        let id = Uuid::new_v4().to_string();
        let display_name = req.display_name.clone().unwrap_or_else(|| username.clone());
        let password_hash = hash_password(&req.password)?;
        let now = unix_timestamp_secs();
        let is_admin = req.is_admin as i64;
        let is_active = 1i64;

        let id_for_query = id.clone();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO users (id, username, display_name, password_hash, auth_source, ldap_dn, is_admin, is_active, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'local', NULL, ?5, ?6, ?7, ?7)",
                params![id, username, display_name, password_hash, is_admin, is_active, now],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await?;

        self.get_user_by_id(&id_for_query).await?.ok_or_else(|| "Failed to retrieve created user".to_string())
    }

    pub async fn create_ldap_user(
        &self,
        username: &str,
        display_name: &str,
        ldap_dn: &str,
        is_admin: bool,
    ) -> Result<User, String> {
        let username = username.trim().to_string();
        if username.is_empty() {
            return Err("Username cannot be empty".to_string());
        }

        let existing = self.get_user_by_username(&username).await?;
        if existing.is_some() {
            return Err(format!("Username '{username}' already exists"));
        }

        let id = Uuid::new_v4().to_string();
        let display_name = display_name.to_string();
        let ldap_dn = ldap_dn.to_string();
        let now = unix_timestamp_secs();

        let id_for_query = id.clone();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO users (id, username, display_name, password_hash, auth_source, ldap_dn, is_admin, is_active, created_at, updated_at)
                 VALUES (?1, ?2, ?3, NULL, 'ldap', ?4, ?5, 1, ?6, ?6)",
                params![id, username, display_name, ldap_dn, is_admin as i64, now],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await?;

        self.get_user_by_id(&id_for_query).await?.ok_or_else(|| "Failed to retrieve created LDAP user".to_string())
    }

    pub async fn update_user(&self, user_id: &str, req: &UpdateUserRequest) -> Result<(), String> {
        let now = unix_timestamp_secs();
        let user_id = user_id.to_string();
        let req = req.clone();
        self.with_conn(move |conn| {
            let mut sets: Vec<String> = Vec::new();
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            // updated_at is always set
            sets.push("updated_at = ?".to_string());
            params_vec.push(Box::new(now));

            if let Some(ref display_name) = req.display_name {
                sets.push("display_name = ?".to_string());
                params_vec.push(Box::new(display_name.clone()));
            }
            if let Some(is_admin) = req.is_admin {
                sets.push("is_admin = ?".to_string());
                params_vec.push(Box::new(is_admin as i64));
            }
            if let Some(is_active) = req.is_active {
                sets.push("is_active = ?".to_string());
                params_vec.push(Box::new(is_active as i64));
            }

            if sets.len() == 1 {
                // only updated_at, nothing meaningful to update
                return Ok(());
            }

            params_vec.push(Box::new(user_id));

            let sql = format!("UPDATE users SET {} WHERE id = ?", sets.join(", "));
            let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
            conn.execute(&sql, rusqlite::params_from_iter(param_refs)).map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
    }

    pub async fn delete_user(&self, user_id: &str) -> Result<(), String> {
        let user = self.get_user_by_id(user_id).await?.ok_or_else(|| "User not found".to_string())?;

        if user.is_admin && self.count_active_admins().await? <= 1 {
            return Err("Cannot delete the last active admin user".to_string());
        }

        let user_id = user_id.to_string();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM user_settings WHERE user_id = ?1", params![user_id])
                .map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM connections WHERE user_id = ?1", params![user_id]).map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM connection_secrets WHERE user_id = ?1", params![user_id])
                .map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM history WHERE user_id = ?1", params![user_id]).map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM ai_conversations WHERE user_id = ?1", params![user_id])
                .map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM mq_token_records WHERE user_id = ?1", params![user_id])
                .map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM saved_sql_folders WHERE user_id = ?1", params![user_id])
                .map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM saved_sql_files WHERE user_id = ?1", params![user_id])
                .map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM prompt_templates WHERE user_id = ?1", params![user_id])
                .map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM tunnel_profiles WHERE user_id = ?1", params![user_id])
                .map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM users WHERE id = ?1", params![user_id]).map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
    }

    pub async fn reset_user_password(&self, user_id: &str, new_password: &str) -> Result<(), String> {
        if new_password.len() < 8 {
            return Err("Password must be at least 8 characters".to_string());
        }
        let hash = hash_password(new_password)?;
        let now = unix_timestamp_secs();
        let user_id = user_id.to_string();
        self.with_conn(move |conn| {
            let result = conn.execute(
                "UPDATE users SET password_hash = ?1, updated_at = ?2 WHERE id = ?3 AND auth_source = 'local'",
                params![hash, now, user_id],
            );
            match result {
                Ok(0) => Err("User not found or is not a local account".to_string()),
                Ok(_) => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        })
        .await
    }

    pub async fn change_own_password(
        &self,
        user_id: &str,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), String> {
        if new_password.len() < 8 {
            return Err("New password must be at least 8 characters".to_string());
        }

        let user = self.get_user_by_id(user_id).await?.ok_or_else(|| "User not found".to_string())?;

        if user.auth_source != AuthSource::Local {
            return Err("LDAP users cannot change password locally".to_string());
        }

        let user_id_owned = user_id.to_string();
        let stored_hash: Option<String> = self
            .with_conn(move |conn| {
                conn.query_row("SELECT password_hash FROM users WHERE id = ?1", params![user_id_owned], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(|e| e.to_string())
            })
            .await?;

        let stored_hash = stored_hash.ok_or_else(|| "No password hash found".to_string())?;
        if !verify_password(old_password, &stored_hash) {
            return Err("Current password is incorrect".to_string());
        }

        let new_hash = hash_password(new_password)?;
        let now = unix_timestamp_secs();
        let user_id_owned = user_id.to_string();
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE users SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
                params![new_hash, now, user_id_owned],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
    }

    pub async fn verify_user_password(&self, username: &str, password: &str) -> Result<Option<User>, String> {
        let user = self.get_user_by_username(username).await?;
        if user.is_none() {
            return Ok(None);
        }
        let user = user.unwrap();
        if !user.is_active {
            return Ok(None);
        }
        if user.auth_source != AuthSource::Local {
            return Ok(None);
        }

        let user_id = user.id.clone();
        let stored_hash: Option<String> = self
            .with_conn(move |conn| {
                conn.query_row("SELECT password_hash FROM users WHERE id = ?1", params![user_id], |row| row.get(0))
                    .optional()
                    .map_err(|e| e.to_string())
            })
            .await?;

        let stored_hash = match stored_hash {
            Some(h) if !h.is_empty() => h,
            _ => return Ok(None),
        };

        if verify_password(password, &stored_hash) {
            Ok(Some(user))
        } else {
            Ok(None)
        }
    }

    pub async fn count_active_admins(&self) -> Result<i64, String> {
        self.with_conn(|conn| {
            conn.query_row("SELECT COUNT(*) FROM users WHERE is_admin = 1 AND is_active = 1", [], |row| row.get(0))
                .map_err(|e| e.to_string())
        })
        .await
    }

    pub async fn count_users(&self) -> Result<i64, String> {
        self.with_conn(|conn| {
            conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0)).map_err(|e| e.to_string())
        })
        .await
    }

    pub async fn get_user_setting(&self, user_id: &str, key: &str) -> Result<Option<String>, String> {
        let user_id = user_id.to_string();
        let key = key.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT value FROM user_settings WHERE user_id = ?1 AND key = ?2",
                params![user_id, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
        })
        .await
    }

    pub async fn save_user_setting(&self, user_id: &str, key: &str, value: &str) -> Result<(), String> {
        let user_id = user_id.to_string();
        let key = key.to_string();
        let value = value.to_string();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO user_settings (user_id, key, value) VALUES (?1, ?2, ?3)",
                params![user_id, key, value],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
    }

    pub async fn delete_user_setting(&self, user_id: &str, key: &str) -> Result<(), String> {
        let user_id = user_id.to_string();
        let key = key.to_string();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM user_settings WHERE user_id = ?1 AND key = ?2", params![user_id, key])
                .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
    }

    pub async fn add_audit_log(
        &self,
        user_id: &str,
        username: &str,
        action: &str,
        details: Option<&str>,
        ip_address: Option<&str>,
        success: bool,
    ) -> Result<(), String> {
        let id = uuid::Uuid::new_v4().to_string();
        let user_id = user_id.to_string();
        let username = username.to_string();
        let action = action.to_string();
        let details = details.map(|s| s.to_string());
        let ip_address = ip_address.map(|s| s.to_string());
        let created_at = chrono::Utc::now().to_rfc3339();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO audit_log (id, user_id, username, action, details, ip_address, success, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, user_id, username, action, details, ip_address, success as i64, created_at],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
    }

    pub async fn load_audit_logs(
        &self,
        limit: usize,
        offset: usize,
        user_id_filter: Option<&str>,
        action_filter: Option<&str>,
    ) -> Result<Vec<AuditLogEntry>, String> {
        let limit = limit as i64;
        let offset = offset as i64;
        let user_filter = user_id_filter.map(|s| s.to_string());
        let action_filter = action_filter.map(|s| s.to_string());
        self.with_conn(move |conn| {
            let mut sql = String::from(
                "SELECT id, user_id, username, action, details, ip_address, success, created_at \
                 FROM audit_log WHERE 1=1",
            );
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            let mut param_idx = 1;

            if let Some(ref uid) = user_filter {
                sql.push_str(&format!(" AND user_id = ?{param_idx}"));
                params_vec.push(Box::new(uid.clone()));
                param_idx += 1;
            }
            if let Some(ref act) = action_filter {
                sql.push_str(&format!(" AND action = ?{param_idx}"));
                params_vec.push(Box::new(act.clone()));
                param_idx += 1;
            }

            sql.push_str(&format!(" ORDER BY created_at DESC LIMIT ?{param_idx} OFFSET ?{}", param_idx + 1));
            params_vec.push(Box::new(limit));
            params_vec.push(Box::new(offset));

            let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params_from_iter(param_refs), |row| {
                    Ok(AuditLogEntry {
                        id: row.get(0)?,
                        user_id: row.get(1)?,
                        username: row.get(2)?,
                        action: row.get(3)?,
                        details: row.get(4)?,
                        ip_address: row.get(5)?,
                        success: row.get::<_, i64>(6)? != 0,
                        created_at: row.get(7)?,
                    })
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
        })
        .await
    }

    pub async fn load_all_history_with_users(
        &self,
        limit: usize,
        offset: usize,
        user_id_filter: Option<&str>,
    ) -> Result<Vec<HistoryWithUser>, String> {
        let limit = limit as i64;
        let offset = offset as i64;
        let user_filter = user_id_filter.map(|s| s.to_string());
        self.with_conn(move |conn| {
            let (sql, uid_param): (String, Option<String>) = if let Some(ref uid) = user_filter {
                (
                    "SELECT h.id, h.connection_name, h.database, h.sql_text, h.executed_at, \
                     h.execution_time_ms, h.success, h.error, h.activity_kind, h.connection_id, \
                     h.user_id, u.username \
                     FROM history h LEFT JOIN users u ON h.user_id = u.id \
                     WHERE h.user_id = ?1 ORDER BY h.executed_at DESC LIMIT ?2 OFFSET ?3"
                        .to_string(),
                    Some(uid.clone()),
                )
            } else {
                (
                    "SELECT h.id, h.connection_name, h.database, h.sql_text, h.executed_at, \
                     h.execution_time_ms, h.success, h.error, h.activity_kind, h.connection_id, \
                     h.user_id, u.username \
                     FROM history h LEFT JOIN users u ON h.user_id = u.id \
                     ORDER BY h.executed_at DESC LIMIT ?1 OFFSET ?2"
                        .to_string(),
                    None,
                )
            };

            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

            fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryWithUser> {
                Ok(HistoryWithUser {
                    id: row.get(0)?,
                    connection_name: row.get(1)?,
                    database: row.get(2)?,
                    sql: row.get(3)?,
                    executed_at: row.get(4)?,
                    execution_time_ms: row.get::<_, i64>(5)? as u64,
                    success: row.get::<_, i64>(6)? != 0,
                    error: row.get(7)?,
                    activity_kind: row.get(8)?,
                    connection_id: row.get(9)?,
                    user_id: row.get(10)?,
                    username: row.get(11)?,
                })
            }

            let rows = if let Some(uid) = uid_param {
                stmt.query_map(params![uid, limit, offset], map_row).map_err(|e| e.to_string())?
            } else {
                stmt.query_map(params![limit, offset], map_row).map_err(|e| e.to_string())?
            };
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
        })
        .await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogEntry {
    pub id: String,
    pub user_id: String,
    pub username: String,
    pub action: String,
    pub details: Option<String>,
    pub ip_address: Option<String>,
    pub success: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryWithUser {
    pub id: String,
    pub connection_name: String,
    pub database: String,
    pub sql: String,
    pub executed_at: String,
    pub execution_time_ms: u64,
    pub success: bool,
    pub error: Option<String>,
    pub activity_kind: String,
    pub connection_id: String,
    pub user_id: String,
    pub username: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("dbx-user-{name}-{}-{stamp}.db", std::process::id()))
    }

    async fn temp_storage(name: &str) -> Storage {
        let path = temp_db_path(name);
        Storage::open(&path).await.unwrap()
    }

    #[tokio::test]
    async fn create_and_verify_user() {
        let storage = temp_storage("create-verify").await;
        let user = storage
            .create_user(&CreateUserRequest {
                username: "testadmin".to_string(),
                password: "test12345".to_string(),
                display_name: Some("TestAdmin".to_string()),
                is_admin: true,
            })
            .await
            .unwrap();

        assert_eq!(user.username, "testadmin");
        assert!(user.is_admin);
        assert_eq!(user.auth_source, AuthSource::Local);

        let verified = storage.verify_user_password("testadmin", "test12345").await.unwrap();
        assert!(verified.is_some());
        assert_eq!(verified.unwrap().id, user.id);
    }

    #[tokio::test]
    async fn verify_wrong_password_returns_none() {
        let storage = temp_storage("test").await;
        storage
            .create_user(&CreateUserRequest {
                username: "alice".to_string(),
                password: "correct123".to_string(),
                display_name: None,
                is_admin: false,
            })
            .await
            .unwrap();

        let result = storage.verify_user_password("alice", "wrong").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn duplicate_username_rejected() {
        let storage = temp_storage("test").await;
        storage
            .create_user(&CreateUserRequest {
                username: "bob".to_string(),
                password: "password1".to_string(),
                display_name: None,
                is_admin: false,
            })
            .await
            .unwrap();

        let result = storage
            .create_user(&CreateUserRequest {
                username: "bob".to_string(),
                password: "password2".to_string(),
                display_name: None,
                is_admin: false,
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn short_password_rejected() {
        let storage = temp_storage("test").await;
        let result = storage
            .create_user(&CreateUserRequest {
                username: "charlie".to_string(),
                password: "short".to_string(),
                display_name: None,
                is_admin: false,
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_last_admin_rejected() {
        let storage = temp_storage("delete-last-admin").await;
        // Fresh install has no users — create one admin first
        let admin = storage
            .create_user(&CreateUserRequest {
                username: "admin".to_string(),
                password: "admin1234".to_string(),
                display_name: None,
                is_admin: true,
            })
            .await
            .unwrap();

        let result = storage.delete_user(&admin.id).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("last active admin"));
    }

    #[tokio::test]
    async fn change_own_password() {
        let storage = temp_storage("test").await;
        let user = storage
            .create_user(&CreateUserRequest {
                username: "dave".to_string(),
                password: "oldpass12".to_string(),
                display_name: None,
                is_admin: false,
            })
            .await
            .unwrap();

        storage.change_own_password(&user.id, "oldpass12", "newpass12").await.unwrap();

        let verified = storage.verify_user_password("dave", "newpass12").await.unwrap();
        assert!(verified.is_some());
    }

    #[tokio::test]
    async fn change_password_wrong_old_rejected() {
        let storage = temp_storage("test").await;
        let user = storage
            .create_user(&CreateUserRequest {
                username: "eve".to_string(),
                password: "password1".to_string(),
                display_name: None,
                is_admin: false,
            })
            .await
            .unwrap();

        let result = storage.change_own_password(&user.id, "wrong", "newpass12").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn user_settings_roundtrip() {
        let storage = temp_storage("test").await;
        let user = storage
            .create_user(&CreateUserRequest {
                username: "frank".to_string(),
                password: "password1".to_string(),
                display_name: None,
                is_admin: false,
            })
            .await
            .unwrap();

        storage.save_user_setting(&user.id, "editor_settings", "{}").await.unwrap();
        let value = storage.get_user_setting(&user.id, "editor_settings").await.unwrap();
        assert_eq!(value.as_deref(), Some("{}"));

        storage.delete_user_setting(&user.id, "editor_settings").await.unwrap();
        let value = storage.get_user_setting(&user.id, "editor_settings").await.unwrap();
        assert!(value.is_none());
    }

    #[tokio::test]
    async fn create_ldap_user_and_cannot_change_password() {
        let storage = temp_storage("test").await;
        let user = storage
            .create_ldap_user("ldapuser", "LDAP User", "uid=ldapuser,ou=users,dc=corp,dc=com", false)
            .await
            .unwrap();

        assert_eq!(user.auth_source, AuthSource::Ldap);
        assert_eq!(user.ldap_dn.as_deref(), Some("uid=ldapuser,ou=users,dc=corp,dc=com"));

        let result = storage.change_own_password(&user.id, "anything", "newpass12").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn update_user_admin_flag() {
        let storage = temp_storage("update-admin").await;

        // Migration already created a default admin; create a second user to test with
        let user = storage
            .create_user(&CreateUserRequest {
                username: "grace".to_string(),
                password: "password1".to_string(),
                display_name: None,
                is_admin: false,
            })
            .await
            .unwrap();

        storage
            .update_user(
                &user.id,
                &UpdateUserRequest { display_name: Some("Grace".to_string()), is_admin: Some(true), is_active: None },
            )
            .await
            .unwrap();

        let updated = storage.get_user_by_id(&user.id).await.unwrap().unwrap();
        assert_eq!(updated.display_name, "Grace");
        assert!(updated.is_admin);

        // Now demote back to non-admin
        storage
            .update_user(&user.id, &UpdateUserRequest { display_name: None, is_admin: Some(false), is_active: None })
            .await
            .unwrap();

        let updated = storage.get_user_by_id(&user.id).await.unwrap().unwrap();
        assert!(!updated.is_admin);
    }

    #[tokio::test]
    async fn audit_log_and_history_with_user_works() {
        let storage = temp_storage("audit-history").await;

        // Create a user
        let user = storage
            .create_user(&CreateUserRequest {
                username: "alice".to_string(),
                password: "password1".to_string(),
                display_name: Some("Alice".to_string()),
                is_admin: false,
            })
            .await
            .unwrap();

        // Add an audit log
        storage.add_audit_log(&user.id, "alice", "login", None, None, true).await.unwrap();

        // Add a history entry
        let entry = HistoryEntry {
            id: "test-1".to_string(),
            connection_name: "MyDB".to_string(),
            database: "testdb".to_string(),
            sql: "SELECT 1".to_string(),
            executed_at: "2026-08-13T10:00:00Z".to_string(),
            execution_time_ms: 42,
            success: true,
            error: None,
            activity_kind: "query".to_string(),
            connection_id: "conn-1".to_string(),
            operation: "".to_string(),
            target: "".to_string(),
            affected_rows: None,
            rollback_sql: None,
            details_json: None,
        };
        storage.save_history_entry(&entry, &user.id).await.unwrap();

        // Load audit logs
        let logs = storage.load_audit_logs(100, 0, None, None).await.unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].username, "alice");
        assert_eq!(logs[0].action, "login");

        // Load all history with users
        let history = storage.load_all_history_with_users(100, 0, None).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].sql, "SELECT 1");
        assert_eq!(history[0].username, "alice");
        assert_eq!(history[0].connection_name, "MyDB");

        // Filter by user_id
        let history = storage.load_all_history_with_users(100, 0, Some(&user.id)).await.unwrap();
        assert_eq!(history.len(), 1);

        // Filter by non-existent user
        let history = storage.load_all_history_with_users(100, 0, Some("nonexistent")).await.unwrap();
        assert_eq!(history.len(), 0);
    }
}
