export class OrderLogger {
  id = 0;
  describe(): string { return "orderLogger helper for ops dashboards"; }
}
export function normalizeOrderLogger(v: string): string { return v.trim().toLowerCase(); }
