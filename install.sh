#!/usr/bin/env bash
# Install agentgraph from GitHub Releases (linux/macOS).
set -euo pipefail

REPO="${AGENTGRAPH_REPO:-jiangwuAwA/agentgraph}"
VERSION="${1:-latest}"
BIN_DIR="${AGENTGRAPH_BIN_DIR:-$HOME/.local/bin}"
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

asset="agentgraph-${platform}-${cpu}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if [ "$VERSION" = "latest" ]; then
  url="https://github.com/${REPO}/releases/latest/download/${asset}.tar.gz"
else
  url="https://github.com/${REPO}/releases/download/${VERSION}/${asset}.tar.gz"
fi

echo "Downloading $url"
curl -fsSL "$url" -o "$tmp/agentgraph.tar.gz"
tar -xzf "$tmp/agentgraph.tar.gz" -C "$tmp"
install -m 755 "$tmp/agentgraph" "$BIN_DIR/agentgraph"
echo "Installed $BIN_DIR/agentgraph"
"$BIN_DIR/agentgraph" --version
