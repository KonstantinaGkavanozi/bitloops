//! Unified diff hunks between file versions, including HEAD-backed changes.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiffHunk {
    pub line_start: usize,
    pub line_end: usize,
    /// Total lines in the HEAD version (not just the hunk).
    pub old_line_count: usize,
    /// Total lines in the current version (not just the hunk).
    pub new_line_count: usize,
    pub diff_hunk: String,
}

fn git(repo_root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(repo_root)
        .env("GIT_LITERAL_PATHSPECS", "1");
    command
}

fn validate_path(file_path: &str) -> io::Result<()> {
    if file_path.is_empty()
        || Path::new(file_path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected repository-relative file path",
        ));
    }
    Ok(())
}

fn git_path(file_path: &str) -> String {
    if cfg!(windows) {
        file_path.replace('\\', "/")
    } else {
        file_path.to_owned()
    }
}

pub(super) fn head_file_exists(repo_root: &Path, file_path: &str) -> io::Result<bool> {
    validate_path(file_path)?;
    let repository = git(repo_root).args(["rev-parse", "--git-dir"]).output()?;
    if !repository.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&repository.stderr).into_owned(),
        ));
    }
    let head = git(repo_root)
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .output()?;
    if head.status.code() == Some(1) {
        return Ok(false);
    }
    if !head.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&head.stderr).into_owned(),
        ));
    }
    let output = git(repo_root)
        .args(["ls-tree", "-z", "HEAD", "--", &git_path(file_path)])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(!output.stdout.is_empty())
}

