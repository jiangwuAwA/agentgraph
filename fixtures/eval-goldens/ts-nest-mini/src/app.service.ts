import { Injectable } from '@nestjs/common';

/** Synthetic Nest-like service (public domain shape — not a real app). */
@Injectable()
export class AppService {
  getHello(): string {
    return 'golden-hello';
  }
}
