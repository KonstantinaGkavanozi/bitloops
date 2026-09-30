use super::*;
use serde_json::Value;
use std::ffi::OsString;
use std::process::Command;

const V2_ENV: &str = "BITLOOPS_CODE_EXPORT_V2";
const TIMESTAMP: &str = "2026-09-27T10:30:00Z";

struct Fixture {
    _temp: tempfile::TempDir,
    repo: PathBuf,
    output: PathBuf,
    saved_env: Vec<(&'static str, Option<OsString>)>,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("project");
        let output = temp.path().join("archive");
        fs::create_dir_all(&repo).unwrap();
        let saved_env = [ENV_DIR, ENV_DISABLE, V2_ENV]
            .into_iter()
            .map(|key| (key, env::var_os(key)))
            .collect();
        unsafe {
            env::set_var(ENV_DIR, &output);
            env::remove_var(ENV_DISABLE);
            env::set_var(V2_ENV, "1");
        }
        let fixture = Self {
            _temp: temp,
            repo,
            output,
            saved_env,
        };
        fixture.git(&["init", "--quiet"]);
        fixture.git(&["config", "core.autocrlf", "false"]);
        fixture
    }

    fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(&self.repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn write(&self, path: &str, content: &str) {
        let path = self.repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn commit(&self) {
        self.git(&["add", "."]);
        self.git(&[
            "-c",
            "user.name=Archive Test",
            "-c",
            "user.email=archive@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "baseline",
        ]);
    }

    fn export(&self, changed: &[&str], deleted: &[&str]) {
        export_turn_code_v2(
            &self.repo,
            "test-model",
            "session-123",
            "turn-456",
            TIMESTAMP,
            None,
            &changed.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &deleted.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        );
    }

    fn records(&self, relative_dir: &str) -> Vec<Value> {
        let dir = self.output.join("v2/project").join(relative_dir);
        let mut records = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file())
            .map(|path| serde_json::from_slice(&fs::read(path).unwrap()).unwrap())
            .collect::<Vec<Value>>();
        records.sort_by_key(|record| record["step"].as_u64().unwrap());
        records
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for (key, value) in &self.saved_env {
            unsafe {
                match value {
                    Some(value) => env::set_var(key, value),
                    None => env::remove_var(key),
                }
            }
        }
    }
}

#[test]
fn modified_file_preserves_separate_hunks_and_total_line_counts() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    let original = (1..=30)
        .map(|line| format!("line {line}\n"))
        .collect::<String>();
    f.write("src/main.rs", &original);
    f.commit();
    f.write(
        "src/main.rs",
        &original
            .replace("line 4\n", "changed four\n")
            .replace("line 25\n", "changed twenty-five\n"),
    );
    f.export(&["src/main.rs"], &[]);
    let records = f.records("src");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["step"], 0);
    assert_eq!(records[0]["snippets"], serde_json::json!([]));
    assert_eq!(records[0]["current_file"], original);
    assert_eq!(records[1]["step"], 1);
    let snippets = records[1]["snippets"].as_array().unwrap();
    assert_eq!(snippets.len(), 2);
    for snippet in snippets {
        assert_eq!(snippet["file_path"], "src/main.rs");
        assert_eq!(snippet["change_type"], "modified");
        assert_eq!(snippet["old_line_count"], 30);
        assert_eq!(snippet["new_line_count"], 30);
        assert_eq!(snippet["context_lines"], 3);
        assert!(snippet["line_start"].as_u64().unwrap() > 0);
        assert!(snippet["line_end"].as_u64().unwrap() >= snippet["line_start"].as_u64().unwrap());
        assert!(snippet["diff_hunk"].as_str().unwrap().contains("@@"));
    }
}

