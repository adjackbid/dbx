use std::time::Duration;

use ldap3::{LdapConn, LdapConnSettings, Scope, SearchEntry};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::storage::Storage;

const LDAP_CONFIG_KEY: &str = "ldap_config";
const LDAP_PASSWORD_MASK: &str = "********";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LdapConfig {
    pub enabled: bool,
    pub server_url: String,
    pub use_starttls: bool,
    pub bind_dn: String,
    pub bind_password: String,
    pub user_base: String,
    pub user_filter: String,
    pub user_scope: String,
    pub username_attr: String,
    pub display_name_attr: String,
    pub email_attr: String,
    pub auto_create_user: bool,
    pub admin_filter: String,
    pub connection_timeout_secs: u64,
    pub search_timeout_secs: u64,
    pub verify_cert: bool,
    pub ca_cert_pem: String,
}

impl Default for LdapConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server_url: String::new(),
            use_starttls: false,
            bind_dn: String::new(),
            bind_password: String::new(),
            user_base: String::new(),
            user_filter: "(SAMAccountName={username})".to_string(),
            user_scope: "subtree".to_string(),
            username_attr: "sAMAccountName".to_string(),
            display_name_attr: "cn".to_string(),
            email_attr: "mail".to_string(),
            auto_create_user: true,
            admin_filter: String::new(),
            connection_timeout_secs: 10,
            search_timeout_secs: 15,
            verify_cert: true,
            ca_cert_pem: String::new(),
        }
    }
}

impl LdapConfig {
    /// Return config with bind_password masked — for API responses.
    pub fn masked(&self) -> Self {
        let mut c = self.clone();
        if !c.bind_password.is_empty() {
            c.bind_password = LDAP_PASSWORD_MASK.to_string();
        }
        c
    }

    /// If the incoming password is the mask or empty, keep the existing password.
    pub fn merge_password(&self, incoming: &LdapConfig) -> Self {
        let mut merged = incoming.clone();
        if merged.bind_password == LDAP_PASSWORD_MASK || merged.bind_password.is_empty() {
            merged.bind_password = self.bind_password.clone();
        }
        merged
    }

    fn is_complete(&self) -> bool {
        self.enabled && !self.server_url.is_empty() && !self.user_base.is_empty() && !self.user_filter.is_empty()
    }

    fn scope(&self) -> Scope {
        match self.user_scope.as_str() {
            "onelevel" => Scope::OneLevel,
            _ => Scope::Subtree,
        }
    }

    fn conn_settings(&self) -> LdapConnSettings {
        let mut settings = LdapConnSettings::new();
        if self.use_starttls {
            settings = settings.set_starttls(true);
        }
        if !self.verify_cert {
            settings = settings.set_no_tls_verify(true);
        }
        settings
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(self.connection_timeout_secs.max(1))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LdapTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
    pub details: Option<LdapUserDetails>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LdapUserDetails {
    pub user_dn: String,
    pub username: String,
    pub display_name: String,
    pub email: String,
    pub is_admin_by_filter: bool,
}

impl Storage {
    pub async fn load_ldap_config(&self) -> Result<LdapConfig, String> {
        let value = self.get_system_setting(LDAP_CONFIG_KEY).await?;
        match value {
            Some(json) => serde_json::from_str(&json).map_err(|e| format!("Failed to parse LDAP config: {e}")),
            None => Ok(LdapConfig::default()),
        }
    }

    pub async fn save_ldap_config(&self, config: &LdapConfig) -> Result<(), String> {
        let json = serde_json::to_string(config).map_err(|e| format!("Failed to serialize LDAP config: {e}"))?;
        self.save_system_setting(LDAP_CONFIG_KEY, &json).await
    }

    pub async fn get_system_setting(&self, key: &str) -> Result<Option<String>, String> {
        let key = key.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT value FROM user_settings WHERE user_id = '__system__' AND key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
        })
        .await
    }

    pub async fn save_system_setting(&self, key: &str, value: &str) -> Result<(), String> {
        let key = key.to_string();
        let value = value.to_string();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO user_settings (user_id, key, value) VALUES ('__system__', ?1, ?2)",
                params![key, value],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
    }
}

// ===== LDAP Operations =====

