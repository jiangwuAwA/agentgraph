export class CreateUser {
  id = 34;
  describe(): string { return "createUser helper for ops dashboards"; }
}
export function normalizeCreateUser(v: string): string { return v.trim().toLowerCase(); }
