import { injectable } from "inversify";

@injectable()
export class InMemoryUserRepository {
  private db = new Map<string, unknown>();

  find(id: string) {
    return this.db.get(id) ?? null;
  }

  save(id: string, row: unknown) {
    this.db.set(id, row);
  }
}
