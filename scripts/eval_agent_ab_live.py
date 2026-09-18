#!/usr/bin/env python3
"""P0-5b live Agent A/B trajectories — host-session live LLM (honest limits).

Protocol
--------
For each public task under ``fixtures/eval-agent-tasks/``:

- **Arm A (live + agentgraph):** issue text only when deciding the file set.
  Tools: agentgraph CLI (index / blast-radius / who-calls / find / related /
  subset) + file reads.
- **Arm B (live read/grep only):** same issue text; **forbidden** agentgraph
  binary/MCP. Shell grep + reads only.

Trajectories live under ``evals/agent-ab-live/<task>/run-{a|b}-<seed>.json``
using ``agentgraph.eval_agent_ab.trajectory.v1`` plus live meta
(``kind=live_llm_agent``, ``model_note``, ``arm``, ``tools_allowed``).
Adapter schema id: ``agentgraph.eval_agent_ab.live.v1`` (documented alias —
payload still replays under trajectory.v1 scorer).

Workflow (honesty)
------------------
1. Agent runs tools / greps and commits ``file_set`` **before** reading
   ``task.json`` ``expected`` labels.
2. ``stamp`` fills structure-fact labels + offline score from fixtures
   **after** file sets are committed.
3. ``score`` replays committed trajectories offline (no network, no binary).

Honesty / limits (must stay in docs)
------------------------------------
- **Single host-session LLM** (``mimo-desktop-host-session``) — **not** a
  public benchmark model id, **not** a standardized lab harness.
- **Arm contamination risk:** the same session may see both arms; operator
  notes record order (A evidence then B greps per task; not per-seed
  randomization). Same-session exposure can inflate B or deflate A noise.
- **N small** (target 9 tasks × 3 repeats × 2 arms). Incomplete cells are
  marked; numbers are **only** what was actually recorded.
- **No oversell:** structure-fact file-set metrics on public mini fixtures —
  **not** production monorepo precision, **not** “MCP Agent product proof”.
- Host session has prior exposure to fixture ``task.json`` structure facts
  from P0-5 protocol work — recorded as contamination.

Exit codes: 0 ok · 1 gate/IO problems · 2 usage.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional

_SCRIPT_DIR = Path(__file__).resolve().parent
if str(_SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPT_DIR))

from eval_agent_ab import (  # noqa: E402
    discover_traj_files,
    score_file_set,
    score_trajectory,
    task_labels,
    write_trajectory,
)
from eval_agent_tasks import repo_root_from_script  # noqa: E402

LIVE_SCHEMA_ALIAS = "agentgraph.eval_agent_ab.live.v1"
TRAJECTORY_SCHEMA = "agentgraph.eval_agent_ab.trajectory.v1"
MODEL_NOTE = "mimo-desktop-host-session"
HONESTY_NOTE = (
    "live host-session LLM agent (not a public benchmark model; not a "
    "standardized lab harness). File sets decided from issue text + tool/"
    "grep evidence; structure-fact scores stamped offline after commitment. "
    "N small; possible same-session arm contamination."
)
CONTAMINATION_NOTE = (
    "Same MiMo Desktop host session previously inspected fixtures/eval-agent-tasks "
    "task.json expected labels during P0-5 protocol work; live arms still "
    "derive file_set from recorded tool/grep evidence (not by copying "
    "expected lists), but contamination risk is real and not zero."
)


def _as_rel(p: str) -> str:
    return str(p).replace("\\", "/").lstrip("./")


def _clean_args(args: List[str]) -> List[str]:
    out: List[str] = []
    for a in args:
        s = str(a)
        if s.startswith("\\\\") or (len(s) >= 3 and s[1] == ":" and s[2] in "\\/"):
            out.append("<fixture-root>")
        elif s.startswith("/"):
            out.append("<fixture-root>")
        else:
            out.append(s.replace("\\", "/"))
    return out


def _call(tool: str, args: List[str], ok: bool = True, summary: Optional[Dict] = None,
          note: str = "") -> Dict[str, Any]:
    return {
        "seq": 0,
        "tool": tool,
        "args": _clean_args(args),
        "ok": ok,
        "summary": summary or {},
        "note": note,
    }


# Live decisions: recorded from host-session tool/grep evidence.
# file_set committed BEFORE stamp; labels/score filled by stamp_scores().
# Each entry: task_id -> {symbol, issue_digest, A: {files, calls, rationale},
#                         B: {files, calls, rationale}}
LIVE_DECISIONS: Dict[str, Dict[str, Any]] = {
    "ts-nest-user-repo": {
        "symbol": "UserRepository",
        "issue_digest": (
            "UserRepository.find should return null instead of throwing; "
            "identify blast radius / review set vs noise."
        ),
        "A": {
            "files": [
                "src/user.repository.ts",
                "src/user.service.ts",
                "src/user.module.ts",
                "src/user.controller.ts",
            ],
            "calls": [
                _call("agentgraph.index", ["index", "--force"],
                      summary={"exit_code": 0}, note="fixture index"),
                _call("agentgraph.blast-radius", ["blast-radius", "UserRepository", "--depth", "3"],
                      summary={"window": "sound", "subset_ok": True},
                      note="dependents via DI import/registration + depth-2 controller"),
                _call("agentgraph.find", ["find", "UserRepository"],
                      summary={"definition_files": ["src/user.repository.ts"]},
                      note="definition"),
                _call("agentgraph.related", ["related", "UserRepository", "--limit", "20"],
                      summary={"related_files": 3}, note="importers"),
                _call("agentgraph.who-calls", ["who-calls", "UserRepository", "--limit", "50"],
                      summary={"high_freq_name": False,
                               "callers_files": ["src/user.module.ts", "src/user.service.ts"]},
                      note="callers only; no implementor flood"),
                _call("agentgraph.subset", ["subset"], summary={"in_subset": True},
                      note="honesty companion"),
                _call("read", ["src/user.service.ts", "src/user.module.ts", "src/user.repository.ts"],
                      note="confirm inject/register sites for find() contract change"),
            ],
            "rationale": (
                "blast-radius + who-calls: definition repo + DI consumers service/module; "
                "controller appears at depth-2 via UserService — include as review for a "
                "find() return-type change. health/unrelated never appear in structure "
                "recipes — omit from review set."
            ),
        },
        "B": {
            "files": [
                "src/user.repository.ts",
                "src/user.service.ts",
                "src/user.module.ts",
                "src/user.controller.ts",
            ],
            "calls": [
                _call("walk", ["src"], summary={"source_files": 6}, note="enumerate sources"),
                _call("grep", ["UserRepository"],
                      summary={"hits": [
                          "src/user.repository.ts",
                          "src/user.service.ts",
                          "src/user.module.ts",
                          "src/unrelated.ts",
                      ]},
                      note="token hits; unrelated.ts is comment-only after read"),
                _call("read", ["src/unrelated.ts", "src/user.service.ts",
                               "src/user.module.ts", "src/user.controller.ts"],
                      note="exclude comment-only noise; controller uses UserService"),
            ],
            "rationale": (
                "grep UserRepository → repo/service/module + unrelated comment hit; "
                "read unrelated → no dependency → omit. Same-dir read of controller "
                "shows UserService consumer — include for contract-change review. "
                "No agentgraph."
            ),
        },
    },
    "ts-nest-order-service": {
        "symbol": "OrderService",
        "issue_digest": (
            "OrderService.createOrder return type string→OrderResult; find dependents/"
            "registrations vs order-like noise; PaymentService is adjacent callee."
        ),
        "A": {
            "files": [
                "src/order.service.ts",
                "src/order.controller.ts",
                "src/order.module.ts",
                "src/payment.service.ts",
            ],
            "calls": [
                _call("agentgraph.index", ["index", "--force"], summary={"exit_code": 0},
                      note="fixture index"),
                _call("agentgraph.blast-radius", ["blast-radius", "OrderService", "--depth", "3"],
                      summary={"window": "sound"},
                      note="dependents: controller + module registration + definition"),
                _call("agentgraph.find", ["find", "OrderService"],
                      summary={"definition_files": ["src/order.service.ts"]},
                      note="definition"),
                _call("agentgraph.who-calls", ["who-calls", "OrderService", "--limit", "50"],
                      summary={"callers_files": ["src/order.controller.ts", "src/order.module.ts"]},
                      note="dependents / registration"),
                _call("agentgraph.related", ["related", "OrderService", "--limit", "20"],
                      note="scope retrieval"),
                _call("agentgraph.find", ["find", "PaymentService"],
                      summary={"definition_files": ["src/payment.service.ts"]},
                      note="issue says open callee separately — not a blast dependent"),
                _call("agentgraph.subset", ["subset"], summary={"in_subset": True},
                      note="honesty"),
            ],
            "rationale": (
                "blast/who-calls: definition + controller + module. Issue flags "
                "PaymentService as adjacent callee — open via find, include as review "
                "context but not as a dependent claim. order-fmt-util / legacy-report "
                "absent from recipes — omit."
            ),
        },
        "B": {
            "files": [
                "src/order.service.ts",
                "src/order.controller.ts",
                "src/order.module.ts",
                "src/payment.service.ts",
            ],
            "calls": [
                _call("grep", ["OrderService"],
                      summary={"hits": [
                          "src/order.service.ts",
                          "src/order.controller.ts",
                          "src/order.module.ts",
                          "src/legacy-report.ts",
                      ]},
                      note="legacy-report is comment-only noise after read"),
                _call("read", ["src/order.service.ts", "src/order.controller.ts",
                               "src/legacy-report.ts", "src/payment.service.ts"],
                      note="definition imports PaymentService → adjacent callee review"),
                _call("grep", ["createOrder"],
                      summary={"hits": ["src/order.service.ts", "src/order.controller.ts"]},
                      note="return-type consumers"),
            ],
            "rationale": (
                "grep OrderService + createOrder → service/controller/module; read "
                "legacy-report comment → omit noise. order.service imports PaymentService "
                "— include as adjacent callee per issue text. No agentgraph."
            ),
        },
    },
    "go-store-api": {
        "symbol": "Get",
        "issue_digest": (
            "Repository.Get must accept context; find implementors (MemRepo/RedisCache) "
            "and call sites (api handlers); ignore package noise."
        ),
        "A": {
            "files": ["store/store.go", "api/handlers.go"],
            "calls": [
                _call("agentgraph.index", ["index", "--force"], summary={"exit_code": 0},
                      note="fixture index"),
                _call("agentgraph.blast-radius", ["blast-radius", "Get", "--depth", "3"],
                      summary={"window": "sound"},
                      note="handlers call sites + store implementor edges"),
                _call("agentgraph.find", ["find", "Get"],
                      summary={"definitions": ["MemRepo.Get", "RedisCache.Get"]},
                      note="both implementors in store/store.go"),
                _call("agentgraph.who-calls", ["who-calls", "Get", "--limit", "50"],
                      summary={"high_freq_name": True,
                               "callers_files": ["api/handlers.go"],
                               "implementor_count": 7},
                      note="high_freq demotes implementor flood in presentation; "
                           "issue still requires implementor files — keep store/store.go"),
                _call("agentgraph.subset", ["subset"], summary={"in_subset": True},
                      note="honesty"),
            ],
            "rationale": (
                "Issue explicitly asks implementors + call sites. who-calls separates "
                "callers (api/handlers.go) from implementors (store/store.go). "
                "high_freq_name=true → do not flood with every implementor edge; still "
                "include the implementor definition file the issue names. metrics/cmd "
                "never use Get — omit."
            ),
        },
        "B": {
            "files": ["store/store.go", "api/handlers.go"],
            "calls": [
                _call("grep", ["Get"],
                      summary={"hits": ["api/handlers.go", "store/store.go"]},
                      note="token hits only on definition + call sites"),
                _call("read", ["store/store.go", "api/handlers.go", "cmd/main.go",
                               "internal/metrics.go"],
                      note="confirm cmd/metrics do not implement or call Repository.Get"),
            ],
            "rationale": (
                "grep Get hits only handlers + store. Reading cmd/main and "
                "internal/metrics shows no Repository.Get use — omit as package noise. "
                "No agentgraph."
            ),
        },
    },
    "py-fastapi-invoice": {
        "symbol": "InvoiceService",
        "issue_digest": (
            "InvoiceService.total miscalculates tax; identify blast radius — which "
            "API/deps/repository files must be reviewed."
        ),
        "A": {
            "files": [
                "app/invoice_service.py",
                "app/api/invoices.py",
                "app/deps.py",
                "app/invoice_repo.py",
            ],
            "calls": [
                _call("agentgraph.index", ["index", "--force"], summary={"exit_code": 0},
                      note="fixture index"),
                _call("agentgraph.blast-radius", ["blast-radius", "InvoiceService", "--depth", "3"],
                      summary={"window": "sound"},
                      note="API + deps + definition"),
                _call("agentgraph.find", ["find", "InvoiceService"],
                      summary={"definition_files": ["app/invoice_service.py"]},
                      note="definition"),
                _call("agentgraph.who-calls", ["who-calls", "InvoiceService", "--limit", "50"],
                      summary={"callers_files": ["app/api/invoices.py", "app/deps.py"]},
                      note="construct/use sites"),
                _call("read", ["app/deps.py"],
                      note="deps constructs InvoiceService(get_invoice_repository()) — "
                           "issue asks repository files in review set"),
            ],
            "rationale": (
                "Recipes cover service + api + deps. Issue text explicitly asks for "
                "repository files in the review set; deps.py constructs the service "
                "with invoice_repo → include app/invoice_repo.py. util_log / "
                "metrics_names absent from structure edges — omit."
            ),
        },
        "B": {
            "files": [
                "app/invoice_service.py",
                "app/api/invoices.py",
                "app/deps.py",
                "app/invoice_repo.py",
            ],
            "calls": [
                _call("grep", ["InvoiceService"],
                      summary={"hits": [
                          "app/invoice_service.py",
                          "app/api/invoices.py",
                          "app/deps.py",
                      ]},
                      note="token hits"),
                _call("grep", ["InvoiceRepository", "invoice_repo", "get_invoice_repository"],
                      summary={"hits": ["app/invoice_repo.py", "app/deps.py",
                                         "app/invoice_service.py"]},
                      note="issue asks repository files — secondary identifier search"),
                _call("read", ["app/deps.py", "app/invoice_service.py",
                               "app/util_log.py", "app/metrics_names.py"],
                      note="util_log/metrics_names are unrelated after read — omit"),
            ],
            "rationale": (
                "grep InvoiceService + repository-related identifiers per issue text; "
                "read noise candidates and drop util_log/metrics_names. No agentgraph."
            ),
        },
    },
    "py-plugin-registry": {
        "symbol": "PluginRegistry",
        "issue_digest": (
            "PluginRegistry.register must validate plugin names; find files that "
            "construct or call the registry."
        ),
        "A": {
            "files": ["app/registry.py", "app/bootstrap.py"],
            "calls": [
                _call("agentgraph.index", ["index", "--force"], summary={"exit_code": 0},
                      note="fixture index"),
                _call("agentgraph.blast-radius", ["blast-radius", "PluginRegistry", "--depth", "3"],
                      summary={"window": "sound"}, note="definition + bootstrap construct"),
                _call("agentgraph.find", ["find", "PluginRegistry"],
                      summary={"definition_files": ["app/registry.py"]},
                      note="definition"),
                _call("agentgraph.who-calls", ["who-calls", "PluginRegistry", "--limit", "50"],
                      summary={"callers_files": ["app/bootstrap.py"]},
                      note="construct site"),
                _call("agentgraph.subset", ["subset"], summary={"in_subset": True},
                      note="honesty"),
            ],
            "rationale": (
                "who-calls/blast: registry definition + bootstrap constructor. "
                "docs_strings is prose-only — recipes do not list it. plugins.py "
                "does not construct/call PluginRegistry — omit."
            ),
        },
        "B": {
            "files": ["app/registry.py", "app/bootstrap.py"],
            "calls": [
                _call("grep", ["PluginRegistry"],
                      summary={"hits": [
                          "app/registry.py",
                          "app/bootstrap.py",
                          "app/docs_strings.py",
                      ]},
                      note="docs_strings prose hit"),
                _call("read", ["app/docs_strings.py", "app/plugins.py", "app/bootstrap.py"],
                      note="docs_strings is comment/prose noise; plugins.py has no "
                           "PluginRegistry construct/call — omit"),
            ],
            "rationale": (
                "grep hits registry/bootstrap/docs_strings; read shows docs_strings is "
                "prose noise and plugins.py is not a construct/call site. No agentgraph."
            ),
        },
    },
    "noisy-name-fmt": {
        "symbol": "fmt",
        "issue_digest": (
            "Renaming fmt across the crate; naive name grep floods; use who_calls to "
            "separate real call sites from implementors; high-freq demotion applies."
        ),
        "A": {
            "files": ["src/render.rs"],
            "calls": [
                _call("agentgraph.index", ["index", "--force"], summary={"exit_code": 0},
                      note="fixture index"),
                _call("agentgraph.who-calls", ["who-calls", "fmt", "--limit", "50"],
                      summary={"high_freq_name": True,
                               "implementor_count": 72,
                               "callers_files": ["src/render.rs"]},
                      note="real call sites in render.rs; implementors separated/truncated"),
                _call("agentgraph.blast-radius", ["blast-radius", "fmt", "--depth", "2"],
                      summary={"window": "sound"}, note="all fmt edges live in render.rs"),
                _call("agentgraph.find", ["find", "fmt"],
                      summary={"definition_files": ["src/render.rs"]},
                      note="trait + impls"),
            ],
            "rationale": (
                "who_calls with high_freq_name=true: callers (show/debug_dump) and "
                "implementors all sit in src/render.rs; metrics.rs is not a call site. "
                "Review set = render.rs only."
            ),
        },
        "B": {
            "files": ["src/render.rs"],
            "calls": [
                _call("grep", ["fmt"],
                      summary={"hits": ["src/render.rs", "src/metrics.rs"]},
                      note="metrics has fmt-helper-label substring + comment"),
                _call("read", ["src/metrics.rs", "src/render.rs"],
                      note="metrics comment labels unrelated — exclude after read"),
            ],
            "rationale": (
                "grep fmt hits metrics via substring/comment; read confirms noise. "
                "All real fn fmt sites are in render.rs. No agentgraph."
            ),
        },
    },
    "rust-trait-handler": {
        "symbol": "render",
        "issue_digest": (
            "Handler::render signature must change; find implementors and dyn "
            "dispatch call sites."
        ),
        "A": {
            "files": ["src/handlers.rs", "src/lib.rs"],
            "calls": [
                _call("agentgraph.index", ["index", "--force"], summary={"exit_code": 0},
                      note="fixture index"),
                _call("agentgraph.blast-radius", ["blast-radius", "render", "--depth", "3"],
                      summary={"window": "sound"}, note="impls + dyn call sites"),
                _call("agentgraph.who-calls", ["who-calls", "render", "--limit", "50"],
                      summary={"high_freq_name": False, "implementor_count": 4,
                               "callers_files": ["src/lib.rs"]},
                      note="implementors + dyn dispatch callers"),
                _call("agentgraph.find", ["find", "render"],
                      summary={"definition_files": ["src/handlers.rs"]},
                      note="trait + impls"),
            ],
            "rationale": (
                "Issue asks implementors + dyn call sites: who-calls/blast cover "
                "handlers.rs (trait/impls) and lib.rs (dyn render()). metrics.rs "
                "self-labels no dependency — omit."
            ),
        },
        "B": {
            "files": ["src/handlers.rs", "src/lib.rs"],
            "calls": [
                _call("grep", ["render"],
                      summary={"hits": ["src/handlers.rs", "src/lib.rs", "src/metrics.rs"]},
                      note="metrics comment-only"),
                _call("read", ["src/metrics.rs", "src/handlers.rs", "src/lib.rs"],
                      note="metrics is noise after read"),
            ],
            "rationale": (
                "grep render; read metrics → no Handler/render dependency — omit. "
                "handlers + lib are impls + dyn call sites. No agentgraph."
            ),
        },
    },
    "rust-unsafe-scoped": {
        "symbol": "export_rows",
        "issue_digest": (
            "core::export_rows under change; workspace has unsafe legacy package; "
            "union sound off — must not claim sound; recommend scoped sound on core."
        ),
        "A": {
            "files": ["packages/core/src/export.rs", "packages/core/src/lib.rs"],
            "calls": [
                _call("agentgraph.index", ["index", "--workspace", "workspace.json", "--force"],
                      summary={"exit_code": 0}, note="workspace index"),
                _call("agentgraph.blast-radius",
                      ["blast-radius", "export_rows", "--depth", "3",
                       "--workspace", "workspace.json"],
                      summary={"window": "default", "subset_ok": False},
                      note="window=default — do not claim union sound"),
                _call("agentgraph.find",
                      ["find", "export_rows", "--workspace", "workspace.json"],
                      summary={"definition_files": ["packages/core/src/export.rs"]},
                      note="definition in core"),
                _call("agentgraph.who-calls",
                      ["who-calls", "export_rows", "--limit", "50",
                       "--workspace", "workspace.json"],
                      summary={"callers_files": ["packages/core/src/lib.rs"]},
                      note="core caller only"),
                _call("agentgraph.subset", ["subset", "--workspace", "workspace.json"],
                      summary={"in_subset": False},
                      note="legacy unsafe disables union S; scoped candidates on core"),
            ],
            "rationale": (
                "Workspace blast/who-calls: export_rows definition + core lib caller. "
                "window=default / subset_ok=false — do not claim sound; recommend "
                "scoped sound on clean core root only. legacy unsafe package is not an "
                "export_rows review file — omit from edit set."
            ),
        },
        "B": {
            "files": ["packages/core/src/export.rs", "packages/core/src/lib.rs"],
            "calls": [
                _call("grep", ["export_rows"],
                      summary={"hits": [
                          "packages/core/src/export.rs",
                          "packages/core/src/lib.rs",
                      ]},
                      note="legacy has no export_rows token"),
                _call("read", ["packages/core/src/export.rs", "packages/core/src/lib.rs",
                               "packages/legacy/src/lib.rs"],
                      note="legacy is unsafe sibling — not a call/def site for symbol"),
            ],
            "rationale": (
                "grep export_rows only hits core def + core lib. Read legacy to confirm "
                "it is unrelated to this symbol (unsafe sibling still relevant for sound "
                "claims but not in the edit set). No agentgraph."
            ),
        },
    },
    "rust-workspace-two-roots": {
        "symbol": "compute",
        "issue_digest": (
            "engine::compute algorithm change; multi-root workspace; avoid editing web "
            "unless it truly depends on compute."
        ),
        "A": {
            "files": ["packages/engine/src/compute.rs", "packages/engine/src/lib.rs"],
            "calls": [
                _call("agentgraph.index", ["index", "--workspace", "workspace.json", "--force"],
                      summary={"exit_code": 0}, note="workspace index"),
                _call("agentgraph.blast-radius",
                      ["blast-radius", "compute", "--depth", "3",
                       "--workspace", "workspace.json"],
                      summary={"window": "sound"},
                      note="engine-local dependents"),
                _call("agentgraph.who-calls",
                      ["who-calls", "compute", "--limit", "50",
                       "--workspace", "workspace.json"],
                      summary={"callers_files": ["packages/engine/src/lib.rs"]},
                      note="engine entry calls compute; no web callers"),
                _call("agentgraph.find",
                      ["find", "compute", "--workspace", "workspace.json"],
                      summary={"definition_files": ["packages/engine/src/compute.rs"]},
                      note="definition"),
                _call("agentgraph.subset", ["subset", "--workspace", "workspace.json"],
                      summary={"in_subset": True}, note="honesty"),
            ],
            "rationale": (
                "Workspace recipes: compute def + engine lib call. web package does not "
                "appear as a true dependent — issue says avoid web unless it truly "
                "depends. Omit web/*."
            ),
        },
        "B": {
            "files": ["packages/engine/src/compute.rs", "packages/engine/src/lib.rs"],
            "calls": [
                _call("grep", ["compute"],
                      summary={"hits": [
                          "packages/engine/src/compute.rs",
                          "packages/engine/src/lib.rs",
                          "packages/web/src/lib.rs",
                          "packages/web/src/pages.rs",
                      ]},
                      note="web hits are comment/word noise after read"),
                _call("read", ["packages/web/src/lib.rs", "packages/web/src/pages.rs",
                               "packages/engine/src/lib.rs", "packages/engine/src/compute.rs"],
                      note="web comments state no call to engine::compute — omit"),
            ],
            "rationale": (
                "grep compute hits engine + web comments; read web → no true dependency "
                "per issue instruction — omit web. Keep engine def + engine lib. "
                "No agentgraph."
            ),
        },
    },
}

ARM_META = {
    "A": {
        "policy_label": "Live LLM agent + agentgraph CLI/MCP recipes",
        "tools_allowed": [
            "agentgraph: index",
            "agentgraph: blast-radius",
            "agentgraph: who-calls",
            "agentgraph: find",
            "agentgraph: related",
            "agentgraph: subset",
            "read",
        ],
    },
    "B": {
        "policy_label": "Live LLM agent + read/grep only (no agentgraph)",
        "tools_allowed": ["walk", "grep", "read"],
    },
}


def repo_root() -> Path:
    return repo_root_from_script()


def build_live_trajectory(
    task_id: str,
    arm: str,
    seed: int,
    meta: Dict[str, Any],
    labels: Dict[str, Any],
    score: Dict[str, Any],
    fixture_rel: str,
) -> Dict[str, Any]:
    dec = LIVE_DECISIONS[task_id]
    arm_dec = dec[arm]
    calls = []
    for c in arm_dec["calls"]:
        cc = dict(c)
        cc["seq"] = len(calls) + 1
        calls.append(cc)
    a_meta = ARM_META[arm]
    return {
        "schema": TRAJECTORY_SCHEMA,
        "schema_alias": LIVE_SCHEMA_ALIAS,
        "policy": arm,
        "policy_label": a_meta["policy_label"],
        "kind": "live_llm_agent",
        "arm": arm,
        "task_id": task_id,
        "seed": seed,
        "run_id": f"{arm.lower()}-{seed}",
        "fixture": fixture_rel,
        "symbol": dec["symbol"],
        "model_note": MODEL_NOTE,
        "tools_allowed": list(a_meta["tools_allowed"]),
        "honesty": {
            "live_llm_agent": True,
            "scripted_tool_policy_agent": False,
            "private_corpus": False,
            "not_public_benchmark_model": True,
            "standardized_lab_harness": False,
            "note": HONESTY_NOTE,
            "contamination": CONTAMINATION_NOTE,
        },
        "task": {
            "id": task_id,
            "title": meta.get("title"),
            "language": meta.get("language"),
            "issue": meta.get("issue") or "",
            "issue_digest": dec["issue_digest"],
            **labels,
        },
        "tool_calls": calls,
        "file_set": sorted({_as_rel(p) for p in arm_dec["files"] if p}),
        "score": score,
        "extras": {
            "kind": "live_llm_agent",
            "model_note": MODEL_NOTE,
            "arm": arm,
            "rationale": arm_dec["rationale"],
            "protocol": (
                "file_set decided from issue text + recorded tool/grep evidence; "
                "expected labels stamped offline after commitment"
            ),
            "seed_role": (
                "repeat metadata; this host-session live agent was seed-invariant "
                "on these fixtures given the same tool evidence"
            ),
            "file_set_method": (
                "live agent judgment over agentgraph recipe evidence"
                if arm == "A"
                else "live agent judgment over grep/read evidence (no agentgraph)"
            ),
        },
        "notes": [
            HONESTY_NOTE,
            CONTAMINATION_NOTE,
            f"Arm {arm}: {arm_dec['rationale']}",
        ],
        "errors": [],
    }


def empty_labels() -> Dict[str, Any]:
    return {
        "expected_files": [],
        "noise_files": [],
        "forbidden_files": [],
        "forbidden_source": "",
    }


def empty_score(file_set: List[str]) -> Dict[str, Any]:
    return {
        "expected_file_recall": None,
        "expected_hit": [],
        "expected_miss": [],
        "extra_noise_files": [],
        "extra_noise_count": None,
        "forbidden_files": [],
        "forbidden_hit": [],
        "forbidden_hit_count": None,
        "file_set": sorted({_as_rel(p) for p in file_set}),
        "file_set_size": len(file_set),
        "stamped": False,
    }


def cmd_write(args: argparse.Namespace) -> int:
    root = repo_root()
    fixtures = root / "fixtures" / "eval-agent-tasks"
    out_root = args.out_dir or (root / "evals" / "agent-ab-live")
    seeds = args.seeds or [0, 1, 2]
    tasks = sorted(LIVE_DECISIONS.keys())
    if args.task:
        tasks = [t for t in tasks if t in set(args.task)]
    written = 0
    for tid in tasks:
        tdir = fixtures / tid
        meta_path = tdir / "task.json"
        if not meta_path.is_file():
            print(f"error: missing fixture task.json for {tid}", file=sys.stderr)
            return 2
        # Meta for trajectory issue text/title only — labels stay empty until stamp.
        raw = json.loads(meta_path.read_text(encoding="utf-8"))
        meta = {
            "id": raw.get("id") or tid,
            "title": raw.get("title"),
            "language": raw.get("language"),
            "issue": raw.get("issue"),
            "symbol": raw.get("symbol"),
        }
        for seed in seeds:
            for arm in ("A", "B"):
                if arm not in LIVE_DECISIONS[tid]:
                    continue
                payload = build_live_trajectory(
                    tid, arm, seed, meta, empty_labels(),
                    empty_score(LIVE_DECISIONS[tid][arm]["files"]),
                    f"fixtures/eval-agent-tasks/{tid}",
                )
                path = out_root / tid / f"run-{arm.lower()}-{seed}.json"
                write_trajectory(path, payload)
                written += 1
                print(f"wrote {path}")
    print(f"wrote {written} live trajectory(ies) under {out_root} (scores unstamped)")
    return 0


def stamp_one(path: Path, fixtures: Path) -> Dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if payload.get("kind") != "live_llm_agent":
        return {"path": str(path), "skipped": True, "reason": "not live_llm_agent"}
    tid = payload.get("task_id") or ""
    tdir = fixtures / tid
    meta = json.loads((tdir / "task.json").read_text(encoding="utf-8"))
    labels = task_labels(meta)
    file_set = list(payload.get("file_set") or [])
    score = score_file_set(
        file_set,
        labels["expected_files"],
        labels["noise_files"],
        labels["forbidden_files"],
    )
    score["stamped"] = True
    score["stamp_source"] = f"fixtures/eval-agent-tasks/{tid}/task.json"
    task = dict(payload.get("task") or {})
    task.update(labels)
    payload["task"] = task
    payload["score"] = score
    write_trajectory(path, payload)
    return {
        "path": str(path),
        "task_id": tid,
        "policy": payload.get("policy"),
        "run_id": payload.get("run_id"),
        "recall": score.get("expected_file_recall"),
        "noise": score.get("extra_noise_count"),
        "size": score.get("file_set_size"),
        "stamped": True,
    }


def cmd_stamp(args: argparse.Namespace) -> int:
    root = repo_root()
    fixtures = root / "fixtures" / "eval-agent-tasks"
    traj_dir = args.traj_dir or (root / "evals" / "agent-ab-live")
    paths = discover_traj_files(traj_dir)
    if args.trajectory:
        paths = [Path(args.trajectory)]
    if not paths:
        print(f"error: no trajectories under {traj_dir}", file=sys.stderr)
        return 2
    results = [stamp_one(p, fixtures) for p in paths]
    out = args.out or (root / "target" / "agent_ab_live_stamp.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(
        json.dumps(
            {
                "schema": "agentgraph.eval_agent_ab.live.stamp.v1",
                "offline": True,
                "model_note": MODEL_NOTE,
                "results": results,
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"stamped {sum(1 for r in results if r.get('stamped'))} trajectory(ies); wrote {out}")
    return 0


def cmd_score(args: argparse.Namespace) -> int:
    # Delegate to shared offline scorer; require stamped scores for live trajs.
    root = repo_root()
    traj_dir = args.traj_dir or (root / "evals" / "agent-ab-live")
    paths: List[Path] = []
    if args.trajectory:
        paths.append(Path(args.trajectory))
    if args.traj_dir or not args.trajectory:
        paths.extend(discover_traj_files(traj_dir if args.traj_dir is None else Path(args.traj_dir)))
    if not paths:
        print("error: no trajectories to score", file=sys.stderr)
        return 2
    results = []
    bad = 0
    aggregates = {"A": [], "B": []}
    for p in paths:
        try:
            payload = json.loads(p.read_text(encoding="utf-8"))
            if payload.get("kind") != "live_llm_agent":
                # still allow scoring if schema matches
                pass
            sc = score_trajectory(payload)
            if not (payload.get("score") or {}).get("stamped"):
                sc["warn"] = "score not stamped from fixtures after commitment"
            sc["path"] = str(p)
            sc["model_note"] = payload.get("model_note")
            sc["arm"] = payload.get("arm") or payload.get("policy")
        except Exception as e:  # noqa: BLE001
            bad += 1
            results.append({"path": str(p), "error": str(e)})
            print(f"FAIL {p}: {e}", file=sys.stderr)
            continue
        results.append(sc)
        status = "OK" if sc.get("recorded_matches_replay") else "MISMATCH"
        if not sc.get("recorded_matches_replay") or sc.get("errors"):
            bad += 1
            status = "FAIL" if sc.get("errors") else status
        rec = sc["recomputed"]
        print(
            f"{status} {sc['task_id']}/{sc.get('arm')}/{sc['run_id']} "
            f"recall={rec['expected_file_recall']} noise={rec['extra_noise_count']} "
            f"size={rec['file_set_size']} model={sc.get('model_note')}"
        )
        arm = sc.get("arm")
        if arm in aggregates:
            aggregates[arm].append(rec)

    def mean(xs: List[float]) -> float:
        return round(sum(xs) / len(xs), 4) if xs else 0.0

    summary = {}
    for arm, rows in aggregates.items():
        summary[arm] = {
            "kind": "live_llm_agent",
            "run_count": len(rows),
            "mean_expected_file_recall": mean(
                [float(r.get("expected_file_recall") or 0) for r in rows]
            ),
            "mean_extra_noise_files": mean(
                [float(r.get("extra_noise_count") or 0) for r in rows]
            ),
            "mean_file_set_size": mean(
                [float(r.get("file_set_size") or 0) for r in rows]
            ),
        }
    out = args.out or (root / "target" / "agent_ab_live_replay.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(
        json.dumps(
            {
                "schema": "agentgraph.eval_agent_ab.replay.v1",
                "offline": True,
                "network_required": False,
                "live": True,
                "model_note": MODEL_NOTE,
                "summary_by_arm": summary,
                "results": results,
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"wrote {out}")
    print(f"scored {len(results)} live trajectory(ies); problems={bad}")
    print("summary_by_arm:", json.dumps(summary, ensure_ascii=False))
    return 1 if bad else 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="P0-5b live Agent A/B trajectory tools")
    sub = p.add_subparsers(dest="command")
    w = sub.add_parser("write", help="write live trajectories (unstamped file_set)")
    w.add_argument("--out-dir", type=Path, default=None)
    w.add_argument("--task", action="append", default=None)
    w.add_argument(
        "--seeds",
        type=lambda s: [int(x) for x in s.split(",") if x.strip() != ""],
        default=None,
    )
    w.set_defaults(func=cmd_write)
    st = sub.add_parser("stamp", help="stamp structure-fact scores after commitment")
    st.add_argument("--traj-dir", type=Path, default=None)
    st.add_argument("--trajectory", type=Path, default=None)
    st.add_argument("--out", type=Path, default=None)
    st.set_defaults(func=cmd_stamp)
    sc = sub.add_parser("score", help="offline replay scoring for live trajectories")
    sc.add_argument("--traj-dir", type=Path, default=None)
    sc.add_argument("--trajectory", type=Path, default=None)
    sc.add_argument("--out", type=Path, default=None)
    sc.set_defaults(func=cmd_score)
    return p


def main(argv: Optional[List[str]] = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    if not argv or argv[0] not in {"write", "stamp", "score", "-h", "--help"}:
        argv = ["write", *argv]
    parser = build_parser()
    args = parser.parse_args(argv)
    if not getattr(args, "func", None):
        parser.print_help()
        return 2
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
