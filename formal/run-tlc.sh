#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
JAVA_BIN="${JAVA_HOME:+$JAVA_HOME/bin/}java"
if ! command -v "$JAVA_BIN" >/dev/null 2>&1; then
  JAVA_BIN=java
fi
exec "$JAVA_BIN" -cp "$HERE/tools/tla2tools.jar" tlc2.TLC \
  -config "$HERE/IncrementalIndex.cfg" "$HERE/IncrementalIndex"
