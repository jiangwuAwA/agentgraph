//! TDD: L3 I4 — mini-language runtime calls ⊆ static call-closure.

use agentgraph::formal::mini_lang::{interpret, runtime_subset_of_static, Program, Stmt};

#[test]
fn direct_call_chain_is_contained() {
    let mut p = Program::new();
    p.add("main", vec![Stmt::Call("a".into())]);
    p.add("a", vec![Stmt::Call("b".into())]);
    p.add("b", vec![Stmt::Nop]);
    assert!(runtime_subset_of_static(&p).is_ok());
    let rt = interpret(&p);
    assert!(rt.edges.contains(&("main".into(), "a".into())));
    assert!(rt.edges.contains(&("a".into(), "b".into())));
}

#[test]
fn literal_dispatch_is_contained() {
    let mut p = Program::new();
    p.add("main", vec![Stmt::DispatchLit("handler".into())]);
    p.add("handler", vec![Stmt::Call("leaf".into())]);
    p.add("leaf", vec![]);
    assert!(runtime_subset_of_static(&p).is_ok());
    let rt = interpret(&p);
    assert!(rt.edges.contains(&("main".into(), "handler".into())));
    assert!(rt.edges.contains(&("handler".into(), "leaf".into())));
}

#[test]
fn if_both_arms_explored_and_contained() {
    let mut p = Program::new();
    p.add(
        "main",
        vec![Stmt::If(
            vec![Stmt::Call("t".into())],
            vec![Stmt::Call("e".into())],
        )],
    );
    p.add("t", vec![]);
    p.add("e", vec![]);
    assert!(runtime_subset_of_static(&p).is_ok());
    let rt = interpret(&p);
    assert!(rt.edges.contains(&("main".into(), "t".into())));
    assert!(rt.edges.contains(&("main".into(), "e".into())));
}

#[test]
fn static_closure_is_transitive() {
    let mut p = Program::new();
    p.add("main", vec![Stmt::Call("a".into())]);
    p.add("a", vec![Stmt::Call("b".into())]);
    p.add("b", vec![Stmt::Call("c".into())]);
    p.add("c", vec![]);
    let sc = p.static_closure();
    assert!(
        sc.contains(&("main".into(), "c".into())),
        "transitive: {sc:?}"
    );
    assert!(sc.contains(&("main".into(), "b".into())));
}

/// Bounded exhaustive: small programs with 3 funcs; keeps CI fast (<5s).
#[test]
fn exhaustive_bounded_programs_satisfy_i4() {
    let mut checked = 0u32;
    let alphabet: Vec<Stmt> = vec![
        Stmt::Nop,
        Stmt::Call("main".into()),
        Stmt::Call("a".into()),
        Stmt::Call("b".into()),
        Stmt::DispatchLit("a".into()),
        Stmt::DispatchLit("b".into()),
        Stmt::If(vec![Stmt::Call("a".into())], vec![Stmt::Call("b".into())]),
    ];

    let bodies_of_len = |len: usize| -> Vec<Vec<Stmt>> {
        if len == 0 {
            return vec![vec![]];
        }
        let mut out = Vec::new();
        for s in &alphabet {
            if len == 1 {
                out.push(vec![s.clone()]);
            } else {
                for s2 in &alphabet {
                    out.push(vec![s.clone(), s2.clone()]);
                }
            }
        }
        out
    };

    // main: len 0..=1; a,b: len 0..=1 → (1+7)^3 = 512 programs (fast).
    for b_main in bodies_of_len(0).into_iter().chain(bodies_of_len(1)) {
        for b_a in bodies_of_len(0).into_iter().chain(bodies_of_len(1)) {
            for b_b in bodies_of_len(0).into_iter().chain(bodies_of_len(1)) {
                let mut p = Program::new();
                p.add("main", b_main.clone());
                p.add("a", b_a.clone());
                p.add("b", b_b.clone());
                if let Err(e) = runtime_subset_of_static(&p) {
                    panic!("I4 violated for program: {p:?}\n{e}");
                }
                checked += 1;
            }
        }
    }
    assert!(checked >= 200, "exhaustive space too small: {checked}");
}

/// Cyclic programs must not panic; property still holds (fuel-bounded runtime).
#[test]
fn cyclic_programs_do_not_break_i4() {
    let mut p = Program::new();
    p.add("main", vec![Stmt::Call("a".into())]);
    p.add("a", vec![Stmt::Call("b".into())]);
    p.add("b", vec![Stmt::Call("a".into())]);
    assert!(runtime_subset_of_static(&p).is_ok());
}
