export class OrderQueue {
  id = 8;
  describe(): string { return "orderQueue helper for ops dashboards"; }
}
export function normalizeOrderQueue(v: string): string { return v.trim().toLowerCase(); }
