#!/usr/bin/env bash
# Build SliceCraft for Linux and put the release files in $DIST:
#
#   slicecraft-<version>-linux-<arch>.tar.gz              the programs, icon, desktop file and docs
#   slicecraft-<version>-linux-<arch>-SHA256SUMS.txt      its checksum
#
# <arch> is x86_64 or aarch64 (the host's). `cargo xtask dist` does the work; this script is the
# entry point the suite's release workflow calls, and it then tries the packed programs:
# the window's `--version` and a slice of the example cube.
#
# Usage: packaging/linux/package.sh
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"

case "$(uname -m)" in
  x86_64) ARCH=x86_64 ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) echo "unsupported architecture $(uname -m)" >&2; exit 2 ;;
esac
NAME="slicecraft-$VERSION-linux-$ARCH"

cd "$ROOT"
cargo xtask dist
cp "target/dist/$NAME.tar.gz" "target/dist/$NAME-SHA256SUMS.txt" "$DIST/"

# Try what was packed, not what was built.
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
tar -xzf "$DIST/$NAME.tar.gz" -C "$WORK"
"$WORK/$NAME/slicecraft" --version
"$WORK/$NAME/slicecraft-cli" slice "$WORK/$NAME/examples/cube.stl" -o "$WORK/cube.gcode"
test -s "$WORK/cube.gcode"
(cd "$DIST" && sha256sum -c "$NAME-SHA256SUMS.txt")

echo "==> done"
ls -lh "$DIST"
