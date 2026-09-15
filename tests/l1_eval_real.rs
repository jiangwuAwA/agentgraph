//! L1 eval on framework-idiom multi-file corpus (PLAN §3.5 “真实框架” stand-in).
//!
//! These are multi-module NestJS/Inversify / FastAPI / Gin-like trees, not a
//! single vendored monorepo. Numbers still demonstrate L0→L1 lift + noise
//! proxy; full production-repo eval remains a product follow-up.

use agentgraph::index::extract::extract_file;
use agentgraph::model::{Confidence, ConfidenceFilter, Language};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/eval-l1-real")
}

#[derive(Debug, Deserialize)]
struct GoldenEdge {
    symbol: String,
    site_file: String,
    #[allow(dead_code)]
    site_contains: String,
    #[allow(dead_code)]
    kind: String,
    #[serde(default)]
    min_confidence: String,
}

#[derive(Debug, Deserialize)]
struct Golden {
    corpora: std::collections::BTreeMap<String, Vec<GoldenEdge>>,
}

fn lang_for(p: &str) -> Language {
    Language::from_path(p).unwrap_or(Language::TypeScript)
}

fn conf_rank(c: &str) -> u8 {
    match c {
        "exact" => 0,
        "heuristic" => 1,
        "dynamic_candidate" => 2,
        _ => 3,
    }
}

fn edge_found(
    refs: &[(String, Confidence, Option<String>)],
    g: &GoldenEdge,
    filter: ConfidenceFilter,
) -> bool {
    let min = conf_rank(&g.min_confidence);
    refs.iter().any(|(name, conf, sn)| {
        if !conf.included_in(filter) || conf_rank(conf.as_str()) < min {
            return false;
        }
        let name_ok = name == &g.symbol
            || g.symbol.ends_with(&format!(".{name}"))
            || g.symbol.starts_with(name.as_str())
            || name.contains(&g.symbol)
            || g.symbol.contains(name.as_str());
        if name_ok {
            return true;
        }
        sn.as_ref()
            .map(|s| s.contains(&g.symbol) || g.site_contains.contains(name.as_str()))
            .unwrap_or(false)
    })
}

fn corpus_refs(base: &Path, files: &[String]) -> Vec<(String, Confidence, Option<String>)> {
    let mut out = Vec::new();
    for f in files {
        let src = std::fs::read_to_string(base.join(f)).expect("read");
        let known = HashSet::from([f.clone()]);
        let parsed = extract_file(&src, lang_for(f), f, &known).expect("extract");
        for r in parsed.references {
            out.push((r.name, r.confidence, r.evidence.map(|e| e.snippet)));
        }
    }
    out
}

fn eval_all() -> Vec<(String, usize, usize, usize, usize, usize)> {
    let text = std::fs::read_to_string(root().join("golden.json")).unwrap();
    let golden: Golden = serde_json::from_str(&text).unwrap();
    let mut rows = Vec::new();
    for (name, edges) in &golden.corpora {
        let files: Vec<String> = edges
            .iter()
            .map(|e| e.site_file.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        // Also include every .ts/.py/.go under corpus dir for noise count.
        let base = root().join(name);
        let mut all_files = files.clone();
        if let Ok(walk) = std::fs::read_dir(&base) {
            for e in walk.flatten() {
                let p = e.path();
                if p.is_file() {
                    if let Some(fn_) = p.file_name().and_then(|n| n.to_str()) {
                        all_files.push(fn_.to_string());
                    }
                } else if p.is_dir() {
                    collect_rel(&p, &base, &mut all_files);
                }
            }
        }
        all_files.sort();
        all_files.dedup();
        let refs = corpus_refs(&base, &all_files);
        let mut l0 = 0;
        let mut l1 = 0;
        for g in edges {
            if edge_found(&refs, g, ConfidenceFilter::ExactOnly) {
                l0 += 1;
            }
            let f = if g.min_confidence == "dynamic_candidate" {
                ConfidenceFilter::IncludeDynamic
            } else {
                ConfidenceFilter::Default
            };
            if edge_found(&refs, g, f) {
                l1 += 1;
            }
        }
        let heur = refs
            .iter()
            .filter(|(_, c, _)| *c == Confidence::Heuristic)
            .count();
        let matched = refs
            .iter()
            .filter(|(name, c, sn)| {
                *c == Confidence::Heuristic
                    && edges.iter().any(|g| {
                        g.symbol.contains(name.as_str())
                            || name.contains(&g.symbol)
                            || sn.as_ref().map(|s| s.contains(&g.symbol)).unwrap_or(false)
                    })
            })
            .count();
        rows.push((name.clone(), edges.len(), l0, l1, heur, matched));
    }
    rows
}

fn collect_rel(dir: &Path, base: &Path, out: &mut Vec<String>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_rel(&p, base, out);
            } else if let Ok(rel) = p.strip_prefix(base) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

#[test]
fn framework_corpus_l1_lift_meets_m2() {
    let rows = eval_all();
    assert!(!rows.is_empty());
    let mut tg = 0;
    let mut t0 = 0;
    let mut t1 = 0;
    let mut th = 0;
    let mut tm = 0;
    for (name, g, l0, l1, heur, matched) in &rows {
        eprintln!("{name}: golden={g} L0={l0} L1={l1} heur={heur} matched={matched}");
        tg += g;
        t0 += l0;
        t1 += l1;
        th += heur;
        tm += matched;
    }
    let l0r = t0 as f64 / tg as f64;
    let l1r = t1 as f64 / tg as f64;
    let lift = if l0r > 0.0 { (l1r - l0r) / l0r } else { 1.0 };
    assert!(
        l1r >= l0r && (lift >= 0.15 || t0 == 0),
        "M2 lift: l0={t0}/{tg} l1={t1}/{tg} lift={lift:.2}"
    );
    assert!(th > 0, "must emit heuristic edges");
    let noise = 1.0 - (tm as f64 / th as f64);
    assert!(
        noise <= 0.30,
        "noise proxy {noise:.2} > 0.30 (matched {tm}/{th})"
    );
}
