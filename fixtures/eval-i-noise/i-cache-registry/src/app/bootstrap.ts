import { CacheRegistry } from "../cache/registry";

export function bootstrap(): CacheRegistry {
  const reg = new CacheRegistry();
  reg.register("boot", true);
  return reg;
}