pub(super) fn get_file_content(repo_root: &Path, file_path: &str) -> io::Result<Option<String>> {
    validate_path(file_path)?;
    match std::fs::read_to_string(repo_root.join(file_path)) {
        Ok(content) => Ok((!content.contains('\0')).then_some(content)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::InvalidData
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn get_head_content(repo_root: &Path, file_path: &str) -> io::Result<Option<String>> {
    validate_path(file_path)?;
    let output = git(repo_root)
        .args(["show", &format!("HEAD:{}", git_path(file_path))])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(String::from_utf8(output.stdout)
        .ok()
        .filter(|content| !content.contains('\0')))
}

pub(super) fn generate_diff_hunks(
    repo_root: &Path,
    file_path: &str,
    context_lines: usize,
) -> io::Result<Vec<DiffHunk>> {
    validate_path(file_path)?;
    let old_content = if head_file_exists(repo_root, file_path)? {
        get_head_content(repo_root, file_path)?
    } else {
        None
    };
    let new_content = get_file_content(repo_root, file_path)?;
    if new_content.is_none() && repo_root.join(file_path).exists() {
        return Ok(Vec::new());
    }
    generate_diff_hunks_between(
        repo_root,
        old_content.as_deref(),
        new_content.as_deref(),
        context_lines,
    )
}

struct TemporaryDiffDirectory(PathBuf);

impl Drop for TemporaryDiffDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn generate_diff_hunks_between(
    repo_root: &Path,
    old_content: Option<&str>,
    new_content: Option<&str>,
    context_lines: usize,
) -> io::Result<Vec<DiffHunk>> {
    if old_content == new_content {
        return Ok(Vec::new());
    }
    if old_content.unwrap_or_default().is_empty() && new_content.unwrap_or_default().is_empty() {
        return Ok(vec![addition_hunk("")]);
    }

    let temporary = TemporaryDiffDirectory(
        std::env::temp_dir().join(format!("bitloops-code-diff-{}", uuid::Uuid::new_v4())),
    );
    fs::create_dir(&temporary.0)?;
    let old_path = temporary.0.join("previous");
    let new_path = temporary.0.join("current");
    fs::write(&old_path, old_content.unwrap_or_default())?;
    fs::write(&new_path, new_content.unwrap_or_default())?;

    let output = git(repo_root)
        .args([
            "-c",
            "core.quotePath=false",
            "diff",
            "--no-index",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-renames",
            &format!("--unified={context_lines}"),
            "--",
            &old_path.to_string_lossy(),
            &new_path.to_string_lossy(),
        ])
        .output()?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    let Ok(diff) = String::from_utf8(output.stdout) else {
        return Ok(Vec::new());
    };
    if diff.contains('\0') {
        return Ok(Vec::new());
    }
    let mut hunks = parse_hunks(&diff)?;
    for hunk in &mut hunks {
        hunk.old_line_count = old_content
            .unwrap_or_default()
            .split_inclusive('\n')
            .count();
        hunk.new_line_count = new_content
            .unwrap_or_default()
            .split_inclusive('\n')
            .count();
    }
    Ok(hunks)
}

fn addition_hunk(content: &str) -> DiffHunk {
    let count = content.split_inclusive('\n').count();
    let start = usize::from(count > 0);
    let mut diff_hunk = format!("@@ -0,0 +{start},{count} @@\n");
    for line in content.split_inclusive('\n') {
        diff_hunk.push('+');
        diff_hunk.push_str(line);
    }
    if !content.is_empty() && !content.ends_with('\n') {
        diff_hunk.push_str("\n\\ No newline at end of file\n");
    }
    DiffHunk {
        line_start: start,
        line_end: count,
        old_line_count: 0,
        new_line_count: count,
        diff_hunk,
    }
}

fn parse_range(value: &str) -> Option<(usize, usize)> {
    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
    Some((start.parse().ok()?, count.parse().ok()?))
}

fn parse_hunks(diff: &str) -> io::Result<Vec<DiffHunk>> {
    let mut hunks: Vec<DiffHunk> = Vec::new();
    for line in diff.split_inclusive('\n') {
        if let Some(header) = line.strip_prefix("@@ -") {
            let parsed = (|| {
                let (old, rest) = header.split_once(" +")?;
                let (new, _) = rest.split_once(" @@")?;
                Some((parse_range(old)?, parse_range(new)?))
            })();
            let Some(((old_start, old_count), (new_start, new_count))) = parsed else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid unified diff header",
                ));
            };
            let (start, count) = if new_count == 0 {
                (old_start, old_count)
            } else {
                (new_start, new_count)
            };
            hunks.push(DiffHunk {
                line_start: start,
                line_end: start.saturating_add(count.saturating_sub(1)),
                old_line_count: old_count,
                new_line_count: new_count,
                diff_hunk: line.to_owned(),
            });
        } else if let Some(hunk) = hunks.last_mut() {
            hunk.diff_hunk.push_str(line);
        }
    }
    Ok(hunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), &["init"]);
        run(dir.path(), &["config", "core.autocrlf", "false"]);
        run(dir.path(), &["config", "commit.gpgsign", "false"]);
        dir
    }

    fn run(root: &Path, args: &[&str]) {
        let output = git(root).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn commit(root: &Path, content: &str) {
        std::fs::write(root.join("file.rs"), content).unwrap();
        run(root, &["add", "file.rs"]);
        run(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "initial",
            ],
        );
    }

    #[test]
    fn modified_and_staged_changes_compare_against_head() {
        let repo = repo();
        let original = (1..=10).map(|i| format!("line {i}\n")).collect::<String>();
        commit(repo.path(), &original);
        let modified = original.replace("line 5\n", "replacement\n");
        std::fs::write(repo.path().join("file.rs"), &modified).unwrap();
        let hunks = generate_diff_hunks(repo.path(), "file.rs", 3).unwrap();
        assert_eq!((hunks[0].line_start, hunks[0].line_end), (2, 8));
        assert!(hunks[0].diff_hunk.contains("-line 5\n+replacement\n"));
        run(repo.path(), &["add", "file.rs"]);
        assert_eq!(
            generate_diff_hunks(repo.path(), "file.rs", 3).unwrap(),
            hunks
        );
        std::fs::write(
            repo.path().join("file.rs"),
            modified.replace("replacement", "later"),
        )
        .unwrap();
        assert!(
            generate_diff_hunks(repo.path(), "file.rs", 3).unwrap()[0]
                .diff_hunk
                .contains("+later\n")
        );
    }

    #[test]
    fn untracked_and_unborn_files_preserve_newlines() {
        let repo = repo();
        std::fs::write(repo.path().join("new.rs"), "first\r\nlast").unwrap();
        let hunks = generate_diff_hunks(repo.path(), "new.rs", 3).unwrap();
        assert_eq!(
            hunks[0].diff_hunk,
            "@@ -0,0 +1,2 @@\n+first\r\n+last\n\\ No newline at end of file\n"
        );
        commit(repo.path(), "base\n");
        assert_eq!(
            generate_diff_hunks(repo.path(), "new.rs", 3).unwrap(),
            hunks
        );
        run(repo.path(), &["add", "new.rs"]);
        assert_eq!(
            generate_diff_hunks(repo.path(), "new.rs", 3).unwrap(),
            hunks
        );
    }

    #[test]
    fn deletion_preserves_head_content() {
        let repo = repo();
        commit(repo.path(), "first\nlast");
        std::fs::remove_file(repo.path().join("file.rs")).unwrap();
        let hunks = generate_diff_hunks(repo.path(), "file.rs", 3).unwrap();
        assert_eq!((hunks[0].line_start, hunks[0].line_end), (1, 2));
        assert_eq!((hunks[0].old_line_count, hunks[0].new_line_count), (2, 0));
        assert!(
            hunks[0]
                .diff_hunk
                .contains("-last\n\\ No newline at end of file\n")
        );
        assert_eq!(
            get_head_content(repo.path(), "file.rs").unwrap().as_deref(),
            Some("first\nlast")
        );
    }

    #[test]
    fn tracked_hunks_preserve_crlf_and_missing_final_newline() {
        let repo = repo();
        commit(repo.path(), "first\r\nold");
        std::fs::write(repo.path().join("file.rs"), "first\r\nnew").unwrap();
        let hunks = generate_diff_hunks(repo.path(), "file.rs", 3).unwrap();
        assert!(hunks[0].diff_hunk.contains(
            " first\r\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n"
        ));
    }

    #[test]
    fn unchanged_file_has_no_hunks() {
        let repo = repo();
        commit(repo.path(), "unchanged\n");
        assert!(
            generate_diff_hunks(repo.path(), "file.rs", 3)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn multiple_hunks_and_context_are_preserved() {
        let repo = repo();
        let original = (1..=30).map(|i| format!("line {i}\n")).collect::<String>();
        commit(repo.path(), &original);
        std::fs::write(
            repo.path().join("file.rs"),
            original
                .replace("line 5\n", "five\n")
                .replace("line 25\n", "twenty five\n"),
        )
        .unwrap();
        let hunks = generate_diff_hunks(repo.path(), "file.rs", 1).unwrap();
        assert_eq!(hunks.len(), 2);
        assert_eq!((hunks[0].line_start, hunks[0].line_end), (4, 6));
        assert_eq!((hunks[1].line_start, hunks[1].line_end), (24, 26));
        assert_eq!(
            generate_diff_hunks(repo.path(), "file.rs", 3).unwrap()[0].new_line_count,
            30
        );
    }

    #[test]
    fn binary_files_are_skipped() {
        let repo = repo();
        std::fs::write(repo.path().join("new.rs"), b"binary\0data").unwrap();
        assert!(
            generate_diff_hunks(repo.path(), "new.rs", 3)
                .unwrap()
                .is_empty()
        );
        commit(repo.path(), "old\0data");
        std::fs::write(repo.path().join("file.rs"), b"new\0data").unwrap();
        assert!(
            generate_diff_hunks(repo.path(), "file.rs", 3)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn empty_files_have_creation_and_deletion_markers() {
        let repo = repo();
        std::fs::write(repo.path().join("file.rs"), "").unwrap();
        let hunks = generate_diff_hunks(repo.path(), "file.rs", 3).unwrap();
        assert_eq!(hunks[0].diff_hunk, "@@ -0,0 +0,0 @@\n");
        commit(repo.path(), "");
        std::fs::remove_file(repo.path().join("file.rs")).unwrap();
        assert_eq!(
            generate_diff_hunks(repo.path(), "file.rs", 3).unwrap(),
            hunks
        );
    }

    #[test]
    fn non_repository_and_unsafe_paths_return_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(generate_diff_hunks(dir.path(), "file.rs", 3).is_err());
        assert!(generate_diff_hunks(dir.path(), "../file.rs", 3).is_err());
        assert!(parse_hunks("@@ -invalid +1 @@\n").is_err());
        assert!(get_head_content(dir.path(), "file.rs").is_err());
        std::fs::create_dir(dir.path().join("directory.rs")).unwrap();
        assert!(get_file_content(dir.path(), "directory.rs").is_err());
    }

    #[test]
    fn special_paths_are_literal_and_windows_separators_work() {
        let repo = repo();
        commit(repo.path(), "initial\n");
        std::fs::create_dir(repo.path().join("nested")).unwrap();
        let file = "nested/[literal] file.rs";
        std::fs::write(repo.path().join(file), "before\n").unwrap();
        run(repo.path(), &["add", file]);
        run(
            repo.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "special path",
            ],
        );
        std::fs::write(repo.path().join(file), "after\n").unwrap();
        let path = if cfg!(windows) {
            file.replace('/', "\\")
        } else {
            file.to_owned()
        };
        assert!(head_file_exists(repo.path(), &path).unwrap());
        let hunks = generate_diff_hunks(repo.path(), &path, 3).unwrap();
        assert_eq!(hunks.len(), 1);
        assert!(hunks[0].diff_hunk.contains("-before\n+after\n"));
    }
}