#[test]
fn new_files_export_once_each_with_metadata_and_tokens() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    f.write("a.rs", "first\nsecond\n");
    f.write("b.rs", "third\n");
    let tokens = crate::adapters::agents::TokenUsage {
        input_tokens: 100,
        cache_creation_tokens: 20,
        cache_read_tokens: 30,
        output_tokens: 40,
        api_call_count: 2,
        ..Default::default()
    };
    export_turn_code_v2(
        &f.repo,
        " test-model ",
        "session-123",
        "turn-456",
        TIMESTAMP,
        Some(&tokens),
        &["a.rs".into(), "b.rs".into(), "a.rs".into()],
        &[],
    );
    let records = f.records("");
    assert_eq!(records.len(), 4);
    assert_eq!(
        records.iter().filter(|record| record["step"] == 0).count(),
        2
    );
    for record in records.into_iter().filter(|record| record["step"] == 1) {
        assert_eq!(record["version"], 2);
        assert_eq!(record["session_id"], "session-123");
        assert_eq!(record["turn_id"], "turn-456");
        assert_eq!(record["timestamp"], TIMESTAMP);
        assert_eq!(record["model"]["name"], "test-model");
        assert_eq!(
            record["model"]["token_usage"],
            serde_json::json!({
                "input_tokens": 100, "cache_creation_tokens": 20, "cache_read_tokens": 30,
                "output_tokens": 40, "api_call_count": 2
            })
        );
        assert!(record.get("code").is_none());
        assert!(record["current_file"].as_str().is_some());
        let snippet = &record["snippets"][0];
        assert_eq!(snippet["change_type"], "new");
        assert_eq!(snippet["old_line_count"], 0);
        let expected = if snippet["file_path"] == "a.rs" { 2 } else { 1 };
        assert_eq!(snippet["new_line_count"], expected);
        assert!(snippet["diff_hunk"].as_str().unwrap().contains("+"));
    }
}

#[test]
fn deletion_preserves_content_and_deduplicates() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    f.write("gone.rs", "first\nsecond\n");
    f.commit();
    fs::remove_file(f.repo.join("gone.rs")).unwrap();
    f.export(&[], &["gone.rs"]);
    f.export(&[], &["gone.rs"]);
    let records = f.records("");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["current_file"], "first\nsecond\n");
    let snippet = &records[1]["snippets"][0];
    assert_eq!(snippet["change_type"], "deleted");
    assert_eq!(snippet["old_line_count"], 2);
    assert_eq!(snippet["new_line_count"], 0);
    assert_eq!(snippet["line_start"], 1);
    assert_eq!(snippet["line_end"], 2);
    let diff = snippet["diff_hunk"].as_str().unwrap();
    assert!(diff.contains("-first"));
    assert!(diff.contains("-second"));
    assert!(records[1]["current_file"].is_null());
}

#[test]
fn identical_hunks_deduplicate_but_changed_hunks_export_again() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    f.write("a.rs", "baseline\n");
    f.commit();
    f.write("a.rs", "first change\n");
    f.export(&["a.rs"], &[]);
    f.export(&["a.rs"], &[]);
    assert_eq!(f.records("").len(), 2);
    f.write("a.rs", "second change\n");
    f.export(&["a.rs"], &[]);
    let records = f.records("");
    assert_eq!(records.len(), 3);
    assert_eq!(records[2]["step"], 2);
    let diff = records[2]["snippets"][0]["diff_hunk"].as_str().unwrap();
    assert!(diff.contains("-first change"));
    assert!(diff.contains("+second change"));
    assert!(!diff.contains("-baseline"));
    assert_eq!(records[2]["current_file"], "second change\n");
}

#[test]
fn a_new_session_restarts_steps_from_the_latest_archived_file() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    f.write("a.rs", "head baseline\n");
    f.commit();
    f.write("a.rs", "first session\n");
    f.export(&["a.rs"], &[]);

    f.write("a.rs", "second session\n");
    export_turn_code_v2(
        &f.repo,
        "test-model",
        "session-456",
        "turn-789",
        TIMESTAMP,
        None,
        &["a.rs".into()],
        &[],
    );

    let records = f.records("");
    let second_session = records
        .iter()
        .filter(|record| record["session_id"] == "session-456")
        .collect::<Vec<_>>();
    assert_eq!(second_session.len(), 2);
    assert_eq!(second_session[0]["step"], 0);
    assert_eq!(second_session[0]["current_file"], "first session\n");
    assert_eq!(second_session[1]["step"], 1);
    let diff = second_session[1]["snippets"][0]["diff_hunk"]
        .as_str()
        .unwrap();
    assert!(diff.contains("-first session"));
    assert!(diff.contains("+second session"));
    assert!(!diff.contains("-head baseline"));
}

