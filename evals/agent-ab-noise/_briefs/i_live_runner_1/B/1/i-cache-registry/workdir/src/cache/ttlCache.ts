export class TtlCache {
  private map = new Map<string, unknown>();
  set(key: string, value: unknown): void {
    this.map.set(key, value);
  }
  get(key: string): unknown {
    return this.map.get(key);
  }
}
