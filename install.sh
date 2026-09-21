#!/usr/bin/env bash
#
# One-command installer for the Cycloops research build of Bitloops.
#
#   curl -fsSL https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.sh | bash
#
# Options (env vars):
#   CYCLOOPS_VERSION      tag to install, e.g. v0.0.31-archiver.1  (default: latest)
#   CYCLOOPS_INSTALL_DIR  where to put the binary  (default: ~/.local/bin)
#   CYCLOOPS_EXPORT_DIR   archive destination      (default: the binary's own default)
#   CYCLOOPS_FULL_CLI=1   install the full Bitloops pipeline instead of
#                         archiver-only mode (daemon, DuckDB, DevQL, sync)
#   CYCLOOPS_NO_PATH=1    do not touch shell rc files

set -euo pipefail

REPO="KonstantinaGkavanozi/bitloops"
BIN_NAME="cycloops"

VERSION="${CYCLOOPS_VERSION:-latest}"
INSTALL_DIR="${CYCLOOPS_INSTALL_DIR:-$HOME/.local/bin}"

info() { printf '  %s\n' "$*"; }
warn() { printf '  ! %s\n' "$*" >&2; }
die()  { printf '\nError: %s\n' "$*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not installed."; }

# --- 1. platform ------------------------------------------------------------

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os" in
    Darwin)
      case "$arch" in
        arm64|aarch64) echo "aarch64-apple-darwin zip" ;;
        x86_64)        echo "x86_64-apple-darwin zip" ;;
        *) die "Unsupported macOS architecture: $arch" ;;
      esac ;;
    Linux)
      case "$arch" in
        x86_64|amd64)  echo "x86_64-unknown-linux-musl tar.gz" ;;
        aarch64|arm64) echo "aarch64-unknown-linux-musl tar.gz" ;;
        *) die "Unsupported Linux architecture: $arch" ;;
      esac ;;
    *) die "Unsupported OS: $os. On Windows use install.ps1." ;;
  esac
}

# --- 2. resolve the tag -----------------------------------------------------

resolve_tag() {
  if [ "$VERSION" != "latest" ]; then
    printf '%s' "$VERSION"
    return
  fi
  # Follow the /releases/latest redirect rather than using the API, so an
  # unauthenticated install is not subject to the API rate limit.
  local url
  url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
         "https://github.com/${REPO}/releases/latest")" \
    || die "Could not reach GitHub to resolve the latest release."
  local tag="${url##*/}"
  [ -n "$tag" ] && [ "$tag" != "releases" ] \
    || die "No published release found. Set CYCLOOPS_VERSION to a specific tag."
  printf '%s' "$tag"
}

# --- 3. install -------------------------------------------------------------

