-------------------------- MODULE IncrementalIndex --------------------------
(*
 * L3 model: incremental code index + full resolved_symbol_id relink.
 *
 * Abstracts agentgraph's SQLite store:
 *   - files  : path -> hash, language
 *   - symbols: path -> set of symbol names (from extract)
 *   - refs   : path -> set of (refName, resolvedOpt)
 *
 * Actions:
 *   IndexFile(path, C)     — replace path's rows with extract(C)
 *   PruneMissing(keep)     — drop files not in keep
 *   RelinkSids             — recompute every ref.resolved from current symbols
 *
 * Properties (I2):
 *   TypeInvariant     — DB shape stays well-formed
 *   ReindexCorrect    — after IndexFile, that path's rows = extract(content)
 *   NoDanglingSid     — Relink never leaves resolved pointing at a missing symbol
 *
 * TLC: tlc2 IncrementalIndex
 *)

EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS
  Paths,          \* finite set of file paths
  Contents,       \* finite set of possible file contents (abstract strings)
  Names           \* finite set of possible symbol / ref names

VARIABLES
  fileHash,       \* path ∈ Paths → hash ∈ {"", "H"}  ("" = absent)
  fileContent,    \* path ∈ Paths → content ∈ Contents ∪ {NoContent}
  symbols,        \* path ∈ Paths → SUBSET Names
  refs,           \* path ∈ Paths → SUBSET [name: Names]
  sid,            \* [path ∈ Paths, name ∈ Names] → path ∈ Paths ∪ {NoPath}
  dbOK            \* boolean: last operation succeeded (model bookkeeping)

NoContent == CHOOSE c : c \notin Contents
NoPath    == CHOOSE p : p \notin Paths

(* Abstract extract: content c maps to a deterministic symbol/ref set.
   Shrink Contents so TLC can enumerate; hash is just "changed or not". *)
ExtractSymbols(c) ==
  IF c = NoContent THEN {}
  ELSE IF c % 2 = 0 THEN { "f0", "g0" }
  ELSE { "f1" }

ExtractRefs(c) ==
  IF c = NoContent THEN {}
  ELSE IF c % 2 = 0 THEN { [name |-> "f0"], [name |-> "g0"] }
  ELSE { [name |-> "f1"] }

TypeInvariant ==
  /\ fileHash \in [Paths -> {"", "H"}]
  /\ fileContent \in [Paths -> Contents \cup {NoContent}]
  /\ \A p \in Paths :
       /\ symbols[p] \subseteq Names
       /\ \A r \in refs[p] : r.name \in Names
       /\ (fileHash[p] = "") => (fileContent[p] = NoContent)
       /\ (fileHash[p] # "") => (fileContent[p] \in Contents)

(* Replace one file's rows — models store.replace_file *)
IndexFile(p, c) ==
  /\ p \in Paths
  /\ c \in Contents
  /\ fileHash'   = [fileHash EXCEPT ![p] = "H"]
  /\ fileContent'= [fileContent EXCEPT ![p] = c]
  /\ symbols'    = [symbols EXCEPT ![p] = ExtractSymbols(c)]
  /\ refs'       = [refs EXCEPT ![p] = ExtractRefs(c)]
  \* sid relink happens as a separate Relink step (matches resolve_symbol_ids after batch)
  /\ UNCHANGED <<sid, dbOK>>

PruneMissing(keep) ==
  /\ keep \subseteq Paths
  /\ fileHash' = [p \in Paths |-> IF p \in keep THEN fileHash[p] else ""]
  /\ fileContent' = [p \in Paths |-> IF p \in keep THEN fileContent[p] else NoContent]
  /\ symbols' = [p \in Paths |-> IF p \in keep THEN symbols[p] else {}]
  /\ refs' = [p \in Paths |-> IF p \in keep THEN refs[p] else {}]
  /\ UNCHANGED <<sid, dbOK>>

(* Full relink: every ref resolves to some path that defines that name,
   or NoPath when unresolvable. Mirrors "clear all, then re-match". *)
RelinkSids ==
  /\ sid' = [ r \in [Paths -> Names] |->
                LET p == r.path  n == r.name
                IN  IF \E q \in Paths : n \in symbols[q]
                    THEN (CHOOSE q \in Paths : n \in symbols[q])
                    ELSE NoPath ]
  /\ UNCHANGED <<fileHash, fileContent, symbols, refs, dbOK>>

Init ==
  /\ fileHash = [p \in Paths |-> ""]
  /\ fileContent = [p \in Paths |-> NoContent]
  /\ symbols = [p \in Paths |-> {}]
  /\ refs = [p \in Paths |-> {}]
  /\ sid = [r \in [Paths -> Names] |-> NoPath]
  /\ dbOK = TRUE

Next ==
  \/ \E p \in Paths, c \in Contents : IndexFile(p, c)
  \/ \E keep \subseteq Paths : PruneMissing(keep)
  \/ RelinkSids

Spec == Init /\ [][Next]_<<fileHash, fileContent, symbols, refs, sid, dbOK>>

(* I2: after indexing, path rows match extract of stored content. *)
ReindexCorrect ==
  \A p \in Paths :
    fileHash[p] = "H" =>
      /\ symbols[p] = ExtractSymbols(fileContent[p])
      /\ refs[p] = ExtractRefs(fileContent[p])

(* After Relink, no ref name resolves to a path that lacks that symbol.
   Checked only on states reached after Relink; as invariant over all
   reachable states it may fail before first Relink — use as action property. *)
NoDanglingSid ==
  \A p \in Paths, n \in Names :
    (sid[p, n] # NoPath) => (n \in symbols[sid[p, n]])

=============================================================================
