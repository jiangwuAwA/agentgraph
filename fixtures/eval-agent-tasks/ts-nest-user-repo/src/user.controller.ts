import { Controller, Get, Param } from '@nestjs/common';
import { UserService } from './user.service';

@Controller('users')
export class UserController {
  constructor(private readonly users: UserService) {}

  @Get(':id')
  load(@Param('id') id: string) {
    return this.users.load(id);
  }
}
