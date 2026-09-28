import { TtlCache } from "./ttlCache";

export class CacheRegistry {
  private cache = new TtlCache();
  register(key: string, value: unknown): void {
    this.cache.set(key, value);
  }
  lookup(key: string): unknown {
    return this.cache.get(key);
  }
}
