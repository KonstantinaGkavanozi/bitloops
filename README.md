# Cycloops

A research build of the [Bitloops](https://github.com/bitloops/bitloops) CLI
that archives the code AI agents write.

Every time an agent finishes a turn in a repo it is watching, Cycloops saves a
copy of each file the agent created or modified, together with the name of the
model that wrote it. The result is a timestamped record of what each model
produced, as JSON, on disk.

The command is `cycloops`. It installs alongside an official Bitloops
install without either one disturbing the other.

## Install

**macOS / Linux**

```bash
curl -fsSL https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.sh | bash
```

**Windows (PowerShell)**

Download the script, then run it:

```powershell
curl.exe -fsSL -o install.ps1 https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.ps1
.\install.ps1
```

Deliberately two steps. Piping a downloaded script straight into `iex` is the
shape of a common malware delivery technique, and Windows Defender flags it
(`Trojan:Win32/ClickFix`). Downloading first lets Defender scan the file, and
lets you read it before running it.

**Windows, without running a script at all**

Take the zip for your architecture from the
[latest release](https://github.com/KonstantinaGkavanozi/bitloops/releases/latest),
extract `cycloops.exe` and `duckdb.dll` together into a folder, and add that
folder to your PATH. Keep the two files side by side.

No Rust toolchain and no build required. The installer picks the right
prebuilt binary, checks it against the published SHA-256, and puts it on your
PATH.

## Use

In a **new** terminal, from the repository you want to record:

```bash
cd path/to/your/repo
cycloops init
```

Tick the agents you use. That's the whole setup — there is no daemon to keep
running.

Now work as usual. At the end of each agent turn, files land in
`~/Desktop/cycloops-code/<repo>/...` as JSON:

```json
{
  "model": "claude-sonnet-5",
  "code": "...the full contents of the file after the turn..."
}
```

To put them somewhere else, set `BITLOOPS_CODE_EXPORT_DIR` — in the
environment of the terminal or app you launch your agent from, not just any
shell.

```bash
CYCLOOPS_EXPORT_DIR="$HOME/research-archive" \
  curl -fsSL https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.sh | bash
```

## What gets saved

- Only files **created or modified** during the turn. Deletions are not recorded.
- The **whole file**, not a diff.
- A file is written again only when its content differs from its newest
  archived copy, so an unchanged file is not re-saved every turn.
- Each save carries its own timestamp, so earlier versions stay alongside later
  ones.
- Unreadable files (binaries, files removed again) are skipped silently.
- Archiving never blocks or fails an agent turn.

**There is no ignore list.** A changed `.env` or key file is archived like any
other file, in plain text. If people outside your team will run this on their
own repositories, say so in your consent material, or add a denylist first.

## Settings

| Variable | Effect |
|---|---|
| `BITLOOPS_CODE_EXPORT_DIR` | Where to write the archive. Default `~/Desktop/cycloops-code` |
| `BITLOOPS_CODE_EXPORT_DISABLE` | Any non-empty value turns archiving off |
| `BITLOOPS_CODE_EXPORT_TRACE` | Print to stderr which files were seen and why each was saved or skipped |
| `CYCLOOPS_FULL_CLI` | Unset by default. Set it to run the full Bitloops pipeline — see below |
| `BITLOOPS_TELEMETRY_OPTIN` | Telemetry is off. Setting this turns it on |

The environment variables keep their `BITLOOPS_` prefix: they are read by
unchanged upstream code. The command is `cycloops`, the settings are
`BITLOOPS_`. This is not a typo.

## Two modes

**Archiver-only** (the default, with nothing to configure). The agent hook saves the
turn's code and stops. No daemon, no database, no sync, no checkpoints, and
`init` asks only which agents to hook. This is the mode to give study
participants — there is nothing to keep running and nothing that can fail in a
way they would have to debug.

**Full CLI.** Set `CYCLOOPS_FULL_CLI=1` and everything the upstream
project does is still there: the daemon, DevQL, the dashboard, embeddings,
checkpoints, commit history import. Start it with `cycloops daemon start`.
Note that `init` will then offer Bitloops Cloud for embeddings, which
authenticates against upstream's real service.

## Supported agents

Claude Code, Codex, Cursor, Gemini, Copilot, OpenCode.

## Telemetry

Off. This build reports nothing unless `BITLOOPS_TELEMETRY_OPTIN` is set —
and note that the PostHog key compiled into the source belongs to upstream
Bitloops, so anything sent would land in their project rather than ours.

## Build from source

Only needed if you are changing the code.

```bash
cargo build --release -p bitloops
```

The binary is `target/release/cycloops`. The crate is still named `bitloops`;
only the binary was renamed. Use `--release` — debug builds overflow the main
thread's stack on Windows.

Prerequisites: Rust via [rustup](https://rustup.rs) (the 1.95.0 toolchain is
pinned in `rust-toolchain.toml` and fetched automatically), plus a C++
toolchain — Visual Studio Build Tools with "Desktop development with C++" on
Windows, `xcode-select --install` on macOS, `build-essential pkg-config cmake`
on Linux. Budget 10–25 minutes and about 10 GB for the first build.

## More

- [Code_Archiver.md](Code_Archiver.md) — what the archiver records, every
  setting, and troubleshooting
- [DEPLOY.md](DEPLOY.md) — cutting a release
- [README.bitloops.md](README.bitloops.md) — the upstream project's own README

## Licence and attribution

Apache-2.0, see [LICENSE](LICENSE).

Cycloops is a fork of [Bitloops](https://github.com/bitloops/bitloops).
Essentially all of the code is theirs. This fork adds the code archiver and
changes five things: the binary name, telemetry off by default, the update
check pointed at this repository, the branding, and archiver-only mode.
