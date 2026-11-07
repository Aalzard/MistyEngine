#!/usr/bin/env bash
# Build the Linux x64 release package of Misty Engine: the folder, the .tar.gz and its SHA-256
# file. The Linux counterpart of tools/make_release.ps1 (see docs/release.md); the release
# workflow (.github/workflows/release.yml) runs it on ubuntu-22.04.
#
#   tools/make_release_linux.sh [--out-dir DIR] [--target-dir DIR] [--skip-build]
#
# 1. cargo build --release -p misty_editor -p misty_player (skip with --skip-build)
# 2. <out>/MistyEngine-v<version>-linux-x64/ with MistyEngine (misty renamed), misty_player,
#    Examples/SampleProject (from git HEAD, so uncommitted edits are left out; no Library/ or
#    Builds/), README.txt (tools/release/README-linux.txt), CHANGELOG.txt and LICENSE.txt
# 3. <out>/MistyEngine-v<version>-linux-x64.tar.gz (the folder inside, executables 755) and
#    <tar.gz>.sha256.txt ("<hash>  <file name>", what the updater checks)
#
# The version is the workspace version in Cargo.toml. Default out dir: a MistyRelease folder
# next to the repository; default target dir: $CARGO_TARGET_DIR or <repo>/target.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out_dir="$(dirname "$repo")/MistyRelease"
target_dir="${CARGO_TARGET_DIR:-$repo/target}"
skip_build=0
while [ $# -gt 0 ]; do
    case "$1" in
        --out-dir) out_dir="$2"; shift 2 ;;
        --target-dir) target_dir="$2"; shift 2 ;;
        --skip-build) skip_build=1; shift ;;
        -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
        *) echo "unknown option $1" >&2; exit 2 ;;
    esac
done

version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$repo/Cargo.toml" | head -n 1)"
[ -n "$version" ] || { echo "No workspace version in Cargo.toml." >&2; exit 1; }
name="MistyEngine-v$version-linux-x64"
echo "Misty Engine $version -> $out_dir/$name"

if [ -n "$(git -C "$repo" status --porcelain -- crates Cargo.toml Cargo.lock)" ]; then
    echo "warning: uncommitted changes in the engine source; the binaries include them" >&2
fi

if [ "$skip_build" = 0 ]; then
    CARGO_INCREMENTAL=0 cargo build --release -p misty_editor -p misty_player \
        --manifest-path "$repo/Cargo.toml" --target-dir "$target_dir"
fi
bin="$target_dir/release"
for exe in misty misty_player; do
    [ -f "$bin/$exe" ] || { echo "$exe not found in $bin." >&2; exit 1; }
done

# --- the folder ---------------------------------------------------------------------------
mkdir -p "$out_dir"
pkg="$out_dir/$name"
rm -rf "$pkg"
mkdir -p "$pkg/Examples"
install -m 755 "$bin/misty" "$pkg/MistyEngine"
install -m 755 "$bin/misty_player" "$pkg/misty_player"

# The example project exactly as committed.
git -C "$repo" archive --format=tar HEAD SampleProject | tar -x -C "$pkg/Examples"
rm -rf "$pkg/Examples/SampleProject/Library" "$pkg/Examples/SampleProject/Builds"

sed "s/{VERSION}/$version/g" "$repo/tools/release/README-linux.txt" > "$pkg/README.txt"
cp "$repo/tools/release/CHANGELOG.txt" "$pkg/CHANGELOG.txt"
{
    cat "$repo/LICENSE"
    printf '\n\n%s\n\n' "$(printf '=%.0s' $(seq 77))"
    cat "$repo/RUNTIME-EXCEPTION.md"
} > "$pkg/LICENSE.txt"

# Plain permissions whatever the umask: folders 755, files 644, the two programs 755.
find "$pkg" -type d -exec chmod 755 {} +
find "$pkg" -type f -exec chmod 644 {} +
chmod 755 "$pkg/MistyEngine" "$pkg/misty_player"

# --- the archive and its checksum --------------------------------------------------------
archive="$name.tar.gz"
rm -f "$out_dir/$archive" "$out_dir/$archive.sha256.txt"
tar --format=gnu --owner=0 --group=0 --numeric-owner --sort=name \
    -C "$out_dir" -czf "$out_dir/$archive" "$name"
(cd "$out_dir" && sha256sum "$archive" > "$archive.sha256.txt")

size="$(du -m "$out_dir/$archive" | cut -f1)"
echo "$archive  ${size} MB  sha256 $(cut -d' ' -f1 "$out_dir/$archive.sha256.txt")"
