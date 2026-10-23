#!/usr/bin/env bash
# agent-jsonl-compact installer (prebuilt Linux x86_64 musl / macOS arm64 / macOS x86_64)
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/yuki-inaho/agent-jsonl-compact/main/install.sh | bash
#
# Environment:
#   AJC_VERSION   install a specific tag (e.g. v0.3.1). Default: latest release.
#   INSTALL_DIR   install directory. Default: ~/.local/bin
set -euo pipefail

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

for tool in curl tar mktemp; do
  command -v "$tool" >/dev/null 2>&1 || err "required tool not found: $tool"
done

# --- checksum tool (GNU coreutils or macOS shasum) ---
if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | awk '{print $1}'; }
else
  err "required tool not found: sha256sum or shasum"
fi

# --- resolve version ---
if [ "$VERSION" = "latest" ]; then
  info "resolving latest release tag"
  VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' | head -1 | sed -E 's/.*"tag_name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/')"
  [ -n "$VERSION" ] || err "could not resolve latest release tag (no releases yet? set AJC_VERSION=vX.Y.Z)"
fi
info "version: ${VERSION}"

asset="${BIN}-${TARGET}.tar.gz"
base="https://github.com/${REPO}/releases/download/${VERSION}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

info "downloading ${asset}"
curl -fsSL "${base}/${asset}" -o "${tmp}/${asset}" \
  || err "download failed: ${base}/${asset}"
curl -fsSL "${base}/${asset}.sha256" -o "${tmp}/${asset}.sha256" \
  || err "checksum file not found for ${VERSION}"

info "verifying checksum"
expected="$(awk '{print $1}' "${tmp}/${asset}.sha256")"
actual="$(sha256 "${tmp}/${asset}")"
[ -n "$expected" ] || err "empty checksum"
[ "$expected" = "$actual" ] || err "checksum mismatch (expected ${expected}, got ${actual})"

info "extracting"
tar -C "$tmp" -xzf "${tmp}/${asset}"
src="$(find "$tmp" -type f -name "$BIN" | head -1)"
[ -n "$src" ] || err "binary ${BIN} not found in archive"

mkdir -p "$INSTALL_DIR"
install -m 0755 "$src" "${INSTALL_DIR}/${BIN}"
info "installed: ${INSTALL_DIR}/${BIN}"

if [ "$os" = "Darwin" ]; then
  echo "note: if macOS blocks the binary, remove the quarantine attribute:"
  echo "      xattr -d com.apple.quarantine \"${INSTALL_DIR}/${BIN}\""
fi

# --- PATH hint ---
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) : ;;
  *) echo "note: ${INSTALL_DIR} is not on PATH. Add to your shell rc:"; echo "      export PATH=\"${INSTALL_DIR}:\$PATH\"" ;;
esac

"${INSTALL_DIR}/${BIN}" --version 2>/dev/null || true
