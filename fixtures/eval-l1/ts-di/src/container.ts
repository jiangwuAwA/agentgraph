/**
 * Minimal Inversify-style DI bootstrap — golden corpus for L1 eval.
 */
import { Container } from "inversify";

export class UserRepository {
  find(id: string) {
    return { id };
  }
}

export class UserService {
  constructor(private repo: UserRepository) {}
  load(id: string) {
    return this.repo.find(id);
  }
}

export class AuditLogger {
  log(msg: string) {
    return msg;
  }
}

export function bootstrap(c: Container) {
  c.register(UserRepository);
  c.bind(UserService).to(UserService);
  c.register(AuditLogger);
}

export function handleRequest(svc: UserService) {
  return svc.load("1");
}