/// Test stage 1: Connection + service bind
pub fn test_ldap_connection(config: &LdapConfig) -> LdapTestResult {
    if config.server_url.is_empty() {
        return LdapTestResult {
            ok: false,
            latency_ms: 0,
            error: Some("Server URL is required".to_string()),
            details: None,
        };
    }

    let start = std::time::Instant::now();
    match connect_and_bind(config) {
        Ok((_ldap, _)) => {
            LdapTestResult { ok: true, latency_ms: start.elapsed().as_millis() as u64, error: None, details: None }
        }
        Err(e) => {
            LdapTestResult { ok: false, latency_ms: start.elapsed().as_millis() as u64, error: Some(e), details: None }
        }
    }
}

/// Test stage 2: User search
pub fn test_ldap_search(config: &LdapConfig, test_username: &str) -> LdapTestResult {
    let start = std::time::Instant::now();
    match search_user(config, test_username) {
        Ok(details) => LdapTestResult {
            ok: true,
            latency_ms: start.elapsed().as_millis() as u64,
            error: None,
            details: Some(details),
        },
        Err(e) => {
            LdapTestResult { ok: false, latency_ms: start.elapsed().as_millis() as u64, error: Some(e), details: None }
        }
    }
}

/// Test stage 3: User bind (login test)
pub fn test_ldap_bind(config: &LdapConfig, test_username: &str, test_password: &str) -> LdapTestResult {
    let start = std::time::Instant::now();

    // First find the user DN
    let details = match search_user(config, test_username) {
        Ok(d) => d,
        Err(e) => {
            return LdapTestResult {
                ok: false,
                latency_ms: start.elapsed().as_millis() as u64,
                error: Some(format!("User search failed: {e}")),
                details: None,
            };
        }
    };

    // Then try to bind as that user
    match bind_as_user(config, &details.user_dn, test_password) {
        Ok(()) => LdapTestResult {
            ok: true,
            latency_ms: start.elapsed().as_millis() as u64,
            error: None,
            details: Some(details),
        },
        Err(e) => LdapTestResult {
            ok: false,
            latency_ms: start.elapsed().as_millis() as u64,
            error: Some(format!("Bind failed: {e}")),
            details: Some(details),
        },
    }
}

/// Authenticate a user via LDAP. Returns user details on success.
pub fn authenticate_ldap(config: &LdapConfig, username: &str, password: &str) -> Result<LdapUserDetails, String> {
    if !config.is_complete() {
        return Err("LDAP is not configured or incomplete".to_string());
    }

    let details = search_user(config, username)?;
    bind_as_user(config, &details.user_dn, password)?;
    Ok(details)
}

fn connect_and_bind(config: &LdapConfig) -> Result<(LdapConn, ()), String> {
    let settings = config.conn_settings();
    let mut ldap = LdapConn::with_settings(settings, &config.server_url)
        .map_err(|e| format!("Failed to connect to LDAP server: {e}"))?;

    // Bind: use service account if configured, otherwise anonymous bind
    if !config.bind_dn.is_empty() {
        ldap.simple_bind(&config.bind_dn, &config.bind_password)
            .map_err(|e| format!("Service bind failed: {e}"))?
            .success()
            .map_err(|e| format!("Service bind failed: {e}"))?;
    } else {
        // Anonymous bind — may not work for all AD servers
        ldap.simple_bind("", "")
            .map_err(|e| format!("Anonymous bind failed: {e}"))?
            .success()
            .map_err(|e| format!("Anonymous bind failed: {e}"))?;
    }

    Ok((ldap, ()))
}

