//! Local archive of generated code, independent of the relational/event/blob
//! stores.
//!
//! On every lifecycle turn end, [`export_turn_code`] writes one
//! `{"model": ..., "code": ...}` JSON file per file that was modified or
//! newly created during the turn, holding that file's final content and the
//! model that produced it. This mirrors the layout used by the standalone
//! code-archiver hook plugins for Claude Code / Codex / Copilot / Antigravity
//! (one JSON snapshot per changed file, per prompt), but is captured natively
//! by the daemon instead of a per-agent Python hook script, using the model
//! name the daemon has already resolved for this turn.
//!
//! This is purely a convenience export for browsing "what code got written,
//! by which model" outside of DevQL/the dashboard. It is best-effort and
//! never allowed to fail the surrounding turn-end pipeline: any I/O error for
//! an individual file is skipped rather than propagated.
//!
//! Enabled by default. Disable with `BITLOOPS_CODE_EXPORT_DISABLE` set to any
//! non-empty value. Override the destination directory with
//! `BITLOOPS_CODE_EXPORT_DIR`; it defaults to `~/Desktop/bitloops code`.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const ENV_DISABLE: &str = "BITLOOPS_CODE_EXPORT_DISABLE";
const ENV_DIR: &str = "BITLOOPS_CODE_EXPORT_DIR";
const DEFAULT_MODEL_LABEL: &str = "unknown";
const DEFAULT_PROJECT_LABEL: &str = "unnamed-project";

