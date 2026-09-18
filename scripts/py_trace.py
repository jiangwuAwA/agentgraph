"""Minimal pure-Python call tracer for S_py differential (Track M2).

Usage:
    python scripts/py_trace.py <module.py> <entry_function>

Imports the module under a call profiler (`sys.setprofile`), invokes the
entry function, and prints JSON edges observed at runtime:

    {"entry": "main", "edges": [{"from": "main", "to": "login_handler"}, ...]}

Only *direct* Python function-call edges whose callee lives in the target
module are recorded (same claim class as `scripts/diff_trace.cjs`).
"""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional, Set


def main(argv: List[str]) -> int:
    if len(argv) < 2:
        print("usage: py_trace.py <module.py> [entry]", file=sys.stderr)
        return 2
    mod_path = Path(argv[1]).resolve()
    entry = argv[2] if len(argv) > 2 else "main"
    if not mod_path.is_file():
        print(f"module not found: {mod_path}", file=sys.stderr)
        return 2

    spec = importlib.util.spec_from_file_location(mod_path.stem, mod_path)
    if spec is None or spec.loader is None:
        print(f"cannot load module: {mod_path}", file=sys.stderr)
        return 2
    module = importlib.util.module_from_spec(spec)
    # Keep module-level defs addressable by co_filename matching.
    sys.modules[mod_path.stem] = module
    spec.loader.exec_module(module)

    target_file = str(mod_path)
    edges: List[Dict[str, str]] = []
    seen: Set[str] = set()
    stack: List[str] = []

    def name_of(frame: Any) -> Optional[str]:
        code = frame.f_code
        if code.co_filename != target_file:
            return None
        return code.co_name

    def profiler(frame: Any, event: str, arg: Any) -> None:
        if event == "call":
            callee = name_of(frame)
            if callee is not None:
                caller = next((n for n in reversed(stack) if n), None)
                if caller is not None:
                    key = f"{caller}->{callee}"
                    if key not in seen:
                        seen.add(key)
                        edges.append({"from": caller, "to": callee})
                stack.append(callee)
            else:
                stack.append("")
        elif event == "return":
            if stack:
                stack.pop()

    fn = getattr(module, entry, None)
    if not callable(fn):
        print(f"entry not found: {entry}", file=sys.stderr)
        return 2

    sys.setprofile(profiler)
    try:
        fn()
    finally:
        sys.setprofile(None)

    print(json.dumps({"entry": entry, "edges": edges}, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
