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
//! A file is only archived again when its content differs from its newest
//! archived copy, so files that stay uncommitted across turns are not
//! re-exported every turn.
//!
//! This is purely a convenience export for browsing "what code got written,
//! by which model" outside of DevQL/the dashboard. It is best-effort and
//! never allowed to fail the surrounding turn-end pipeline: any I/O error for
//! an individual file is skipped rather than propagated.
//!
//! Enabled by default. Disable with `BITLOOPS_CODE_EXPORT_DISABLE` set to any
//! non-empty value. Override the destination directory with
//! `BITLOOPS_CODE_EXPORT_DIR`; it defaults to `~/Desktop/cycloops-code`.
//!
//! Set `BITLOOPS_CODE_EXPORT_V2=1` to archive HEAD-relative unified diff
//! snippets under `<export_root>/v2/<project>/` instead of the v1 snapshots;
//! only one format is written at a time. Each record
//! includes session/turn identifiers, timestamp and available token usage.
//! Hunks use three context lines; old/new line counts describe whole files.
//! Deleted files retain their HEAD content as removal hunks. Restoring a
//! modified file requires the corresponding HEAD version as a base.

#[path = "code_export_v2.rs"]
mod v2;
pub(crate) use v2::export_turn_code_v2;
#[cfg(test)]
#[path = "code_export_v2_tests.rs"]
mod v2_tests;

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const ENV_DISABLE: &str = "BITLOOPS_CODE_EXPORT_DISABLE";
const ENV_DIR: &str = "BITLOOPS_CODE_EXPORT_DIR";
const ENV_TRACE: &str = "BITLOOPS_CODE_EXPORT_TRACE";
const DEFAULT_MODEL_LABEL: &str = "unknown";
const DEFAULT_PROJECT_LABEL: &str = "unnamed-project";

fn is_disabled() -> bool {
    env::var_os(ENV_DISABLE)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn is_hidden_path(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(component, Component::Normal(name) if name.to_string_lossy().starts_with('.'))
    })
}

/// Prints archiver decisions to stderr when `BITLOOPS_CODE_EXPORT_TRACE` is set,
/// since the archiver otherwise skips files without saying why.
pub(crate) fn trace(message: &str) {
    log::debug!(target: "code_export", "{message}");
    if env::var_os(ENV_TRACE).is_some_and(|value| !value.is_empty()) {
        eprintln!("[cycloops-code-export] {message}");
    }
}

/// Content fingerprint of every file that was already uncommitted when a
/// session's turn began (`None` marks a file that was already deleted).
type TurnStartBaseline = BTreeMap<String, Option<String>>;

fn file_fingerprint(repo_root: &Path, rel_path: &str) -> Option<String> {
    let bytes = fs::read(repo_root.join(rel_path)).ok()?;
    Some(hex::encode(Sha256::digest(bytes)))
}

fn turn_start_state_path(repo_root: &Path, session_id: &str) -> PathBuf {
    let project = repo_root
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| DEFAULT_PROJECT_LABEL.to_string());
    let session: String = session_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    export_root()
        .join(".turn-start")
        .join(project)
        .join(format!("{session}.json"))
}

/// Remembers which files are already uncommitted, and with what content, when a
/// turn starts. Several agents can share one working tree, and `git status` at
/// turn end cannot tell whose edits it is looking at; comparing against this
/// baseline keeps a session from being credited with changes it did not make.
pub(crate) fn record_turn_start(repo_root: &Path, session_id: &str) {
    if is_disabled() || session_id.trim().is_empty() {
        return;
    }
    let (modified, new_files, deleted) =
        super::git_workspace::detect_file_changes_for_turn_end(repo_root, None);
    let mut baseline = TurnStartBaseline::new();
    for path in modified.into_iter().chain(new_files) {
        let fingerprint = file_fingerprint(repo_root, &path);
        baseline.insert(path, fingerprint);
    }
    for path in deleted {
        baseline.insert(path, None);
    }
    let state_path = turn_start_state_path(repo_root, session_id);
    let written = state_path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .map_err(|err| err.to_string())
        .and_then(|()| serde_json::to_vec(&baseline).map_err(|err| err.to_string()))
        .and_then(|bytes| fs::write(&state_path, bytes).map_err(|err| err.to_string()));
    match written {
        Ok(()) => trace(&format!(
            "turn start for session {session_id}: {} file(s) already uncommitted",
            baseline.len()
        )),
        Err(err) => trace(&format!("cannot record turn start baseline: {err}")),
    }
}

