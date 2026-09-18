// Public synthetic golden — Express-style router (M3-D / Track M5).
// Candidates only — not sound.

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
  router.get('/users', getUsers);
  router.post('/users', createUser);
  app.use(authMiddleware);
}
