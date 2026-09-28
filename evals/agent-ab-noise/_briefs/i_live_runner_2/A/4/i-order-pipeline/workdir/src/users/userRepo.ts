export class UserRepo {
  id = 35;
  describe(): string { return "userRepo helper for ops dashboards"; }
}
export function normalizeUserRepo(v: string): string { return v.trim().toLowerCase(); }
