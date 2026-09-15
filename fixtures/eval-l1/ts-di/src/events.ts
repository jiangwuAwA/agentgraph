/**
 * Event bus + computed access — golden DynamicCandidate / Heuristic corpus.
 */
export class ClickHandler {
  onClick() {
    return "click";
  }
}

export class ResizeHandler {
  onResize() {
    return "resize";
  }
}

export function wireEvents(bus: any, registry: any) {
  bus.on("click", ClickHandler.prototype.onClick);
  const key = "onResize";
  // computed property with string literal → DynamicCandidate
  registry["onResize"]();
  // template key is NOT captured (dynamic)
  const dyn = `on${key}`;
  registry[dyn]();
}

export function createFromRegistry(registry: any) {
  return new (registry["ClickHandler"])();
}
