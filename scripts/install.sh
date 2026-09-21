#!/usr/bin/env bash
#
# Moved. The installer for this fork lives at the repository root.
#
# This file used to be upstream's installer with REPO="bitloops/bitloops",
# which installed the OFFICIAL Bitloops binary rather than this research
# build. It now forwards to the right script so the old path cannot quietly
# install the wrong thing.

set -euo pipefail

root_installer="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/install.sh"

echo "Note: the installer moved to the repository root; forwarding." >&2

if [ -f "$root_installer" ]; then
  exec bash "$root_installer" "$@"
fi

exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.sh)" -- "$@"
