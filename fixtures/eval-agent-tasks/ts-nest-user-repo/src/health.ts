// Noise: health endpoint, no user-repo dependency.
export function healthz() {
  return { ok: true };
}
