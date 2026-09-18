// Handler implementations for the Express-style corpus.

export function getUsers() {
  return [];
}

export function createUser() {
  return { ok: true };
}

export function authMiddleware(_req: any, _res: any, next: any) {
  next();
}

export function metricsHandler() {
  return 1;
}