fn is_disabled() -> bool {
    env::var_os(ENV_DISABLE)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

fn export_root() -> PathBuf {
    if let Some(dir) = env::var_os(ENV_DIR).filter(|value| !value.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Desktop")
        .join("bitloops code")
}

/// Microsecond-resolution timestamp component so multiple files exported in
/// the same turn (or the same file exported across turns) never collide.
fn timestamp_component() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{:06}", now.as_secs(), now.subsec_micros())
}

/// Writes one `{model, code}` JSON file per changed file for this turn.
///
/// `changed_files` should be repo-root-relative paths of files that were
/// modified or newly created this turn (deleted files have nothing to
/// archive and should not be passed in). Duplicate paths are only exported
/// once. Best-effort throughout: this must never panic or return an error
/// that could disrupt turn-end handling.
pub(crate) fn export_turn_code(repo_root: &Path, model: &str, changed_files: &[String]) {
    if is_disabled() || changed_files.is_empty() {
        return;
    }

    let model = {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            DEFAULT_MODEL_LABEL
        } else {
            trimmed
        }
    };

    let project_name = repo_root
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| DEFAULT_PROJECT_LABEL.to_string());

    let export_root = export_root();
    let mut seen: HashSet<&str> = HashSet::new();

    for rel_path in changed_files {
        if rel_path.trim().is_empty() || !seen.insert(rel_path.as_str()) {
            continue;
        }

        let rel = Path::new(rel_path);
        let file_name = match rel.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => continue,
        };

        let code = match fs::read_to_string(repo_root.join(rel)) {
            Ok(content) => content,
            // Binary file, deleted again since, permissions issue, etc. --
            // skip rather than fail the turn-end pipeline over it.
            Err(_) => continue,
        };

        let sub_dir = rel.parent().unwrap_or_else(|| Path::new(""));
        let dest_dir = export_root.join(&project_name).join(sub_dir);
        if fs::create_dir_all(&dest_dir).is_err() {
            continue;
        }

        let dest_name = format!("{}__{}.json", timestamp_component(), file_name);
        let record = serde_json::json!({ "model": model, "code": code });
        if let Ok(serialized) = serde_json::to_string_pretty(&record) {
            let _ = fs::write(dest_dir.join(dest_name), serialized);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    // Environment variables are process-global, so serialize tests that
    // touch BITLOOPS_CODE_EXPORT_DIR / BITLOOPS_CODE_EXPORT_DISABLE.
    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let mut dir = env::temp_dir();
        dir.push(format!(
            "bitloops-code-export-test-{label}-{}",
            timestamp_component()
        ));
        dir
    }

    #[test]
    fn writes_one_json_file_per_changed_file_with_final_content_and_model() {
        let _guard = env_lock().lock().unwrap();

        let repo_root = unique_temp_dir("repo");
        fs::create_dir_all(repo_root.join("src")).unwrap();
        fs::write(repo_root.join("src/a.rs"), "fn a() {}\n").unwrap();
        fs::write(repo_root.join("b.rs"), "fn b() {}\n").unwrap();

        let export_dir = unique_temp_dir("export");
        unsafe {
            env::set_var(ENV_DIR, &export_dir);
            env::remove_var(ENV_DISABLE);
        }

        export_turn_code(
            &repo_root,
            "claude-sonnet-5",
            &["src/a.rs".to_string(), "b.rs".to_string()],
        );

        let project_name = repo_root.file_name().unwrap().to_string_lossy().to_string();
        let a_dir = export_dir.join(&project_name).join("src");
        let b_dir = export_dir.join(&project_name);

        let a_files: Vec<_> = fs::read_dir(&a_dir).unwrap().collect();
        assert_eq!(a_files.len(), 1, "expected exactly one exported file for src/a.rs");
        let a_entry = a_files.into_iter().next().unwrap().unwrap();
        let a_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(a_entry.path()).unwrap()).unwrap();
        assert_eq!(a_json["model"], "claude-sonnet-5");
        assert_eq!(a_json["code"], "fn a() {}\n");

        let b_entries: Vec<_> = fs::read_dir(&b_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .collect();
        assert_eq!(b_entries.len(), 1, "expected exactly one exported file for b.rs");

        unsafe {
            env::remove_var(ENV_DIR);
        }
        let _ = fs::remove_dir_all(&repo_root);
        let _ = fs::remove_dir_all(&export_dir);
    }

    #[test]
    fn disable_env_var_suppresses_export() {
        let _guard = env_lock().lock().unwrap();

        let repo_root = unique_temp_dir("repo-disabled");
        fs::create_dir_all(&repo_root).unwrap();
        fs::write(repo_root.join("a.rs"), "fn a() {}\n").unwrap();

        let export_dir = unique_temp_dir("export-disabled");
        unsafe {
            env::set_var(ENV_DIR, &export_dir);
            env::set_var(ENV_DISABLE, "1");
        }

        export_turn_code(&repo_root, "gpt-5.4", &["a.rs".to_string()]);

        assert!(!export_dir.exists(), "export dir should not be created when disabled");

        unsafe {
            env::remove_var(ENV_DIR);
            env::remove_var(ENV_DISABLE);
        }
        let _ = fs::remove_dir_all(&repo_root);
    }

    #[test]
    fn missing_model_falls_back_to_unknown_label() {
        let _guard = env_lock().lock().unwrap();

        let repo_root = unique_temp_dir("repo-nomodel");
        fs::create_dir_all(&repo_root).unwrap();
        fs::write(repo_root.join("a.rs"), "fn a() {}\n").unwrap();

        let export_dir = unique_temp_dir("export-nomodel");
        unsafe {
            env::set_var(ENV_DIR, &export_dir);
            env::remove_var(ENV_DISABLE);
        }

        export_turn_code(&repo_root, "   ", &["a.rs".to_string()]);

        let project_name = repo_root.file_name().unwrap().to_string_lossy().to_string();
        let dir = export_dir.join(&project_name);
        let entry = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .find(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(entry.path()).unwrap()).unwrap();
        assert_eq!(json["model"], "unknown");

        unsafe {
            env::remove_var(ENV_DIR);
        }
        let _ = fs::remove_dir_all(&repo_root);
        let _ = fs::remove_dir_all(&export_dir);
    }
}
