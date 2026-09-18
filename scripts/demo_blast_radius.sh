#!/usr/bin/env bash
# P0-2 onboarding demo: index sample fixture → blast-radius JSON.
# Offline from repo fixtures. Exit 0 on success.
# Usage: bash scripts/demo_blast_radius.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BIN="${AGENTGRAPH_BIN:-}"
SYMBOL="${SYMBOL:-createUser}"
KEEP_TEMP="${KEEP_TEMP:-0}"

resolve_bin() {
  if [[ -n "$BIN" && -x "$BIN" ]]; then
    echo "$BIN"
    return
  fi
  if [[ -n "${AGENTGRAPH_BIN:-}" && -x "${AGENTGRAPH_BIN}" ]]; then
    echo "$AGENTGRAPH_BIN"
    return
  fi
  local cands=(
    "$ROOT/target/debug/agentgraph"
    "$ROOT/target/release/agentgraph"
  )
  local c help
  for c in "${cands[@]}"; do
    [[ -x "$c" ]] || continue
    if help="$("$c" blast-radius --help 2>&1)" && grep -q "blast-radius" <<<"$help"; then
      echo "$c"
      return
    fi
  done
  if command -v agentgraph >/dev/null 2>&1; then
    if help="$(agentgraph blast-radius --help 2>&1)" && grep -q "blast-radius" <<<"$help"; then
      command -v agentgraph
      return
    fi
  fi
  echo "No agentgraph binary with blast-radius; building debug…" >&2
  (cd "$ROOT" && cargo build --bin agentgraph)
  if [[ -x "$ROOT/target/debug/agentgraph" ]]; then
    echo "$ROOT/target/debug/agentgraph"
  else
    echo "built binary not found" >&2
    return 1
  fi
}

AG="$(resolve_bin)"
echo "Using binary: $AG"
"$AG" --version

FIXTURE="$ROOT/fixtures/sample-app"
if [[ ! -f "$FIXTURE/src/auth.ts" ]]; then
  echo "missing fixture $FIXTURE (need fixtures/sample-app)" >&2
  exit 1
fi

TMP="$(mktemp -d "${TMPDIR:-/tmp}/agentgraph-demo-br.XXXXXX")"
echo "Temp sample: $TMP"

cleanup() {
  if [[ "$KEEP_TEMP" != "1" ]]; then
    rm -rf "$TMP"
  else
    echo "Kept temp: $TMP"
  fi
}
trap cleanup EXIT

mkdir -p "$TMP"
cp -R "$FIXTURE/src" "$TMP/src"

echo "== index =="
"$AG" --root "$TMP" index --force

echo "== blast-radius $SYMBOL =="
JSON_TEXT="$("$AG" --root "$TMP" blast-radius "$SYMBOL" --depth 3)"
if [[ -z "${JSON_TEXT// }" ]]; then
  echo "blast-radius produced empty stdout" >&2
  exit 1
fi

echo ""
echo "== recommendation / window =="
# Prefer a working python; Windows Git Bash often has a broken WindowsApps python3 stub.
PY=""
for cand in python python3; do
  if command -v "$cand" >/dev/null 2>&1 && "$cand" -c "import json" >/dev/null 2>&1; then
    PY="$(command -v "$cand")"
    break
  fi
done
if [[ -n "$PY" ]]; then
  printf '%s\n' "$JSON_TEXT" | "$PY" -c '
import json,sys
p=json.load(sys.stdin)
for k in ("window","subset_ok","promise_tier","recommendation","note"):
    print("%-16s%s" % (k+":", p.get(k)))
for req in ("window","recommendation","note"):
    if p.get(req) in (None, ""):
        raise SystemExit("payload missing %s" % req)
'
else
  printf '%s\n' "$JSON_TEXT"
  for key in '"window"' '"recommendation"' '"note"'; do
    grep -q "$key" <<<"$JSON_TEXT" || { echo "payload missing $key" >&2; exit 1; }
  done
fi

echo ""
echo "== full blast_radius JSON =="
printf '%s\n' "$JSON_TEXT"
echo ""

echo "== who-calls validateEmail (smoke) =="
"$AG" --root "$TMP" who-calls validateEmail >/dev/null

echo "DEMO OK"
exit 0
