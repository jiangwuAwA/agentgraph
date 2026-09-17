/-
  MiniLang — L3 I4 containment theorem (Lean 4, stdlib only).

  Mirrors `src/formal/mini_lang.rs`:
    Stmt  = Call | DispatchLit | If (both arms) | Nop
    Program = finite map name → body
    direct edges from Call / DispatchLit (If collects both arms)
    static closure = transitive closure of direct edges
    runtime calls = edges recorded while entering functions from `main`

  Theorem (I4):
    every runtime call edge is contained in the static call closure.

  Non-goals: JS/TS/Python/Go ecosystem soundness; L2 `--sound`; full agentgraph graph.
-/

namespace MiniLang

abbrev Name := String

inductive Stmt where
  | call (callee : Name)
  | dispatchLit (key : Name)
  | ite (thenB elseB : List Stmt)
  | nop
  deriving Repr, Inhabited

abbrev Program := List (Name × List Stmt)

namespace Program

def hasName (P : Program) (n : Name) : Bool :=
  P.any (fun p => p.1 == n)

/-- Body lookup, same as Rust `BTreeMap::get` (missing → `[]`). -/
def body : Program → Name → List Stmt
  | [], _ => []
  | (n', b) :: rest, n => if n' == n then b else body rest n

theorem body_eq_of_head (b : List Stmt) (rest : Program) (n : Name) :
    body ((n, b) :: rest) n = b := by
  simp [body]

theorem eq_of_beq_name {a b : Name} (h : (a == b) = true) : a = b :=
  LawfulBEq.eq_of_beq h

theorem exists_body_of_hasName :
    ∀ (P : Program) (n : Name), P.hasName n = true →
      ∃ bd, (n, bd) ∈ P ∧ P.body n = bd := by
  intro P
  induction P with
  | nil =>
    intro n h
    cases h
  | cons p P ih =>
    intro n h
    have hany : (p.1 == n) = true ∨ (P.any fun q => q.1 == n) = true := by
      simpa [hasName, List.any, Bool.or_eq_true] using h
    cases hany with
    | inl hhead =>
      have hpn : p.1 = n := eq_of_beq_name hhead
      cases p with
      | mk fst snd =>
        have hfst : fst = n := hpn
        subst hfst
        exact ⟨snd, List.Mem.head _, body_eq_of_head snd P fst⟩
    | inr htail =>
      obtain ⟨bd, hmem, hbody⟩ := ih n htail
      cases p with
      | mk fst snd =>
        by_cases heq : fst == n
        · have hfst : fst = n := eq_of_beq_name heq
          subst hfst
          exact ⟨snd, List.Mem.head _, body_eq_of_head snd P fst⟩
        · have hfind : body ((fst, snd) :: P) n = body P n := by
            simp [body, heq]
          refine ⟨bd, List.Mem.tail _ hmem, ?_⟩
          rw [hfind]
          exact hbody

end Program

/-! ## Static analysis -/

def directEdgesOf (caller : Name) : List Stmt → List (Name × Name)
  | [] => []
  | .call c :: rest => (caller, c) :: directEdgesOf caller rest
  | .dispatchLit k :: rest => (caller, k) :: directEdgesOf caller rest
  | .ite t e :: rest =>
      directEdgesOf caller t ++ directEdgesOf caller e ++ directEdgesOf caller rest
  | .nop :: rest => directEdgesOf caller rest

def directEdges : Program → List (Name × Name)
  | [] => []
  | (n, bd) :: rest => directEdgesOf n bd ++ directEdges rest

inductive Reaches (P : Program) : Name → Name → Prop where
  | ofDirect {a b : Name} (h : (a, b) ∈ directEdges P) : Reaches P a b
  | ofTail {a b c : Name} (h1 : Reaches P a b) (h2 : (b, c) ∈ directEdges P) :
      Reaches P a c

theorem reaches_trans {P : Program} {b c : Name} (hbc : Reaches P b c) :
    ∀ {a}, Reaches P a b → Reaches P a c := by
  induction hbc with
  | ofDirect h =>
    intro a hab
    exact Reaches.ofTail hab h
  | ofTail h1 h2 ih =>
    intro a hab
    exact Reaches.ofTail (ih hab) h2

/-! ## Runtime call relation -/

inductive StmtCalls (caller : Name) : Stmt → Name → Prop where
  | mkCall {c : Name} : StmtCalls caller (.call c) c
  | mkDispatch {k : Name} : StmtCalls caller (.dispatchLit k) k

inductive BodyCalls (caller : Name) : List Stmt → Name → Prop where
  | here {s : Stmt} {rest : List Stmt} {c : Name}
      (h : StmtCalls caller s c) : BodyCalls caller (s :: rest) c
  | there {s : Stmt} {rest : List Stmt} {c : Name}
      (h : BodyCalls caller rest c) : BodyCalls caller (s :: rest) c
  | inThen {t e rest : List Stmt} {c : Name}
      (h : BodyCalls caller t c) : BodyCalls caller (.ite t e :: rest) c
  | inElse {t e rest : List Stmt} {c : Name}
      (h : BodyCalls caller e c) : BodyCalls caller (.ite t e :: rest) c

inductive Enters (P : Program) : Name → Prop where
  | main (h : P.hasName "main" = true) : Enters P "main"
  | step {a b : Name}
      (ent : Enters P a)
      (calls : BodyCalls a (P.body a) b)
      (hb : P.hasName b = true) : Enters P b

theorem enters_hasName {P : Program} {a : Name} (h : Enters P a) :
    P.hasName a = true := by
  induction h with
  | main h => exact h
  | step _ _ hb _ => exact hb

inductive RuntimeEdge (P : Program) : Name → Name → Prop where
  | mk {a b : Name}
      (ent : Enters P a)
      (calls : BodyCalls a (P.body a) b) : RuntimeEdge P a b

/-! ## Collector soundness -/

theorem bodyCalls_mem {caller : Name} {body : List Stmt} {c : Name}
    (h : BodyCalls caller body c) :
    (caller, c) ∈ directEdgesOf caller body := by
  induction h with
  | @here s rest c hsc =>
    cases hsc with
    | mkCall => simp [directEdgesOf]
    | mkDispatch => simp [directEdgesOf]
  | @there s rest c h ih =>
    cases s with
    | call c' => simp [directEdgesOf]; exact Or.inr ih
    | dispatchLit k => simp [directEdgesOf]; exact Or.inr ih
    | nop => simp [directEdgesOf]; exact ih
    | ite t e =>
      simp [directEdgesOf, List.mem_append]
      exact Or.inr (Or.inr ih)
  | @inThen t e rest c h ih =>
    simp [directEdgesOf, List.mem_append]
    exact Or.inl ih
  | @inElse t e rest c h ih =>
    simp [directEdgesOf, List.mem_append]
    exact Or.inr (Or.inl ih)

theorem mem_directEdges {P : Program} {a b : Name} {bd : List Stmt}
    (hmem : (a, bd) ∈ P)
    (h : (a, b) ∈ directEdgesOf a bd) :
    (a, b) ∈ directEdges P := by
  induction P with
  | nil => cases hmem
  | cons p P ih =>
    cases p with
    | mk n bd' =>
      cases hmem with
      | head =>
        simp [directEdges]
        exact Or.inl h
      | tail _ hrest =>
        simp [directEdges]
        exact Or.inr (ih hrest)

theorem runtimeEdge_direct {P : Program} {a b : Name}
    (h : RuntimeEdge P a b) :
    (a, b) ∈ directEdges P := by
  cases h with
  | mk ent calls =>
    obtain ⟨bd, hmem, hbody⟩ := Program.exists_body_of_hasName P a (enters_hasName ent)
    have hcall : (a, b) ∈ directEdgesOf a bd := by
      rw [← hbody]
      exact bodyCalls_mem calls
    exact mem_directEdges hmem hcall

/-! ## Main theorem (I4) -/

/-- I4: every runtime call edge is in the static transitive call-closure. -/
theorem runtime_subset_static (P : Program) (a b : Name)
    (h : RuntimeEdge P a b) : Reaches P a b :=
  Reaches.ofDirect (runtimeEdge_direct h)

/-- Direct edges sit inside the static closure. -/
theorem directEdges_subset_static (P : Program) (a b : Name)
    (h : (a, b) ∈ directEdges P) : Reaches P a b :=
  Reaches.ofDirect h

/-- Static closure is transitive (matches Floyd-style expansion). -/
theorem static_closure_transitive (P : Program) {a b c : Name}
    (h1 : Reaches P a b) (h2 : Reaches P b c) : Reaches P a c :=
  reaches_trans h2 h1

end MiniLang
