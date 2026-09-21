# Code Archiver

The code archiver is a small feature inside this research build of Bitloops. Every time an AI agent finishes a turn in a repo it is tracking, it saves a copy of each file the agent created or modified, together with the name of the model that wrote it.

It is built into the CLI (`bitloops/src/host/checkpoints/lifecycle/code_export.rs`). There is no separate plugin or Python hook to install.

## The binary is called `cycloops`

This build ships as `cycloops`, not `bitloops`, so it can be installed alongside an official Bitloops without either one overwriting the other's agent hooks. Everywhere the upstream documentation says `bitloops <command>`, use `cycloops <command>`.

The **environment variables keep their `BITLOOPS_` prefix** — they are read by unchanged upstream code. So the command is `cycloops` but the settings are `BITLOOPS_CODE_EXPORT_DIR` and friends. This trips people up; it is not a typo.

## Install

One command. No Rust, no build tools, no replacing an existing binary.

**macOS / Linux**

```bash
curl -fsSL https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.sh | bash
```

**Windows (PowerShell)**

```powershell
irm https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.ps1 | iex
```

**Windows (CMD)**

```cmd
curl -fsSL https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.cmd -o install.cmd && install.cmd && del install.cmd
```

The installer downloads the latest release for your platform, checks it against the published SHA-256, and puts `cycloops` on your PATH. Telemetry is off in the build itself, so the installer sets nothing for it. On Windows it also installs `duckdb.dll` next to the binary; keep the two together.

Then, in a **new** terminal:

```bash
cd path/to/your/repo
cycloops init                # tick the agents you use
```

There is no daemon to start: the installer turns on archiver-only mode.

## Archiver-only mode

With `CYCLOOPS_ARCHIVER_ONLY` set, the hook saves the turn's code and stops.
Nothing is queued, so the daemon, DuckDB, checkpoints, sync and ingest never
run, and `init` asks only which agents to hook — no embedding prompts, no
final checklist.

This is the mode to hand to participants. It removes every failure listed
under Troubleshooting except "the binary is not on PATH": no database locks,
no daemon that must be restarted after an upgrade, no 45-second readiness
timeout.

Unset the variable to get the full Bitloops CLI back, in which case the rest
of this document's daemon guidance applies and you start it with
`cycloops daemon start`. The choices `init` offers in that mode are listed
below.

To install a specific version instead of the latest:

```bash
CYCLOOPS_VERSION=v0.0.2 curl -fsSL .../install.sh | bash
```
```powershell
.\install.ps1 -Version v0.0.2
```

Run `cycloops --version` and note what it prints. For a study, record that string alongside the archived data — it identifies the exact build that produced it.

## What it produces

One JSON file per changed file, per turn:

```
<export root>/<repo folder name>/<path of the file inside the repo>/<timestamp>__<filename>.json
```

For example, if the agent edits `src/app.rs` in a repo called `my-project`:

```
~/Desktop/cycloops-code/my-project/src/1789900000-123456__app.rs.json
```

Each file contains:

```json
{
  "model": "claude-sonnet-5",
  "code": "...the full final contents of the file after the turn..."
}
```

Behaviour worth knowing:

- Only files that were **created or modified** are saved. Deleted files are not.
- The **whole file** is saved, not a diff.
- A file is saved again only when its content **differs from its newest archived copy**. Bitloops reports every file that is still uncommitted at the end of each turn, so without this check the same unchanged files would be re-saved every turn. If a file changes and later goes back to an older version, that counts as a change and is saved again.
- Each save gets its own timestamp, so older versions of a file are kept next to the newer ones.
- Files that can't be read as text (binaries, files removed again) are silently skipped.
- Archiving never blocks or fails the agent's turn. If a write fails, that file is skipped and nothing is reported.
- If the model name cannot be worked out, `"model"` is `"unknown"`.
- There is **no ignore list**. A changed `.env` or key file is archived like any other file, in plain text.

That last point matters if anyone outside the team runs this on their own repositories. Say so in your consent material, or add a denylist before handing the tool out.

## Settings