#[test]
fn opt_in_and_disable_are_respected() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    f.write("a.rs", "new\n");
    unsafe {
        env::remove_var(V2_ENV);
    }
    f.export(&["a.rs"], &[]);
    unsafe {
        env::set_var(V2_ENV, "true");
    }
    f.export(&["a.rs"], &[]);
    unsafe {
        env::set_var(V2_ENV, "1");
        env::set_var(ENV_DISABLE, "0");
    }
    f.export(&["a.rs"], &[]);
    assert!(!f.output.exists());
    unsafe {
        env::set_var(ENV_DISABLE, "");
    }
    f.export(&["a.rs"], &[]);
    assert_eq!(f.records("").len(), 2);
}

#[test]
fn empty_changes_unchanged_and_binary_files_create_no_archive() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    f.write("same.rs", "unchanged\n");
    f.commit();
    fs::write(f.repo.join("binary.bin"), [0, 255, 0, 1]).unwrap();
    f.export(&[], &[]);
    f.export(&["same.rs", "binary.bin", "missing.rs", ""], &[]);
    assert!(!f.output.exists());
}

#[test]
fn markdown_files_are_not_archived_for_any_change_type() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    f.write("docs/modified.md", "before\n");
    f.write("docs/deleted.MD", "removed\n");
    f.commit();
    f.write("docs/modified.md", "after\n");
    f.write("docs/new.md", "new\n");
    fs::remove_file(f.repo.join("docs/deleted.MD")).unwrap();

    f.export(&["docs/modified.md", "docs/new.md"], &["docs/deleted.MD"]);

    assert!(!f.output.exists());
}

#[test]
fn dot_prefixed_files_and_directories_are_not_archived_for_any_change_type() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    f.write(".matrixx/modified.json", "before\n");
    f.write("nested/.gemini/deleted.json", "removed\n");
    f.commit();
    f.write(".matrixx/modified.json", "after\n");
    f.write(".codex/new.rs", "fn hidden() {}\n");
    fs::remove_file(f.repo.join("nested/.gemini/deleted.json")).unwrap();

    f.export(
        &[".matrixx/modified.json", ".codex/new.rs"],
        &["nested/.gemini/deleted.json"],
    );

    assert!(!f.output.exists());
}

#[test]
fn io_failure_does_not_prevent_later_files_from_exporting() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    f.write("blocked/a.rs", "blocked\n");
    f.write("good.rs", "good\n");
    fs::create_dir_all(f.output.join("v2/project")).unwrap();
    fs::write(f.output.join("v2/project/blocked"), "not a directory").unwrap();
    f.export(&["blocked/a.rs", "good.rs"], &[]);
    let records: Vec<_> = fs::read_dir(f.output.join("v2/project"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    assert_eq!(records.len(), 2);
    let record = records
        .iter()
        .map(|path| serde_json::from_slice::<Value>(&fs::read(path).unwrap()).unwrap())
        .find(|record| record["step"] == 1)
        .unwrap();
    assert_eq!(record["snippets"][0]["file_path"], "good.rs");
}

#[test]
fn v1_and_v2_archives_coexist_without_changing_v1_schema() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    f.write("a.rs", "hello\n");
    export_turn_code(&f.repo, "test-model", &["a.rs".into()]);
    f.export(&["a.rs"], &[]);
    let files: Vec<_> = fs::read_dir(f.output.join("project")).unwrap().collect();
    assert_eq!(files.len(), 1);
    let path = files.into_iter().next().unwrap().unwrap().path();
    let record: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        record,
        serde_json::json!({"model": "test-model", "code": "hello\n"})
    );
    assert_eq!(f.records("").len(), 2);
}

