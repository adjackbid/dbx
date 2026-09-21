use axum::{extract::Query, Json};
use dbx_core::{changelog, update};

use crate::error::AppError;

pub async fn get_version() -> Json<serde_json::Value> {
    Json(build_info())
}

/// Version plus the identity of the build that is actually running, so an
/// operator can tell two builds of the same release apart.
pub fn build_info() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "commit": env!("DBX_BUILD_COMMIT"),
        "buildTimeMs": env!("DBX_BUILD_TIME"),
    })
}

#[cfg(test)]
mod tests {
    use super::build_info;

    #[test]
    fn build_info_reports_version_commit_and_build_time() {
        let info = build_info();
        let field = |name: &str| info.get(name).and_then(serde_json::Value::as_str).unwrap_or_default();
        assert_eq!(field("version"), env!("CARGO_PKG_VERSION"), "version must come from the manifest");
        assert!(!field("commit").is_empty(), "commit must be embedded at build time");
        assert!(
            field("buildTimeMs").parse::<u64>().is_ok(),
            "buildTimeMs must be Unix milliseconds, got `{}`",
            field("buildTimeMs")
        );
    }
}

#[derive(serde::Deserialize)]
pub struct UpdateCheckParams {
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub source: Option<dbx_core::DownloadSource>,
}

pub async fn check_for_updates(Query(params): Query<UpdateCheckParams>) -> Result<Json<serde_json::Value>, AppError> {
    let locale = params.locale.unwrap_or_else(|| "zh-CN".to_string());
    let release =
        update::fetch_latest_release(&locale, params.source.unwrap_or_default()).await.map_err(AppError::from)?;
    let info = update::build_update_info(release, env!("CARGO_PKG_VERSION"));
    Ok(Json(serde_json::to_value(info).map_err(|e| AppError::from(e.to_string()))?))
}

#[derive(serde::Deserialize)]
pub struct ChangelogParams {
    #[serde(default)]
    pub lang: Option<String>,
}

pub async fn fetch_changelog(
    Query(params): Query<ChangelogParams>,
) -> Result<Json<changelog::ChangelogData>, AppError> {
    let lang = params.lang.unwrap_or_else(|| "en".to_string());
    let data = changelog::fetch_changelog(&lang).await.map_err(AppError::from)?;
    Ok(Json(data))
}
