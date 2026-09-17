import { Module } from '@nestjs/common';
import { AppController } from './app.controller';
import { AppService } from './app.service';

/** Stand-in for a dynamic forRoot module factory (public-domain shape). */
export const ObserveModule = {
  forRoot(_opts: Record<string, unknown>) {
    return class ObserveRoot {};
  },
};

export const CONFIG = Symbol('CONFIG');

export class ConfigService {}

@Module({
  imports: [ObserveModule.forRoot({ appKey: 'demo' })],
  controllers: [AppController],
  providers: [
    AppService,
    { provide: CONFIG, useClass: ConfigService },
  ],
})
export class AppModule {}
