//! P1-3 Golden Agent Suites — release gate for recipe/window/honesty key shapes.
//!
//! Loads `fixtures/eval-agent-goldens/goldens.json`, indexes public fixtures,
//! runs CLI recipes (`blast-radius` / `who-calls` / `callers`), and asserts
//! **stable keys** plus flexible recommendation checks (contains/regex —
//! never full-string equality).
//!
//! Non-claims (fixtures/eval-agent-goldens/README.md + docs/agent-goldens.md):
//! public synthetic fixtures only; window=sound is the ast_modeled engineering
//! S gate; note is never a complete runtime graph / zero-miss / ecosystem sound.

use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod common;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixtures_dir() -> PathBuf {
    repo_root().join("fixtures").join("eval-agent-goldens")
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentgraph")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn run_in(root: &Path, args: &[String]) -> Output {
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_in_str(root, &refs)
}

fn run_in_str(root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run agentgraph")
}

fn load_goldens() -> Value {
    let p = fixtures_dir().join("goldens.json");
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!("cannot read {}: {e}", p.display());
    });
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!("invalid goldens.json: {e}");
    })
}

fn as_str(v: &Value) -> Option<&str> {
    v.as_str()
}

fn get_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}

fn check_stable_keys(payload: &Value, keys: &[&str], ctx: &str) {
    for key in keys {
        assert!(
            payload.get(key).is_some(),
            "{ctx}: stable key missing: {key} in {payload}"
        );
    }
}

fn check_note(payload: &Value, must_contain: &[String], must_not: &[String], ctx: &str) {
    let note = get_str(payload, "note").unwrap_or_default();
    for s in must_contain {
        assert!(
            note.contains(s.as_str()),
            "{ctx}: note must contain '{s}': {note}"
        );
    }
    let lower = note.to_lowercase();
    for s in must_not {
        assert!(
            !lower.contains(&s.to_lowercase()),
            "{ctx}: note must not oversell '{s}': {note}"
        );
    }
}

fn contains_all(hay: &str, needles: &[Value], ctx: &str) {
    for n in needles {
        let s = as_str(n).unwrap_or_default();
        assert!(hay.contains(s), "{ctx}: expected substring '{s}' in: {hay}");
    }
}

fn not_contains_any(hay: &str, needles: &[Value], ctx: &str) {
    for n in needles {
        let s = as_str(n).unwrap_or_default();
        if s.is_empty() {
            continue;
        }
        // Case-sensitive for command-like locks; oversell checks use lowercase.
        assert!(
            !hay.contains(s),
            "{ctx}: forbidden substring '{s}' in: {hay}"
        );
    }
}

fn regex_any(hay: &str, patterns: &[Value], ctx: &str) {
    for p in patterns {
        let pat = as_str(p).unwrap_or_default();
        let re = regex_lite(pat);
        assert!(
            re.is_match(hay),
            "{ctx}: recommendation must match /{pat}/ : {hay}"
        );
    }
}

