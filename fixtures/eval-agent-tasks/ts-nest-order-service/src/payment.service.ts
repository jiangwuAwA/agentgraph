import { Injectable } from '@nestjs/common';

@Injectable()
export class PaymentService {
  charge(userId: string, amount: number) {
    return { userId, amount, ok: true };
  }
}
