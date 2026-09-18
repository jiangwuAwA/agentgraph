// Public synthetic golden — type-only typeof Function (M2 over-flag / Track M5).
// Type-position Function / typeof Function must stay **in S** (no violation).
// Value-use of Function/eval would leave S — not present in this fixture.

export type Handler = (...args: unknown[]) => unknown;

export type Factory = typeof Function;

export interface Config {
  // type-only position — not a runtime Function constructor call
  handlerKind?: typeof Function;
  run: Handler;
}

export function createHandler(cfg: Config): Handler {
  // Exact call — no dynamic Function/eval here
  return (...args: unknown[]) => cfg.run(...args);
}

export function invoke(h: Handler) {
  return h(1, 2);
}
