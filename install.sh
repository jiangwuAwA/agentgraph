#!/usr/bin/env bash
# Install agentgraph from GitHub Releases (linux/macOS).
set -euo pipefail

REPO="${AGENTGRAPH_REPO:-jiangwuAwA/agentgraph}"
VERSION="${1:-latest}"
BIN_DIR="${AGENTGRAPH_BIN_DIR:-$HOME/.local/bin}"
# Fail closed on checksum problems unless AGENTGRAPH_SKIP_CHECKSUM=1.
SKIP_CHECKSUM="${AGENTGRAPH_SKIP_CHECKSUM:-0}"
mkdir -p "$BIN_DIR"

os="$(uname -s | tr '[:upper:]' '[:lower:]')"
arch="$(uname -m)"
case "$os" in
  linux) platform="linux" ;;
  darwin) platform="macos" ;;
  *) echo "unsupported OS: $os" >&2; exit 1 ;;
esac
case "$arch" in
  x86_64|amd64) cpu="x86_64" ;;
  aarch64|arm64) cpu="aarch64" ;;
  *) echo "unsupported arch: $arch" >&2; exit 1 ;;
esac

# Detect musl (Alpine / musl-linked glibc-less systems) and pick the musl asset.
is_musl() {
  [ "$os" = "linux" ] || return 1
  if [ -r /etc/os-release ]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    if [ "${ID:-}" = "alpine" ]; then
      return 0
    fi
  fi
  # ldd --version writes to stderr on glibc; musl prints something like
  # "musl libc (x86_64)\nVersion 1.2.x".
  if command -v ldd >/dev/null 2>&1; then
    if ldd --version 2>&1 | grep -qi musl; then
      return 0
    fi
  fi
  return 1
}

asset="agentgraph-${platform}-${cpu}"
if [ "$platform" = "linux" ] && [ "$cpu" = "x86_64" ] && is_musl; then
  asset="${asset}-musl"
  echo "Detected musl libc, using ${asset}"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if [ "$VERSION" = "latest" ]; then
  base_url="https://github.com/${REPO}/releases/latest/download"
else
  base_url="https://github.com/${REPO}/releases/download/${VERSION}"
fi
url="${base_url}/${asset}.tar.gz"
sum_url="${url}.sha256"

echo "Downloading $url"
curl -fsSL "$url" -o "$tmp/agentgraph.tar.gz"

compute_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print tolower($1)}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print tolower($1)}'
  else
    echo "error: neither sha256sum nor shasum is available; set AGENTGRAPH_SKIP_CHECKSUM=1 to skip" >&2
    exit 1
  fi
}

if [ "$SKIP_CHECKSUM" != "1" ]; then
  echo "Downloading checksum $sum_url"
  if ! curl -fsSL "$sum_url" -o "$tmp/agentgraph.tar.gz.sha256"; then
    echo "error: checksum asset missing or download failed ($sum_url)." >&2
    echo "       Set AGENTGRAPH_SKIP_CHECKSUM=1 to install without verification." >&2
    exit 1
  fi
  expected="$(awk '{print tolower($1)}' "$tmp/agentgraph.tar.gz.sha256" | head -n1)"
  if [ -z "$expected" ]; then
    echo "error: checksum file is empty or malformed; set AGENTGRAPH_SKIP_CHECKSUM=1 to skip" >&2
    exit 1
  fi
  actual="$(compute_sha256 "$tmp/agentgraph.tar.gz")"
  if [ "$expected" != "$actual" ]; then
    echo "error: SHA256 mismatch for ${asset}.tar.gz" >&2
    echo "       expected: $expected" >&2
    echo "       actual:   $actual" >&2
    exit 1
  fi
  echo "SHA256 verified"
else
  echo "WARNING: checksum verification skipped (AGENTGRAPH_SKIP_CHECKSUM=1)" >&2
fi

tar -xzf "$tmp/agentgraph.tar.gz" -C "$tmp"
# Release tarball may contain `agentgraph` or a platform-suffixed name like
# `agentgraph-linux-x86_64`. Find whichever agentgraph* binary was packed.
bin_src="$(find "$tmp" -maxdepth 1 -type f -name 'agentgraph*' ! -name '*.tar.gz' ! -name '*.sha256' | head -n1)"
if [ -z "$bin_src" ]; then
  echo "error: no agentgraph binary found in release archive" >&2
  ls -la "$tmp" >&2
  exit 1
fi
install -m 755 "$bin_src" "$BIN_DIR/agentgraph"
echo "Installed $BIN_DIR/agentgraph"
"$BIN_DIR/agentgraph" --version
