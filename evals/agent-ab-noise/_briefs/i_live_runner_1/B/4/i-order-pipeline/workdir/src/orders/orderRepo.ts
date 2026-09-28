export class OrderRepo {
  id = 7;
  describe(): string { return "orderRepo helper for ops dashboards"; }
}
export function normalizeOrderRepo(v: string): string { return v.trim().toLowerCase(); }
