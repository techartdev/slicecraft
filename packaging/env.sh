# shellcheck shell=bash
# Shared setup for the packaging scripts and the suite's release workflow (its version step
# sources this file). Source it: `. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"`.
#
# Exports:
#   ROOT     the workspace root
#   VERSION  `[workspace.package] version` from Cargo.toml (override: SLICECRAFT_VERSION)
#   DIST     where release files go (default: $ROOT/dist/release)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ROOT

# The version lives in one place: `[workspace.package] version` in the root Cargo.toml.
workspace_version() {
  awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]"); next }
    in_pkg && $1 == "version" { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$ROOT/Cargo.toml"
}

VERSION="${SLICECRAFT_VERSION:-$(workspace_version)}"
if [ -z "$VERSION" ]; then
  echo "error: could not read [workspace.package] version from $ROOT/Cargo.toml" >&2
  exit 1
fi
export VERSION

DIST="${DIST:-$ROOT/dist/release}"
mkdir -p "$DIST"
export DIST
