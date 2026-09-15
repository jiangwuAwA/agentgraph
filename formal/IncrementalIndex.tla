-------------------------- MODULE IncrementalIndex --------------------------
(*
 * L3 model: incremental code index + full resolved_symbol_id relink.
 *
 * Checked with TLC (see formal/README.md / run-tlc.cmd).
 * Properties: TypeInvariant, ReindexCorrect (I2).
 *)

EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
  Paths,      \* {p1, p2}
  ContentA,   \* abstract content that defines {symF0, symG0}
  ContentB,   \* abstract content that defines {symF1}
  NoContent,  \* file absent
  NoPath,     \* sid unresolved
  symF0, symG0, symF1

Contents == {ContentA, ContentB}
Names    == {symF0, symG0, symF1}

VARIABLES
  fileHash,    \* Paths -> {"", "H"}
  fileContent, \* Paths -> Contents \cup {NoContent}
  symbols,     \* Paths -> SUBSET Names
  refs,        \* Paths -> SUBSET Names
  sid          \* [path: Paths, name: Names] -> Paths \cup {NoPath}

vars == <<fileHash, fileContent, symbols, refs, sid>>

ExtractSymbols(c) ==
  IF c = NoContent THEN {}
  ELSE IF c = ContentA THEN {symF0, symG0}
  ELSE {symF1}

ExtractRefs(c) == ExtractSymbols(c)

TypeInvariant ==
  /\ fileHash \in [Paths -> {"", "H"}]
  /\ fileContent \in [Paths -> Contents \cup {NoContent}]
  /\ symbols \in [Paths -> SUBSET Names]
  /\ refs \in [Paths -> SUBSET Names]
  /\ sid \in [[path: Paths, name: Names] -> Paths \cup {NoPath}]
  /\ \A p \in Paths :
       /\ (fileHash[p] = "")  => (fileContent[p] = NoContent)
       /\ (fileHash[p] = "H") => (fileContent[p] \in Contents)

Init ==
  /\ fileHash    = [p \in Paths |-> ""]
  /\ fileContent = [p \in Paths |-> NoContent]
  /\ symbols     = [p \in Paths |-> {}]
  /\ refs        = [p \in Paths |-> {}]
  /\ sid         = [k \in [path: Paths, name: Names] |-> NoPath]

IndexFile(p, c) ==
  /\ p \in Paths
  /\ c \in Contents
  /\ fileHash'    = [fileHash EXCEPT ![p] = "H"]
  /\ fileContent' = [fileContent EXCEPT ![p] = c]
  /\ symbols'     = [symbols EXCEPT ![p] = ExtractSymbols(c)]
  /\ refs'        = [refs EXCEPT ![p] = ExtractRefs(c)]
  /\ UNCHANGED sid

PruneMissing(keep) ==
  /\ keep \in SUBSET Paths
  /\ fileHash'    = [p \in Paths |-> IF p \in keep THEN fileHash[p] ELSE ""]
  /\ fileContent' = [p \in Paths |-> IF p \in keep THEN fileContent[p] ELSE NoContent]
  /\ symbols'     = [p \in Paths |-> IF p \in keep THEN symbols[p] ELSE {}]
  /\ refs'        = [p \in Paths |-> IF p \in keep THEN refs[p] ELSE {}]
  /\ UNCHANGED sid

RelinkSids ==
  /\ sid' = [k \in [path: Paths, name: Names] |->
               IF \E q \in Paths : k.name \in symbols[q]
               THEN CHOOSE q \in Paths : k.name \in symbols[q]
               ELSE NoPath]
  /\ UNCHANGED <<fileHash, fileContent, symbols, refs>>

Next ==
  \/ \E p \in Paths, c \in Contents : IndexFile(p, c)
  \/ \E keep \in SUBSET Paths : PruneMissing(keep)
  \/ RelinkSids

Spec == Init /\ [][Next]_vars

ReindexCorrect ==
  \A p \in Paths :
    fileHash[p] = "H" =>
      /\ symbols[p] = ExtractSymbols(fileContent[p])
      /\ refs[p]    = ExtractRefs(fileContent[p])

NoDanglingSid ==
  \A p \in Paths, n \in Names :
    sid[[path |-> p, name |-> n]] # NoPath
      => n \in symbols[sid[[path |-> p, name |-> n]]]

=============================================================================
