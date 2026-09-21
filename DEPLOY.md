# Deploy

Release flow for this research fork.

## Workflows

`.github/workflows/release.yml` is the only workflow. The eight others were
inherited from upstream and removed: they need `self-hosted` runners, a
`develop` branch, or secrets (`OPENAI_API_KEY_PR_REVIEW`,
`BITLOOPS_ACTIONS_VARIABLES_TOKEN`) that this fork does not have. One of
them, `allow-main-only-via-develop`, failed every PR into `main` that did not
come from `develop`.

There is therefore **no CI**. Run the tests locally before merging:

```bash
cargo build --release -p bitloops
cargo test -p bitloops
```

Worth replacing with a plain `ubuntu-latest` workflow at some point; the
first build takes 10-25 minutes because `libduckdb-sys` compiles DuckDB from
source.

## 1. Ship code to `main`

Merge your branch into `main`. Nothing runs on push.

Merging makes `install.sh` and `install.ps1` live at their
`raw.githubusercontent.com/.../main/` URLs. Until a release exists those
scripts exit with "No published release found", so merge and tag close
together.

## 2. Tag

The workflow stamps the version from the tag (`BITLOOPS_BUILD_VERSION`), so
the tag alone decides what `cycloops --version` reports.
`bitloops/Cargo.toml` does not need bumping.

Tags must be semver-parseable - `major.minor.patch[-prerelease]` - or the
update check cannot compare them. Prereleases sort *below* their release, so
`0.0.31-archiver.2` is newer than `0.0.31-archiver.1` but older than
`0.0.31`.

```bash
git checkout main
git pull --ff-only origin main
git tag v0.0.31-archiver.2
git push origin v0.0.31-archiver.2
```

`scripts/release.sh` is upstream's helper. It derives the tag from
`bitloops/Cargo.toml`, which will not match the scheme above unless you bump
that file to the same prerelease string. Tagging by hand is simpler.

To exercise the build matrix without cutting a release, run the workflow
manually - `publish` and `verify` are gated on a tag ref and will skip.

## 3. Watch the run

Six targets, two of them `cross` musl builds. Budget 25-40 minutes cold.

Success:

- A GitHub Release with `cycloops-<target>.{tar.gz,zip}` for all six targets
  and `checksums-sha256.txt`
- The `verify` job green - it re-downloads the published assets, checks the
  SHA-256s, and runs the Linux x86_64 and arm64 binaries (the latter under
  qemu)

If `verify` passes, the release is installable.

## 4. Check the install

On a machine that has never had this installed:

```bash
curl -fsSL https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.sh | bash
cycloops --version
```

```powershell
irm https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.ps1 | iex
cycloops --version
```

Then in a scratch repo run `cycloops init`, do one agent turn, and confirm a
JSON file lands in the export folder. Run `init` a second time and confirm it
reports nothing to install - that exercises hook detection, which is easy to
break when renaming.

## 5. Rollback

1. Delete the GitHub Release and the tag
2. Fix forward
3. Publish a new tag

Users are not upgraded automatically. At most once every 24 hours the binary
checks this fork's latest release and prints the install command if a newer
one exists; they have to re-run it. For a study, tell participants directly
when a version matters.
