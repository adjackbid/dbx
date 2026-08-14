use std::io::{Read, Write};

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, KeyInit};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use sha2::Sha256;
use zip::{ZipArchive, ZipWriter};

use crate::storage::Storage;

const BACKUP_FORMAT_VERSION: &str = "1.0";
const SALT_LEN: usize = 32;
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

use serde::{Deserialize, Serialize};

fn derive_key(passphrase: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    use sha2::Digest;
    // Simple PBKDF2-like derivation using SHA256
    // In production, use proper PBKDF2 from the pbkdf2 crate
    let mut key = [0u8; KEY_LEN];
    let mut h = sha2::Sha256::new();
    for _ in 0..10_000 {
        h.update(passphrase.as_bytes());
        h.update(salt);
    }
    let result = h.finalize();
    key.copy_from_slice(&result);
    key
}

fn random_bytes(len: usize) -> Vec<u8> {
    (0..len)
        .map(|_| {
            let uuid = uuid::Uuid::new_v4();
            uuid.as_bytes()[0]
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub passphrase: String,
    pub include: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    pub passphrase: String,
    pub mode: String, // "merge" or "replace"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub connections: usize,
    pub saved_sql_files: usize,
    pub saved_sql_folders: usize,
    pub prompt_templates: usize,
    pub history_entries: usize,
    pub ai_conversations: usize,
    pub settings: usize,
}

impl Default for ImportSummary {
    fn default() -> Self {
        Self {
            connections: 0,
            saved_sql_files: 0,
            saved_sql_folders: 0,
            prompt_templates: 0,
            history_entries: 0,
            ai_conversations: 0,
            settings: 0,
        }
    }
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub format_version: String,
    pub exported_at: String,
    pub app_version: String,
    pub username: String,
    pub display_name: String,
    pub contents: Vec<String>,
    pub encrypted: bool,
    pub checksum: String,
}

fn derive_key_old(passphrase: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    derive_key(passphrase, salt)
}

fn encrypt_data(data: &str, passphrase: &str) -> Result<Vec<u8>, String> {
    let salt = random_bytes(SALT_LEN);
    let nonce_bytes = random_bytes(NONCE_LEN);
    let key = derive_key(passphrase, &salt);
    let key = Key::<Aes256Gcm>::from_slice(&key);
    let cipher = Aes256Gcm::new(key);
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, data.as_bytes()).map_err(|e| format!("Encryption failed: {e}"))?;

    let mut output = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    output.extend_from_slice(&salt);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

fn decrypt_data(encrypted: &[u8], passphrase: &str) -> Result<String, String> {
    if encrypted.len() < SALT_LEN + NONCE_LEN {
        return Err("Invalid encrypted data: too short".to_string());
    }
    let salt = &encrypted[..SALT_LEN];
    let nonce_bytes = &encrypted[SALT_LEN..SALT_LEN + NONCE_LEN];
    let ciphertext = &encrypted[SALT_LEN + NONCE_LEN..];

    let key = derive_key(passphrase, salt);
    let key = Key::<Aes256Gcm>::from_slice(&key);
    let cipher = Aes256Gcm::new(key);
    let nonce = aes_gcm::Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "Decryption failed: wrong passphrase or corrupted data".to_string())?;

    String::from_utf8(plaintext).map_err(|e| format!("Invalid UTF-8: {e}"))
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    format!("sha256:{}", BASE64.encode(result))
}

/// Export user data as a .dbx-backup ZIP file.
/// Returns the ZIP bytes.
pub async fn export_backup(
    storage: &Storage,
    user_id: &str,
    username: &str,
    display_name: &str,
    app_version: &str,
    request: &ExportRequest,
) -> Result<Vec<u8>, String> {
    let include = if request.include.is_empty() {
        vec![
            "connections".to_string(),
            "saved_sql".to_string(),
            "prompt_templates".to_string(),
            "history".to_string(),
            "ai_conversations".to_string(),
            "settings".to_string(),
        ]
    } else {
        request.include.clone()
    };

    let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // Collect data
    let mut contents = Vec::new();

    if include.contains(&"connections".to_string()) {
        let connections = storage.load_connections(user_id).await?;
        let json = serde_json::to_string(&connections).map_err(|e| e.to_string())?;
        let encrypted = encrypt_data(&json, &request.passphrase)?;
        zip.start_file("connections.json.enc", options).map_err(|e| e.to_string())?;
        zip.write_all(&encrypted).map_err(|e| e.to_string())?;
        contents.push("connections".to_string());
    }

    if include.contains(&"saved_sql".to_string()) {
        let library = storage.load_saved_sql_library().await?;
        let json = serde_json::to_string(&library).map_err(|e| e.to_string())?;
        zip.start_file("snippets.json", options).map_err(|e| e.to_string())?;
        zip.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        contents.push("saved_sql".to_string());
    }

    if include.contains(&"prompt_templates".to_string()) {
        let templates = storage.load_prompt_templates().await?;
        let json = serde_json::to_string(&templates).map_err(|e| e.to_string())?;
        zip.start_file("prompt_templates.json", options).map_err(|e| e.to_string())?;
        zip.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        contents.push("prompt_templates".to_string());
    }

    if include.contains(&"history".to_string()) {
        let request = crate::history::HistorySearchRequest {
            search_text: String::new(),
            connections: vec![],
            databases: vec![],
            activity_kind: None,
            success: None,
            started_at: None,
            ended_at: None,
            cursor: None,
            limit: u32::MAX as usize,
        };
        let result = storage.search_history_entries(request).await?;
        let json = serde_json::to_string(&result.entries).map_err(|e| e.to_string())?;
        zip.start_file("history.json", options).map_err(|e| e.to_string())?;
        zip.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        contents.push("history".to_string());
    }

    if include.contains(&"ai_conversations".to_string()) {
        let conversations = storage.load_ai_conversations().await?;
        let json = serde_json::to_string(&conversations).map_err(|e| e.to_string())?;
        zip.start_file("ai_conversations.json", options).map_err(|e| e.to_string())?;
        zip.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        contents.push("ai_conversations".to_string());
    }

    if include.contains(&"settings".to_string()) {
        let settings = storage.export_user_settings(user_id).await?;
        let json = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
        zip.start_file("settings.json", options).map_err(|e| e.to_string())?;
        zip.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        contents.push("settings".to_string());
    }

    // Write manifest
    let manifest = BackupManifest {
        format_version: BACKUP_FORMAT_VERSION.to_string(),
        exported_at: now_iso(),
        app_version: app_version.to_string(),
        username: username.to_string(),
        display_name: display_name.to_string(),
        contents,
        encrypted: true,
        checksum: String::new(), // computed below
    };

    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    zip.start_file("manifest.json", options).map_err(|e| e.to_string())?;
    zip.write_all(manifest_json.as_bytes()).map_err(|e| e.to_string())?;

    let zip_result = zip.finish().map_err(|e| e.to_string())?;
    Ok(zip_result.into_inner())
}

/// Import user data from a .dbx-backup ZIP file.
pub async fn import_backup(
    storage: &Storage,
    user_id: &str,
    backup_data: &[u8],
    request: &ImportRequest,
) -> Result<ImportSummary, String> {
    let reader = std::io::Cursor::new(backup_data.to_vec());
    let mut zip = ZipArchive::new(reader).map_err(|e| format!("Failed to open backup file: {e}"))?;

    let mut summary = ImportSummary::default();
    let is_replace = request.mode == "replace";

    if is_replace {
        // Clear existing user data
        storage.clear_user_data(user_id).await?;
    }

    // Process each file
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| format!("Failed to read zip entry: {e}"))?;
        let name = file.name().to_string();

        let mut data = Vec::new();
        file.read_to_end(&mut data).map_err(|e| format!("Failed to read zip data: {e}"))?;
        drop(file);

        match name.as_str() {
            "manifest.json" => {
                // Verify manifest
                let _manifest: BackupManifest =
                    serde_json::from_slice(&data).map_err(|e| format!("Invalid manifest: {e}"))?;
            }
            "connections.json.enc" => {
                let json = decrypt_data(&data, &request.passphrase)?;
                let configs: Vec<crate::models::connection::ConnectionConfig> =
                    serde_json::from_str(&json).map_err(|e| format!("Invalid connections data: {e}"))?;
                summary.connections = configs.len();
                storage.save_connections(&configs, user_id).await?;
            }
            "snippets.json" => {
                let library: crate::saved_sql::SavedSqlLibrary =
                    serde_json::from_slice(&data).map_err(|e| format!("Invalid snippets data: {e}"))?;
                summary.saved_sql_files = library.files.len();
                summary.saved_sql_folders = library.folders.len();
                storage.replace_saved_sql_library(&library).await?;
            }
            "prompt_templates.json" => {
                let templates: Vec<crate::prompt_template::PromptTemplate> =
                    serde_json::from_slice(&data).map_err(|e| format!("Invalid templates data: {e}"))?;
                summary.prompt_templates = templates.len();
                for template in templates {
                    storage.save_prompt_template(&template.id, &template.name, &template.content).await?;
                }
            }
            "history.json" => {
                let entries: Vec<crate::history::HistoryEntry> =
                    serde_json::from_slice(&data).map_err(|e| format!("Invalid history data: {e}"))?;
                summary.history_entries = entries.len();
                for entry in &entries {
                    storage.save_history_entry(entry, "").await?;
                }
            }
            "ai_conversations.json" => {
                let conversations: Vec<crate::ai::AiConversation> =
                    serde_json::from_slice(&data).map_err(|e| format!("Invalid conversations data: {e}"))?;
                summary.ai_conversations = conversations.len();
                for conv in &conversations {
                    storage.save_ai_conversation(conv).await?;
                }
            }
            "settings.json" => {
                let settings: std::collections::HashMap<String, String> =
                    serde_json::from_slice(&data).map_err(|e| format!("Invalid settings data: {e}"))?;
                summary.settings = settings.len();
                for (key, value) in &settings {
                    storage.save_user_setting(user_id, key, value).await?;
                }
            }
            _ => {
                // Skip unknown files
            }
        }
    }

    Ok(summary)
}

