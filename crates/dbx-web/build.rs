// Embeds the build identity of the Web server so a running deployment can be
// told apart from another build of the same release version.
//
// `DBX_BUILD_COMMIT` / `DBX_BUILD_TIME` win when the builder provides them
// (Docker builds have no `.git` in the context); otherwise the local checkout
// is used.

use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Max characters of the revision kept in the binary (matches `--short=12`).
const COMMIT_LEN: usize = 12;

fn env_or_else(name: &str, fallback: impl Fn() -> String) -> String {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(fallback)
}

fn main() {
    println!("cargo:rerun-if-env-changed=DBX_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=DBX_BUILD_TIME");
    // A missing `.git` (Docker build context) must not keep the crate dirty.
    if Path::new("../../.git/HEAD").exists() {
        println!("cargo:rerun-if-changed=../../.git/HEAD");
    }

    let commit = env_or_else("DBX_BUILD_COMMIT", || {
        Command::new("git")
            .args(["rev-parse", "--short=12", "HEAD"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    });
    let commit = commit.chars().take(COMMIT_LEN).collect::<String>();
    println!("cargo:rustc-env=DBX_BUILD_COMMIT={commit}");

    // Milliseconds since the Unix epoch; the frontend formats it for display.
    let build_time = env_or_else("DBX_BUILD_TIME", || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().to_string())
            .unwrap_or_else(|_| "0".to_string())
    });
    println!("cargo:rustc-env=DBX_BUILD_TIME={build_time}");
}
