#!/usr/bin/env bash
# P2-2 local smoke: index public fixture → blast-radius / who-calls → markdown.
# Demo only — not a required product gate. Offline from repo fixtures.
# Usage: bash scripts/ci_blast_radius_demo.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}/")/.." && pwd)"
cd "$ROOT"

FIXTURE="${CI_FIXTURE:-fixtures/eval-agent-goldens/clean-ts}"
SYMBOL="${CI_SYMBOL:-createUser}"
WHO_SYMBOL="${CI_WHO_SYMBOL:-validateEmail}"
OUT_DIR="${CI_OUT_DIR:-$ROOT/target/ci-blast-radius-demo}"

if [[ ! -d "$FIXTURE" ]]; then
  echo "missing fixture $FIXTURE" >&2
  exit 1
fi

resolve_bin() {
  local cands=(
    "${AGENTGRAPH_BIN:-}"
    "$ROOT/target/release/agentgraph"
    "$ROOT/target/debug/agentgraph"
  )
  local c help
  for c in "${cands[@]}"; do
    [[ -n "$c" && -x "$c" ]] || continue
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
  echo "No agentgraph binary with blast-radius; building release…" >&2
  (cd "$ROOT" && cargo build --release --bin agentgraph)
  echo "$ROOT/target/release/agentgraph"
}

AG="$(resolve_bin)"
echo "Using binary: $AG"
"$AG" --version

mkdir -p "$OUT_DIR"
BLAST_JSON="$OUT_DIR/blast-radius.json"
WHO_JSON="$OUT_DIR/who-calls.json"
MD="$OUT_DIR/summary.md"

echo "== index $FIXTURE =="
"$AG" --root "$FIXTURE" index --force

echo "== blast-radius $SYMBOL =="
"$AG" --root "$FIXTURE" blast-radius "$SYMBOL" --depth 3 > "$BLAST_JSON"

echo "== who-calls $WHO_SYMBOL =="
"$AG" --root "$FIXTURE" who-calls "$WHO_SYMBOL" > "$WHO_JSON"

python3 - "$BLAST_JSON" <<'PY'
import json, sys
from pathlib import Path
br = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
for key in ("window", "subset_ok", "promise_tier", "recommendation", "note"):
    if key not in br or br.get(key) in (None, ""):
        raise SystemExit("blast-radius payload missing %s" % key)
print("honesty keys ok:", br.get("window"), br.get("subset_ok"), br.get("promise_tier"))
PY

# Helper flags are script-only (not agentgraph CLI surface).
python3 scripts/ci_blast_radius_markdown.py \
  --fixture "$FIXTURE" \
  --symbol "$SYMBOL" \
  --who-symbol "$WHO_SYMBOL" \
  --blast "$BLAST_JSON" \
  --who-calls "$WHO_JSON" \
  --command "agentgraph --root $FIXTURE index --force" \
  --command "agentgraph --root $FIXTURE blast-radius $SYMBOL --depth 3" \
  --command "agentgraph --root $FIXTURE who-calls $WHO_SYMBOL" \
  --out "$MD" >/dev/null

echo ""
echo "Summary markdown: $MD"
head -n 40 "$MD"
echo "CI BLAST-RADIUS DEMO OK"
exit 0