#[test]
fn hook_archives_deletion_only_turn_with_session_and_turn_metadata() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    f.write("gone.rs", "removed\n");
    f.commit();
    fs::remove_file(f.repo.join("gone.rs")).unwrap();
    export_turn_code_from_hook(&f.repo, "test-model", "", "hook-session", Some("hook-turn"));
    let records = f.records("");
    assert_eq!(records.len(), 2);
    let record = &records[1];
    assert_eq!(record["session_id"], "hook-session");
    assert_eq!(record["turn_id"], "hook-turn");
    assert_eq!(record["model"]["name"], "test-model");
    assert!(chrono::DateTime::parse_from_rfc3339(record["timestamp"].as_str().unwrap()).is_ok());
    assert_eq!(record["snippets"][0]["change_type"], "deleted");
    assert!(
        record["snippets"][0]["diff_hunk"]
            .as_str()
            .unwrap()
            .contains("-removed")
    );
}

#[test]
fn daemon_enriches_same_turn_hook_archive_without_duplicating_hunks() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    f.write("a.rs", "before\n");
    f.commit();
    f.write("a.rs", "after\n");
    export_turn_code_from_hook(&f.repo, "test-model", "", "session-123", Some("turn-456"));
    let initial = f.records("");
    assert_eq!(initial.len(), 2);
    assert!(initial[1]["model"]["token_usage"].is_null());
    let tokens = crate::adapters::agents::TokenUsage {
        input_tokens: 101,
        cache_creation_tokens: 21,
        cache_read_tokens: 31,
        output_tokens: 41,
        api_call_count: 3,
        ..Default::default()
    };
    for _ in 0..2 {
        export_turn_code_v2(
            &f.repo,
            "test-model",
            "session-123",
            "turn-456",
            TIMESTAMP,
            Some(&tokens),
            &["a.rs".into()],
            &[],
        );
    }
    let records = f.records("");
    assert_eq!(records.len(), 2);
    assert_eq!(records[1]["snippets"], initial[1]["snippets"]);
    assert_eq!(records[1]["session_id"], "session-123");
    assert_eq!(records[1]["turn_id"], "turn-456");
    assert_eq!(
        records[1]["model"]["token_usage"],
        serde_json::json!({
            "input_tokens": 101, "cache_creation_tokens": 21, "cache_read_tokens": 31,
            "output_tokens": 41, "api_call_count": 3
        })
    );
    // A later hook retry without usage must not erase daemon enrichment.
    export_turn_code_from_hook(&f.repo, "test-model", "", "session-123", Some("turn-456"));
    assert_eq!(f.records(""), records);
    let later_tokens = crate::adapters::agents::TokenUsage {
        input_tokens: 999,
        output_tokens: 888,
        ..Default::default()
    };
    for (session, turn) in [("session-123", "later-turn"), ("other-session", "turn-456")] {
        export_turn_code_v2(
            &f.repo,
            "another-model",
            session,
            turn,
            TIMESTAMP,
            Some(&later_tokens),
            &["a.rs".into()],
            &[],
        );
        assert_eq!(f.records(""), records);
    }
}

#[test]
fn hook_generates_turn_id_when_missing() {
    let _guard = super::tests::env_lock().lock().unwrap();
    let f = Fixture::new();
    f.write("new.rs", "new file\n");
    export_turn_code_from_hook(&f.repo, "test-model", "", "hook-session", None);
    let records = f.records("");
    assert_eq!(records.len(), 2);
    assert_eq!(records[1]["session_id"], "hook-session");
    let turn_id = records[1]["turn_id"].as_str().unwrap();
    assert_eq!(turn_id.len(), 12);
    assert!(!turn_id.trim().is_empty());
    assert_eq!(records[1]["snippets"][0]["change_type"], "new");
}