fn load_turn_start(repo_root: &Path, session_id: &str) -> Option<TurnStartBaseline> {
    let bytes = fs::read(turn_start_state_path(repo_root, session_id)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Keeps the files whose content differs from what the turn started with.
fn changed_since_turn_start(
    repo_root: &Path,
    baseline: &TurnStartBaseline,
    files: Vec<String>,
) -> Vec<String> {
    files
        .into_iter()
        .filter(|path| match baseline.get(path) {
            Some(Some(before)) => file_fingerprint(repo_root, path).as_ref() != Some(before),
            Some(None) | None => true,
        })
        .inspect(|path| trace(&format!("{path}: changed during this turn")))
        .collect()
}

fn export_root() -> PathBuf {
    if let Some(dir) = env::var_os(ENV_DIR).filter(|value| !value.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Desktop")
        .join("cycloops-code")
}

/// Microsecond-resolution timestamp component so multiple files exported in
/// the same turn (or the same file exported across turns) never collide.
fn timestamp_component() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{:06}", now.as_secs(), now.subsec_micros())
}

/// Archives changed files straight from the agent's turn-end hook process.
///
/// Claude Code and other agents hand turn-end work to the daemon through a
/// SQLite queue. If the hook cannot reach that database, or the daemon is not
/// running, the queued job never runs and nothing would be archived. This path
/// needs neither: it lists files changed since the last commit with `git
/// status` and reads the model from the transcript. The daemon's own export
/// later skips anything already archived with the same content.
pub(crate) fn export_turn_code_from_hook(
    repo_root: &Path,
    model_hint: &str,
    transcript_path: &str,
    session_id: &str,
    turn_id: Option<&str>,
) {
    if is_disabled() {
        trace("disabled by BITLOOPS_CODE_EXPORT_DISABLE");
        return;
    }
    let (modified, new_files, deleted) =
        super::git_workspace::detect_file_changes_for_turn_end(repo_root, None);
    let mut changed_files: Vec<String> = modified.into_iter().chain(new_files).collect();
    let mut deleted = deleted;
    if let Some(baseline) = load_turn_start(repo_root, session_id) {
        changed_files = changed_since_turn_start(repo_root, &baseline, changed_files);
        deleted.retain(|path| !matches!(baseline.get(path), Some(None)));
    } else {
        trace("no turn-start baseline for this session; archiving every uncommitted file");
    }
    trace(&format!(
        "hook-side archive in {}: {} changed file(s): {:?}",
        repo_root.display(),
        changed_files.len(),
        changed_files
    ));
    if changed_files.is_empty() && deleted.is_empty() {
        return;
    }
    let transcript = fs::read(transcript_path).unwrap_or_default();
    let model = crate::host::interactions::model::resolve_interaction_model_from_bytes(
        model_hint,
        &transcript,
    );
    export_turn_code(repo_root, &model, &changed_files);
    let turn_id = turn_id
        .filter(|id| !id.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(super::time_and_ids::generate_lifecycle_turn_id);
    v2::export_turn_code_v2_from_hook(
        repo_root,
        &model,
        session_id,
        &turn_id,
        &super::time_and_ids::now_rfc3339(),
        &changed_files,
        &deleted,
    );
}

/// Parses `{secs}-{micros}__{file_name}.json` back into its timestamp, or
/// returns `None` if `dest_name` is not an archive of exactly `file_name`.
fn parse_archive_stamp(dest_name: &str, file_name: &str) -> Option<(u64, u64)> {
    let stem = dest_name.strip_suffix(".json")?;
    let (stamp, rest) = stem.split_once("__")?;
    if rest != file_name {
        return None;
    }
    let (secs, micros) = stamp.split_once('-')?;
    Some((secs.parse().ok()?, micros.parse().ok()?))
}

/// Content of the newest archived copy of `file_name` in `dest_dir`, if any.
fn latest_archived_code(dest_dir: &Path, file_name: &str) -> Option<String> {
    let (_, newest_path) = fs::read_dir(dest_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            parse_archive_stamp(&name, file_name).map(|stamp| (stamp, entry.path()))
        })
        .max_by_key(|(stamp, _)| *stamp)?;
    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(newest_path).ok()?).ok()?;
    record.get("code")?.as_str().map(str::to_owned)
}

/// Writes one `{model, code}` JSON file per changed file for this turn.
///
/// `changed_files` should be repo-root-relative paths of files that were
/// modified or newly created (deleted files have nothing to archive and
/// should not be passed in). The caller's list can include files changed in
/// earlier, still-uncommitted turns, so a file whose content is identical to
/// its newest archived copy is skipped. Duplicate paths are only exported
/// once. Best-effort throughout: this must never panic or return an error
/// that could disrupt turn-end handling.
pub(crate) fn export_turn_code(repo_root: &Path, model: &str, changed_files: &[String]) {
    if is_disabled() || changed_files.is_empty() {
        return;
    }
    if v2::is_v2_enabled() {
        trace("skip v1 snapshots: BITLOOPS_CODE_EXPORT_V2=1 archives v2 records only");
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
        if is_hidden_path(rel) {
            trace(&format!(
                "skip {rel_path}: dot-prefixed files and directories are not archived"
            ));
            continue;
        }
        if is_markdown_path(rel) {
            trace(&format!("skip {rel_path}: Markdown files are not archived"));
            continue;
        }
        let file_name = match rel.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => continue,
        };

        let code = match fs::read_to_string(repo_root.join(rel)) {
            Ok(content) => content,
            // Binary file, deleted again since, permissions issue, etc. --
            // skip rather than fail the turn-end pipeline over it.
            Err(err) => {
                trace(&format!("skip {rel_path}: cannot read as text ({err})"));
                continue;
            }
        };

        let sub_dir = rel.parent().unwrap_or_else(|| Path::new(""));
        let dest_dir = export_root.join(&project_name).join(sub_dir);
        if latest_archived_code(&dest_dir, &file_name).as_deref() == Some(code.as_str()) {
            trace(&format!(
                "skip {rel_path}: unchanged since its newest archived copy"
            ));
            continue;
        }
        if let Err(err) = fs::create_dir_all(&dest_dir) {
            trace(&format!(
                "skip {rel_path}: cannot create {} ({err})",
                dest_dir.display()
            ));
            continue;
        }

        let dest_name = format!("{}__{}.json", timestamp_component(), file_name);
        let record = serde_json::json!({ "model": model, "code": code });
        match serde_json::to_string_pretty(&record) {
            Ok(serialized) => match fs::write(dest_dir.join(&dest_name), serialized) {
                Ok(()) => trace(&format!(
                    "archived {rel_path} -> {}",
                    dest_dir.join(&dest_name).display()
                )),
                Err(err) => trace(&format!("skip {rel_path}: write failed ({err})")),
            },
            Err(err) => trace(&format!("skip {rel_path}: cannot serialise ({err})")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    // Environment variables are process-global, so serialize tests that
    // touch BITLOOPS_CODE_EXPORT_DIR / BITLOOPS_CODE_EXPORT_DISABLE.
    pub(super) fn env_lock() -> &'static Mutex<()> {
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
        assert_eq!(
            a_files.len(),
            1,
            "expected exactly one exported file for src/a.rs"
        );
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
        assert_eq!(
            b_entries.len(),
            1,
            "expected exactly one exported file for b.rs"
        );

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

        assert!(
            !export_dir.exists(),
            "export dir should not be created when disabled"
        );

        unsafe {
            env::remove_var(ENV_DIR);
            env::remove_var(ENV_DISABLE);
        }
        let _ = fs::remove_dir_all(&repo_root);
    }

    #[test]
    fn markdown_files_are_not_archived() {
        let _guard = env_lock().lock().unwrap();

        let repo_root = unique_temp_dir("repo-markdown");
        fs::create_dir_all(&repo_root).unwrap();
        fs::write(repo_root.join("README.md"), "# README\n").unwrap();
        fs::write(repo_root.join("NOTES.MD"), "notes\n").unwrap();

        let export_dir = unique_temp_dir("export-markdown");
        unsafe {
            env::set_var(ENV_DIR, &export_dir);
            env::remove_var(ENV_DISABLE);
        }

        export_turn_code(
            &repo_root,
            "test-model",
            &["README.md".to_string(), "NOTES.MD".to_string()],
        );

        assert!(!export_dir.exists());

        unsafe {
            env::remove_var(ENV_DIR);
        }
        let _ = fs::remove_dir_all(&repo_root);
    }

    #[test]
    fn dot_prefixed_files_and_directories_are_not_archived() {
        let _guard = env_lock().lock().unwrap();

        let repo_root = unique_temp_dir("repo-hidden");
        fs::create_dir_all(repo_root.join(".matrixx")).unwrap();
        fs::create_dir_all(repo_root.join("nested/.gemini")).unwrap();
        fs::write(repo_root.join(".env"), "SECRET=value\n").unwrap();
        fs::write(repo_root.join(".matrixx/plan.json"), "{}\n").unwrap();
        fs::write(repo_root.join("nested/.gemini/settings.json"), "{}\n").unwrap();

        let export_dir = unique_temp_dir("export-hidden");
        unsafe {
            env::set_var(ENV_DIR, &export_dir);
            env::remove_var(ENV_DISABLE);
        }

        export_turn_code(
            &repo_root,
            "test-model",
            &[
                ".env".to_string(),
                ".matrixx/plan.json".to_string(),
                "nested/.gemini/settings.json".to_string(),
            ],
        );

        assert!(!export_dir.exists());

        unsafe {
            env::remove_var(ENV_DIR);
        }
        let _ = fs::remove_dir_all(&repo_root);
    }

    fn archived_codes(dir: &Path, file_name: &str) -> Vec<String> {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                parse_archive_stamp(&name, file_name).map(|stamp| (stamp, entry.path()))
            })
            .collect();
        entries.sort_by_key(|(stamp, _)| *stamp);
        entries
            .into_iter()
            .map(|(_, path)| {
                let json: serde_json::Value =
                    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
                json["code"].as_str().unwrap().to_string()
            })
            .collect()
    }

    #[test]
    fn unchanged_file_is_not_archived_again_but_changed_or_reverted_content_is() {
        let _guard = env_lock().lock().unwrap();

        let repo_root = unique_temp_dir("repo-dedupe");
        fs::create_dir_all(&repo_root).unwrap();
        let export_dir = unique_temp_dir("export-dedupe");
        unsafe {
            env::set_var(ENV_DIR, &export_dir);
            env::remove_var(ENV_DISABLE);
        }
        let files = ["a.rs".to_string()];
        let project_name = repo_root.file_name().unwrap().to_string_lossy().to_string();
        let dest_dir = export_dir.join(&project_name);
        let pause = || std::thread::sleep(std::time::Duration::from_millis(3));

        fs::write(repo_root.join("a.rs"), "v1").unwrap();
        export_turn_code(&repo_root, "m", &files);
        pause();
        export_turn_code(&repo_root, "m", &files);
        assert_eq!(archived_codes(&dest_dir, "a.rs"), ["v1"]);

        pause();
        fs::write(repo_root.join("a.rs"), "v2").unwrap();
        export_turn_code(&repo_root, "m", &files);
        assert_eq!(archived_codes(&dest_dir, "a.rs"), ["v1", "v2"]);

        pause();
        fs::write(repo_root.join("a.rs"), "v1").unwrap();
        export_turn_code(&repo_root, "m", &files);
        assert_eq!(archived_codes(&dest_dir, "a.rs"), ["v1", "v2", "v1"]);

        unsafe {
            env::remove_var(ENV_DIR);
        }
        let _ = fs::remove_dir_all(&repo_root);
        let _ = fs::remove_dir_all(&export_dir);
    }

    #[test]
    fn archive_stamp_only_matches_the_exact_file_name() {
        assert_eq!(
            parse_archive_stamp("1789891344-493779__ariadne1.js.json", "ariadne1.js"),
            Some((1789891344, 493779))
        );
        assert_eq!(
            parse_archive_stamp("1789891344-493779__a__b.js.json", "b.js"),
            None
        );
        assert_eq!(
            parse_archive_stamp("1789891344-493779__a__b.js.json", "a__b.js"),
            Some((1789891344, 493779))
        );
        assert_eq!(parse_archive_stamp("notes.json", "notes"), None);
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
