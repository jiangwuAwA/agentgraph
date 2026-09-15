import { Container } from "inversify";
import { TYPES } from "./types";
import { UserService } from "./users/user.service";
import { InMemoryUserRepository } from "./users/user.repository";
import { ConsoleLogger } from "./logger";

export function buildContainer() {
  const c = new Container();
  c.bind(TYPES.UserRepository).to(InMemoryUserRepository);
  c.bind(TYPES.Logger).to(ConsoleLogger);
  c.bind(TYPES.UserService).to(UserService);
  return c;
}

export function main() {
  const c = buildContainer();
  return c.get<UserService>(TYPES.UserService);
}
