import { Module } from '@nestjs/common';
import { AppController } from './app.controller';
import { AppService } from './app.service';

/** Synthetic Nest-like module (public domain shape — not a real app). */
export const ConfigToken = Symbol('ConfigToken');

export class ConfigService {}

@Module({
  controllers: [AppController],
  providers: [
    AppService,
    { provide: ConfigToken, useClass: ConfigService },
  ],
})
export class AppModule {}
