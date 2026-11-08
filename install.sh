#!/bin/sh
# Download a published release; no sudo or shell startup edits.
set -eu
fail() { printf '%s\n' "agent-latex: $*" >&2; exit 1; }
[ "$#" -ge 1 ] && [ "$#" -le 2 ] || fail 'usage: sh install.sh OWNER/REPO [v0.1.0]'
repo=$1
case "$repo" in *[!A-Za-z0-9_./-]*|/*|*/|*..*) fail 'invalid OWNER/REPO';; esac
case "$repo" in */*) ;; *) fail 'expected OWNER/REPO';; esac
owner=${repo%%/*}
project=${repo#*/}
case "$project" in */*) fail 'expected exactly OWNER/REPO';; esac
[ -n "$owner" ] && [ -n "$project" ] || fail 'expected OWNER/REPO'
version=${2:-latest}
destination=${INSTALL_DIR:-"$HOME/.local/bin"}
case "$destination" in /*) ;; *) fail 'INSTALL_DIR must be an absolute path';; esac
case "$version" in latest|v[0-9]*) ;; *) fail 'version must be latest or a v-prefixed tag';; esac
case "$version" in *[!A-Za-z0-9._-]*) fail 'invalid version';; esac
case "$(uname -s)" in Linux) os=linux;; Darwin) os=macos;; *) fail 'supported systems: Linux and macOS';; esac
case "$(uname -m)" in x86_64|amd64) arch=x86_64;; arm64|aarch64) arch=aarch64;; *) fail 'supported architectures: x86_64 and arm64';; esac
for utility in curl tar mktemp install; do command -v "$utility" >/dev/null || fail "missing $utility"; done
if command -v sha256sum >/dev/null; then checksum=sha256sum
elif command -v shasum >/dev/null; then checksum=shasum
else fail 'sha256sum or shasum is required'; fi
base="https://github.com/$repo/releases"
if [ "$version" = latest ]; then
    # Resolve once so archive and checksum cannot come from different releases.
    resolved=$(curl --proto '=https' --tlsv1.2 -fsSL -o /dev/null -w '%{url_effective}' "$base/latest")
    version=${resolved##*/}
    case "$version" in v[0-9]*) ;; *) fail 'no published release found';; esac
    case "$version" in *[!A-Za-z0-9._-]*) fail 'invalid release tag';; esac
fi
base="$base/download/$version"
archive="agent-latex-$os-$arch.tar.gz"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
curl --proto '=https' --tlsv1.2 -fsSL "$base/$archive" -o "$work/$archive"
curl --proto '=https' --tlsv1.2 -fsSL "$base/SHA256SUMS" -o "$work/SHA256SUMS"
cd "$work"
awk -v name="$archive" '$2 == name {print}' SHA256SUMS > selected.sha256
[ "$(wc -l < selected.sha256 | tr -d ' ')" = 1 ] || fail 'missing or duplicate checksum'
if [ "$checksum" = sha256sum ]; then sha256sum -c selected.sha256
else shasum -a 256 -c selected.sha256; fi
# Extract only the executable, not paths supplied by the archive.
tar -xzf "$archive" agent-latex
[ -f agent-latex ] && [ ! -L agent-latex ] || fail 'invalid executable in archive'
mkdir -p "$destination"
install -m 755 agent-latex "$destination/agent-latex"
printf '\nInstalled %s to %s/agent-latex\n' "$version" "$destination"
printf 'Add this directory to PATH if needed, then run: agent-latex demo\n'
