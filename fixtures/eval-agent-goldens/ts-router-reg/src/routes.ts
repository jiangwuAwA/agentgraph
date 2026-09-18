// Public synthetic golden — Express-style registration (P1-3 edge_role).
// Candidates only — not a complete runtime graph.

export function getUsers() {
  return [];
}

export function createUser() {
  return {};
}

export function authMiddleware() {
  return true;
}

export function bootstrap(app: any, router: any) {
  router.get("/users", getUsers);
  router.post("/users", createUser);
  app.use(authMiddleware);
}
