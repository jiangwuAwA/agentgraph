//! L1 edge model: confidence + evidence on refs (schema v2 + migration).

use agentgraph::index::extract::{extract_file, ExtractedRef};
use agentgraph::index::store::Store;
use agentgraph::model::{
    Confidence, ConfidenceFilter, EdgeKind, Evidence, Language, ReferenceRecord,
};
use std::collections::HashSet;
use std::path::PathBuf;

mod common;

fn temp_db(name: &str) -> PathBuf {
    common::temp_db(&format!("agentgraph-l1-schema-{name}"))
}

#[test]
fn l0_refs_default_to_exact_confidence() {
    let src = r#"
export function authenticate(email: string, password: string) {
  return { email, password };
}
export function loginHandler(email: string, password: string) {
  return authenticate(email, password);
}
"#;
    let known = HashSet::from(["src/auth.ts".to_string()]);
    let out = extract_file(src, Language::TypeScript, "src/auth.ts", &known).unwrap();
    let calls: Vec<_> = out
        .references
        .iter()
        .filter(|r| matches!(r.kind, EdgeKind::Call))
        .collect();
    assert!(!calls.is_empty(), "expected call refs");
    for c in calls {
        assert_eq!(c.confidence, Confidence::Exact, "L0 call must be Exact");
        assert!(c.evidence.is_none(), "L0 Exact edge has no rule evidence");
    }
}

#[test]
fn store_persists_confidence_and_evidence() {
    let db = temp_db("persist");
    let mut store = Store::open(&db).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file(
            "src/a.ts",
            "h1",
            "typescript",
            &agentgraph::index::extract::ExtractedFile {
                symbols: vec![],
                references: vec![ExtractedRef {
                    name: "UserService".to_string(),
                    kind: EdgeKind::Call,
                    line: 10,
                    enclosing: Some("bootstrap".to_string()),
                    module: None,
                    resolved: None,
                    qualifier: None,
                    confidence: Confidence::Heuristic,
                    evidence: Some(Evidence {
                        rule_id: "ts.di.register".to_string(),
                        snippet: "container.register(UserService)".to_string(),
                    }),
                }],
            },
        )
        .unwrap();
    store.commit_batch().unwrap();

    let hits = store.callers("UserService", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].confidence, Confidence::Heuristic);
    let ev = hits[0].evidence.as_ref().expect("evidence required");
    assert_eq!(ev.rule_id, "ts.di.register");
    assert!(ev.snippet.contains("container.register"));
}

#[test]
fn legacy_db_migrates_refs_to_exact() {
    // Build a v1-shaped DB without confidence columns, then open Store (migrates).
    let db = temp_db("legacy");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE files (path TEXT PRIMARY KEY, hash TEXT NOT NULL, language TEXT NOT NULL);
            CREATE TABLE symbols (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL, name TEXT NOT NULL, qualified_name TEXT NOT NULL,
                kind TEXT NOT NULL, start_line INTEGER NOT NULL, end_line INTEGER NOT NULL,
                parent TEXT, description TEXT,
                start_col INTEGER NOT NULL DEFAULT 0, end_col INTEGER NOT NULL DEFAULT 0,
                return_type TEXT
            );
            CREATE TABLE refs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL, kind TEXT NOT NULL, path TEXT NOT NULL,
                line INTEGER NOT NULL, enclosing TEXT, module TEXT, resolved TEXT,
                qualifier TEXT, resolved_symbol_id INTEGER
            );
            INSERT INTO files(path, hash, language) VALUES('src/x.ts','h','typescript');
            INSERT INTO refs(name, kind, path, line) VALUES('legacyFn','call','src/x.ts',3);
            "#,
        )
        .unwrap();
    }
    let store = Store::open(&db).unwrap();
    let hits = store.callers("legacyFn", 5).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].confidence,
        Confidence::Exact,
        "pre-L1 rows must migrate to Exact"
    );
}

#[test]
fn confidence_filter_exact_only_excludes_heuristic() {
    let db = temp_db("filter");
    let mut store = Store::open(&db).unwrap();
    store.begin_batch().unwrap();
    store
        .replace_file(
            "src/m.ts",
            "h",
            "typescript",
            &agentgraph::index::extract::ExtractedFile {
                symbols: vec![],
                references: vec![
                    ExtractedRef {
                        name: "doWork".to_string(),
                        kind: EdgeKind::Call,
                        line: 1,
                        enclosing: None,
                        module: None,
                        resolved: None,
                        qualifier: None,
                        confidence: Confidence::Exact,
                        evidence: None,
                    },
                    ExtractedRef {
                        name: "doWork".to_string(),
                        kind: EdgeKind::Call,
                        line: 2,
                        enclosing: None,
                        module: None,
                        resolved: None,
                        qualifier: None,
                        confidence: Confidence::Heuristic,
                        evidence: Some(Evidence {
                            rule_id: "ts.di.register".into(),
                            snippet: "c.register(doWork)".into(),
                        }),
                    },
                    ExtractedRef {
                        name: "doWork".to_string(),
                        kind: EdgeKind::Call,
                        line: 3,
                        enclosing: None,
                        module: None,
                        resolved: None,
                        qualifier: None,
                        confidence: Confidence::DynamicCandidate,
                        evidence: Some(Evidence {
                            rule_id: "ts.dynamic.computed".into(),
                            snippet: "obj['doWork']".into(),
                        }),
                    },
                ],
            },
        )
        .unwrap();
    store.commit_batch().unwrap();

    let exact_only = store
        .callers_filtered("doWork", 10, ConfidenceFilter::ExactOnly)
        .unwrap();
    assert_eq!(exact_only.len(), 1);
    assert_eq!(exact_only[0].confidence, Confidence::Exact);

    let default = store
        .callers_filtered("doWork", 10, ConfidenceFilter::Default)
        .unwrap();
    assert_eq!(default.len(), 2, "Default = Exact + Heuristic");

    let with_dyn = store
        .callers_filtered("doWork", 10, ConfidenceFilter::IncludeDynamic)
        .unwrap();
    assert_eq!(with_dyn.len(), 3);
}

#[test]
fn reference_record_serializes_confidence() {
    let r = ReferenceRecord {
        name: "f".into(),
        kind: EdgeKind::Call,
        path: "a.ts".into(),
        line: 1,
        enclosing: None,
        module: None,
        resolved: None,
        qualifier: None,
        confidence: Confidence::Heuristic,
        evidence: Some(Evidence {
            rule_id: "ts.di.register".into(),
            snippet: "x".into(),
        }),
        root_id: String::new(),
    };
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["confidence"], "heuristic");
    assert_eq!(v["evidence"]["rule_id"], "ts.di.register");
}
