import { Injectable } from '@nestjs/common';
import { UserRepository } from './user.repository';

@Injectable()
export class UserService {
  constructor(private readonly userRepo: UserRepository) {}

  load(id: string) {
    return this.userRepo.find(id);
  }

  create(id: string, email: string) {
    return this.userRepo.save(id, email);
  }
}
