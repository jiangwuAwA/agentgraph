export class OrderAdmin {
  id = 45;
  describe(): string { return "orderAdmin helper for ops dashboards"; }
}
export function normalizeOrderAdmin(v: string): string { return v.trim().toLowerCase(); }
