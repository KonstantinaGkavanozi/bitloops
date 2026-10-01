//! Optional session-stepped code archive. The v1 reader/writer remains separate.
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use super::super::diff_hunks;
use super::{
    export_root, is_disabled, is_hidden_path, is_markdown_path, parse_archive_stamp,
    timestamp_component, trace,
};
use crate::adapters::agents::TokenUsage;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ChangeType {
    Modified,
    New,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CodeSnippet {
    file_path: String,
    line_start: usize,
    line_end: usize,
    old_line_count: usize,
    new_line_count: usize,
    context_lines: usize,
    diff_hunk: String,
    change_type: ChangeType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ArchiveTokenUsage {
    input_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
    output_tokens: u64,
    api_call_count: u64,
}

impl From<&TokenUsage> for ArchiveTokenUsage {
    fn from(t: &TokenUsage) -> Self {
        Self {
            input_tokens: t.input_tokens.max(0) as u64,
            cache_creation_tokens: t.cache_creation_tokens.max(0) as u64,
            cache_read_tokens: t.cache_read_tokens.max(0) as u64,
            output_tokens: t.output_tokens.max(0) as u64,
            api_call_count: t.api_call_count.max(0) as u64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ModelContext {
    name: String,
    token_usage: Option<ArchiveTokenUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ArchiveMetadata {
    version: u32,
    session_id: String,
    turn_id: String,
    #[serde(default)]
    step: u64,
    timestamp: String,
    model: ModelContext,
    snippets: Vec<CodeSnippet>,
    #[serde(default)]
    current_file: Option<String>,
}

fn latest_archive(
    dir: &Path,
    name: &str,
    session_id: Option<&str>,
) -> Option<(std::path::PathBuf, ArchiveMetadata)> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let stamp = parse_archive_stamp(&entry.file_name().to_string_lossy(), name)?;
            let path = entry.path();
            let record: ArchiveMetadata = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
            (record.version == 2
                && session_id.map_or(true, |expected| record.session_id == expected))
            .then_some((stamp, path, record))
        })
        .max_by_key(|(stamp, _, _)| *stamp)
        .map(|(_, path, record)| (path, record))
}

fn write_archive(
    dir: &Path,
    name: &str,
    record: &ArchiveMetadata,
) -> anyhow::Result<std::path::PathBuf> {
    use std::io::Write;

    let serialized = serde_json::to_vec_pretty(record)?;
    fs::create_dir_all(dir)?;
    for _ in 0..100 {
        let path = dir.join(format!("{}__{name}.json", timestamp_component()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut output) => {
                output.write_all(&serialized)?;
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(anyhow::anyhow!(
        "could not allocate a unique archive filename for {name}"
    ))
}

#[derive(Clone, Copy)]
enum LogSink {
    Daemon,
    Hook,
}

impl LogSink {
    fn warn(self, session: &str, message: &str) {
        match self {
            Self::Daemon => log::warn!(target: "code_export", "{message}"),
            Self::Hook => {
                use crate::telemetry::logging;
                let ctx = logging::with_component(
                    logging::with_session(logging::background(), session),
                    "code_export",
                );
                logging::warn(&ctx, message, &[]);
            }
        }
    }
}

pub(super) fn is_v2_enabled() -> bool {
    std::env::var("BITLOOPS_CODE_EXPORT_V2").as_deref() == Ok("1")
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn export_turn_code_v2(
    repo_root: &Path,
    model: &str,
    session_id: &str,
    turn_id: &str,
    timestamp: &str,
    token_usage: Option<&TokenUsage>,
    changed_files: &[String],
    deleted_files: &[String],
) {
    export(
        repo_root,
        model,
        session_id,
        turn_id,
        timestamp,
        token_usage,
        changed_files,
        deleted_files,
        LogSink::Daemon,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn export_turn_code_v2_from_hook(
    repo_root: &Path,
    model: &str,
    session_id: &str,
    turn_id: &str,
    timestamp: &str,
    changed_files: &[String],
    deleted_files: &[String],
) {
    export(
        repo_root,
        model,
        session_id,
        turn_id,
        timestamp,
        None,
        changed_files,
        deleted_files,
        LogSink::Hook,
    );
}

#[allow(clippy::too_many_arguments)]
fn export(
    repo_root: &Path,
    model: &str,
    session_id: &str,
    turn_id: &str,
    timestamp: &str,
    token_usage: Option<&TokenUsage>,
    changed_files: &[String],
    deleted_files: &[String],
    sink: LogSink,
) {
    if is_disabled() || !is_v2_enabled() {
        return;
    }
    let project = repo_root
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new(super::DEFAULT_PROJECT_LABEL));
    let root = export_root().join("v2").join(project);
    let mut seen = HashSet::new();
    for file in changed_files.iter().chain(deleted_files) {
        let rel = Path::new(file);
        if file.trim().is_empty() || !seen.insert(file) {
            continue;
        }
        if is_hidden_path(rel) {
            trace(&format!(
                "skip {file}: dot-prefixed files and directories are not archived"
            ));
            continue;
        }
        if is_markdown_path(rel) {
            trace(&format!("skip {file}: Markdown files are not archived"));
            continue;
        }
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            sink.warn(
                session_id,
                &format!("skip {file}: expected repository-relative path"),
            );
            continue;
        }
        let Some(name) = rel.file_name() else {
            continue;
        };
        let result = (|| -> anyhow::Result<()> {
            let current_file = diff_hunks::get_file_content(repo_root, file)?;
            if current_file.is_none() && repo_root.join(rel).exists() {
                // Existing non-text files are not deletions and cannot be archived.
                return Ok(());
            }
            let dir = root.join(rel.parent().unwrap_or_else(|| Path::new("")));
            let archive_name = name.to_string_lossy();
            let session_previous = latest_archive(&dir, &archive_name, Some(session_id));
            if let Some((path, mut previous)) = session_previous.as_ref().cloned()
                && previous.current_file == current_file
            {
                // The hook runs before the daemon. Fill missing usage for
                // that same turn without creating a duplicate code record.
                if previous.turn_id == turn_id
                    && previous.model.token_usage.is_none()
                    && token_usage.is_some()
                {
                    previous.model.token_usage = token_usage.map(ArchiveTokenUsage::from);
                    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
                    let update = (|| -> anyhow::Result<()> {
                        use std::io::Write;
                        let mut output = fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&temporary)?;
                        output.write_all(&serde_json::to_vec_pretty(&previous)?)?;
                        drop(output);
                        fs::rename(&temporary, &path)?;
                        Ok(())
                    })();
                    if update.is_err()
                        && temporary.exists()
                        && let Err(err) = fs::remove_file(&temporary)
                    {
                        sink.warn(
                            session_id,
                            &format!("cannot clean up {}: {err}", temporary.display()),
                        );
                    }
                    update?;
                }
                trace(&format!(
                    "skip {file}: unchanged since its newest v2 session step"
                ));
                return Ok(());
            }

            // Several agents can edit the same working tree. The newest copy from any
            // session is the true "before" for this change: diffing against this
            // session's own last step would credit it with edits another agent made
            // in between.
            let latest_any = latest_archive(&dir, &archive_name, None);
            if let Some((_, other)) = latest_any.as_ref()
                && other.current_file == current_file
            {
                trace(&format!(
                    "skip {file}: already archived by session {}",
                    other.session_id
                ));
                return Ok(());
            }

            let (previous_file, step) = if let Some((_, previous)) = session_previous {
                let base =
                    latest_any.map_or(previous.current_file, |(_, newest)| newest.current_file);
                (base, previous.step.saturating_add(1))
            } else {
                let baseline = if let Some((_, record)) = latest_any {
                    record.current_file
                } else if diff_hunks::head_file_exists(repo_root, file)? {
                    diff_hunks::get_head_content(repo_root, file)?
                } else {
                    None
                };
                if baseline == current_file {
                    trace(&format!("skip {file}: unchanged from session baseline"));
                    return Ok(());
                }
                let baseline_record = ArchiveMetadata {
                    version: 2,
                    session_id: session_id.to_owned(),
                    turn_id: turn_id.to_owned(),
                    step: 0,
                    timestamp: timestamp.to_owned(),
                    model: ModelContext {
                        name: if model.trim().is_empty() {
                            super::DEFAULT_MODEL_LABEL
                        } else {
                            model.trim()
                        }
                        .to_owned(),
                        token_usage: None,
                    },
                    snippets: Vec::new(),
                    current_file: baseline.clone(),
                };
                let path = write_archive(&dir, &archive_name, &baseline_record)?;
                trace(&format!(
                    "archived v2 {file} session baseline -> {}",
                    path.display()
                ));
                (baseline, 1)
            };

            let hunks = diff_hunks::generate_diff_hunks_between(
                repo_root,
                previous_file.as_deref(),
                current_file.as_deref(),
                3,
            )?;
            if hunks.is_empty() {
                return Ok(());
            }
            let change_type = match (&previous_file, &current_file) {
                (_, None) => ChangeType::Deleted,
                (None, Some(_)) => ChangeType::New,
                (Some(_), Some(_)) => ChangeType::Modified,
            };
            let snippets = hunks
                .into_iter()
                .map(|h| CodeSnippet {
                    file_path: if cfg!(windows) {
                        file.replace('\\', "/")
                    } else {
                        file.to_owned()
                    },
                    line_start: h.line_start,
                    line_end: h.line_end,
                    old_line_count: h.old_line_count,
                    new_line_count: h.new_line_count,
                    context_lines: 3,
                    diff_hunk: h.diff_hunk,
                    change_type: change_type.clone(),
                })
                .collect();
            let record = ArchiveMetadata {
                version: 2,
                session_id: session_id.to_owned(),
                turn_id: turn_id.to_owned(),
                step,
                timestamp: timestamp.to_owned(),
                model: ModelContext {
                    name: if model.trim().is_empty() {
                        super::DEFAULT_MODEL_LABEL
                    } else {
                        model.trim()
                    }
                    .to_owned(),
                    token_usage: token_usage.map(ArchiveTokenUsage::from),
                },
                snippets,
                current_file,
            };
            let path = write_archive(&dir, &archive_name, &record)?;
            trace(&format!(
                "archived v2 {file} session step {step} -> {}",
                path.display()
            ));
            Ok(())
        })();
        if let Err(err) = result {
            sink.warn(session_id, &format!("skip v2 archive for {file}: {err:#}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_round_trip_preserves_all_change_types_and_token_fields() {
        for change_type in ["modified", "new", "deleted"] {
            let value = serde_json::json!({
                "version": 2, "session_id": "session", "turn_id": "turn",
                "step": 2,
                "timestamp": "2026-09-27T10:30:00Z",
                "model": { "name": "model", "token_usage": {
                    "input_tokens": 1000, "cache_creation_tokens": 200,
                    "cache_read_tokens": 300, "output_tokens": 500, "api_call_count": 1
                }},
                "snippets": [{"file_path": "src/file.rs", "line_start": 42,
                    "line_end": 48, "old_line_count": 200, "new_line_count": 210,
                    "context_lines": 3, "diff_hunk": "@@ -42 +42 @@\n-old\n+new\n",
                    "change_type": change_type}],
                "current_file": "complete current file\n"
            });
            let record: ArchiveMetadata = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&record).unwrap(), value);
        }
    }
}
