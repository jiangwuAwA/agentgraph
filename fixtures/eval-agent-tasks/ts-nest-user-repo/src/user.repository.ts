// Synthetic Nest-like repository (public domain shape).
export class UserRepository {
  private rows: Record<string, { id: string; email: string }> = {};

  find(id: string) {
    const row = this.rows[id];
    if (!row) throw new Error('not found');
    return row;
  }

  save(id: string, email: string) {
    this.rows[id] = { id, email };
    return this.rows[id];
  }
}
