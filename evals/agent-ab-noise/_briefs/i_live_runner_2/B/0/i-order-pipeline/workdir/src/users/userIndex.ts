export class UserIndex {
  id = 32;
  describe(): string { return "userIndex helper for ops dashboards"; }
}
export function normalizeUserIndex(v: string): string { return v.trim().toLowerCase(); }
