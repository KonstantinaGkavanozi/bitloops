---
title: Code Archiver
description: Local code snapshots and optional version 2 diff snippets.
---

# Code Archiver

Turn-end hooks and lifecycle processing export code independently of the
interaction store. Exports are best-effort: a failed file does not fail the turn
or prevent other files from being archived.

The default v1 export remains one `{ "model": "...", "code": "..." }` JSON
snapshot per changed text file. The release installers set
`BITLOOPS_CODE_EXPORT_V2=1` so new installations additionally enable v2. For
manual or source installations, set it in the hook and daemon environment.
Existing v1 archives are neither modified nor migrated.

- `BITLOOPS_CODE_EXPORT_DIR` overrides the root (default: `~/Desktop/cycloops-code`).
- V1 writes under `<root>/<project>/<relative-parent>/`.
- V2 writes under `<root>/v2/<project>/<relative-parent>/`.
- Any nonempty `BITLOOPS_CODE_EXPORT_DISABLE` disables both versions.
- `BITLOOPS_CODE_EXPORT_TRACE` enables additional stderr diagnostics; v2 failures
  also use the owning daemon or hook logger.

V2 records contain `version`, `session_id`, `turn_id`, `step`, an RFC 3339
`timestamp`, `model`, `snippets`, and `current_file`. Model context includes the
resolved `name` and nullable `token_usage`. When available, usage includes `input_tokens`,
`cache_creation_tokens`, `cache_read_tokens`, `output_tokens`, and `api_call_count`.
Provider and model version are not currently available and are omitted.

Each file has its own per-session sequence. Step 0 stores the original complete
file in `current_file` and has no snippets. Step 1 compares the first captured
modification with step 0; every later step compares with the preceding step in
that session. Each changed step contains one snippet per unified diff hunk with
three context lines. `current_file` stores the complete file after that step and
is `null` when the file is absent. `old_line_count` and `new_line_count` count
whole-file lines; `line_start` and `line_end` describe the hunk in the new file,
or in the old file when the hunk contains only removals. New files store
additions, deleted files store the preceding step as removals, and empty file
creation/deletion uses a zero-line marker. Binary files are skipped.

An unchanged full-file state is deduplicated against the newest step for that
file and session. If a new session first observes the same state as the newest
archive, it does not create an unnecessary baseline. When a hook record has no
usage and the daemon later provides usage for the same session and turn, it
enriches that record without creating a duplicate. Hooks use the existing
lifecycle turn ID when available; standalone archiver hooks generate a fallback
ID and leave token usage null.

Run focused verification with:

```powershell
cargo nextest run --manifest-path bitloops/Cargo.toml --no-default-features --lib -E 'test(code_export) | test(diff_hunks)'
```

The repository test aliases select whole lanes, so this direct nextest command
limits execution to the archiver tests.
