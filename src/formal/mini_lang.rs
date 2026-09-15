//! L3 I4 — mini-language call-closure over-approx (executable formal).
//!
//! Subset S_L: direct calls + string-literal table dispatch only.
//! No eval / computed non-literal keys / reflection.
//!
//! Claim checked by `tests/l4_mini_lang.rs`:
//!   for every program in the bounded generator space,
//!   runtime call edges ⊆ static transitive call-closure (over-approx OK).

use std::collections::{BTreeMap, BTreeSet};

/// One function body: a sequence of statements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// Direct call to a named function.
    Call(String),
    /// `tbl["key"]()` — dispatch through a local table of function names.
    DispatchLit(String),
    /// `if cond { then } else { else }` — cond is a free boolean input.
    If(Vec<Stmt>, Vec<Stmt>),
    /// No-op.
    Nop,
}

/// Program = map of function name → body. Entry is `main`.
#[derive(Debug, Clone, Default)]
pub struct Program {
    pub funcs: BTreeMap<String, Vec<Stmt>>,
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, name: impl Into<String>, body: Vec<Stmt>) {
        self.funcs.insert(name.into(), body);
    }

    /// Static call edges: caller → callee for Call and DispatchLit.
    /// DispatchLit(key) may call **any** function whose name equals `key`
    /// (finite domain = string literal). Unresolved names are still edges
    /// (over-approx).
    pub fn direct_edges(&self) -> BTreeSet<(String, String)> {
        let mut edges = BTreeSet::new();
        for (caller, body) in &self.funcs {
            collect_edges(caller, body, &mut edges);
        }
        edges
    }

    /// Transitive closure over direct_edges, restricted to functions that exist
    /// as callers (callees may be external — still included as edge targets).
    pub fn static_closure(&self) -> BTreeSet<(String, String)> {
        let base = self.direct_edges();
        let mut closure = base.clone();
        // Floyd-style expansion on function names present in the program.
        let names: Vec<String> = self.funcs.keys().cloned().collect();
        loop {
            let mut grew = false;
            for a in &names {
                for b in &names {
                    for c in &names {
                        if closure.contains(&(a.clone(), b.clone()))
                            && closure.contains(&(b.clone(), c.clone()))
                            && !closure.contains(&(a.clone(), c.clone()))
                        {
                            closure.insert((a.clone(), c.clone()));
                            grew = true;
                        }
                    }
                }
            }
            if !grew {
                break;
            }
        }
        closure
    }
}

fn collect_edges(caller: &str, body: &[Stmt], out: &mut BTreeSet<(String, String)>) {
    for s in body {
        match s {
            Stmt::Call(callee) => {
                out.insert((caller.to_string(), callee.clone()));
            }
            Stmt::DispatchLit(key) => {
                out.insert((caller.to_string(), key.clone()));
            }
            Stmt::If(then_b, else_b) => {
                collect_edges(caller, then_b, out);
                collect_edges(caller, else_b, out);
            }
            Stmt::Nop => {}
        }
    }
}

/// Runtime environment: a call stack of function names; edges recorded as (from, to).
#[derive(Debug, Default, Clone)]
pub struct Runtime {
    pub edges: BTreeSet<(String, String)>,
}

/// Interpret `main` with all `if` branches explored (both arms).
/// Records every dynamic call edge `from → to`.
///
/// Fuel prevents infinite recursion on cyclic programs.
pub fn interpret(p: &Program) -> Runtime {
    let mut rt = Runtime::default();
    if p.funcs.contains_key("main") {
        run_fn(p, "main", &mut rt, 0, 64);
    }
    rt
}

fn run_fn(p: &Program, name: &str, rt: &mut Runtime, depth: usize, fuel: usize) {
    if depth > 16 || fuel == 0 {
        return;
    }
    let Some(body) = p.funcs.get(name) else {
        return;
    };
    let body = body.clone();
    run_body(p, name, &body, rt, depth, fuel);
}

fn run_body(p: &Program, caller: &str, body: &[Stmt], rt: &mut Runtime, depth: usize, fuel: usize) {
    for s in body {
        let fuel = fuel.saturating_sub(1);
        if fuel == 0 {
            return;
        }
        match s {
            Stmt::Nop => {}
            Stmt::Call(callee) => {
                rt.edges.insert((caller.to_string(), callee.clone()));
                run_fn(p, callee, rt, depth + 1, fuel);
            }
            Stmt::DispatchLit(key) => {
                rt.edges.insert((caller.to_string(), key.clone()));
                run_fn(p, key, rt, depth + 1, fuel);
            }
            Stmt::If(then_b, else_b) => {
                // Explore both branches (angelic / all-paths over-approx).
                run_body(p, caller, then_b, rt, depth, fuel);
                run_body(p, caller, else_b, rt, depth, fuel);
            }
        }
    }
}

/// I4 property: every runtime edge is in the static closure.
pub fn runtime_subset_of_static(p: &Program) -> Result<(), String> {
    let rt = interpret(p);
    let sc = p.static_closure();
    for e in &rt.edges {
        if !sc.contains(e) {
            return Err(format!(
                "runtime edge {e:?} missing from static closure {:?}",
                sc
            ));
        }
    }
    Ok(())
}