/// Derive UPN domain from base DN: "DC=txc,DC=com,DC=tw" → "txc.com.tw"
fn upn_domain_from_base_dn(base_dn: &str) -> String {
    base_dn
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            part.strip_prefix("DC=").or_else(|| part.strip_prefix("dc="))
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Connect and bind as a specific user (for AD password verification).
/// Uses UPN format: username@domain (derived from base DN).
fn connect_and_bind_as_user(config: &LdapConfig, username: &str, password: &str) -> Result<LdapConn, String> {
    let settings = config.conn_settings();
    let mut ldap = LdapConn::with_settings(settings, &config.server_url)
        .map_err(|e| format!("Failed to connect to LDAP server: {e}"))?;

    // Try UPN format first: username@domain
    let domain = upn_domain_from_base_dn(&config.user_base);
    if !domain.is_empty() {
        let upn = format!("{username}@{domain}");
        match ldap.simple_bind(&upn, password).map_err(|e| format!("Bind failed: {e}"))?.success() {
            Ok(_) => return Ok(ldap),
            Err(e) => {
                // UPN failed — try username@server_domain as fallback
                log::debug!("UPN bind failed ({upn}): {e}, trying alternative formats");
            }
        }
    }

    // Try plain username
    ldap.simple_bind(username, password)
        .map_err(|e| format!("User bind failed: {e}"))?
        .success()
        .map_err(|e| format!("Invalid credentials: {e}"))?;

    Ok(ldap)
}

fn search_user(config: &LdapConfig, username: &str) -> Result<LdapUserDetails, String> {
    // If no service account configured, bind as the user to search (AD requires authed bind)
    if config.bind_dn.is_empty() {
        // Can't search without binding — return a minimal details from what we know
        // The actual authentication will happen in bind_as_user
        let domain = upn_domain_from_base_dn(&config.user_base);
        let user_dn = if !domain.is_empty() { format!("{username}@{domain}") } else { username.to_string() };
        return Ok(LdapUserDetails {
            user_dn,
            username: username.to_string(),
            display_name: username.to_string(),
            email: String::new(),
            is_admin_by_filter: false,
        });
    }

    let (mut ldap, _) = connect_and_bind(config)?;

    let filter = config.user_filter.replace("{username}", username);
    let scope = config.scope();

    let (rs, _res) = ldap
        .search(&config.user_base, scope, &filter, &Vec::<&str>::new())
        .map_err(|e| format!("Search failed: {e}"))?
        .success()
        .map_err(|e| format!("Search failed: {e}"))?;

    if rs.is_empty() {
        ldap.unbind().ok();
        return Err(format!("No user found for '{username}' with filter '{filter}'"));
    }

    if rs.len() > 1 {
        ldap.unbind().ok();
        return Err(format!("Multiple users found for '{username}': expected exactly one match"));
    }

    let entry = SearchEntry::construct(rs[0].clone());
    let user_dn = entry.dn;

    let username = entry.attrs.get(&config.username_attr).and_then(|vals| vals.first()).cloned().unwrap_or_default();

    let display_name =
        entry.attrs.get(&config.display_name_attr).and_then(|vals| vals.first()).cloned().unwrap_or_default();

    let email = entry.attrs.get(&config.email_attr).and_then(|vals| vals.first()).cloned().unwrap_or_default();

    // Check admin filter
    let is_admin_by_filter =
        if config.admin_filter.is_empty() { false } else { check_admin_filter(config, &user_dn).unwrap_or(false) };

    ldap.unbind().ok();

    Ok(LdapUserDetails { user_dn, username, display_name, email, is_admin_by_filter })
}

fn bind_as_user(config: &LdapConfig, user_dn: &str, password: &str) -> Result<(), String> {
    let settings = config.conn_settings();
    let mut ldap = LdapConn::with_settings(settings, &config.server_url)
        .map_err(|e| format!("Failed to connect for user bind: {e}"))?;

    ldap.simple_bind(user_dn, password)
        .map_err(|e| format!("User bind failed: {e}"))?
        .success()
        .map_err(|e| format!("Invalid credentials: {e}"))?;

    ldap.unbind().ok();
    Ok(())
}

fn check_admin_filter(config: &LdapConfig, user_dn: &str) -> Result<bool, String> {
    let (mut ldap, _) = connect_and_bind(config)?;

    let filter = config.admin_filter.clone();
    let scope = config.scope();

    let (rs, _res) = ldap
        .search(&config.user_base, scope, &filter, &Vec::<&str>::new())
        .map_err(|e| format!("Admin filter search failed: {e}"))?
        .success()
        .map_err(|e| format!("Admin filter search failed: {e}"))?;

    ldap.unbind().ok();

    Ok(rs.iter().any(|entry| SearchEntry::construct(entry.clone()).dn == user_dn))
}