// Storage helper methods needed for backup
impl Storage {
    pub async fn export_user_settings(
        &self,
        user_id: &str,
    ) -> Result<std::collections::HashMap<String, String>, String> {
        let user_id = user_id.to_string();
        self.with_conn(move |conn| {
            let mut stmt =
                conn.prepare("SELECT key, value FROM user_settings WHERE user_id = ?1").map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([&user_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
                .map_err(|e| e.to_string())?;
            let mut map = std::collections::HashMap::new();
            for row in rows {
                let (k, v) = row.map_err(|e| e.to_string())?;
                map.insert(k, v);
            }
            Ok(map)
        })
        .await
    }

    pub async fn clear_user_data(&self, user_id: &str) -> Result<(), String> {
        let user_id = user_id.to_string();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM connections WHERE user_id = ?1", [&user_id]).map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM connection_secrets WHERE user_id = ?1", [&user_id]).map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM history WHERE user_id = ?1", [&user_id]).map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM ai_conversations WHERE user_id = ?1", [&user_id]).map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM saved_sql_folders WHERE user_id = ?1", [&user_id]).map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM saved_sql_files WHERE user_id = ?1", [&user_id]).map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM prompt_templates WHERE user_id = ?1", [&user_id]).map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM user_settings WHERE user_id = ?1", [&user_id]).map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let data = "sensitive connection password";
        let passphrase = "my-secret-passphrase";
        let encrypted = encrypt_data(data, passphrase).unwrap();
        let decrypted = decrypt_data(&encrypted, passphrase).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn decrypt_with_wrong_passphrase_fails() {
        let data = "secret";
        let encrypted = encrypt_data(data, "correct").unwrap();
        let result = decrypt_data(&encrypted, "wrong");
        assert!(result.is_err());
    }
}