/// Tiny subset of regex: case-insensitive `a|b|c` alternation of literals.
/// Avoids adding a regex crate dependency just for goldens contains checks.
fn regex_lite(pattern: &str) -> RegexLite {
    let lower = pattern.to_lowercase();
    let body = lower.strip_prefix("(?i)").unwrap_or(&lower);
    let alts: Vec<String> = body
        .split('|')
        .map(|s| {
            s.trim()
                .trim_start_matches('(')
                .trim_end_matches(')')
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();
    RegexLite { alts }
}

struct RegexLite {
    alts: Vec<String>,
}

impl RegexLite {
    fn is_match(&self, hay: &str) -> bool {
        let lower = hay.to_lowercase();
        self.alts.iter().any(|a| lower.contains(a.as_str()))
    }
}

/// Collect `edge_role` strings from a row array.
fn roles_of(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .filter_map(|r| {
            r.get("edge_role")
                .and_then(|e| e.as_str())
                .map(|s| s.to_string())
        })
        .collect()
}

/// Extract the primary row array from a callers payload (array or object.callers).
fn callers_rows(payload: &Value) -> Vec<Value> {
    match payload {
        Value::Array(rows) => rows.clone(),
        Value::Object(obj) => obj
            .get("callers")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => vec![],
    }
}

fn implementors_rows(payload: &Value) -> Vec<Value> {
    match payload {
        Value::Object(obj) => obj
            .get("implementors")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => vec![],
    }
}

fn implementor_count(payload: &Value) -> u64 {
    payload
        .get("implementor_count")
        .and_then(|n| n.as_u64())
        .unwrap_or(0)
}

fn index_classic(root: &Path) {
    let out = run_in_str(root, &["index", "--force"]);
    assert!(out.status.success(), "index failed: {}", stderr(&out));
}

fn index_workspace(root: &Path, manifest: &str) {
    let abs = root.join(manifest);
    let abs = abs.to_string_lossy().into_owned();
    let out = run_in_str(root, &["index", "--workspace", abs.as_str(), "--force"]);
    assert!(
        out.status.success(),
        "workspace index failed: {}",
        stderr(&out)
    );
}

fn copy_fixture(name: &str) -> PathBuf {
    let src = fixtures_dir().join(name);
    assert!(src.is_dir(), "missing golden fixture: {}", src.display());
    common::copy_fixture_to_temp(&src, &format!("agentgraph-goldens-{name}"))
}

/// Run one golden case. Indexes are reused via a session map keyed by
/// `(fixture, index key)` so scoped-root cases share a dirty multi-root DB.
fn run_case(case: &Value, goldens: &Value, index_cache: &mut HashMap<String, PathBuf>) {
    let id = case
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();
    let fixture = case
        .get("fixture")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("{id}: fixture required"));
    let index = case.get("index").cloned().unwrap_or(json_obj());
    let query = case
        .get("query")
        .cloned()
        .unwrap_or_else(|| panic!("{id}: query required"));
    let expect = case
        .get("expect")
        .cloned()
        .unwrap_or_else(|| panic!("{id}: expect required"));

    let mode = index
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("classic");
    let reuse = index.get("reuse_index_of").and_then(|v| v.as_str());
    let index_key = match reuse {
        Some(prev) => format!("{fixture}::{prev}"),
        None => {
            let manifest = index
                .get("workspace_manifest")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!("{fixture}::{mode}::{manifest}")
        }
    };

    if !index_cache.contains_key(&index_key) {
        let root = copy_fixture(fixture);
        if mode == "workspace" {
            let manifest = index
                .get("workspace_manifest")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("{id}: workspace_manifest required"));
            index_workspace(&root, manifest);
        } else {
            index_classic(&root);
        }
        index_cache.insert(index_key.clone(), root);
    }
    let root = index_cache.get(&index_key).unwrap().clone();

    let tool = query
        .get("tool")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("{id}: query.tool required"));
    let symbol = query
        .get("symbol")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("{id}: query.symbol required"));
    let mut args: Vec<String> = Vec::new();
    match tool {
        "blast_radius" => args.push("blast-radius".into()),
        "who_calls" => args.push("who-calls".into()),
        "callers" => args.push("callers".into()),
        other => panic!("{id}: unknown tool {other}"),
    }
    args.push(symbol.into());
    if let Some(extra) = query.get("args").and_then(|v| v.as_array()) {
        for a in extra {
            let mut s = as_str(a).unwrap_or_default().to_string();
            // Resolve workspace manifest paths against the temp fixture root
            // (CLI resolves --workspace relative to process cwd, not --root).
            if s.ends_with("workspace.json") {
                let abs = root.join(&s);
                s = abs.to_string_lossy().into_owned();
            }
            args.push(s);
        }
    }

    let out = run_in(&root, &args);
    assert!(
        out.status.success(),
        "{id}: command failed\nargs={args:?}\nstdout={}\nstderr={}",
        stdout(&out),
        stderr(&out)
    );
    let text = stdout(&out);
    let payload: Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{id}: JSON parse failed: {e}\n{text}"));

    let stable_blast = goldens
        .get("stable_keys_blast_radius")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let stable_who = goldens
        .get("stable_keys_who_calls")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let note_must = goldens
        .get("note_must_contain")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|x| as_str(x).map(|s| s.to_string()))
        .collect::<Vec<_>>();
    let note_must_not = goldens
        .get("note_must_not_contain")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|x| as_str(x).map(|s| s.to_string()))
        .collect::<Vec<_>>();
    let allowed_roles: Vec<String> = goldens
        .get("allowed_edge_roles")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|x| as_str(x).map(|s| s.to_string()))
        .collect();

    // Stable keys per tool.
    if tool == "blast_radius" {
        let keys: Vec<&str> = stable_blast.iter().filter_map(|v| as_str(v)).collect();
        check_stable_keys(&payload, &keys, &id);
        check_note(&payload, &note_must, &note_must_not, &id);
    }
    if tool == "who_calls" {
        let keys: Vec<&str> = stable_who.iter().filter_map(|v| as_str(v)).collect();
        check_stable_keys(&payload, &keys, &id);
        check_note(&payload, &note_must, &note_must_not, &id);
    }

    // Scalar expectations.
    if let Some(w) = expect.get("window").and_then(|v| v.as_str()) {
        assert_eq!(
            get_str(&payload, "window"),
            Some(w),
            "{id}: window mismatch: {payload}"
        );
    }
    if let Some(arr) = expect.get("window_not_in").and_then(|v| v.as_array()) {
        let w = get_str(&payload, "window").unwrap_or("");
        for bad in arr {
            let bad = as_str(bad).unwrap_or_default();
            assert_ne!(w, bad, "{id}: window must not be '{bad}': {payload}");
        }
    }
    if let Some(sok) = expect.get("subset_ok").and_then(|v| v.as_bool()) {
        assert_eq!(
            payload.get("subset_ok").and_then(|v| v.as_bool()),
            Some(sok),
            "{id}: subset_ok mismatch: {payload}"
        );
    }
    if let Some(arr) = expect.get("promise_tier_in").and_then(|v| v.as_array()) {
        let pt = get_str(&payload, "promise_tier").unwrap_or("");
        let ok = arr.iter().any(|x| as_str(x) == Some(pt));
        assert!(ok, "{id}: promise_tier '{pt}' not in {arr:?}: {payload}");
    }
    if let Some(t) = expect.get("tool").and_then(|v| v.as_str()) {
        assert_eq!(get_str(&payload, "tool"), Some(t), "{id}: tool: {payload}");
    }
    if let Some(n) = expect.get("noisy").and_then(|v| v.as_bool()) {
        assert_eq!(
            payload.get("noisy").and_then(|v| v.as_bool()),
            Some(n),
            "{id}: noisy mismatch: {payload}"
        );
    }
    if let Some(hf) = expect.get("high_freq_name").and_then(|v| v.as_bool()) {
        assert_eq!(
            payload.get("high_freq_name").and_then(|v| v.as_bool()),
            Some(hf),
            "{id}: high_freq_name mismatch: {payload}"
        );
    }
    if let Some(im) = expect.get("include_macro").and_then(|v| v.as_bool()) {
        assert_eq!(
            payload.get("include_macro").and_then(|v| v.as_bool()),
            Some(im),
            "{id}: include_macro: {payload}"
        );
    }

    // Recommendation flexibility.
    if expect
        .get("recommendation_present")
        .and_then(|v| v.as_bool())
        == Some(true)
        || expect.get("recommendation_contains").is_some()
        || expect.get("recommendation_not_contains").is_some()
        || expect.get("recommendation_regex_any").is_some()
    {
        let rec = get_str(&payload, "recommendation").unwrap_or_default();
        assert!(
            !rec.is_empty(),
            "{id}: recommendation key must be present and non-empty: {payload}"
        );
        if let Some(arr) = expect
            .get("recommendation_contains")
            .and_then(|v| v.as_array())
        {
            contains_all(rec, arr, &format!("{id}: recommendation"));
        }
        if let Some(arr) = expect
            .get("recommendation_not_contains")
            .and_then(|v| v.as_array())
        {
            not_contains_any(rec, arr, &format!("{id}: recommendation"));
        }
        if let Some(arr) = expect
            .get("recommendation_regex_any")
            .and_then(|v| v.as_array())
        {
            regex_any(rec, arr, &format!("{id}: recommendation"));
        }
        if expect
            .get("if_recommendation_mentions_recall_then_must_prohibit")
            .and_then(|v| v.as_bool())
            == Some(true)
            && rec.contains("--recall")
        {
            let lower = rec.to_lowercase();
            assert!(
                lower.contains("do not")
                    || lower.contains("never")
                    || lower.contains("don't")
                    || lower.contains("do not pass")
                    || rec.contains("do NOT"),
                "{id}: if --recall is mentioned it must be prohibited: {rec}"
            );
        }
    }

    // blast-specific honesty / scoped guidance.
    if tool == "blast_radius" {
        if expect
            .get("sound_candidates_is_array")
            .and_then(|v| v.as_bool())
            == Some(true)
        {
            assert!(
                payload
                    .get("sound_candidates")
                    .and_then(|v| v.as_array())
                    .is_some(),
                "{id}: sound_candidates must be an array: {payload}"
            );
        }
        if let Some(first) = expect
            .get("sound_candidates_first_root_id")
            .and_then(|v| v.as_str())
        {
            let cands = payload
                .get("sound_candidates")
                .and_then(|v| v.as_array())
                .unwrap_or_else(|| panic!("{id}: sound_candidates required"));
            let first_id = cands
                .first()
                .and_then(|c| c.get("root_id"))
                .and_then(|r| r.as_str())
                .unwrap_or("");
            assert_eq!(
                first_id, first,
                "{id}: sound_candidates eligible-first mismatch: {cands:?}"
            );
        }
        if let Some(root_id) = expect
            .get("sound_candidates_has_eligible")
            .and_then(|v| v.as_str())
        {
            let cands = payload
                .get("sound_candidates")
                .and_then(|v| v.as_array())
                .unwrap_or_else(|| panic!("{id}: sound_candidates required"));
            let hit = cands.iter().find(|c| {
                c.get("root_id").and_then(|r| r.as_str()) == Some(root_id)
                    && c.get("sound_eligible").and_then(|b| b.as_bool()) == Some(true)
            });
            assert!(
                hit.is_some(),
                "{id}: expected eligible candidate root_id={root_id}: {cands:?}"
            );
        }
        if let Some(arr) = expect
            .get("example_command_contains")
            .and_then(|v| v.as_array())
        {
            let cmd = payload
                .get("example_command")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            assert!(
                !cmd.is_empty(),
                "{id}: example_command required for scoped guidance: {payload}"
            );
            contains_all(cmd, arr, &format!("{id}: example_command"));
        }
        if expect.get("nodes_is_array").and_then(|v| v.as_bool()) == Some(true) {
            assert!(
                payload.get("nodes").and_then(|v| v.as_array()).is_some(),
                "{id}: nodes must be an array: {payload}"
            );
        }
        // Blast nodes, when present, must use allowed edge_role values.
        if let Some(nodes) = payload.get("nodes").and_then(|v| v.as_array()) {
            for n in nodes {
                if let Some(role) = n.get("edge_role").and_then(|r| r.as_str()) {
                    assert!(
                        allowed_roles.iter().any(|a| a == role),
                        "{id}: unexpected blast node edge_role '{role}': {n}"
                    );
                }
            }
        }
    }

    // who_calls / callers row-role locks.
    if tool == "who_calls" {
        if let Some(min) = expect.get("implementor_count_min").and_then(|v| v.as_u64()) {
            let c = implementor_count(&payload);
            assert!(
                c >= min,
                "{id}: implementor_count {c} < min {min}: {payload}"
            );
        }
        let callers = callers_rows(&payload);
        let imps = implementors_rows(&payload);
        if let Some(arr) = expect
            .get("callers_edge_role_not")
            .and_then(|v| v.as_array())
        {
            let roles = roles_of(&callers);
            for bad in arr {
                let bad = as_str(bad).unwrap_or_default();
                assert!(
                    !roles.iter().any(|r| r == bad),
                    "{id}: callers must not contain edge_role '{bad}': {roles:?}"
                );
            }
        }
        if let Some(arr) = expect
            .get("callers_edge_role_any")
            .and_then(|v| v.as_array())
        {
            let roles = roles_of(&callers);
            let ok = arr
                .iter()
                .filter_map(|x| as_str(x))
                .any(|want| roles.iter().any(|r| r == want));
            assert!(ok, "{id}: callers must contain one of {arr:?}: {roles:?}");
        }
        if let Some(arr) = expect
            .get("implementors_edge_role_any")
            .and_then(|v| v.as_array())
        {
            let roles = roles_of(&imps);
            let ok = arr
                .iter()
                .filter_map(|x| as_str(x))
                .any(|want| roles.iter().any(|r| r == want));
            assert!(
                ok,
                "{id}: implementors must contain one of {arr:?}: {roles:?}"
            );
        }
        if let Some(rule) = expect.get("implementors_truncated_if_count_gt") {
            let threshold = rule.get("threshold").and_then(|v| v.as_u64()).unwrap_or(0);
            let field = rule
                .get("field")
                .and_then(|v| v.as_str())
                .unwrap_or("implementors_truncated");
            let want = rule.get("value").and_then(|v| v.as_bool()).unwrap_or(true);
            let c = implementor_count(&payload);
            if c > threshold {
                assert_eq!(
                    payload.get(field).and_then(|v| v.as_bool()),
                    Some(want),
                    "{id}: {field} should be {want} when count {c} > {threshold}: {payload}"
                );
            }
        }
        if let Some(arr) = expect
            .get("if_implementors_present_edge_role_any")
            .and_then(|v| v.as_array())
        {
            if !imps.is_empty() {
                let roles = roles_of(&imps);
                let ok = arr
                    .iter()
                    .filter_map(|x| as_str(x))
                    .any(|want| roles.iter().any(|r| r == want));
                assert!(
                    ok,
                    "{id}: non-empty implementors must contain one of {arr:?}: {roles:?}"
                );
            }
        }
    }

    if tool == "callers" {
        // callers CLI: array (no implementors) or object {callers, implementors}
        let all_rows: Vec<Value> = match &payload {
            Value::Array(rows) => rows.clone(),
            Value::Object(obj) => {
                let mut rows = obj
                    .get("callers")
                    .and_then(|c| c.as_array())
                    .cloned()
                    .unwrap_or_default();
                if let Some(imps) = obj.get("implementors").and_then(|c| c.as_array()) {
                    rows.extend(imps.iter().cloned());
                }
                rows
            }
            _ => vec![],
        };
        if let Some(arr) = expect
            .get("any_row_edge_role_any")
            .and_then(|v| v.as_array())
        {
            let roles = roles_of(&all_rows);
            let ok = arr
                .iter()
                .filter_map(|x| as_str(x))
                .any(|want| roles.iter().any(|r| r == want));
            assert!(
                ok,
                "{id}: some row must have edge_role in {arr:?}: roles={roles:?} payload={payload}"
            );
        }
        if let Some(arr) = expect
            .get("all_row_edge_roles_in")
            .and_then(|v| v.as_array())
        {
            let roles = roles_of(&all_rows);
            for role in &roles {
                let ok = arr.iter().filter_map(|x| as_str(x)).any(|a| a == role);
                assert!(
                    ok,
                    "{id}: edge_role '{role}' not in allowed {arr:?}: {roles:?}"
                );
            }
        }
    }

    eprintln!("agent_goldens PASS {id}");
}

