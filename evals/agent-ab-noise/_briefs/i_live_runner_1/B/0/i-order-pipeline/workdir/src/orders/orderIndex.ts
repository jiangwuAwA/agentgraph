export class OrderIndex {
  id = 4;
  describe(): string { return "orderIndex helper for ops dashboards"; }
}
export function normalizeOrderIndex(v: string): string { return v.trim().toLowerCase(); }