main() {
  need curl
  need uname

  read -r TARGET EXT <<<"$(detect_target)"
  local tag; tag="$(resolve_tag)"
  local asset="${BIN_NAME}-${TARGET}.${EXT}"
  local base="https://github.com/${REPO}/releases/download/${tag}"

  printf '\nInstalling %s %s (%s)\n\n' "$BIN_NAME" "$tag" "$TARGET"

  local tmp; tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  info "Downloading ${asset}"
  curl -fL --retry 3 --progress-bar -o "${tmp}/${asset}" "${base}/${asset}" \
    || die "Download failed. Does ${tag} have an asset named ${asset}?"

  # --- checksum ---
  if curl -fsSL -o "${tmp}/checksums-sha256.txt" "${base}/checksums-sha256.txt"; then
    info "Verifying checksum"
    local expected actual
    expected="$(awk -v a="$asset" '$2 == a || $2 == "*"a {print $1; exit}' "${tmp}/checksums-sha256.txt")"
    [ -n "$expected" ] || die "No checksum listed for ${asset}."
    if command -v sha256sum >/dev/null 2>&1; then
      actual="$(sha256sum "${tmp}/${asset}" | awk '{print $1}')"
    else
      actual="$(shasum -a 256 "${tmp}/${asset}" | awk '{print $1}')"
    fi
    [ "$expected" = "$actual" ] \
      || die "Checksum mismatch for ${asset}. Expected ${expected}, got ${actual}. Aborting."
  else
    warn "checksums-sha256.txt not published for ${tag}; skipping verification."
  fi

  # --- extract ---
  info "Extracting"
  case "$EXT" in
    tar.gz) tar -xzf "${tmp}/${asset}" -C "$tmp" ;;
    zip)
      if command -v ditto >/dev/null 2>&1; then
        ditto -x -k "${tmp}/${asset}" "$tmp"
      else
        need unzip; unzip -q "${tmp}/${asset}" -d "$tmp"
      fi ;;
  esac

  # Tolerate a nested directory inside the archive.
  local extracted
  extracted="$(find "$tmp" -type f -name "$BIN_NAME" -perm -u+x -print -quit 2>/dev/null || true)"
  [ -n "$extracted" ] || extracted="$(find "$tmp" -type f -name "$BIN_NAME" -print -quit)"
  [ -n "$extracted" ] || die "Could not find '${BIN_NAME}' inside ${asset}."

  # --- place ---
  mkdir -p "$INSTALL_DIR" || die "Cannot create ${INSTALL_DIR}."
  local dest="${INSTALL_DIR}/${BIN_NAME}"
  if [ -e "$dest" ]; then
    info "Backing up existing binary to ${BIN_NAME}.bak"
    cp -f "$dest" "${dest}.bak"
  fi
  install -m 0755 "$extracted" "$dest" 2>/dev/null || {
    cp -f "$extracted" "$dest"; chmod 0755 "$dest";
  }

  # --- macOS: quarantine + ad-hoc signature ---
  if [ "$(uname -s)" = "Darwin" ]; then
    info "Clearing quarantine attribute"
    xattr -dr com.apple.quarantine "$dest" 2>/dev/null || true
    if ! codesign --verify "$dest" >/dev/null 2>&1; then
      info "Applying ad-hoc code signature"
      codesign --force --sign - "$dest" >/dev/null 2>&1 \
        || warn "Ad-hoc signing failed; macOS may refuse to run the binary."
    fi
  fi

  # --- PATH ---
  if [ -z "${CYCLOOPS_NO_PATH:-}" ] && ! command -v "$BIN_NAME" >/dev/null 2>&1; then
    local line="export PATH=\"${INSTALL_DIR}:\$PATH\""
    for rc in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.profile"; do
      [ -f "$rc" ] || continue
      grep -Fqs "$INSTALL_DIR" "$rc" && continue
      printf '\n# added by %s installer\n%s\n' "$BIN_NAME" "$line" >> "$rc"
      info "Added ${INSTALL_DIR} to PATH in ${rc}"
    done
  fi

  # --- environment ---
  # Archiver-only by default: no daemon, no database, nothing to keep running.
  # Telemetry needs no switch here: this build reports nothing unless
  # BITLOOPS_TELEMETRY_OPTIN is set explicitly.
  if [ -z "${CYCLOOPS_NO_PATH:-}" ]; then
    lines=""
    [ -z "${CYCLOOPS_FULL_CLI:-}" ] && lines="export CYCLOOPS_ARCHIVER_ONLY=1"
    if [ -n "${CYCLOOPS_EXPORT_DIR:-}" ]; then
      lines="${lines}${lines:+
}export BITLOOPS_CODE_EXPORT_DIR=\"${CYCLOOPS_EXPORT_DIR}\""
    fi
    if [ -n "$lines" ]; then
      for rc in "$HOME/.zshrc" "$HOME/.bashrc"; do
        [ -f "$rc" ] || continue
        grep -Fqs "CYCLOOPS_ARCHIVER_ONLY\|BITLOOPS_CODE_EXPORT_DIR" "$rc" && continue
        printf '\n# added by %s installer\n%s\n' "$BIN_NAME" "$lines" >> "$rc"
      done
    fi
  fi

  printf '\nInstalled to %s\n' "$dest"
  "$dest" --version || true

  cat <<EOF

Next steps — open a NEW terminal, then:

  cd /path/to/your/repo
  ${BIN_NAME} init              # tick every agent you use

Archiver-only mode is on, so there is no daemon to start and init asks
nothing beyond which agents to hook. Unset CYCLOOPS_ARCHIVER_ONLY for the
full Bitloops pipeline.

Archives are written to \${BITLOOPS_CODE_EXPORT_DIR:-~/Desktop/cycloops code}/.
Set BITLOOPS_CODE_EXPORT_DIR in the environment of the terminal or app you
launch your agent from, not just any shell.

Note: there is no ignore list. A changed .env or key file is archived in
plain text. Keep the export folder somewhere private.
EOF
}

main "$@"
