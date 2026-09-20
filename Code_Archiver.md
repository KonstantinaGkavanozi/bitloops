# Code Archiver

The code archiver is a small feature inside Bitloops. Every time an AI agent finishes a turn in a repo Bitloops is tracking, it saves a copy of each file the agent created or modified, together with the name of the model that wrote it.

It is built into the Bitloops CLI (`bitloops/src/host/checkpoints/lifecycle/code_export.rs`). There is no separate plugin or Python hook to install.

## What it produces

One JSON file per changed file, per turn:

```
<export root>/<repo folder name>/<path of the file inside the repo>/<timestamp>__<filename>.json
```

For example, if the agent edits `src/app.rs` in a repo called `my-project`:

```
~/Desktop/bitloops code/my-project/src/1789900000-123456__app.rs.json
```

Each file contains:

```json
{
  "model": "claude-sonnet-5",
  "code": "...the full final contents of the file after the turn..."
}
```

Behaviour worth knowing:

- Only files that were **created or modified** in the turn are saved. Deleted files are not.
- The **whole file** is saved, not a diff. Saving the same file in two turns gives two files (the timestamp keeps them apart).
- Files that can't be read as text (binaries, files removed again) are silently skipped.
- Archiving never blocks or fails the agent's turn. If a write fails, that file is skipped and nothing is reported.
- If Bitloops cannot work out the model name, `"model"` is `"unknown"`.
- There is **no ignore list**. A changed `.env` or key file is archived like any other file, in plain text. Keep the export folder somewhere private.

## Settings

Two optional environment variables:

| Variable | Effect |
|---|---|
| `BITLOOPS_CODE_EXPORT_DIR` | Where to write the archive. Default: `~/Desktop/bitloops code` |
| `BITLOOPS_CODE_EXPORT_DISABLE` | Any non-empty value turns archiving off. It is on by default. |

**Important:** the archiving code runs inside the `bitloops hooks ...` command that your agent launches at the end of a turn, not inside the background daemon. So the variables must be set in the environment of the **agent** (the terminal or app you start Claude Code, Cursor, etc. from), not just any shell.

Setting them:

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
- On Windows, if your Desktop is redirected (for example by OneDrive), the default `%USERPROFILE%\Desktop\bitloops code` may not be your visible Desktop. Set `BITLOOPS_CODE_EXPORT_DIR` explicitly.

## Important: it is not in the released version

The archiver was added after the public Bitloops release (v0.0.31). Installing Bitloops with the official installer does **not** give you this feature. You have to build the CLI from this repository at commit `1fae5d9` ("code archiving") or later, and put that build in place of the installed `bitloops` binary.

The overall recipe is the same on every OS:

1. Install the official release once (this creates the default config and the DuckDB runtime library the binary needs).
2. Build this repo in release mode.
3. Replace the installed `bitloops` binary with your build.
4. Run `bitloops init` in each repo you want archived.

## Choices you will be asked to make

Setup is interactive. Running `bitloops init` in a repo asks a series of questions, and the installer can ask some too. Run it in a real terminal, because it needs one to show the prompts. You can accept the defaults by pressing Enter. For the code archiver only the first question really matters.

| Prompt | What it means | For the archiver |
|---|---|---|
| **Select agents to integrate** (space to tick, Enter to confirm) | Which AI agents get Bitloops hooks installed in this repo (Claude Code, Cursor, Codex, Gemini, Opencode, Copilot, as offered on your machine). | **Tick every agent you use in this repo.** Archiving runs from that agent's hooks, so an agent you don't tick is never archived. |
| **Enable DevQL Guidance** (a checkbox under the agent list) | Lets Bitloops feed codebase context back to the agent. | Not needed. Leave it as you like. |
| **Configure embeddings**: Bitloops Cloud / Local embeddings / Skip for now | Powers semantic code search. Cloud opens a browser sign-in. Local needs about 4 GB RAM and a GPU is recommended. | Choose **Skip for now**. |
| **Configure summary embeddings**: Enable / Skip for now (and, if code embeddings were skipped, a provider choice: Cloud / Local / Skip) | Semantic search over generated summaries. | Choose **Skip for now**. |
| **Final setup checklist**: Sync codebase, Import commit history, Enable anonymous telemetry, Start Bitloops daemon automatically when you sign in | Optional extras. Press Enter for the defaults, type option numbers, or `all` / `none`. | Sync and Import history are not needed for archiving and can take a while on a large repo. Telemetry is your choice. Auto-start is a convenience only; on Windows it has not been checked. |

