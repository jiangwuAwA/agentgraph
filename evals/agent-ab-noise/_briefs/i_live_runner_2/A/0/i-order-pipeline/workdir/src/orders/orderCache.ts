export class OrderCache {
  id = 6;
  describe(): string { return "orderCache helper for ops dashboards"; }
}
export function normalizeOrderCache(v: string): string { return v.trim().toLowerCase(); }