| Variable | Effect |
|---|---|
| `BITLOOPS_CODE_EXPORT_DIR` | Where to write the archive. Default: `~/Desktop/cycloops-code` |
| `BITLOOPS_CODE_EXPORT_DISABLE` | Any non-empty value turns archiving off. It is on by default. |
| `BITLOOPS_CODE_EXPORT_TRACE` | Any non-empty value makes the archiver print to stderr which files it saw and why each was saved or skipped. For troubleshooting. |
| `CYCLOOPS_ARCHIVER_ONLY` | Set by the installer. Archive turns and nothing else: no daemon, no database, no sync or ingest, and `init` stops asking about them. Unset it for the full Bitloops pipeline. |
| `BITLOOPS_TELEMETRY_OPTIN` | Telemetry is off in this build and needs no switch. Setting this to a non-empty value turns reporting on; note the compiled-in PostHog key belongs to upstream Bitloops, so the data would land in their project. `BITLOOPS_TELEMETRY_OPTOUT` still works and overrides it. |

**Where to set them.** At the end of each agent turn, the `cycloops hooks ...` command that your agent launches archives the changed files itself. It needs neither the daemon nor the database. It also queues the turn for the daemon, which archives too but skips anything the hook already saved with the same content. So the variables must be set in the environment of the **agent** — the terminal or app you start Claude Code, Cursor, etc. from — not just any shell. If you run the daemon, restart it after changing the variables, because a running daemon keeps using the old ones.

The installer writes `BITLOOPS_CODE_EXPORT_DIR` to your shell profile on macOS/Linux, or to your user environment on Windows, if you passed one. To set the export directory afterwards:

- **Windows (PowerShell), permanent:**
  ```powershell
  [Environment]::SetEnvironmentVariable("BITLOOPS_CODE_EXPORT_DIR", "D:\bitloops-archive", "User")
  ```
  Then restart the agent or editor. Programs that were already open won't see the change.
- **macOS / Linux (zsh or bash):** add to `~/.zshrc` or `~/.bashrc`, then open a new terminal:
  ```bash
  export BITLOOPS_CODE_EXPORT_DIR="$HOME/bitloops-archive"
  ```
- **macOS, agent launched from the Dock or Spotlight (not a terminal):** shell files aren't read. Use `launchctl setenv BITLOOPS_CODE_EXPORT_DIR "$HOME/bitloops-archive"` and restart the app.
- On Windows, if your Desktop is redirected (for example by OneDrive), the default `%USERPROFILE%\Desktop\cycloops-code` may not be your visible Desktop. Set `BITLOOPS_CODE_EXPORT_DIR` explicitly.

## Choices you will be asked to make (full mode only)

In archiver-only mode `init` asks only which agents to hook, and the table below does not apply.

`cycloops init` is interactive, so run it in a real terminal. You can accept the defaults by pressing Enter. For the code archiver only the first question really matters.

| Prompt | What it means | For the archiver |
|---|---|---|
| **Select agents to integrate** (space to tick, Enter to confirm) | Which AI agents get hooks installed in this repo (Claude Code, Cursor, Codex, Gemini, Opencode, Copilot, as offered on your machine). | **Tick every agent you use in this repo.** Archiving runs from that agent's hooks, so an agent you don't tick is never archived. |
| **Enable DevQL Guidance** (a checkbox under the agent list) | Lets Bitloops feed codebase context back to the agent. | Not needed. Leave it as you like. |
| **Configure embeddings**: Bitloops Cloud / Local embeddings / Skip for now | Powers semantic code search. Cloud opens a browser sign-in. Local needs about 4 GB RAM and a GPU is recommended. | Choose **Skip for now**. |
| **Configure summary embeddings**: Enable / Skip for now | Semantic search over generated summaries. | Choose **Skip for now**. |
| **Final setup checklist**: Sync codebase, Import commit history, Enable anonymous telemetry, Start daemon automatically when you sign in | Optional extras. Press Enter for the defaults, type option numbers, or `all` / `none`. | Sync and Import history are not needed for archiving and can take a while on a large repo. Auto-start is a convenience only; on Windows it has not been checked. |

The exact list can differ a little depending on what is already configured. Skipping embeddings has no effect on archiving, and you can run `cycloops init` again later to change these answers.

## Checking that it works

