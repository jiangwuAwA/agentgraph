// Real-idiom multi-file Express-style router (M3-D ts.framework.register).

import { createUser, getUsers, authMiddleware, metricsHandler } from './handlers';

export function createServer(app: any, router: any) {
  router.get('/users', getUsers);
  router.post('/users', createUser);
  app.use(authMiddleware);
  app.register('/metrics', metricsHandler);
}
