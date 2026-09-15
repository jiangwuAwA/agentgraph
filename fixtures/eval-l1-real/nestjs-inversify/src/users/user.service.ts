import { injectable, inject } from "inversify";
import { TYPES } from "./types";
import type { Logger } from "./logger";
import type { UserRepository } from "./user.repository";

@injectable()
export class UserService {
  constructor(
    @inject(TYPES.UserRepository) private repo: UserRepository,
    @inject(TYPES.Logger) private log: Logger,
  ) {}

  async findById(id: string) {
    this.log.info(`find ${id}`);
    return this.repo.find(id);
  }
}
