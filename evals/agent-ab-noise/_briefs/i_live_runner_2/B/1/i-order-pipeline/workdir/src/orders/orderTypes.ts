export class OrderTypes {
  id = 5;
  describe(): string { return "orderTypes helper for ops dashboards"; }
}
export function normalizeOrderTypes(v: string): string { return v.trim().toLowerCase(); }
