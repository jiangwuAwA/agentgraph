// M3-D: Express/Fastify-style router registration beyond Nest.
// Heuristic `ts.framework.register`. Candidates only — not sound.

export function getUsers() {
  return [];
}

export function createUser() {
  return {};
}

export function authMiddleware() {
  return true;
}

export function metricsHandler() {
  return 1;
}

export function bootstrap(app: any, router: any) {
  router.get('/users', getUsers);
  router.post('/users', createUser);
  app.use(authMiddleware);
  app.register('/metrics', metricsHandler);
}
