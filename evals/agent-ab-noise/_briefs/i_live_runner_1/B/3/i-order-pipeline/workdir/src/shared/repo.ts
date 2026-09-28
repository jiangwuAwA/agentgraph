export class Repo {
  id = 44;
  describe(): string { return "repo helper for ops dashboards"; }
}
export function normalizeRepo(v: string): string { return v.trim().toLowerCase(); }
