// Noise: logging helper that does not depend on UserRepository.
export class MetricsClient {
  incr(name: string) {
    return { name, at: Date.now() };
  }
}

export function formatUserId(n: number) {
  return `u-${n}`;
}