fn json_obj() -> Value {
    serde_json::json!({})
}

// ---------------------------------------------------------------------------
// Cases from goldens.json
// ---------------------------------------------------------------------------

#[test]
fn agent_goldens_fixtures_and_expectations() {
    let dir = fixtures_dir();
    assert!(
        dir.is_dir(),
        "missing fixtures/eval-agent-goldens (P1-3 release gate)"
    );
    assert!(dir.join("goldens.json").is_file(), "missing goldens.json");
    assert!(
        dir.join("README.md").is_file(),
        "golden fixtures README required (honesty non-claims)"
    );

    let goldens = load_goldens();
    let schema = goldens.get("schema").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(schema, "agentgraph.eval_agent_goldens.v1");

    let cases = goldens
        .get("cases")
        .and_then(|v| v.as_array())
        .expect("cases[] required");
    assert!(
        cases.len() >= 5,
        "P1-3 goldens require multiple cases locking window/edge_role/recommendation/note/who_calls"
    );

    // Required themes must be represented across cases.
    let blob = goldens.to_string();
    for theme in [
        "window",
        "edge_role",
        "recommendation",
        "note",
        "high_freq",
        "implementor",
        "registration",
        "workspace",
    ] {
        assert!(
            blob.contains(theme),
            "goldens.json must cover theme '{theme}'"
        );
    }

    let mut index_cache: HashMap<String, PathBuf> = HashMap::new();
    for case in cases {
        run_case(case, &goldens, &mut index_cache);
    }
}

