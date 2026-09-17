import { Controller, Get } from '@nestjs/common';
import { AppService } from './app.service';

/** Synthetic Nest-like controller (public domain shape — not a real app). */
@Controller()
export class AppController {
  constructor(private readonly appService: AppService) {}

  @Get()
  getHello(): string {
    return this.appService.getHello();
  }
}