1. Run `cycloops --version` and confirm it is the build you expect.
2. In a repo where you ran `cycloops init`, ask your agent to create or change a file.
3. When the turn ends, look in the export folder (default `~/Desktop/cycloops-code/<repo name>/`). There should be a `.json` file for each changed file.

The core logic — writing `{model, code}` files, the disable switch, the `unknown` model fallback, skipping unchanged files — has unit tests in `code_export.rs`. The hook-side archiving has been exercised by hand on Windows against a temporary folder: the first run saved every uncommitted file and a second run saved none. A full run with a real agent turn has not been confirmed yet.

## Troubleshooting

- **No files appear.**
  - Run `cycloops --version` from the same terminal your agent uses. If the command isn't found there, the PATH change hasn't reached it (`where cycloops` on Windows, `which -a cycloops` on macOS/Linux).
  - Make sure `BITLOOPS_CODE_EXPORT_DISABLE` is not set.
  - Make sure you ran `cycloops init` in that repo, and ticked the agent you are actually using.
  - Check the folder you set in `BITLOOPS_CODE_EXPORT_DIR`.
  - Set `BITLOOPS_CODE_EXPORT_TRACE=1` in the agent's environment and look at the hook's stderr. It lists the changed files it found and says why each was saved or skipped.
- **Hooks call `bitloops` instead of `cycloops`.** The repo was initialised by an official Bitloops install. Run `cycloops init` again to install this build's hooks; they coexist rather than replacing each other.
- **`configuring SQLite pragmas`, `locking protocol` or DuckDB "file is being used by another process" (Windows).** The database is locked by another process, usually the running daemon. This can stop the turn being recorded in history. It does not stop archiving, because the hook archives before it touches any database.
- **The same unchanged files keep being saved again.** A daemon started before the last upgrade is still running the old code. Stop it and start it again.
- **macOS refuses to run the binary.** Releases are ad-hoc signed, not notarized. The installer clears the quarantine attribute; if you moved the binary by hand, run `xattr -dr com.apple.quarantine "$(command -v cycloops)"`.
- **Windows says the file is in use during an upgrade.** A daemon is still running. Stop it (`cycloops daemon stop`, or Ctrl+C in its terminal) and run the installer again.
- **`Bitloops daemon did not become ready within 45 seconds`.** Starting the daemon in the background can fail. Running `cycloops daemon start` in a terminal and leaving it open works. This does not affect archiving.

## Turning it off or undoing it

- Turn off archiving: set `BITLOOPS_CODE_EXPORT_DISABLE=1` in the agent's environment.
- Stop capture in one repo: `cycloops disable` in that repo.
- Remove hooks and other artefacts: `cycloops uninstall`. It is interactive and asks whether to clean the system, known repositories, or both.
- Remove the binary: delete it from the install directory (`~/.local/bin/cycloops`, or `%USERPROFILE%\.cycloops\bin` on Windows).
- Delete the archive: remove the export folder. Nothing ever reads it back.

## Building from source

Only needed if you are changing the code. Releases are built by `.github/workflows/release.yml` on any `v*` tag.

**Prerequisites**

- Rust via [rustup](https://rustup.rs). The toolchain (1.95.0) is pinned in `rust-toolchain.toml` and fetched automatically.
- Windows: **Visual Studio Build Tools** with the **"Desktop development with C++"** workload. Without it every build fails with `linker link.exe not found`. Git Bash ships a `link.exe` that is *not* the compiler's linker and causes a confusing `link: extra operand` error — build from PowerShell or an "x64 Native Tools" prompt.
- macOS: `xcode-select --install`.
- Linux: `sudo apt install build-essential pkg-config cmake curl`.
- About 10 GB of free disk space, and 10 to 25 minutes for the first release build.

```bash
cargo build --release -p bitloops
```

The binary is `target/release/cycloops` (`cycloops.exe` on Windows). The crate is still named `bitloops` — only the binary was renamed. Use `--release`: debug builds overflow the main thread's stack on Windows and won't start.

On unusual Linux targets (musl, for example) the prebuilt DuckDB library doesn't exist; build with `--features duckdb-bundled`, as the release workflow does for every non-Windows target.

Cutting a release:

```bash
git tag v0.0.2
git push origin v0.0.2
```
