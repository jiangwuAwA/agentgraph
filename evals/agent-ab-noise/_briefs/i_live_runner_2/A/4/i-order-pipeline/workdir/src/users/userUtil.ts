export class UserUtil {
  id = 31;
  describe(): string { return "userUtil helper for ops dashboards"; }
}
export function normalizeUserUtil(v: string): string { return v.trim().toLowerCase(); }
