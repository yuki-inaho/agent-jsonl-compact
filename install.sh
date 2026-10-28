#!/usr/bin/env bash
# agent-jsonl-compact installer (prebuilt Linux x86_64 musl / macOS arm64 / macOS x86_64)
#
# Usage:
#   Download this script, review it, then run: bash install.sh
#
# Environment:
#   AJC_VERSION   release tag (vX.Y.Z, optionally -rc.N). Default: latest release.
#   INSTALL_DIR   install directory. Default: ~/.local/bin
set -euo pipefail
umask 077

REPO="yuki-inaho/agent-jsonl-compact"
BIN="agent-jsonl-compact"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${AJC_VERSION:-latest}"

err()  { echo "error: $*" >&2; exit 1; }
info() { echo "==> $*"; }

# --- platform detection ---
os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Linux)
    case "$arch" in
      x86_64 | amd64) TARGET="x86_64-unknown-linux-musl" ;;
      *) err "unsupported Linux arch: $arch (supported: x86_64). Build from source: https://github.com/${REPO}" ;;
    esac
    ;;
  Darwin)
    case "$arch" in
      arm64 | aarch64) TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *) err "unsupported macOS arch: $arch (supported: arm64, x86_64). Build from source: https://github.com/${REPO}" ;;
    esac
    ;;
  *)
    err "unsupported OS: $os. Build from source: https://github.com/${REPO}"
    ;;
esac
info "platform: ${os} ${arch} (${TARGET})"

for tool in curl tar mktemp install mkdir mv awk sed cat rm; do
  command -v "$tool" >/dev/null 2>&1 || err "required tool not found: $tool"
done
[[ "$INSTALL_DIR" = /* ]] || err "INSTALL_DIR must be an absolute path"

# --- checksum tool (GNU coreutils or macOS shasum) ---
if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | awk '{print $1}'; }
else
  err "required tool not found: sha256sum or shasum"
fi

download() {
  curl --proto '=https' --proto-redir '=https' --tlsv1.2 \
    --connect-timeout 15 --max-time 300 --retry 2 -fsSL "$@"
}

# --- resolve version ---
if [ "$VERSION" = "latest" ]; then
  info "resolving latest release tag"
  metadata="$(download "https://api.github.com/repos/${REPO}/releases/latest")"
  VERSION="$(printf '%s\n' "$metadata" | sed -nE 's/^[[:space:]]*"tag_name"[[:space:]]*:[[:space:]]*"([^"]+)"[,]?[[:space:]]*$/\1/p')"
  [ -n "$VERSION" ] || err "could not resolve latest release tag (no releases yet? set AJC_VERSION=vX.Y.Z)"
fi
[[ "$VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9]+(\.[a-zA-Z0-9]+)*)?$ ]] \
  || err "invalid release tag: expected vX.Y.Z or vX.Y.Z-rc.N"
info "version: ${VERSION}"

asset="${BIN}-${TARGET}.tar.gz"
base="https://github.com/${REPO}/releases/download/${VERSION}"

tmp="$(mktemp -d)"
staged=""
cleanup() {
  rm -rf "$tmp"
  if [ -n "$staged" ]; then rm -f "$staged"; fi
}
trap cleanup EXIT

info "downloading ${asset}"
download "${base}/${asset}" -o "${tmp}/${asset}" \
  || err "download failed: ${base}/${asset}"
download "${base}/${asset}.sha256" -o "${tmp}/${asset}.sha256" \
  || err "checksum file not found for ${VERSION}"

info "verifying checksum"
checksum="$(cat "${tmp}/${asset}.sha256")"
[[ "$checksum" != *$'\n'* ]] || err "checksum must contain exactly one record"
read -r expected checksum_name extra <<< "$checksum"
[[ "$expected" =~ ^[0-9a-f]{64}$ ]] || err "invalid SHA-256 checksum"
[[ -z "$extra" && ( "$checksum_name" = "$asset" || "$checksum_name" = "*$asset" ) ]] \
  || err "checksum filename does not match archive"
actual="$(sha256 "${tmp}/${asset}")"
[ -n "$expected" ] || err "empty checksum"
[ "$expected" = "$actual" ] || err "checksum mismatch (expected ${expected}, got ${actual})"

info "extracting binary"
member="${BIN}-${TARGET}/${BIN}"
listing="$(tar -tzf "${tmp}/${asset}")" || err "cannot list archive"
binary_count=0
while IFS= read -r entry; do
  case "$entry" in
    "$member") binary_count=$((binary_count + 1)) ;;
    "${BIN}-${TARGET}/" | "${BIN}-${TARGET}/README.md" | "${BIN}-${TARGET}/README.en.md" | \
      "${BIN}-${TARGET}/SECURITY.md" | "${BIN}-${TARGET}/SECURITY.en.md" | \
      "${BIN}-${TARGET}/CHANGELOG.md" | "${BIN}-${TARGET}/CHANGELOG.ja.md" | \
      "${BIN}-${TARGET}/LICENSE" | "${BIN}-${TARGET}/docs/"*) : ;;
    *) err "unexpected archive entry" ;;
  esac
done <<< "$listing"
[ "$binary_count" -eq 1 ] || err "archive must contain exactly one binary"
details="$(tar -tvzf "${tmp}/${asset}" "$member")" || err "cannot inspect binary"
[[ "$details" = -* && "$details" != *$'\n'* ]] || err "archive binary must be a regular file"
# Stream only the validated regular member; tar never creates archive paths or links.
src="${tmp}/${BIN}"
tar -xOzf "${tmp}/${asset}" "$member" > "$src" || err "cannot extract binary"
[ -s "$src" ] || err "archive binary is empty"

mkdir -p "$INSTALL_DIR"
[ ! -L "${INSTALL_DIR}/${BIN}" ] || err "refusing symlink destination"
[ ! -e "${INSTALL_DIR}/${BIN}" ] || [ -f "${INSTALL_DIR}/${BIN}" ] || err "destination is not a regular file"
staged="$(mktemp "${INSTALL_DIR}/.${BIN}.XXXXXX")"
install -m 0755 "$src" "$staged"
mv -f "$staged" "${INSTALL_DIR}/${BIN}"
staged=""
info "installed: ${INSTALL_DIR}/${BIN}"

# --- PATH hint ---
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) : ;;
  *) echo "note: ${INSTALL_DIR} is not on PATH. Add to your shell rc:"; echo "      export PATH=\"${INSTALL_DIR}:\$PATH\"" ;;
esac

info "verify with: ${INSTALL_DIR}/${BIN} --version"