The exact list can differ a little between versions and depending on what is already configured.

The installer has its own choice. With `-DefaultConfig` (Windows) or `--default-config` (macOS/Linux) it applies Bitloops's default configuration without asking. Without that flag, run `bitloops configure --web` afterwards to set things up by hand.

Skipping embeddings has no effect on archiving. You can run `bitloops init` again later to change these answers.

## Setup on Windows

This is the path that has been run and checked.

**Prerequisites**

- Rust via [rustup](https://rustup.rs). The repo pins the toolchain (1.95.0) in `rust-toolchain.toml`, and rustup fetches it automatically.
- **Visual Studio Build Tools** with the **"Desktop development with C++"** workload. Without it every build fails with `linker link.exe not found`. Git Bash also ships a `link.exe` that is *not* the compiler's linker and causes a confusing `link: extra operand` error. Build from PowerShell or a "x64 Native Tools" prompt.
- About 10 GB of free disk space, and 10 to 25 minutes for the first release build.

**Steps**

1. Install the official release (PowerShell):
   ```powershell
   & ([scriptblock]::Create((irm https://bitloops.com/install.ps1))) -DefaultConfig
   ```
   It installs to `%USERPROFILE%\.bitloops\bin` and adds that folder to your PATH. If the last step reports `Bitloops daemon did not become ready within 45 seconds`, see Troubleshooting. Your config and databases are already created by then, so you can carry on.
2. Build, from the repo root, in a shell where the C++ tools are loaded (open "x64 Native Tools Command Prompt for VS 2022", or run `vcvars64.bat` first):
   ```powershell
   cargo build --release -p bitloops
   ```
   The result is `target\release\bitloops.exe`.
   Use `--release`. Debug builds of this project overflow the main thread's stack on Windows and won't start.
3. Replace the installed binary:
   ```powershell
   $bin = "$env:USERPROFILE\.bitloops\bin"
   Copy-Item "$bin\bitloops.exe" "$bin\bitloops.exe.bak"          # backup
   Copy-Item target\release\bitloops.exe "$bin\bitloops.exe" -Force
   ```
   If Windows says the file is in use, a Bitloops daemon is still running. Stop it (Ctrl+C in its terminal, or `bitloops daemon stop`) and copy again. Renaming the running file first (`Move-Item`) also works. Keep `duckdb.dll` where it is, next to `bitloops.exe`.
4. Open a **new** terminal (old ones don't see the PATH change) and check:
   ```powershell
   bitloops --version      # the "commit:" line should be 1fae5d9 or later
   ```
5. Start the daemon in a terminal and leave it open:
   ```powershell
   bitloops daemon start
   ```
6. In each repo you want archived, in another terminal:
   ```powershell
   cd path\to\your\repo
   bitloops init
   ```
   `init` asks several questions, described in "Choices you will be asked to make" above. Tick the agent(s) you use, and choose **Skip for now** for both embeddings prompts.

## Setup on macOS

Not run on a Mac yet. These steps follow the repo's own build docs (`DEVELOPMENT.md`) and install script.

**Prerequisites**

- Xcode Command Line Tools: `xcode-select --install`
- Rust via rustup: `curl https://sh.rustup.rs -sSf | sh`
- About 10 GB of free disk space.

**Steps**

1. Install the official release:
   ```bash
   curl -fsSL https://bitloops.com/install.sh | bash -s -- --default-config
   ```
   It installs to `/usr/local/bin`, or `~/.local/bin` if that isn't writable. `libduckdb.dylib` is installed next to the binary.
2. One-time build setup, from the repo root (skip if `bitloops/config/dashboard_urls.json` already exists):
   ```bash
   cp bitloops/config/dashboard_urls.template.json bitloops/config/dashboard_urls.json
   ```
3. Build:
   ```bash
   cargo build --release --manifest-path bitloops/Cargo.toml
   ```
   The result is `target/release/bitloops`.
4. Replace the installed binary (use the directory the installer printed):
   ```bash
   cp "$(command -v bitloops)" "$(command -v bitloops).bak"
   cp target/release/bitloops "$(command -v bitloops)"
   ```
   If macOS refuses to run the copied binary, sign it locally: `codesign --force --sign - "$(command -v bitloops)"`.
   The repo also has `cargo dev-install`, which installs a build into your Cargo bin directory and signs it automatically. If you use it, make sure that directory comes first on your PATH.
5. Check the version, start the daemon and run `bitloops init` in each repo, exactly as in the Windows steps 4 to 6 (`bitloops --version`, `bitloops daemon start`, `bitloops init`). Answer the setup questions as described in "Choices you will be asked to make".

## Setup on Linux

Not run on Linux yet. Same approach as macOS.

**Prerequisites** (typical for this project's dependencies, not confirmed)

- A C toolchain, `pkg-config` and `cmake`. On Debian/Ubuntu: `sudo apt install build-essential pkg-config cmake curl`
- Rust via rustup: `curl https://sh.rustup.rs -sSf | sh`
- About 10 GB of free disk space.

**Steps**

Follow the macOS steps 1 to 5. The installer places `libduckdb.so` next to the binary, and there is no code signing step.

On unusual Linux targets (for example musl), the prebuilt DuckDB library doesn't exist. `DEVELOPMENT.md` says to build with `--features duckdb-bundled` instead.

## Checking that it works

1. Run `bitloops --version` and confirm the commit is `1fae5d9` or later.
2. In a repo where you ran `bitloops init`, ask your agent to create or change a file.
3. When the turn ends, look in the export folder (default `~/Desktop/bitloops code/<repo name>/`). There should be a `.json` file for each changed file.

The core logic (writing `{model, code}` files, the disable switch, the `unknown` model fallback) has three unit tests in `code_export.rs`. They pass on Windows when the file is built as its own small crate. The full flow with a real agent turn has not been confirmed yet.

## Troubleshooting

- **No files appear.**
  - Run `bitloops --version` from the same terminal your agent uses. A commit older than `1fae5d9` means the old binary is still first on PATH (`where bitloops` on Windows, `which -a bitloops` on macOS/Linux).
  - Make sure `BITLOOPS_CODE_EXPORT_DISABLE` is not set.
  - Make sure you ran `bitloops init` in that repo.
  - Check the folder you set in `BITLOOPS_CODE_EXPORT_DIR`.
- **`bitloops` is not recognized (Windows).** The installer changes PATH for new terminals only. Open a new one, or call `%USERPROFILE%\.bitloops\bin\bitloops.exe` directly.
- **`Bitloops daemon did not become ready within 45 seconds` (Windows).** Starting the daemon in the background (`bitloops daemon start -d`, which the installer uses) can fail. Running `bitloops daemon start` in a terminal and leaving it open works. This does not affect archiving.
- **`linker link.exe not found` or `link: extra operand` (Windows build).** Install the Visual Studio C++ Build Tools and build from PowerShell or a Native Tools prompt, not Git Bash.
- **`no space on device` during the build.** Free up disk space. The `target` folder grows to several GB.
- **`cargo test -p bitloops` doesn't compile on Windows.** Some existing tests use Unix-only code. This doesn't affect building or running Bitloops.

## Turning it off or undoing it

- Turn off archiving: set `BITLOOPS_CODE_EXPORT_DISABLE=1` in the agent's environment.
- Go back to the released CLI: copy the `.bak` file over the installed `bitloops` binary.
- Delete the archive: remove the export folder. Bitloops never reads it back.