// ---------------------------------------------------------------------------
// Recipe-builder unit locks (flexible recommendation — same contract as goldens)
// ---------------------------------------------------------------------------

#[test]
fn unit_recipe_builders_lock_stable_keys_and_note() {
    use agentgraph::model::{Confidence, EdgeKind, Evidence, ReferenceRecord};
    use agentgraph::query::recipes::{
        build_blast_radius_payload, build_who_calls_payload, decide_blast_window,
        BlastRadiusPayloadInput, RECIPE_NOTE,
    };

    assert!(RECIPE_NOTE.contains("not a complete runtime graph"));

    let d = decide_blast_window(false, Some("legacy"));
    let v = build_blast_radius_payload(BlastRadiusPayloadInput {
        symbol: "export_rows".into(),
        depth: 3,
        limit: 50,
        nodes: vec![],
        window: d,
        promise_tier: "disabled".into(),
        languages: vec!["rust".into()],
        include_macro: false,
        include_macro_reason: None,
        stale: None,
        scoped_sound: Default::default(),
    });
    for key in [
        "window",
        "promise_tier",
        "subset_ok",
        "recommendation",
        "note",
        "sound_candidates",
    ] {
        assert!(v.get(key).is_some(), "stable key missing: {key} in {v}");
    }
    assert_eq!(v["window"], "default");
    let rec = v["recommendation"].as_str().unwrap();
    assert!(rec.contains("sound") || rec.to_lowercase().contains("disabled"));
    assert!(v["note"]
        .as_str()
        .unwrap()
        .contains("not a complete runtime graph"));

    fn row(rule: Option<&str>) -> ReferenceRecord {
        ReferenceRecord {
            name: "area".into(),
            kind: EdgeKind::Call,
            path: "src/a.rs".into(),
            line: 1,
            enclosing: None,
            module: None,
            resolved: None,
            qualifier: None,
            confidence: Confidence::Heuristic,
            evidence: rule.map(|r| Evidence {
                rule_id: r.into(),
                snippet: String::new(),
            }),
            root_id: String::new(),
        }
    }
    let hits = vec![row(Some("rs.di.impl_trait")), row(None)];
    let w = build_who_calls_payload("fmt", false, 50, &hits, true, "ast_modeled");
    for key in [
        "tool",
        "symbol",
        "noisy",
        "window",
        "subset_ok",
        "promise_tier",
        "high_freq_name",
        "callers",
        "implementors",
        "implementor_count",
        "recommendation",
        "note",
    ] {
        assert!(w.get(key).is_some(), "who_calls stable key missing: {key}");
    }
    assert_eq!(w["high_freq_name"], true);
    assert_eq!(w["implementor_count"], 1);
    let wrec = w["recommendation"].as_str().unwrap();
    assert!(wrec.contains("high_freq_name="));
}

#[test]
fn docs_agent_goldens_exist_and_pointer() {
    let root = repo_root();
    let doc = root.join("docs").join("agent-goldens.md");
    assert!(
        doc.is_file(),
        "docs/agent-goldens.md required (P1-3 pointer)"
    );
    let text = std::fs::read_to_string(&doc).unwrap();
    assert!(
        text.contains("agent_goldens") || text.contains("eval-agent-goldens"),
        "agent-goldens.md must point at the golden suite"
    );
    assert!(
        text.contains("not a complete runtime graph") || text.contains("Non-claim"),
        "agent-goldens.md must stay honest"
    );
    // Pointer from eval-agent-tasks (or agent-recipes) keeps discovery easy.
    let tasks = std::fs::read_to_string(root.join("docs/eval-agent-tasks.md")).unwrap_or_default();
    let recipes = std::fs::read_to_string(root.join("docs/agent-recipes.md")).unwrap_or_default();
    assert!(
        tasks.contains("agent-goldens") || recipes.contains("agent-goldens"),
        "eval-agent-tasks.md or agent-recipes.md must link agent-goldens.md"
    );
}
